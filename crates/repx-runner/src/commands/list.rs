use crate::cli::{ListArgs, ListEntity};
use crate::error::CliError;
use repx_core::{
    config,
    constants::dirs,
    errors::{ConfigError, DomainError},
    lab,
    model::{JobId, Lab, RunId},
    resolver,
};
use std::path::Path;
use std::str::FromStr;

pub fn handle_list(args: ListArgs, lab_path: &Path) -> Result<(), CliError> {
    let lab = lab::load_from_path(lab_path)?;

    match args.entity.unwrap_or(ListEntity::Runs { name: None }) {
        ListEntity::Runs { name } => match name {
            Some(n) => list_jobs(&lab, Some(&n), None, false),
            None => list_runs(&lab, lab_path),
        },
        ListEntity::Jobs(job_args) => list_jobs(
            &lab,
            job_args.name.as_deref(),
            job_args.stage.as_deref(),
            job_args.output_paths,
        ),
        ListEntity::Dependencies { job_id } => list_dependencies(&lab, &job_id),
    }
}

fn list_runs(lab: &Lab, lab_path: &Path) -> Result<(), CliError> {
    println!("Available runs in '{}':", lab_path.display());

    let mut run_ids: Vec<_> = lab.runs.keys().collect();
    run_ids.sort();

    for run_id in run_ids {
        println!("  {}", run_id);
    }
    Ok(())
}

fn list_jobs(
    lab: &Lab,
    run_id_opt: Option<&str>,
    stage_filter: Option<&str>,
    show_output_paths: bool,
) -> Result<(), CliError> {
    let store_path = if show_output_paths {
        let config = config::load_config()?;
        let target_name = config.submission_target.as_ref().ok_or_else(|| {
            CliError::Config(ConfigError::General(
                "No submission target configured".to_string(),
            ))
        })?;
        let target = config.targets.get(target_name).ok_or_else(|| {
            CliError::Config(ConfigError::General(format!(
                "Target '{}' not found in config",
                target_name
            )))
        })?;
        Some(target.base_path.clone())
    } else {
        None
    };

    let run_id_str = match run_id_opt {
        Some(s) => s,
        None => {
            let mut run_ids: Vec<_> = lab.runs.keys().collect();
            run_ids.sort();
            for (i, run_id) in run_ids.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                println!("Jobs in run '{}':", run_id);
                let run = &lab.runs[*run_id];
                let mut jobs: Vec<_> = run.jobs.iter().collect();
                jobs.sort();

                let jobs: Vec<_> = if let Some(stage) = stage_filter {
                    jobs.into_iter()
                        .filter(|job_id| job_id.0.contains(stage))
                        .collect()
                } else {
                    jobs
                };

                for job in jobs {
                    print_job_line(job, show_output_paths, &store_path);
                }
            }
            return Ok(());
        }
    };

    let run_id = RunId::from_str(run_id_str)
        .map_err(|e| CliError::Config(ConfigError::General(e.to_string())))?;

    let matched_run = if let Some(run) = lab.runs.get(&run_id) {
        Some((&run_id, run))
    } else {
        let matches: Vec<_> = lab
            .runs
            .iter()
            .filter(|(k, _)| k.0.starts_with(&run_id.0))
            .collect();

        if matches.len() == 1 {
            Some(matches[0])
        } else if matches.len() > 1 {
            let options: Vec<String> = matches.iter().map(|(k, _)| k.0.clone()).collect();
            return Err(CliError::Domain(DomainError::AmbiguousJobId {
                input: run_id.0,
                matches: options,
            }));
        } else {
            None
        }
    };

    if let Some((id, run)) = matched_run {
        println!("Jobs in run '{}':", id);
        let mut jobs: Vec<_> = run.jobs.iter().collect();
        jobs.sort();

        let jobs: Vec<_> = if let Some(stage) = stage_filter {
            jobs.into_iter()
                .filter(|job_id| job_id.0.contains(stage))
                .collect()
        } else {
            jobs
        };

        if jobs.is_empty() && stage_filter.is_some() {
            println!(
                "  (no jobs matching stage filter '{}')",
                stage_filter.unwrap()
            );
        }

        for job in jobs {
            print_job_line(job, show_output_paths, &store_path);
        }
        Ok(())
    } else {
        let job_id_query = run_id_str;
        let matching_jobs: Vec<_> = lab
            .jobs
            .keys()
            .filter(|jid| jid.0.starts_with(job_id_query))
            .collect();

        if !matching_jobs.is_empty() {
            let mut found_runs = Vec::new();
            for (rid, r) in &lab.runs {
                for match_job in &matching_jobs {
                    if r.jobs.contains(match_job) {
                        found_runs.push(rid);
                    }
                }
            }
            found_runs.sort();
            found_runs.dedup();

            if !found_runs.is_empty() {
                println!("Job '{}' found in the following runs:", job_id_query);
                for rid in &found_runs {
                    println!("  {}", rid);
                }
                if found_runs.len() == 1 {
                    println!();
                    return list_jobs(lab, Some(&found_runs[0].0), stage_filter, show_output_paths);
                }
                return Ok(());
            }
        }

        Err(CliError::Domain(DomainError::TargetNotFound(run_id.0)))
    }
}

fn print_job_line(
    job_id: &JobId,
    show_output_paths: bool,
    store_path: &Option<std::path::PathBuf>,
) {
    if show_output_paths {
        if let Some(store) = store_path {
            let output_path = store.join(dirs::OUTPUTS).join(&job_id.0).join(dirs::OUT);
            if output_path.exists() {
                println!("  {}  {}", job_id, output_path.display());
            } else {
                println!("  {}  (not executed)", job_id);
            }
        } else {
            println!("  {}", job_id);
        }
    } else {
        println!("  {}", job_id);
    }
}

fn list_dependencies(lab: &Lab, job_id_str: &str) -> Result<(), CliError> {
    let target_input = RunId(job_id_str.to_string());
    let job_id = resolver::resolve_target_job_id(lab, &target_input)?;

    println!("Dependency tree for job '{}':", job_id.0);
    print_dependency_tree(lab, job_id, 0);
    Ok(())
}

fn print_dependency_tree(lab: &Lab, job_id: &JobId, level: usize) {
    let indent = "  ".repeat(level);
    println!("{}{}", indent, job_id.0);

    if let Some(job) = lab.jobs.get(job_id) {
        let mut dependencies = Vec::new();
        for executable in job.executables.values() {
            for input in &executable.inputs {
                if let Some(dep_id) = &input.job_id {
                    dependencies.push(dep_id);
                }
            }
        }
        dependencies.sort();
        dependencies.dedup();

        for dep_id in dependencies {
            print_dependency_tree(lab, dep_id, level + 1);
        }
    }
}

use crate::cli::{ListArgs, ListEntity};
use repx_core::error::AppError;
use repx_core::lab;
use repx_core::model::{JobId, Lab, RunId};
use repx_core::resolver;
use std::path::Path;
use std::str::FromStr;

pub fn handle_list(args: ListArgs, lab_path: &Path) -> Result<(), AppError> {
    let lab = lab::load_from_path(lab_path)?;

    match args.entity.unwrap_or(ListEntity::Runs) {
        ListEntity::Runs => list_runs(&lab, lab_path),
        ListEntity::Jobs { run_id } => list_jobs(&lab, &run_id),
        ListEntity::Dependencies { job_id } => list_dependencies(&lab, &job_id),
    }
}

fn list_runs(lab: &Lab, lab_path: &Path) -> Result<(), AppError> {
    println!("Available runs in '{}':", lab_path.display());

    let mut run_ids: Vec<_> = lab.runs.keys().collect();
    run_ids.sort();

    for run_id in run_ids {
        println!("  {}", run_id);
    }
    Ok(())
}

fn list_jobs(lab: &Lab, run_id_str: &str) -> Result<(), AppError> {
    let run_id =
        RunId::from_str(run_id_str).map_err(|e| AppError::ConfigurationError(e.to_string()))?;

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
            return Err(AppError::AmbiguousJobId {
                input: run_id.0,
                matches: options,
            });
        } else {
            None
        }
    };

    if let Some((id, run)) = matched_run {
        println!("Jobs in run '{}':", id);
        let mut jobs: Vec<_> = run.jobs.iter().collect();
        jobs.sort();
        for job in jobs {
            println!("  {}", job);
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
                    return list_jobs(lab, &found_runs[0].0);
                }
                return Ok(());
            }
        }

        Err(AppError::TargetNotFound(run_id.0))
    }
}

fn list_dependencies(lab: &Lab, job_id_str: &str) -> Result<(), AppError> {
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

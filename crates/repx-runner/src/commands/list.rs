use crate::cli::{ListArgs, ListEntity, ListJobsArgs};
use crate::commands::trace::compute_all_effective_params;
use crate::error::CliError;
use repx_core::{
    config,
    constants::dirs,
    errors::{ConfigError, DomainError},
    lab,
    model::{JobId, Lab, RunId},
    resolver,
};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

pub fn handle_list(args: ListArgs, lab_path: &Path) -> Result<(), CliError> {
    let lab = lab::load_from_path(lab_path)?;

    match args.entity.unwrap_or(ListEntity::Runs { name: None }) {
        ListEntity::Runs { name } => match name {
            Some(n) => list_jobs(
                &lab,
                &ListJobsArgs {
                    name: Some(n),
                    stage: None,
                    output_paths: false,
                    param: vec![],
                    group_by_stage: false,
                },
            ),
            None => list_runs(&lab, lab_path),
        },
        ListEntity::Jobs(job_args) => list_jobs(&lab, &job_args),
        ListEntity::Dependencies { job_id } => list_dependencies(&lab, &job_id),
        ListEntity::Groups { name } => list_groups(&lab, name.as_deref()),
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

struct ListJobsContext {
    store_path: Option<std::path::PathBuf>,
    effective_params: Option<HashMap<JobId, Value>>,
    param_keys: Vec<String>,
    group_by_stage: bool,
}

fn list_jobs(lab: &Lab, args: &ListJobsArgs) -> Result<(), CliError> {
    let store_path = if args.output_paths {
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

    let effective_params = if !args.param.is_empty() {
        Some(compute_all_effective_params(lab))
    } else {
        None
    };

    let ctx = ListJobsContext {
        store_path,
        effective_params,
        param_keys: args.param.clone(),
        group_by_stage: args.group_by_stage,
    };

    let run_id_str = match &args.name {
        Some(s) => s.as_str(),
        None => {
            let mut run_ids: Vec<_> = lab.runs.keys().collect();
            run_ids.sort();
            for (i, run_id) in run_ids.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                println!("Jobs in run '{}':", run_id);
                let run = &lab.runs[*run_id];
                let jobs: Vec<_> = run.jobs.iter().collect();
                print_jobs_list(lab, jobs, args.stage.as_deref(), &ctx);
            }
            return Ok(());
        }
    };

    if let Some(group_name) = run_id_str.strip_prefix('@') {
        if group_name.is_empty() {
            return Err(CliError::Domain(
                repx_core::errors::DomainError::EmptyGroupName,
            ));
        }
        match lab.groups.get(group_name) {
            Some(run_ids) => {
                let mut sorted_ids: Vec<_> = run_ids.iter().collect();
                sorted_ids.sort();
                for (i, run_id) in sorted_ids.iter().enumerate() {
                    if i > 0 {
                        println!();
                    }
                    if let Some(run) = lab.runs.get(*run_id) {
                        println!("Jobs in run '{}' (group @{}):", run_id, group_name);
                        let jobs: Vec<_> = run.jobs.iter().collect();
                        print_jobs_list(lab, jobs, args.stage.as_deref(), &ctx);
                    }
                }
                return Ok(());
            }
            None => {
                let available: Vec<_> = {
                    let mut keys: Vec<_> = lab.groups.keys().cloned().collect();
                    keys.sort();
                    keys
                };
                return Err(CliError::Domain(
                    repx_core::errors::DomainError::UnknownGroup {
                        name: group_name.to_string(),
                        available,
                    },
                ));
            }
        }
    }

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
        let jobs: Vec<_> = run.jobs.iter().collect();
        print_jobs_list(lab, jobs, args.stage.as_deref(), &ctx);
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
                    let new_args = ListJobsArgs {
                        name: Some(found_runs[0].0.clone()),
                        stage: args.stage.clone(),
                        output_paths: args.output_paths,
                        param: args.param.clone(),
                        group_by_stage: args.group_by_stage,
                    };
                    return list_jobs(lab, &new_args);
                }
                return Ok(());
            }
        }

        Err(CliError::Domain(DomainError::TargetNotFound(run_id.0)))
    }
}

fn extract_stage_name(job_id: &JobId) -> String {
    let s = &job_id.0;
    if let Some(first_dash) = s.find('-') {
        let after_hash = &s[first_dash + 1..];
        if let Some(last_dash) = after_hash.rfind('-') {
            let potential_version = &after_hash[last_dash + 1..];
            if potential_version.contains('.')
                || potential_version.chars().all(|c| c.is_ascii_digit())
            {
                return after_hash[..last_dash].to_string();
            }
        }
        return after_hash.to_string();
    }
    s.to_string()
}

fn print_jobs_list(
    lab: &Lab,
    jobs: Vec<&JobId>,
    stage_filter: Option<&str>,
    ctx: &ListJobsContext,
) {
    let mut jobs: Vec<_> = jobs;
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
        return;
    }

    if ctx.group_by_stage {
        let mut groups: HashMap<String, Vec<&JobId>> = HashMap::new();
        for job_id in &jobs {
            let stage = extract_stage_name(job_id);
            groups.entry(stage).or_default().push(job_id);
        }

        let mut stage_names: Vec<_> = groups.keys().collect();
        stage_names.sort();

        for stage_name in stage_names {
            println!("  [{}]", stage_name);
            let stage_jobs = &groups[stage_name];
            for job_id in stage_jobs {
                print_job_line(lab, job_id, ctx, 4);
            }
        }
    } else {
        for job_id in jobs {
            print_job_line(lab, job_id, ctx, 2);
        }
    }
}

fn print_job_line(lab: &Lab, job_id: &JobId, ctx: &ListJobsContext, indent: usize) {
    let prefix = " ".repeat(indent);
    let mut line = format!("{}{}", prefix, job_id);

    if !ctx.param_keys.is_empty() {
        if let Some(ref all_params) = ctx.effective_params {
            if let Some(params) = all_params.get(job_id) {
                let mut param_strs = Vec::new();
                for key in &ctx.param_keys {
                    let value = get_nested_value(params, key);
                    param_strs.push(format!("{}={}", key, format_param_value(&value)));
                }
                if !param_strs.is_empty() {
                    line.push_str("  ");
                    line.push_str(&param_strs.join(" "));
                }
            }
        }
    }

    if let Some(ref store) = ctx.store_path {
        let output_path = store.join(dirs::OUTPUTS).join(&job_id.0).join(dirs::OUT);
        if output_path.exists() {
            line.push_str(&format!("  {}", output_path.display()));
        } else {
            line.push_str("  (not executed)");
        }
    }

    println!("{}", line);

    let _ = lab;
}

fn get_nested_value(value: &Value, key: &str) -> Value {
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = value;
    for part in parts {
        match current {
            Value::Object(map) => {
                if let Some(v) = map.get(part) {
                    current = v;
                } else {
                    return Value::Null;
                }
            }
            _ => return Value::Null,
        }
    }
    current.clone()
}

fn format_param_value(value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::String(s) => {
            if s.starts_with("/nix/store/") && s.len() > 40 {
                if let Some(last_slash) = s.rfind('/') {
                    return format!("...{}", &s[last_slash..]);
                }
            }
            s.clone()
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(arr) => format!("[{}]", arr.len()),
        Value::Object(obj) => format!("{{{}}}", obj.len()),
    }
}

fn list_groups(lab: &Lab, name: Option<&str>) -> Result<(), CliError> {
    match name {
        None => {
            if lab.groups.is_empty() {
                println!("No groups defined in this lab.");
                return Ok(());
            }
            println!("Available groups:");
            let mut group_names: Vec<_> = lab.groups.keys().collect();
            group_names.sort();
            for group_name in group_names {
                let run_ids = &lab.groups[group_name];
                println!("  @{} ({} runs)", group_name, run_ids.len());
            }
        }
        Some(group_name) => match lab.groups.get(group_name) {
            Some(run_ids) => {
                println!("Runs in group '@{}':", group_name);
                let mut sorted_ids: Vec<_> = run_ids.iter().collect();
                sorted_ids.sort();
                for run_id in sorted_ids {
                    println!("  {}", run_id);
                }
            }
            None => {
                let available: Vec<_> = {
                    let mut keys: Vec<_> = lab.groups.keys().cloned().collect();
                    keys.sort();
                    keys
                };
                if available.is_empty() {
                    eprintln!(
                        "Unknown group '{}'. No groups are defined in this lab.",
                        group_name
                    );
                } else {
                    eprintln!(
                        "Unknown group '{}'. Available groups: {}",
                        group_name,
                        available.join(", ")
                    );
                }
            }
        },
    }
    Ok(())
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

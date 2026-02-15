use crate::cli::TraceParamsArgs;
use crate::error::CliError;
use repx_core::{
    lab,
    model::{Job, JobId, Lab},
    resolver,
};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn compute_effective_params(lab: &Lab, job_id: &JobId) -> Value {
    let mut memo: HashMap<&JobId, Value> = HashMap::new();
    compute_effective_params_recursive(lab, job_id, &mut HashSet::new(), &mut memo)
}

fn compute_effective_params_recursive<'a>(
    lab: &'a Lab,
    job_id: &'a JobId,
    visiting: &mut HashSet<&'a JobId>,
    memo: &mut HashMap<&'a JobId, Value>,
) -> Value {
    if let Some(cached) = memo.get(job_id) {
        return cached.clone();
    }

    if visiting.contains(job_id) {
        tracing::warn!("Circular dependency detected at job: {}", job_id);
        return Value::Object(Map::new());
    }

    let job = match lab.jobs.get(job_id) {
        Some(j) => j,
        None => {
            tracing::warn!("Job not found: {}", job_id);
            return Value::Object(Map::new());
        }
    };

    visiting.insert(job_id);

    let mut effective = Map::new();

    let dep_ids: Vec<&JobId> = get_dependency_ids(job);

    for dep_id in dep_ids {
        let dep_params = compute_effective_params_recursive(lab, dep_id, visiting, memo);
        if let Value::Object(dep_map) = dep_params {
            merge_json_objects(&mut effective, dep_map);
        }
    }

    if let Value::Object(own_params) = &job.params {
        merge_json_objects(&mut effective, own_params.clone());
    }

    visiting.remove(job_id);

    let result = Value::Object(effective);
    memo.insert(job_id, result.clone());
    result
}

fn get_dependency_ids(job: &Job) -> Vec<&JobId> {
    let mut deps = Vec::new();
    let mut seen = HashSet::new();

    for executable in job.executables.values() {
        for input in &executable.inputs {
            if let Some(dep_id) = &input.job_id {
                if seen.insert(dep_id) {
                    deps.push(dep_id);
                }
            }
        }
    }

    deps.sort();
    deps
}

fn merge_json_objects(target: &mut Map<String, Value>, source: Map<String, Value>) {
    for (key, value) in source {
        target.insert(key, value);
    }
}

pub fn compute_all_effective_params(lab: &Lab) -> HashMap<JobId, Value> {
    let mut memo: HashMap<&JobId, Value> = HashMap::new();
    let mut result = HashMap::new();

    for job_id in lab.jobs.keys() {
        let effective =
            compute_effective_params_recursive(lab, job_id, &mut HashSet::new(), &mut memo);
        result.insert(job_id.clone(), effective);
    }

    result
}

pub fn handle_trace_params(args: TraceParamsArgs, lab_path: &Path) -> Result<(), CliError> {
    let lab = lab::load_from_path(lab_path)?;

    let results: HashMap<JobId, Value> = if let Some(job_id_query) = &args.job_id {
        let run_id = repx_core::model::RunId(job_id_query.clone());
        let job_id = resolver::resolve_target_job_id(&lab, &run_id)?;

        let effective = compute_effective_params(&lab, job_id);
        let mut map = HashMap::new();
        map.insert(job_id.clone(), effective);
        map
    } else {
        compute_all_effective_params(&lab)
    };

    let output: HashMap<String, Value> = results.into_iter().map(|(k, v)| (k.0, v)).collect();

    let json_str =
        serde_json::to_string_pretty(&output).map_err(|e| CliError::ExecutionFailed {
            message: "Failed to serialize params".to_string(),
            log_path: None,
            log_summary: e.to_string(),
        })?;

    println!("{}", json_str);
    Ok(())
}

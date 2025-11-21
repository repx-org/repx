use crate::model::{Job, JobId, Lab, RunId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum JobStatus {
    Succeeded { location: String },
    Failed { location: String },
    Pending,
    Queued,
    Running,
    Blocked { missing_deps: HashSet<JobId> },
}
fn get_all_dependencies(job: &Job) -> impl Iterator<Item = &JobId> {
    job.executables
        .values()
        .flat_map(|exe| exe.inputs.iter())
        .filter_map(|mapping| mapping.job_id.as_ref())
        .collect::<HashSet<_>>()
        .into_iter()
}

pub fn determine_job_statuses(
    lab: &Lab,
    found_statuses: &HashMap<JobId, JobStatus>,
) -> HashMap<JobId, JobStatus> {
    let mut cache: HashMap<JobId, JobStatus> = found_statuses.clone();

    for job_id in lab.jobs.keys() {
        resolve_job_status(job_id, lab, &mut cache);
    }

    cache
}

pub fn resolve_job_status<'a>(
    job_id: &'a JobId,
    lab: &'a Lab,
    cache: &'a mut HashMap<JobId, JobStatus>,
) -> &'a JobStatus {
    if cache.contains_key(job_id) {
        return cache.get(job_id).unwrap();
    }

    let job = lab.jobs.get(job_id).expect("Job ID must exist in lab");

    let mut missing_deps = HashSet::new();
    let mut all_deps_succeeded = true;

    let dependencies = get_all_dependencies(job);
    for dep_id in dependencies {
        let dep_status = resolve_job_status(dep_id, lab, cache);
        if !matches!(dep_status, JobStatus::Succeeded { .. }) {
            all_deps_succeeded = false;
            missing_deps.insert(dep_id.clone());
        }
    }

    let status = if all_deps_succeeded {
        JobStatus::Pending
    } else {
        JobStatus::Blocked { missing_deps }
    };

    cache.insert(job_id.clone(), status);
    cache.get(job_id).unwrap()
}

pub fn determine_run_aggregate_statuses(
    lab: &Lab,
    all_job_statuses: &HashMap<JobId, JobStatus>,
) -> BTreeMap<RunId, JobStatus> {
    lab.runs
        .iter()
        .map(|(run_id, run)| {
            let mut has_failed = false;
            let mut has_running = false;
            let mut has_queued = false;
            let mut has_pending = false;
            let mut has_blocked = false;
            let mut succeeded_count = 0;

            for job_id in &run.jobs {
                match all_job_statuses.get(job_id) {
                    Some(JobStatus::Succeeded { .. }) => succeeded_count += 1,
                    Some(JobStatus::Failed { .. }) => has_failed = true,
                    Some(JobStatus::Running) => has_running = true,
                    Some(JobStatus::Queued) => has_queued = true,
                    Some(JobStatus::Pending) => has_pending = true,
                    Some(JobStatus::Blocked { .. }) => has_blocked = true,
                    None => has_blocked = true,
                }
            }

            let aggregate_status = if has_failed {
                JobStatus::Failed {
                    location: "".to_string(),
                }
            } else if has_running {
                JobStatus::Running
            } else if has_queued {
                JobStatus::Queued
            } else if has_pending {
                JobStatus::Pending
            } else if has_blocked {
                JobStatus::Blocked {
                    missing_deps: Default::default(),
                }
            } else if succeeded_count == run.jobs.len() && !run.jobs.is_empty() {
                JobStatus::Succeeded {
                    location: "".to_string(),
                }
            } else {
                JobStatus::Blocked {
                    missing_deps: Default::default(),
                }
            };
            (run_id.clone(), aggregate_status)
        })
        .collect()
}

pub fn build_dependency_graph(lab: &Lab, final_job_id: &JobId) -> Vec<JobId> {
    let mut stack = vec![final_job_id.clone()];
    let mut visited = HashSet::new();
    let mut sorted = Vec::new();

    while let Some(job_id) = stack.pop() {
        if !visited.contains(&job_id) {
            visited.insert(job_id.clone());
            if let Some(job) = lab.jobs.get(&job_id) {
                for dep in get_all_dependencies(job) {
                    stack.push(dep.clone());
                }
            }
            sorted.push(job_id);
        }
    }
    sorted.reverse();
    sorted
}

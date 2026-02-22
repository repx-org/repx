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
    let mut stack = vec![(final_job_id.clone(), false)];
    let mut visited = HashSet::new();
    let mut sorted = Vec::new();

    while let Some((job_id, children_processed)) = stack.pop() {
        if children_processed {
            sorted.push(job_id);
            continue;
        }

        if visited.contains(&job_id) {
            continue;
        }
        visited.insert(job_id.clone());

        stack.push((job_id.clone(), true));

        if let Some(job) = lab.jobs.get(&job_id) {
            for dep in get_all_dependencies(job) {
                if !visited.contains(dep) {
                    stack.push((dep.clone(), false));
                }
            }
        }
    }

    sorted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::JobId;
    use std::collections::HashMap;

    #[test]
    fn test_diamond_dependency_graph_order() {
        let mut lab = Lab {
            repx_version: "0.2.1".to_string(),
            lab_version: "1.0.0".to_string(),
            git_hash: "123".to_string(),
            content_hash: "123".to_string(),
            runs: HashMap::new(),
            jobs: HashMap::new(),
            groups: HashMap::new(),
            host_tools_path: std::path::PathBuf::new(),
            host_tools_dir_name: "tools".to_string(),
            referenced_files: vec![],
        };

        let define_job = |id: &str, inputs: Vec<&str>| {
            let mut job = Job {
                name: Some(id.to_string()),
                params: serde_json::Value::Null,
                path_in_lab: std::path::PathBuf::new(),
                stage_type: crate::model::StageType::Simple,
                executables: HashMap::new(),
            };

            let mut exe = crate::model::Executable {
                path: std::path::PathBuf::from("echo"),
                inputs: vec![],
                outputs: HashMap::new(),
            };

            for inp in inputs {
                exe.inputs.push(crate::model::InputMapping {
                    job_id: Some(JobId(inp.to_string())),
                    source_output: None,
                    target_input: "x".to_string(),
                    source: None,
                    source_key: None,
                    mapping_type: None,
                    dependency_type: None,
                    source_run: None,
                    source_stage_filter: None,
                });
            }
            job.executables.insert("main".to_string(), exe);
            job
        };

        lab.jobs
            .insert(JobId("A".to_string()), define_job("A", vec!["B", "C"]));
        lab.jobs
            .insert(JobId("B".to_string()), define_job("B", vec!["D"]));
        lab.jobs
            .insert(JobId("C".to_string()), define_job("C", vec!["D"]));
        lab.jobs
            .insert(JobId("D".to_string()), define_job("D", vec![]));

        let sorted = build_dependency_graph(&lab, &JobId("A".to_string()));
        println!("Sorted order: {:?}", sorted);

        let pos_d = sorted.iter().position(|j| j.0 == "D").unwrap();
        let pos_b = sorted.iter().position(|j| j.0 == "B").unwrap();

        let pos_c = sorted.iter().position(|j| j.0 == "C").unwrap();
        let pos_a = sorted.iter().position(|j| j.0 == "A").unwrap();

        assert!(
            pos_d < pos_b,
            "D (dependency) should run before B (dependent). Order was: {:?}",
            sorted
        );
        assert!(
            pos_d < pos_c,
            "D (dependency) should run before C (dependent). Order was: {:?}",
            sorted
        );
        assert!(
            pos_b < pos_a,
            "B (dependency) should run before A (dependent). Order was: {:?}",
            sorted
        );
        assert!(
            pos_c < pos_a,
            "C (dependency) should run before A (dependent). Order was: {:?}",
            sorted
        );
    }
}

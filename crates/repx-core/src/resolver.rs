use crate::{
    errors::DomainError,
    model::{Job, JobId, Lab, RunId},
};
use std::collections::HashSet;

pub fn resolve_run_spec(lab: &Lab, spec: &str) -> Result<Vec<RunId>, DomainError> {
    if let Some(group_name) = spec.strip_prefix('@') {
        if group_name.is_empty() {
            return Err(DomainError::EmptyGroupName);
        }
        match lab.groups.get(group_name) {
            Some(run_ids) => Ok(run_ids.clone()),
            None => {
                let available: Vec<String> = {
                    let mut keys: Vec<_> = lab.groups.keys().cloned().collect();
                    keys.sort();
                    keys
                };
                Err(DomainError::UnknownGroup {
                    name: group_name.to_string(),
                    available,
                })
            }
        }
    } else {
        Ok(vec![RunId(spec.to_string())])
    }
}
fn get_all_dependencies(job: &Job) -> impl Iterator<Item = &JobId> {
    job.executables
        .values()
        .flat_map(|exe| exe.inputs.iter())
        .filter_map(|mapping| mapping.job_id.as_ref())
        .collect::<HashSet<_>>()
        .into_iter()
}

pub fn resolve_all_final_job_ids<'a>(
    lab: &'a Lab,
    run_id: &RunId,
) -> Result<Vec<&'a JobId>, DomainError> {
    if let Some(run) = lab.runs.get(run_id) {
        let run_jobs_set: HashSet<_> = run.jobs.iter().collect();
        let mut dep_ids_in_run: HashSet<&JobId> = HashSet::new();

        for job_id in &run.jobs {
            if let Some(job) = lab.jobs.get(job_id) {
                let dependencies = get_all_dependencies(job);
                for dep_id in dependencies {
                    if run_jobs_set.contains(dep_id) {
                        dep_ids_in_run.insert(dep_id);
                    }
                }
            }
        }

        let final_jobs: Vec<&JobId> = run_jobs_set
            .into_iter()
            .filter(|job_id| !dep_ids_in_run.contains(job_id))
            .collect();

        return Ok(final_jobs);
    }

    let candidates: Vec<&JobId> = lab
        .jobs
        .keys()
        .filter(|job_id| job_id.0.starts_with(&run_id.0))
        .collect();

    match candidates.len() {
        0 => Err(DomainError::TargetNotFound(run_id.0.clone())),
        1 => Ok(vec![candidates[0]]),
        _ => Err(DomainError::AmbiguousJobId {
            input: run_id.0.clone(),
            matches: candidates.iter().map(|id| id.to_string()).collect(),
        }),
    }
}

pub fn resolve_target_job_id<'a>(
    lab: &'a Lab,
    user_input: &RunId,
) -> Result<&'a JobId, DomainError> {
    if let Some(run) = lab.runs.get(user_input) {
        let run_jobs_set: HashSet<_> = run.jobs.iter().collect();
        let mut dep_ids_in_run: HashSet<&JobId> = HashSet::new();

        for job_id in &run.jobs {
            if let Some(job) = lab.jobs.get(job_id) {
                let dependencies = get_all_dependencies(job);
                for dep_id in dependencies {
                    if run_jobs_set.contains(dep_id) {
                        dep_ids_in_run.insert(dep_id);
                    }
                }
            }
        }

        let final_jobs: Vec<&JobId> = run_jobs_set
            .into_iter()
            .filter(|job_id| !dep_ids_in_run.contains(job_id))
            .collect();

        match final_jobs.len() {
            1 => return Ok(final_jobs[0]),
            _ => {
                return Err(DomainError::AmbiguousRun(
                    user_input.0.clone(),
                    run.jobs.clone(),
                ));
            }
        }
    }

    let candidates: Vec<&JobId> = lab
        .jobs
        .keys()
        .filter(|job_id| job_id.0.starts_with(&user_input.0))
        .collect();

    match candidates.len() {
        0 => Err(DomainError::TargetNotFound(user_input.0.clone())),
        1 => Ok(candidates[0]),
        _ => Err(DomainError::AmbiguousJobId {
            input: user_input.0.clone(),
            matches: candidates.iter().map(|id| id.to_string()).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Executable, InputMapping, Job, Run};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn job(deps: &[&str]) -> Job {
        let inputs = deps
            .iter()
            .map(|s| InputMapping {
                job_id: Some(JobId(s.to_string())),
                source_output: Some("default".to_string()),
                target_input: "default".to_string(),
                source: None,
                source_key: None,
                mapping_type: None,
                dependency_type: None,
                source_run: None,
                source_stage_filter: None,
            })
            .collect();

        let main_executable = Executable {
            path: PathBuf::from("bin/executable"),
            inputs,
            outputs: HashMap::new(),
            resource_hints: None,
        };

        Job {
            name: None,
            params: serde_json::Value::Null,
            path_in_lab: PathBuf::new(),
            stage_type: crate::model::StageType::Simple,
            executables: HashMap::from([("main".to_string(), main_executable)]),
            resource_hints: None,
        }
    }

    fn test_lab() -> Lab {
        Lab {
            repx_version: "0.2.1".into(),
            lab_version: "1.0.0".into(),
            git_hash: "test".into(),
            content_hash: "test-hash".to_string(),
            runs: HashMap::from([
                (
                    RunId("run-a".into()),
                    Run {
                        image: None,
                        jobs: vec![JobId("job-a1".into()), JobId("job-a2".into())],
                        dependencies: HashMap::new(),
                    },
                ),
                (
                    RunId("run-b-ambiguous".into()),
                    Run {
                        image: None,
                        jobs: vec![JobId("job-b1".into()), JobId("job-b2".into())],
                        dependencies: HashMap::new(),
                    },
                ),
            ]),
            jobs: HashMap::from([
                (JobId("job-a1".into()), job(&[])),
                (JobId("job-a2".into()), job(&["job-a1"])),
                (JobId("job-b1".into()), job(&[])),
                (JobId("job-b2".into()), job(&[])),
                (JobId("12345-unique-name".into()), job(&[])),
                (JobId("multi-abc-1".into()), job(&[])),
                (JobId("multi-def-2".into()), job(&[])),
            ]),
            groups: HashMap::from([
                (
                    "all".to_string(),
                    vec![RunId("run-a".into()), RunId("run-b-ambiguous".into())],
                ),
                ("only-a".to_string(), vec![RunId("run-a".into())]),
                ("empty".to_string(), vec![]),
            ]),
            host_tools_path: PathBuf::from("host-tools"),
            host_tools_dir_name: "host-tools".to_string(),
            referenced_files: Vec::new(),
        }
    }
    #[test]
    fn resolve_direct_run_id_success() {
        let lab = test_lab();
        let input = RunId("run-a".to_string());
        let result = resolve_target_job_id(&lab, &input).unwrap();
        assert_eq!(result, &JobId("job-a2".to_string()));
    }

    #[test]
    fn resolve_ambiguous_run_id() {
        let lab = test_lab();
        let input = RunId("run-b-ambiguous".to_string());
        let result = resolve_target_job_id(&lab, &input);
        assert!(matches!(result, Err(DomainError::AmbiguousRun(_, _))));
    }

    #[test]
    fn resolve_full_job_id_success() {
        let lab = test_lab();
        let input = RunId("12345-unique-name".to_string());
        let result = resolve_target_job_id(&lab, &input).unwrap();
        assert_eq!(result, &JobId("12345-unique-name".to_string()));
    }

    #[test]
    fn resolve_partial_job_id_unique_match() {
        let lab = test_lab();
        let input = RunId("12345".to_string());
        let result = resolve_target_job_id(&lab, &input).unwrap();
        assert_eq!(result, &JobId("12345-unique-name".to_string()));
    }

    #[test]
    fn resolve_partial_job_id_ambiguous() {
        let lab = test_lab();
        let input = RunId("multi".to_string());
        let result = resolve_target_job_id(&lab, &input);
        assert!(matches!(result, Err(DomainError::AmbiguousJobId { .. })));
    }

    #[test]
    fn resolve_target_not_found() {
        let lab = test_lab();
        let input = RunId("does-not-exist".to_string());
        let result = resolve_target_job_id(&lab, &input);
        assert!(matches!(result, Err(DomainError::TargetNotFound(_))));
    }

    #[test]
    fn resolve_run_spec_group_returns_correct_run_ids() {
        let lab = test_lab();
        let result = resolve_run_spec(&lab, "@all").unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&RunId("run-a".into())));
        assert!(result.contains(&RunId("run-b-ambiguous".into())));
    }

    #[test]
    fn resolve_run_spec_single_group() {
        let lab = test_lab();
        let result = resolve_run_spec(&lab, "@only-a").unwrap();
        assert_eq!(result, vec![RunId("run-a".into())]);
    }

    #[test]
    fn resolve_run_spec_empty_group() {
        let lab = test_lab();
        let result = resolve_run_spec(&lab, "@empty").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn resolve_run_spec_unknown_group_returns_error() {
        let lab = test_lab();
        let result = resolve_run_spec(&lab, "@nonexistent");
        assert!(matches!(result, Err(DomainError::UnknownGroup { .. })));
    }

    #[test]
    fn resolve_run_spec_empty_name_after_at() {
        let lab = test_lab();
        let result = resolve_run_spec(&lab, "@");
        assert!(matches!(result, Err(DomainError::EmptyGroupName)));
    }

    #[test]
    fn resolve_run_spec_plain_run_name_falls_through() {
        let lab = test_lab();
        let result = resolve_run_spec(&lab, "run-a").unwrap();
        assert_eq!(result, vec![RunId("run-a".into())]);
    }
}

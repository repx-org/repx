use crate::{
    errors::DomainError,
    model::{JobId, Lab, RunId},
};
use std::collections::HashSet;

pub fn resolve_name_by_prefix<'a, I>(names: I, input: &str) -> Result<&'a str, DomainError>
where
    I: IntoIterator<Item = &'a str>,
{
    let candidates: Vec<&str> = names
        .into_iter()
        .filter(|name| name.starts_with(input))
        .collect();

    match candidates.len() {
        0 => Err(DomainError::GcRootNotFound(input.to_string())),
        1 => Ok(candidates[0]),
        _ => Err(DomainError::AmbiguousGcRoot {
            input: input.to_string(),
            matches: candidates.iter().map(|s| (*s).to_string()).collect(),
        }),
    }
}

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
        Ok(vec![RunId::from(spec)])
    }
}
fn find_final_jobs_in_run<'a>(lab: &'a Lab, run: &'a crate::model::Run) -> Vec<&'a JobId> {
    let run_jobs_set: HashSet<_> = run.jobs.iter().collect();
    let mut dep_ids_in_run: HashSet<&JobId> = HashSet::new();

    for job_id in &run.jobs {
        if let Some(job) = lab.jobs.get(job_id) {
            for dep_id in job.all_dependencies() {
                if run_jobs_set.contains(dep_id) {
                    dep_ids_in_run.insert(dep_id);
                }
            }
        }
    }

    run_jobs_set
        .into_iter()
        .filter(|job_id| !dep_ids_in_run.contains(job_id))
        .collect()
}

fn resolve_job_id_by_prefix<'a>(lab: &'a Lab, input: &str) -> Result<Vec<&'a JobId>, DomainError> {
    let candidates: Vec<&JobId> = lab
        .jobs
        .keys()
        .filter(|job_id| job_id.as_str().starts_with(input))
        .collect();

    match candidates.len() {
        0 => Err(DomainError::TargetNotFound(input.to_string())),
        1 => Ok(vec![candidates[0]]),
        _ => Err(DomainError::AmbiguousJobId {
            input: input.to_string(),
            matches: candidates.iter().map(|id| id.to_string()).collect(),
        }),
    }
}

pub fn resolve_all_final_job_ids<'a>(
    lab: &'a Lab,
    run_id: &RunId,
) -> Result<Vec<&'a JobId>, DomainError> {
    if let Some(run) = lab.runs.get(run_id) {
        return Ok(find_final_jobs_in_run(lab, run));
    }
    resolve_job_id_by_prefix(lab, run_id.as_str())
}

pub fn resolve_target_job_id<'a>(
    lab: &'a Lab,
    user_input: &RunId,
) -> Result<&'a JobId, DomainError> {
    if let Some(run) = lab.runs.get(user_input) {
        let final_jobs = find_final_jobs_in_run(lab, run);
        return match final_jobs.len() {
            1 => Ok(final_jobs[0]),
            _ => Err(DomainError::AmbiguousRun(
                user_input.to_string(),
                run.jobs.clone(),
            )),
        };
    }

    let candidates = resolve_job_id_by_prefix(lab, user_input.as_str())?;
    Ok(candidates[0])
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
                job_id: Some(JobId::from(*s)),
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
            deps: vec![],
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
                    RunId::from("run-a"),
                    Run {
                        image: None,
                        jobs: vec![JobId::from("job-a1"), JobId::from("job-a2")],
                        dependencies: HashMap::new(),
                    },
                ),
                (
                    RunId::from("run-b-ambiguous"),
                    Run {
                        image: None,
                        jobs: vec![JobId::from("job-b1"), JobId::from("job-b2")],
                        dependencies: HashMap::new(),
                    },
                ),
            ]),
            jobs: HashMap::from([
                (JobId::from("job-a1"), job(&[])),
                (JobId::from("job-a2"), job(&["job-a1"])),
                (JobId::from("job-b1"), job(&[])),
                (JobId::from("job-b2"), job(&[])),
                (JobId::from("12345-unique-name"), job(&[])),
                (JobId::from("multi-abc-1"), job(&[])),
                (JobId::from("multi-def-2"), job(&[])),
            ]),
            groups: HashMap::from([
                (
                    "all".to_string(),
                    vec![RunId::from("run-a"), RunId::from("run-b-ambiguous")],
                ),
                ("only-a".to_string(), vec![RunId::from("run-a")]),
                ("empty".to_string(), vec![]),
            ]),
            host_tools_path: PathBuf::from("host-tools"),
            host_tools_dir_name: "host-tools".to_string(),
            referenced_files: Vec::new(),
            tar_dir_name: None,
        }
    }
    #[test]
    fn resolve_direct_run_id_success() {
        let lab = test_lab();
        let input = RunId::from("run-a");
        let result = resolve_target_job_id(&lab, &input).expect("direct run ID should resolve");
        assert_eq!(result, &JobId::from("job-a2"));
    }

    #[test]
    fn resolve_ambiguous_run_id() {
        let lab = test_lab();
        let input = RunId::from("run-b-ambiguous");
        let result = resolve_target_job_id(&lab, &input);
        assert!(matches!(result, Err(DomainError::AmbiguousRun(_, _))));
    }

    #[test]
    fn resolve_full_job_id_success() {
        let lab = test_lab();
        let input = RunId::from("12345-unique-name");
        let result = resolve_target_job_id(&lab, &input).expect("full job ID should resolve");
        assert_eq!(result, &JobId::from("12345-unique-name"));
    }

    #[test]
    fn resolve_partial_job_id_unique_match() {
        let lab = test_lab();
        let input = RunId::from("12345");
        let result =
            resolve_target_job_id(&lab, &input).expect("partial job ID should resolve uniquely");
        assert_eq!(result, &JobId::from("12345-unique-name"));
    }

    #[test]
    fn resolve_partial_job_id_ambiguous() {
        let lab = test_lab();
        let input = RunId::from("multi");
        let result = resolve_target_job_id(&lab, &input);
        assert!(matches!(result, Err(DomainError::AmbiguousJobId { .. })));
    }

    #[test]
    fn resolve_target_not_found() {
        let lab = test_lab();
        let input = RunId::from("does-not-exist");
        let result = resolve_target_job_id(&lab, &input);
        assert!(matches!(result, Err(DomainError::TargetNotFound(_))));
    }

    #[test]
    fn resolve_run_spec_group_returns_correct_run_ids() {
        let lab = test_lab();
        let result = resolve_run_spec(&lab, "@all").expect("@all group should resolve");
        assert_eq!(result.len(), 2);
        assert!(result.contains(&RunId::from("run-a")));
        assert!(result.contains(&RunId::from("run-b-ambiguous")));
    }

    #[test]
    fn resolve_run_spec_single_group() {
        let lab = test_lab();
        let result = resolve_run_spec(&lab, "@only-a").expect("@only-a group should resolve");
        assert_eq!(result, vec![RunId::from("run-a")]);
    }

    #[test]
    fn resolve_run_spec_empty_group() {
        let lab = test_lab();
        let result = resolve_run_spec(&lab, "@empty").expect("@empty group should resolve");
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
        let result = resolve_run_spec(&lab, "run-a").expect("plain run name should resolve");
        assert_eq!(result, vec![RunId::from("run-a")]);
    }

    #[test]
    fn resolve_name_by_prefix_exact_match() {
        let names = vec!["foo-bar-123", "baz-qux-456", "quux-789"];
        let result = resolve_name_by_prefix(names, "foo-bar-123").expect("exact match should work");
        assert_eq!(result, "foo-bar-123");
    }

    #[test]
    fn resolve_name_by_prefix_unique_prefix() {
        let names = vec!["foo-bar-123", "baz-qux-456", "quux-789"];
        let result = resolve_name_by_prefix(names, "foo").expect("unique prefix should resolve");
        assert_eq!(result, "foo-bar-123");
    }

    #[test]
    fn resolve_name_by_prefix_short_unique_prefix() {
        let names = vec!["abc123def", "xyz789ghi"];
        let result = resolve_name_by_prefix(names, "a").expect("short unique prefix should work");
        assert_eq!(result, "abc123def");
    }

    #[test]
    fn resolve_name_by_prefix_ambiguous() {
        let names = vec!["prefix-abc", "prefix-def", "other-123"];
        let result = resolve_name_by_prefix(names, "prefix");
        assert!(matches!(result, Err(DomainError::AmbiguousGcRoot { .. })));
        if let Err(DomainError::AmbiguousGcRoot { input, matches }) = result {
            assert_eq!(input, "prefix");
            assert_eq!(matches.len(), 2);
            assert!(matches.contains(&"prefix-abc".to_string()));
            assert!(matches.contains(&"prefix-def".to_string()));
        }
    }

    #[test]
    fn resolve_name_by_prefix_not_found() {
        let names = vec!["foo-bar", "baz-qux"];
        let result = resolve_name_by_prefix(names, "nonexistent");
        assert!(matches!(result, Err(DomainError::GcRootNotFound(_))));
    }

    #[test]
    fn resolve_name_by_prefix_empty_list() {
        let names: Vec<&str> = vec![];
        let result = resolve_name_by_prefix(names, "anything");
        assert!(matches!(result, Err(DomainError::GcRootNotFound(_))));
    }
}

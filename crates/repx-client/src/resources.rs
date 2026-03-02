use repx_core::{
    config::{ResourceRule, Resources},
    model::{JobId, ResourceHints},
};
use wildmatch::WildMatch;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SbatchDirectives {
    pub partition: Option<String>,
    pub cpus_per_task: Option<u32>,
    pub mem: Option<String>,
    pub time: Option<String>,
    pub sbatch_opts: Vec<String>,
}

impl SbatchDirectives {
    pub fn to_shell_string(&self) -> String {
        let mut opts = Vec::new();
        if let Some(p) = &self.partition {
            opts.push(format!("--partition={}", p));
        }
        if let Some(c) = self.cpus_per_task {
            opts.push(format!("--cpus-per-task={}", c));
        }
        if let Some(m) = &self.mem {
            opts.push(format!("--mem={}", m));
        }
        if let Some(t) = &self.time {
            opts.push(format!("--time={}", t));
        }
        opts.extend(self.sbatch_opts.clone());
        opts.join(" ")
    }

    pub fn to_args(&self) -> Vec<String> {
        let mut opts = Vec::new();
        if let Some(p) = &self.partition {
            opts.push(format!("--partition={}", p));
        }
        if let Some(c) = self.cpus_per_task {
            opts.push(format!("--cpus-per-task={}", c));
        }
        if let Some(m) = &self.mem {
            opts.push(format!("--mem={}", m));
        }
        if let Some(t) = &self.time {
            opts.push(format!("--time={}", t));
        }
        opts.extend(self.sbatch_opts.clone());
        opts
    }
}

fn merge_hints(current: &mut SbatchDirectives, hints: &ResourceHints) {
    if let Some(val) = &hints.partition {
        current.partition = Some(val.clone());
    }
    if let Some(val) = hints.cpus {
        current.cpus_per_task = Some(val);
    }
    if let Some(val) = &hints.mem {
        current.mem = Some(val.clone());
    }
    if let Some(val) = &hints.time {
        current.time = Some(val.clone());
    }
    if !hints.sbatch_opts.is_empty() {
        current.sbatch_opts = hints.sbatch_opts.clone();
    }
}

pub fn resolve_for_job(
    job_id: &JobId,
    target_name: &str,
    resources: &Option<Resources>,
    hints: Option<&ResourceHints>,
) -> SbatchDirectives {
    let mut current = match resources {
        Some(r) => SbatchDirectives {
            partition: r.defaults.partition.clone(),
            cpus_per_task: r.defaults.cpus_per_task,
            mem: r.defaults.mem.clone(),
            time: r.defaults.time.clone(),
            sbatch_opts: r.defaults.sbatch_opts.clone(),
        },
        None => SbatchDirectives::default(),
    };

    if let Some(h) = hints {
        tracing::debug!("Applying Nix resource hints for job '{}': {:?}", job_id, h);
        merge_hints(&mut current, h);
    }

    if let Some(r) = resources {
        for rule in &r.rules {
            let target_matches = rule.target.as_deref().is_none_or(|t| t == target_name);
            let glob_matches = rule
                .job_id_glob
                .as_ref()
                .is_none_or(|glob| WildMatch::new(glob).matches(&job_id.0));
            if target_matches && glob_matches {
                merge_rule(&mut current, rule);
            }
        }
    }

    tracing::debug!(
        "Resolved sbatch directives for job '{}' on target '{}': {:?}",
        job_id,
        target_name,
        current
    );
    current
}

pub fn resolve_worker_resources(
    orchestrator_job_id: &JobId,
    target_name: &str,
    resources: &Option<Resources>,
    orchestrator_hints: Option<&ResourceHints>,
    worker_hints: Option<&ResourceHints>,
) -> SbatchDirectives {
    let mut worker_directives = resolve_for_job(
        orchestrator_job_id,
        target_name,
        resources,
        orchestrator_hints,
    );

    if let Some(h) = worker_hints {
        tracing::debug!(
            "Applying Nix worker resource hints for job '{}': {:?}",
            orchestrator_job_id,
            h
        );
        merge_hints(&mut worker_directives, h);
    }

    if let Some(r) = resources {
        let final_rule = r.rules.iter().rev().find(|rule| {
            let target_matches = rule.target.as_deref().is_none_or(|t| t == target_name);
            let glob_matches = rule
                .job_id_glob
                .as_ref()
                .is_none_or(|glob| WildMatch::new(glob).matches(&orchestrator_job_id.0));
            target_matches && glob_matches
        });
        if let Some(rule) = final_rule {
            if let Some(worker_rule) = &rule.worker_resources {
                tracing::debug!(
                    "Applying resources.toml worker_resources override for job '{}'",
                    orchestrator_job_id
                );
                merge_rule(&mut worker_directives, worker_rule);
            } else {
                tracing::debug!(
                    "No resources.toml worker_resources override for job '{}'. Workers inherit parent's resources.",
                    orchestrator_job_id
                );
            }
        }
    }

    worker_directives
}

fn merge_rule(current: &mut SbatchDirectives, rule: &ResourceRule) {
    if let Some(val) = &rule.partition {
        current.partition = Some(val.clone());
    }
    if let Some(val) = rule.cpus_per_task {
        current.cpus_per_task = Some(val);
    }
    if let Some(val) = &rule.mem {
        current.mem = Some(val.clone());
    }
    if let Some(val) = &rule.time {
        current.time = Some(val.clone());
    }
    if !rule.sbatch_opts.is_empty() {
        current.sbatch_opts = rule.sbatch_opts.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use repx_core::config::Resources;

    fn get_test_resources() -> Resources {
        toml::from_str(
            r#"
[defaults]
partition = "default"
cpus-per-task = 1
mem = "1G"

[[rules]]
job_id_glob = "*-heavy-*"
mem = "128G"
cpus-per-task = 16

[[rules]]
job_id_glob = "*-gpu-*"
target = "gpu-cluster"
partition = "gpu"
sbatch_opts = ["--gres=gpu:1"]

[[rules]]
job_id_glob = "*-scatter-job"
mem = "500M" # Orchestrator is lightweight
[rules.worker_resources]
mem = "16G" # Workers are heavy
cpus-per-task = 4
"#,
        )
        .expect("test resource config must parse")
    }

    #[test]
    fn test_default_resources() {
        let res = get_test_resources();
        let job_id = JobId("some-random-job".into());
        let directives = resolve_for_job(&job_id, "any-cluster", &Some(res), None);
        assert_eq!(directives.partition, Some("default".into()));
        assert_eq!(directives.cpus_per_task, Some(1));
        assert_eq!(directives.mem, Some("1G".into()));
        assert!(directives.time.is_none());
        assert!(directives.sbatch_opts.is_empty());
    }

    #[test]
    fn test_glob_match_override() {
        let res = get_test_resources();
        let job_id = JobId("my-heavy-job-123".into());
        let directives = resolve_for_job(&job_id, "any-cluster", &Some(res), None);
        assert_eq!(directives.mem, Some("128G".into()));
        assert_eq!(directives.cpus_per_task, Some(16));
        assert_eq!(directives.partition, Some("default".into()));
    }

    #[test]
    fn test_target_and_glob_match() {
        let res = get_test_resources();
        let job_id = JobId("needs-a-gpu-job".into());
        let directives = resolve_for_job(&job_id, "gpu-cluster", &Some(res), None);
        assert_eq!(directives.partition, Some("gpu".into()));
        assert_eq!(directives.sbatch_opts, vec!["--gres=gpu:1"]);
        assert_eq!(directives.mem, Some("1G".into()));
    }

    #[test]
    fn test_target_mismatch() {
        let res = get_test_resources();
        let job_id = JobId("needs-a-gpu-job".into());
        let directives = resolve_for_job(&job_id, "cpu-cluster", &Some(res), None);
        assert_eq!(directives.partition, Some("default".into()));
        assert!(directives.sbatch_opts.is_empty());
    }

    #[test]
    fn test_scatter_orchestrator_resources() {
        let res = get_test_resources();
        let job_id = JobId("my-scatter-job".into());
        let directives = resolve_for_job(&job_id, "any-cluster", &Some(res), None);
        assert_eq!(directives.mem, Some("500M".into()));
    }

    #[test]
    fn test_scatter_worker_resources() {
        let res = get_test_resources();
        let job_id = JobId("my-scatter-job".into());
        let directives = resolve_worker_resources(&job_id, "any-cluster", &Some(res), None, None);
        assert_eq!(directives.mem, Some("16G".into()));
        assert_eq!(directives.cpus_per_task, Some(4));
        assert_eq!(directives.partition, Some("default".into()));
    }

    #[test]
    fn test_scatter_worker_inherits_parent_if_no_override() {
        let res = get_test_resources();
        let job_id = JobId("my-heavy-job-123".into());
        let parent_directives = resolve_for_job(&job_id, "any-cluster", &Some(res.clone()), None);
        let worker_directives =
            resolve_worker_resources(&job_id, "any-cluster", &Some(res), None, None);
        assert_eq!(worker_directives.mem, parent_directives.mem);
        assert_eq!(
            worker_directives.cpus_per_task,
            parent_directives.cpus_per_task
        );
        assert_eq!(worker_directives.partition, parent_directives.partition);
    }

    #[test]
    fn test_nix_hints_applied() {
        let res = get_test_resources();
        let job_id = JobId("some-random-job".into());
        let hints = ResourceHints {
            mem: Some("32G".into()),
            cpus: Some(8),
            time: Some("04:00:00".into()),
            partition: None,
            sbatch_opts: vec![],
        };
        let directives = resolve_for_job(&job_id, "any-cluster", &Some(res), Some(&hints));
        assert_eq!(directives.mem, Some("32G".into()));
        assert_eq!(directives.cpus_per_task, Some(8));
        assert_eq!(directives.time, Some("04:00:00".into()));
        assert_eq!(directives.partition, Some("default".into()));
    }

    #[test]
    fn test_rules_override_hints() {
        let res = get_test_resources();
        let job_id = JobId("my-heavy-job-123".into());
        let hints = ResourceHints {
            mem: Some("8G".into()),
            cpus: Some(2),
            time: None,
            partition: None,
            sbatch_opts: vec![],
        };
        let directives = resolve_for_job(&job_id, "any-cluster", &Some(res), Some(&hints));
        assert_eq!(directives.mem, Some("128G".into()));
        assert_eq!(directives.cpus_per_task, Some(16));
    }

    #[test]
    fn test_worker_hints_applied() {
        let res = get_test_resources();
        let job_id = JobId("some-job".into());
        let orchestrator_hints = ResourceHints {
            mem: Some("1G".into()),
            cpus: Some(1),
            time: None,
            partition: None,
            sbatch_opts: vec![],
        };
        let worker_hints = ResourceHints {
            mem: Some("64G".into()),
            cpus: Some(4),
            time: Some("08:00:00".into()),
            partition: None,
            sbatch_opts: vec![],
        };
        let directives = resolve_worker_resources(
            &job_id,
            "any-cluster",
            &Some(res),
            Some(&orchestrator_hints),
            Some(&worker_hints),
        );
        assert_eq!(directives.mem, Some("64G".into()));
        assert_eq!(directives.cpus_per_task, Some(4));
        assert_eq!(directives.time, Some("08:00:00".into()));
    }
}

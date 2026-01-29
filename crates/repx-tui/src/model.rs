use serde::Deserialize;
use serde::Serialize;
use std::{collections::HashMap, str::FromStr};

#[derive(Clone, Debug, Default)]
pub struct StatusCounts {
    pub succeeded: usize,
    pub failed: usize,
    pub running: usize,
    pub pending: usize,
    pub queued: usize,
    pub blocked: usize,
    pub submitting: usize,
    pub unknown: usize,
    pub total: usize,
}

#[derive(Clone, Debug, PartialEq, Copy, Eq, Hash)]
pub enum TuiScheduler {
    Local,
    Slurm,
}
impl TuiScheduler {
    pub fn as_str(&self) -> &'static str {
        match self {
            TuiScheduler::Local => "local",
            TuiScheduler::Slurm => "slurm",
        }
    }
}
impl FromStr for TuiScheduler {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "local" => Ok(TuiScheduler::Local),
            "slurm" => Ok(TuiScheduler::Slurm),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Copy, Eq, Hash)]
pub enum TuiExecutor {
    Native,
    Podman,
    Docker,
    Bwrap,
}
impl TuiExecutor {
    pub fn as_str(&self) -> &'static str {
        match self {
            TuiExecutor::Native => "native",
            TuiExecutor::Podman => "podman",
            TuiExecutor::Docker => "docker",
            TuiExecutor::Bwrap => "bwrap",
        }
    }
}
impl FromStr for TuiExecutor {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "native" => Ok(TuiExecutor::Native),
            "podman" => Ok(TuiExecutor::Podman),
            "docker" => Ok(TuiExecutor::Docker),
            "bwrap" => Ok(TuiExecutor::Bwrap),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TuiJob {
    pub full_id: repx_core::model::JobId,
    pub id: String,
    pub name: String,
    pub run: String,
    pub params: serde_json::Value,
    pub status: String,
    pub context_depends_on: String,
    pub context_dependents: String,
    pub logs: Vec<String>,
}
#[derive(Clone, Debug)]
pub enum TuiRowItem {
    Run { id: repx_core::model::RunId },
    Job { job: Box<TuiJob> },
}
#[derive(Clone, Debug)]
pub struct TuiDisplayRow {
    pub item: TuiRowItem,
    pub id: String,
    pub depth: usize,
    #[allow(dead_code)]
    pub parent_prefix: String,
    pub is_last_child: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub enum TargetState {
    Active,
    Inactive,
    #[allow(dead_code)]
    Down,
}

pub struct TuiTarget {
    pub name: String,
    pub state: TargetState,
    pub available_schedulers: Vec<TuiScheduler>,
    pub available_executors: HashMap<TuiScheduler, Vec<TuiExecutor>>,
    pub selected_scheduler_idx: usize,
    pub selected_executor_idx: usize,
}

impl TuiTarget {
    pub fn get_selected_scheduler(&self) -> TuiScheduler {
        *self
            .available_schedulers
            .get(self.selected_scheduler_idx)
            .unwrap_or(&TuiScheduler::Local)
    }

    pub fn get_selected_executor(&self) -> TuiExecutor {
        let scheduler = self.get_selected_scheduler();
        self.available_executors
            .get(&scheduler)
            .and_then(|execs| execs.get(self.selected_executor_idx))
            .copied()
            .unwrap_or(TuiExecutor::Native)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tui_scheduler_from_str_local() {
        assert_eq!("local".parse::<TuiScheduler>(), Ok(TuiScheduler::Local));
    }

    #[test]
    fn test_tui_scheduler_from_str_slurm() {
        assert_eq!("slurm".parse::<TuiScheduler>(), Ok(TuiScheduler::Slurm));
    }

    #[test]
    fn test_tui_scheduler_from_str_invalid() {
        assert!("kubernetes".parse::<TuiScheduler>().is_err());
        assert!("SLURM".parse::<TuiScheduler>().is_err());
        assert!("".parse::<TuiScheduler>().is_err());
    }

    #[test]
    fn test_tui_scheduler_as_str_local() {
        assert_eq!(TuiScheduler::Local.as_str(), "local");
    }

    #[test]
    fn test_tui_scheduler_as_str_slurm() {
        assert_eq!(TuiScheduler::Slurm.as_str(), "slurm");
    }

    #[test]
    fn test_tui_scheduler_roundtrip() {
        for scheduler in [TuiScheduler::Local, TuiScheduler::Slurm] {
            let s = scheduler.as_str();
            let parsed: TuiScheduler = s.parse().unwrap();
            assert_eq!(scheduler, parsed);
        }
    }

    #[test]
    fn test_tui_executor_from_str_all_variants() {
        assert_eq!("native".parse::<TuiExecutor>(), Ok(TuiExecutor::Native));
        assert_eq!("podman".parse::<TuiExecutor>(), Ok(TuiExecutor::Podman));
        assert_eq!("docker".parse::<TuiExecutor>(), Ok(TuiExecutor::Docker));
        assert_eq!("bwrap".parse::<TuiExecutor>(), Ok(TuiExecutor::Bwrap));
    }

    #[test]
    fn test_tui_executor_from_str_invalid() {
        assert!("containerd".parse::<TuiExecutor>().is_err());
        assert!("Docker".parse::<TuiExecutor>().is_err());
        assert!("NATIVE".parse::<TuiExecutor>().is_err());
    }

    #[test]
    fn test_tui_executor_as_str() {
        assert_eq!(TuiExecutor::Native.as_str(), "native");
        assert_eq!(TuiExecutor::Podman.as_str(), "podman");
        assert_eq!(TuiExecutor::Docker.as_str(), "docker");
        assert_eq!(TuiExecutor::Bwrap.as_str(), "bwrap");
    }

    #[test]
    fn test_tui_executor_roundtrip() {
        for executor in [
            TuiExecutor::Native,
            TuiExecutor::Podman,
            TuiExecutor::Docker,
            TuiExecutor::Bwrap,
        ] {
            let s = executor.as_str();
            let parsed: TuiExecutor = s.parse().unwrap();
            assert_eq!(executor, parsed);
        }
    }

    fn create_test_target() -> TuiTarget {
        let mut available_executors = HashMap::new();
        available_executors.insert(
            TuiScheduler::Local,
            vec![TuiExecutor::Native, TuiExecutor::Bwrap],
        );
        available_executors.insert(
            TuiScheduler::Slurm,
            vec![TuiExecutor::Docker, TuiExecutor::Podman],
        );

        TuiTarget {
            name: "test-target".to_string(),
            state: TargetState::Active,
            available_schedulers: vec![TuiScheduler::Local, TuiScheduler::Slurm],
            available_executors,
            selected_scheduler_idx: 0,
            selected_executor_idx: 0,
        }
    }

    #[test]
    fn test_tui_target_get_selected_scheduler_first() {
        let target = create_test_target();
        assert_eq!(target.get_selected_scheduler(), TuiScheduler::Local);
    }

    #[test]
    fn test_tui_target_get_selected_scheduler_second() {
        let mut target = create_test_target();
        target.selected_scheduler_idx = 1;
        assert_eq!(target.get_selected_scheduler(), TuiScheduler::Slurm);
    }

    #[test]
    fn test_tui_target_get_selected_scheduler_out_of_bounds() {
        let mut target = create_test_target();
        target.selected_scheduler_idx = 999;
        assert_eq!(target.get_selected_scheduler(), TuiScheduler::Local);
    }

    #[test]
    fn test_tui_target_get_selected_executor_first() {
        let target = create_test_target();
        assert_eq!(target.get_selected_executor(), TuiExecutor::Native);
    }

    #[test]
    fn test_tui_target_get_selected_executor_second() {
        let mut target = create_test_target();
        target.selected_executor_idx = 1;
        assert_eq!(target.get_selected_executor(), TuiExecutor::Bwrap);
    }

    #[test]
    fn test_tui_target_get_selected_executor_for_slurm() {
        let mut target = create_test_target();
        target.selected_scheduler_idx = 1;
        target.selected_executor_idx = 0;
        assert_eq!(target.get_selected_executor(), TuiExecutor::Docker);
    }

    #[test]
    fn test_tui_target_get_selected_executor_out_of_bounds() {
        let mut target = create_test_target();
        target.selected_executor_idx = 999;
        assert_eq!(target.get_selected_executor(), TuiExecutor::Native);
    }

    #[test]
    fn test_tui_target_empty_schedulers() {
        let target = TuiTarget {
            name: "empty".to_string(),
            state: TargetState::Inactive,
            available_schedulers: vec![],
            available_executors: HashMap::new(),
            selected_scheduler_idx: 0,
            selected_executor_idx: 0,
        };
        assert_eq!(target.get_selected_scheduler(), TuiScheduler::Local);
        assert_eq!(target.get_selected_executor(), TuiExecutor::Native);
    }

    #[test]
    fn test_tui_job_serialize_deserialize() {
        let job = TuiJob {
            full_id: repx_core::model::JobId("run-test/stage-a/job-123".to_string()),
            id: "job-123".to_string(),
            name: "Test Job".to_string(),
            run: "run-test".to_string(),
            params: serde_json::json!({"key": "value", "count": 42}),
            status: "pending".to_string(),
            context_depends_on: "job-122".to_string(),
            context_dependents: "job-124, job-125".to_string(),
            logs: vec!["log line 1".to_string(), "log line 2".to_string()],
        };

        let serialized = serde_json::to_string(&job).unwrap();
        let deserialized: TuiJob = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.id, "job-123");
        assert_eq!(deserialized.name, "Test Job");
        assert_eq!(deserialized.logs.len(), 2);
    }
}

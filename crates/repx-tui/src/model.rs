use serde::Deserialize;
use serde::Serialize;
use std::{collections::HashMap, str::FromStr};

#[derive(Clone, Debug, PartialEq, Copy, Eq, Hash)]
pub enum TuiScheduler {
    Local,
    Slurm,
}

impl TuiScheduler {
    pub fn to_str(&self) -> &'static str {
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
    pub fn to_str(&self) -> &'static str {
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
    pub worker: String,
    pub elapsed: String,
    pub status: String,
    pub context_depends_on: String,
    pub context_dependents: String,
    pub logs: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum TuiRowItem {
    Run { id: repx_core::model::RunId },
    Job { job: TuiJob },
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
    Down,
}

pub struct TuiTarget {
    pub name: String,
    pub state: TargetState,
    pub activity: Vec<f64>,
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

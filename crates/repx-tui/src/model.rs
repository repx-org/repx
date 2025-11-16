use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Copy)]
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

#[derive(Clone, Debug, PartialEq, Copy)]
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
    pub scheduler: TuiScheduler,
    pub executor: TuiExecutor,
}

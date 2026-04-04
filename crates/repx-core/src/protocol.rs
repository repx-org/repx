use serde::{Deserialize, Serialize};

pub const WAVE_BOUNDARY: &str = "__WAVE_BOUNDARY__";

pub const STREAM_END: &str = "__END__";

pub const WAVE_DONE: &str = "__WAVE_DONE__";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamJob {
    pub id: String,

    #[serde(rename = "type")]
    pub job_type: StreamJobType,

    pub script: String,

    pub deps: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamJobType {
    Simple,
    ScatterGather,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamJobResult {
    pub id: String,

    pub slurm_id: u32,
}

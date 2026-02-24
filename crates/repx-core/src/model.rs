use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum StageType {
    #[default]
    Simple,
    ScatterGather,
    Worker,
    Gather,
}

impl fmt::Display for StageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StageType::Simple => write!(f, "simple"),
            StageType::ScatterGather => write!(f, "scatter-gather"),
            StageType::Worker => write!(f, "worker"),
            StageType::Gather => write!(f, "gather"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseStageTypeError(pub String);

impl fmt::Display for ParseStageTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid stage type: '{}'. Valid values are: simple, scatter-gather, worker, gather",
            self.0
        )
    }
}

impl std::error::Error for ParseStageTypeError {}

impl FromStr for StageType {
    type Err = ParseStageTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "simple" => Ok(StageType::Simple),
            "scatter-gather" => Ok(StageType::ScatterGather),
            "worker" => Ok(StageType::Worker),
            "gather" => Ok(StageType::Gather),
            _ => Err(ParseStageTypeError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SchedulerType {
    #[default]
    Local,
    Slurm,
}

impl fmt::Display for SchedulerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchedulerType::Local => write!(f, "local"),
            SchedulerType::Slurm => write!(f, "slurm"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSchedulerTypeError(pub String);

impl fmt::Display for ParseSchedulerTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid scheduler type: '{}'. Valid values are: local, slurm",
            self.0
        )
    }
}

impl std::error::Error for ParseSchedulerTypeError {}

impl FromStr for SchedulerType {
    type Err = ParseSchedulerTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "local" => Ok(SchedulerType::Local),
            "slurm" => Ok(SchedulerType::Slurm),
            _ => Err(ParseSchedulerTypeError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
pub struct JobId(pub String);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl JobId {
    pub fn short_id(&self) -> String {
        let s = &self.0;
        if let Some((hash, rest)) = s.split_once('-') {
            if hash.len() >= 7 {
                let short_hash = &hash[..7];
                format!("{}-{}", short_hash, rest)
            } else {
                s.to_string()
            }
        } else {
            s.to_string()
        }
    }
}

impl From<String> for JobId {
    fn from(s: String) -> Self {
        JobId(s)
    }
}

impl FromStr for JobId {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(JobId(s.to_string()))
    }
}

#[derive(Debug)]
pub struct ParseRunIdError(String);

impl fmt::Display for ParseRunIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseRunIdError {}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
pub struct RunId(pub String);

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for RunId {
    type Err = ParseRunIdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "missing" | "pending" => Err(ParseRunIdError(format!(
                "invalid run ID '{}': this is a reserved keyword. Use it as a positional argument without the --run flag.", s
            ))),
            _ => Ok(RunId(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct InputMapping {
    pub job_id: Option<JobId>,
    pub source_output: Option<String>,
    pub target_input: String,

    pub source: Option<String>,
    pub source_key: Option<String>,

    #[serde(rename = "type")]
    pub mapping_type: Option<String>,
    pub dependency_type: Option<String>,
    pub source_run: Option<RunId>,
    pub source_stage_filter: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceHints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sbatch_opts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Executable {
    pub path: PathBuf,
    #[serde(default)]
    pub inputs: Vec<InputMapping>,
    #[serde(default)]
    pub outputs: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_hints: Option<ResourceHints>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub name: Option<String>,
    pub params: serde_json::Value,
    #[serde(skip)]
    pub path_in_lab: PathBuf,
    #[serde(rename = "stage_type", default)]
    pub stage_type: StageType,
    #[serde(default)]
    pub executables: HashMap<String, Executable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_hints: Option<ResourceHints>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub image: Option<PathBuf>,
    pub jobs: Vec<JobId>,
    #[serde(default)]
    pub dependencies: HashMap<RunId, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lab {
    pub repx_version: String,
    pub lab_version: String,
    #[serde(rename = "gitHash")]
    pub git_hash: String,
    #[serde(default, skip_serializing)]
    pub content_hash: String,
    pub runs: HashMap<RunId, Run>,
    pub jobs: HashMap<JobId, Job>,
    #[serde(default)]
    pub groups: HashMap<String, Vec<RunId>>,
    #[serde(skip)]
    pub host_tools_path: PathBuf,
    #[serde(skip)]
    pub host_tools_dir_name: String,
    #[serde(skip)]
    pub referenced_files: Vec<PathBuf>,
}

impl Lab {
    pub fn is_native(&self) -> bool {
        self.runs.values().all(|run| run.image.is_none())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RootMetadata {
    pub runs: Vec<String>,
    #[serde(rename = "gitHash")]
    pub git_hash: String,
    pub repx_version: String,
    #[serde(default)]
    pub groups: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RunMetadataForLoading {
    pub name: RunId,
    pub image: Option<PathBuf>,
    #[serde(default)]
    pub dependencies: HashMap<RunId, String>,
    pub jobs: HashMap<JobId, Job>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct FileEntry {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LabManifest {
    #[serde(rename = "labId")]
    pub lab_id: String,
    pub lab_version: String,
    pub metadata: String,
    pub files: Vec<FileEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runid_from_str_ok() {
        assert_eq!(
            RunId::from_str("my-experiment-run").unwrap(),
            RunId("my-experiment-run".to_string())
        );
    }

    #[test]
    fn test_runid_from_str_err_missing() {
        assert!(RunId::from_str("missing").is_err());
    }

    #[test]
    fn test_runid_from_str_err_pending() {
        assert!(RunId::from_str("pending").is_err());
    }

    #[test]
    fn test_root_metadata_deserialize_with_groups() {
        let json = r#"{
            "repx_version": "0.2.1",
            "type": "root",
            "gitHash": "abc123",
            "runs": ["revision/meta-run-a.json"],
            "groups": {
                "foldability": ["run-a", "run-b"],
                "spec": ["run-c"]
            }
        }"#;
        let meta: RootMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.groups.len(), 2);
        assert_eq!(meta.groups["foldability"], vec!["run-a", "run-b"]);
        assert_eq!(meta.groups["spec"], vec!["run-c"]);
    }

    #[test]
    fn test_root_metadata_deserialize_without_groups() {
        let json = r#"{
            "repx_version": "0.2.1",
            "type": "root",
            "gitHash": "abc123",
            "runs": ["revision/meta-run-a.json"]
        }"#;
        let meta: RootMetadata = serde_json::from_str(json).unwrap();
        assert!(meta.groups.is_empty());
    }
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Executable {
    pub path: PathBuf,
    #[serde(default)]
    pub inputs: Vec<InputMapping>,
    #[serde(default)]
    pub outputs: HashMap<String, serde_json::Value>,
}

fn default_stage_type() -> String {
    "simple".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub name: Option<String>,
    pub params: serde_json::Value,
    #[serde(skip)]
    pub path_in_lab: PathBuf,
    #[serde(rename = "stage_type", default = "default_stage_type")]
    pub stage_type: String,
    #[serde(default)]
    pub executables: HashMap<String, Executable>,
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
    pub schema_version: String,
    #[serde(rename = "gitHash")]
    pub git_hash: String,
    #[serde(default, skip_serializing)]
    pub content_hash: String,
    pub runs: HashMap<RunId, Run>,
    pub jobs: HashMap<JobId, Job>,
    #[serde(skip)]
    pub host_tools_path: PathBuf,
    #[serde(skip)]
    pub host_tools_dir_name: String,
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
    pub schema_version: String,
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
pub(crate) struct LabManifest {
    #[serde(rename = "labId")]
    pub lab_id: String,
    pub metadata: String,
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
}

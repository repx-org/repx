use crate::model::JobId;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug)]
pub struct PathIoError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

impl std::fmt::Display for PathIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "I/O error on path '{}': {}",
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for PathIoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug)]
pub struct JsonPathError {
    pub path: PathBuf,
    pub source: serde_json::Error,
}

impl std::fmt::Display for JsonPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Failed to parse JSON in '{}': {}",
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for JsonPathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug)]
pub struct TomlPathError {
    pub path: PathBuf,
    pub source: toml::de::Error,
}

impl std::fmt::Display for TomlPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Failed to parse TOML in '{}': {}",
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for TomlPathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

impl CoreError {
    pub fn path_io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        CoreError::PathIo(Box::new(PathIoError {
            path: path.into(),
            source,
        }))
    }

    pub fn json_path(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        CoreError::JsonPath(Box::new(JsonPathError {
            path: path.into(),
            source,
        }))
    }

    pub fn toml_path(path: impl Into<PathBuf>, source: toml::de::Error) -> Self {
        CoreError::TomlPath(Box::new(TomlPathError {
            path: path.into(),
            source,
        }))
    }
}

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    PathIo(Box<PathIoError>),

    #[error("Failed to parse metadata file: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    JsonPath(Box<JsonPathError>),

    #[error("Failed to parse TOML configuration: {0}")]
    Toml(#[from] toml::de::Error),

    #[error(transparent)]
    TomlPath(Box<TomlPathError>),

    #[error("Failed to serialize TOML configuration: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("Error walking directory: {0}")]
    WalkDir(#[from] walkdir::Error),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Invalid configuration: {detail}")]
    InvalidConfig { detail: String },

    #[error("A 'local' target must be defined in config.toml.\nTip: You can define a 'data-only' local target by setting a base_path:\n\n[targets.local]\nbase_path = \"~/.local/share/repx\"")]
    MissingLocalTarget,

    #[error("Target '{name}' not found in configuration.")]
    TargetNotConfigured { name: String },

    #[error("No submission target configured. Set 'submission_target' in your config or use the --target flag.")]
    NoSubmissionTarget,

    #[error("Container execution with '{runtime}' requires an --image-tag.")]
    ImageTagRequired { runtime: String },

    #[error("Unsupported {kind}: '{value}'.")]
    UnsupportedValue { kind: String, value: String },

    #[error("Missing required argument '{argument}': {context}")]
    MissingArgument { argument: String, context: String },

    #[error("No result store is configured. Please add one to your config file or use the --stores flag.")]
    StoreNotConfigured,

    #[error("Command failed: {0}")]
    CommandFailed(String),

    #[error("Target setup failed: {0}")]
    TargetSetupFailed(String),

    #[error("Lab not found at path '{0}'.\nPlease specify a valid lab directory with --lab, or run this command in a directory containing the default lab path ('./result').")]
    LabNotFound(PathBuf),

    #[error("Could not find required lab metadata file(s) in '{0}'. Expected 'lab_manifest.json' and 'revision/metadata.json'. Is this a valid lab directory?")]
    MetadataNotFound(PathBuf),

    #[error("Job '{job_id}' missing required executable '{executable}'.")]
    MissingExecutable { job_id: String, executable: String },

    #[error("Inconsistent metadata: {detail}")]
    InconsistentMetadata { detail: String },

    #[error("No lab manifest found for hash '{hash}'.")]
    ManifestNotFound { hash: String },

    #[error("Output not ready at '{path}'. Job may not have been executed yet.")]
    OutputNotReady { path: PathBuf },

    #[error("Cycle detected in {context}.")]
    CycleDetected { context: String },

    #[error("Step error: {detail}")]
    StepError { detail: String },

    #[error("Lab integrity check failed: {0}")]
    IntegrityError(String),

    #[error(
        "Lab integrity check failed: file '{path}' has hash '{actual}', expected '{expected}'."
    )]
    IntegrityHashMismatch {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("Lab integrity check failed: file '{0}' is missing.")]
    IntegrityFileMissing(String),

    #[error("Path traversal rejected: '{path}' escapes the expected base directory.")]
    PathTraversal { path: String },

    #[error("Symlink '{link}' points outside the lab root (target: '{target}').")]
    SymlinkEscape { link: PathBuf, target: PathBuf },

    #[error("No pinned GC root named '{name}'.")]
    GcRootNotFound { name: String },

    #[error("Host tool error: {detail}")]
    HostToolNotFound { detail: String },

    #[error("Cache error for key '{key}': {detail}")]
    CacheError { key: String, detail: String },
}

#[derive(Error, Debug)]
pub enum DomainError {
    #[error("Input '{0}' did not match any known run or job.")]
    TargetNotFound(String),

    #[error("Job '{0}' not found in the lab definition.")]
    JobNotFound(JobId),

    #[error("Run ID '{0}' is ambiguous. It has multiple final jobs: {1:?}. Please specify a more precise job ID to run.")]
    AmbiguousRun(String, Vec<JobId>),

    #[error("Ambiguous input '{input}'. It matches multiple jobs:\n  - {}", matches.join("\n  - "))]
    AmbiguousJobId { input: String, matches: Vec<String> },

    #[error("Ambiguous GC root '{input}'. It matches multiple roots:\n  - {}", matches.join("\n  - "))]
    AmbiguousGcRoot { input: String, matches: Vec<String> },

    #[error("No GC root found matching '{0}'.")]
    GcRootNotFound(String),

    #[error("Invalid output path for job '{job_id}'. Output '{output_name}' path '{path}' must start with '$out/'.")]
    InvalidOutputPath {
        job_id: JobId,
        output_name: String,
        path: String,
    },

    #[error("Could not find executable for job '{0}'. Expected exactly one file in the job's 'bin' directory.")]
    ExecutableNotFound(JobId),

    #[error("The lab is native-only (contains no container images), but container execution was requested. Please run with the --native flag.")]
    NativeLabContainerExecution,

    #[error("Invalid execution target format: {0}. Expected 'local' or 'ssh:user@host'.")]
    InvalidTarget(String),

    #[error("Unknown group '{name}'.\nAvailable groups: {}", available.join(", "))]
    UnknownGroup {
        name: String,
        available: Vec<String>,
    },

    #[error("Empty group name after '@'.")]
    EmptyGroupName,
}

use crate::model::JobId;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("I/O error on path '{path}': {source}")]
    PathIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse metadata file: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Failed to parse TOML configuration: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Failed to serialize TOML configuration: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("Error walking directory: {0}")]
    WalkDir(#[from] walkdir::Error),

    #[error("XDG Base Directory Error: {0}")]
    Xdg(#[from] xdg::BaseDirectoriesError),

    #[error("Invalid configuration: {0}")]
    General(String),

    #[error("No result store is configured. Please add one to your config file or use the --stores flag.")]
    StoreNotConfigured,

    #[error("Lab not found at path '{0}'.\nPlease specify a valid lab directory with --lab, or run this command in a directory containing the default lab path ('./result').")]
    LabNotFound(PathBuf),

    #[error("Could not find required lab metadata file(s) in '{0}'. Expected 'lab_manifest.json' and 'revision/metadata.json'. Is this a valid lab directory?")]
    MetadataNotFound(PathBuf),

    #[error("Could not determine HOME directory.")]
    HomeDirectoryNotFound,

    #[error("Lab integrity check failed: {0}")]
    IntegrityError(String),

    #[error("Incompatible Lab version. This repx binary expects repx_version '{expected}', but the Lab was generated with version '{found}'. Please rebuild your Lab with a compatible repx-nix version.")]
    IncompatibleVersion { expected: String, found: String },

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

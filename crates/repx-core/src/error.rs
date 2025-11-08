use crate::model::JobId;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("I/O error on path '{path}': {source}")]
    PathIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse metadata.json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Failed to parse TOML configuration: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Failed to serialize TOML configuration: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("Error walking directory: {0}")]
    WalkDir(#[from] walkdir::Error),

    #[error("XDG Base Directory Error: {0}")]
    Xdg(#[from] xdg::BaseDirectoriesError),

    #[error("Failed to launch required command: '{command_name}'\n\n[Reason]\n{source}\n\n[Suggestion]\nIf the error is 'No such file or directory', ensure '{command_name}' is installed and in your system's PATH.\nFor local execution, all repx components (repx, repx-scheduler, repx-worker) must be accessible.\nFor remote execution, ensure 'ssh' and 'scp' are installed.")]
    ProcessLaunchFailed {
        command_name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Input '{0}' did not match any known run or job.")]
    TargetNotFound(String),

    #[error("Job '{0}' not found in the lab definition.")]
    JobNotFound(JobId),

    #[error("Invalid execution target format: {0}. Expected 'local' or 'ssh:user@host'.")]
    InvalidTarget(String),

    #[error("Run ID '{0}' is ambiguous. It has multiple final jobs: {1:?}. Please specify a more precise job ID to run.")]
    AmbiguousRun(String, Vec<JobId>),

    #[error("Execution failed: {message}\n{log_summary}")]
    ExecutionFailed {
        message: String,
        log_path: Option<PathBuf>,
        log_summary: String,
    },

    #[error("Orchestrator script failed on target: {stderr}")]
    OrchestratorFailed { stderr: String },

    #[error("Ambiguous input '{input}'. It matches multiple jobs:\n  - {}", matches.join("\n  - "))]
    AmbiguousJobId { input: String, matches: Vec<String> },

    #[error("Operation aborted by user.")]
    UserAborted,

    #[error("Invalid configuration: {0}")]
    ConfigurationError(String),

    #[error("Could not determine HOME directory to create a default store.")]
    HomeDirectoryNotFound,

    #[error("No result store is configured. Please add one to your config file or use the --stores flag.")]
    StoreNotConfigured,

    #[error("Invalid output path for job '{job_id}'. Output '{output_name}' path '{path}' must start with '$out/'.")]
    InvalidOutputPath {
        job_id: JobId,
        output_name: String,
        path: String,
    },

    #[error("Lab not found at path '{0}'.\nPlease specify a valid lab directory with --lab, or run this command in a directory containing the default lab path ('./result').")]
    LabNotFound(PathBuf),

    #[error("Could not find 'metadata.json' in '{0}' or its 'revision' subdirectory. Is this a valid lab directory?")]
    MetadataNotFound(PathBuf),

    #[error("Container runtime not found. Please install 'docker' or 'podman' and ensure it's in your PATH, or specify a runtime in your config file.")]
    ContainerRuntimeNotFound,

    #[error("The lab is native-only (contains no container images), but container execution was requested. Please run with the --native flag.")]
    NativeLabContainerExecution,

    #[error(
        "Could not access job package directory for job '{job_id}' at path '{path}': {source}"
    )]
    JobPackageIoError {
        job_id: JobId,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Could not find executable for job '{0}'. Expected exactly one file in the job's 'bin' directory.")]
    ExecutableNotFound(JobId),
}

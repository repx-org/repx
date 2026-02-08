use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClientError {
    #[error(transparent)]
    Config(#[from] repx_core::errors::ConfigError),

    #[error(transparent)]
    Domain(#[from] repx_core::errors::DomainError),

    #[error(transparent)]
    WalkDir(#[from] walkdir::Error),

    #[error("Failed to execute command on target '{target}': {source}")]
    TargetCommandFailed {
        target: String,
        source: repx_core::errors::ConfigError,
    },

    #[error("Could not find target '{0}' in configuration.")]
    TargetNotFound(String),

    #[error("No submission target configured. Please set 'submission_target' in your config or use the --target flag.")]
    NoSubmissionTarget,

    #[error("Failed to parse SLURM job ID from output: {0}")]
    SlurmIdParse(String),

    #[error("Job '{0}' is not currently managed by SLURM on target '{1}'.")]
    JobNotTracked(repx_core::model::JobId, String),

    #[error("Invalid path '{path}': {reason}")]
    InvalidPath {
        path: std::path::PathBuf,
        reason: String,
    },
}

pub type Result<T> = std::result::Result<T, ClientError>;

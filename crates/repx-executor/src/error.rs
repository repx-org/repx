use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExecutorError {
    #[error(transparent)]
    Config(#[from] repx_core::errors::ConfigError),

    #[error(transparent)]
    Domain(#[from] repx_core::errors::DomainError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to execute command '{command}': {source}")]
    CommandFailed {
        command: String,
        source: std::io::Error,
    },

    #[error("Execution of '{script}' failed with exit code {code}.\n--- STDERR ---\n{stderr}")]
    ScriptFailed {
        script: String,
        code: i32,
        stderr: String,
    },

    #[error("Container execution requires an image tag, but none was provided.")]
    ImageTagMissing,

    #[error(
        "Security violation: Attempted to use system binary '{0}' which is not in the allowlist."
    )]
    SecurityViolation(String),
}

pub type Result<T> = std::result::Result<T, ExecutorError>;

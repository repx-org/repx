use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExecutorError {
    #[error(transparent)]
    Config(#[from] repx_core::errors::CoreError),

    #[error(transparent)]
    Domain(#[from] repx_core::errors::DomainError),

    #[error("I/O error during {operation} on '{path}': {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },

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

    #[error("Image not found: {0}")]
    ImageNotFound(String),

    #[error("Invalid image: {0}")]
    InvalidImage(String),

    #[error("Lock acquisition failed: {0}")]
    LockFailed(String),

    #[error("Execution cancelled for job '{job_id}'.")]
    Cancelled { job_id: String },
}

pub type Result<T> = std::result::Result<T, ExecutorError>;

pub trait IoContext<T> {
    fn io_ctx(self, operation: &'static str, path: &Path) -> Result<T>;
}

impl<T> IoContext<T> for std::result::Result<T, std::io::Error> {
    fn io_ctx(self, operation: &'static str, path: &Path) -> Result<T> {
        self.map_err(|source| ExecutorError::Io {
            operation,
            path: path.to_path_buf(),
            source,
        })
    }
}

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
    #[error(transparent)]
    Config(#[from] repx_core::errors::CoreError),

    #[error(transparent)]
    Domain(#[from] repx_core::errors::DomainError),

    #[error(transparent)]
    Client(#[from] repx_client::error::ClientError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("Execution failed: {message}\nSummary: {log_summary}")]
    ExecutionFailed {
        message: String,
        log_path: Option<std::path::PathBuf>,
        log_summary: String,
    },
}

pub type Result<T> = std::result::Result<T, CliError>;

impl CliError {
    pub fn execution_failed(message: impl Into<String>, summary: impl Into<String>) -> Self {
        Self::ExecutionFailed {
            message: message.into(),
            log_path: None,
            log_summary: summary.into(),
        }
    }
}

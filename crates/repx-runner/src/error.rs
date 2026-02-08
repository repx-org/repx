use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
    #[error(transparent)]
    Config(#[from] repx_core::errors::ConfigError),

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

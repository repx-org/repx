use crate::error::CliError;
use repx_client::Client;
use repx_core::{errors::CoreError, lab::LabSource, model::ExecutionType};
use repx_executor::{ImageTag, Runtime};
use std::path::Path;

pub mod execute;
pub mod gc;
pub mod internal;
pub mod list;
pub mod log;
pub mod run;
pub mod scatter_gather;
pub mod show;
pub mod trace;

pub(crate) fn create_tokio_runtime() -> Result<tokio::runtime::Runtime, CliError> {
    tokio::runtime::Runtime::new().map_err(|e| {
        CliError::Config(CoreError::InvalidConfig {
            detail: format!("Failed to create async runtime: {}", e),
        })
    })
}

pub(crate) fn write_marker(path: &Path) -> std::io::Result<()> {
    let f = std::fs::File::create(path)?;
    f.sync_all()?;
    Ok(())
}

pub(crate) fn parse_runtime(
    execution_type: ExecutionType,
    image_tag: Option<String>,
) -> Result<Runtime, CliError> {
    let parse_tag = |runtime_name: &str, raw: Option<String>| -> Result<ImageTag, CliError> {
        let tag_str = raw.ok_or_else(|| {
            CliError::Config(CoreError::ImageTagRequired {
                runtime: runtime_name.to_string(),
            })
        })?;
        ImageTag::parse(tag_str).map_err(|e| {
            CliError::Config(CoreError::InvalidConfig {
                detail: format!("Invalid image tag for {}: {}", runtime_name, e),
            })
        })
    };

    match execution_type {
        ExecutionType::Native => Ok(Runtime::Native),
        ExecutionType::Podman => Ok(Runtime::Podman {
            image_tag: parse_tag("podman", image_tag)?,
        }),
        ExecutionType::Docker => Ok(Runtime::Docker {
            image_tag: parse_tag("docker", image_tag)?,
        }),
        ExecutionType::Bwrap => Ok(Runtime::Bwrap {
            image_tag: parse_tag("bwrap", image_tag)?,
        }),
    }
}

pub struct AppContext<'a> {
    pub source: &'a LabSource,
    pub client: &'a Client,
    pub submission_target: &'a str,
}

pub(crate) fn resolve_to_local_artifacts(
    path: &Path,
    base_path: &Path,
    local_artifacts_path: &Option<std::path::PathBuf>,
) -> std::path::PathBuf {
    if let Some(local) = local_artifacts_path {
        let artifacts_base = base_path.join("artifacts");
        if let Ok(suffix) = path.strip_prefix(&artifacts_base) {
            let local_path = local.join(suffix);
            if local_path.exists() {
                tracing::debug!(
                    "Resolved to local: {} -> {}",
                    path.display(),
                    local_path.display()
                );
                return local_path;
            }
        }
    }
    path.to_path_buf()
}

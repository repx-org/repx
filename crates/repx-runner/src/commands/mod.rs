use crate::error::CliError;
use repx_client::Client;
use repx_core::{errors::ConfigError, model::ExecutionType};
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
        CliError::Config(ConfigError::InvalidConfig {
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
            CliError::Config(ConfigError::ImageTagRequired {
                runtime: runtime_name.to_string(),
            })
        })?;
        ImageTag::parse(tag_str).map_err(|e| {
            CliError::Config(ConfigError::InvalidConfig {
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
    pub lab_path: &'a Path,
    pub client: &'a Client,
    pub submission_target: &'a str,
}

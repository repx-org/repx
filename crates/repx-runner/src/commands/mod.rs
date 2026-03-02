use crate::error::CliError;
use repx_client::Client;
use repx_core::errors::ConfigError;
use repx_executor::Runtime;
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
        CliError::Config(ConfigError::General(format!(
            "Failed to create async runtime: {}",
            e
        )))
    })
}

pub(crate) fn write_marker(path: &Path) -> std::io::Result<()> {
    let f = std::fs::File::create(path)?;
    f.sync_all()?;
    Ok(())
}

pub(crate) fn parse_runtime(
    runtime_str: &str,
    image_tag: Option<String>,
) -> Result<Runtime, CliError> {
    match runtime_str {
        "native" => Ok(Runtime::Native),
        "podman" => Ok(Runtime::Podman {
            image_tag: image_tag.ok_or_else(|| {
                CliError::Config(ConfigError::General(
                    "Container execution with 'podman' requires an --image-tag.".to_string(),
                ))
            })?,
        }),
        "docker" => Ok(Runtime::Docker {
            image_tag: image_tag.ok_or_else(|| {
                CliError::Config(ConfigError::General(
                    "Container execution with 'docker' requires an --image-tag.".to_string(),
                ))
            })?,
        }),
        "bwrap" => Ok(Runtime::Bwrap {
            image_tag: image_tag.ok_or_else(|| {
                CliError::Config(ConfigError::General(
                    "Container execution with 'bwrap' requires an --image-tag.".to_string(),
                ))
            })?,
        }),
        other => Err(CliError::Config(ConfigError::General(format!(
            "Unsupported runtime: {}",
            other
        )))),
    }
}

pub struct AppContext<'a> {
    pub lab_path: &'a Path,
    pub client: &'a Client,
    pub submission_target: &'a str,
}

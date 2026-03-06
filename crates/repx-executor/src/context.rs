use crate::error::{ExecutorError, Result};
use crate::util::ALLOWED_SYSTEM_BINARIES;
use crate::ExecutionRequest;
use std::collections::HashSet;
use std::path::PathBuf;
use tokio::process::Command as TokioCommand;

pub struct RuntimeContext<'a> {
    pub request: &'a ExecutionRequest,
}

impl<'a> RuntimeContext<'a> {
    pub fn new(request: &'a ExecutionRequest) -> Self {
        Self { request }
    }

    pub async fn find_system_binary_dir(&self, binary_name: &str) -> Option<PathBuf> {
        if let Some(path_var) = std::env::var_os("PATH") {
            for path in std::env::split_paths(&path_var) {
                let candidate = path.join(binary_name);
                if tokio::fs::metadata(&candidate).await.is_ok() {
                    return Some(path);
                }
            }
        }
        None
    }

    pub async fn get_host_tool_path(&self, tool_name: &str) -> Result<PathBuf> {
        let host_tools = self.request.host_tools_bin_dir.as_ref().ok_or_else(|| {
            ExecutorError::Config(repx_core::errors::CoreError::HostToolNotFound {
                detail: format!(
                    "Host tools directory not configured. Cannot resolve '{}'.",
                    tool_name
                ),
            })
        })?;

        let tool_path = host_tools.join(tool_name);
        if tokio::fs::metadata(&tool_path).await.is_ok() {
            return Ok(tool_path);
        }

        Err(ExecutorError::Config(
            repx_core::errors::CoreError::HostToolNotFound {
                detail: format!(
                    "Required host tool '{}' not found in host-tools bin directory ({:?}).",
                    tool_name, host_tools
                ),
            },
        ))
    }

    pub async fn resolve_tool(&self, tool_name: &str) -> Result<PathBuf> {
        if let Ok(path) = self.get_host_tool_path(tool_name).await {
            return Ok(path);
        }

        if ALLOWED_SYSTEM_BINARIES.contains(&tool_name) {
            if let Some(dir) = self.find_system_binary_dir(tool_name).await {
                let path = dir.join(tool_name);
                if tokio::fs::metadata(&path).await.is_ok() {
                    return Ok(path);
                }
            }
        }

        Err(ExecutorError::Config(
            repx_core::errors::CoreError::HostToolNotFound {
                detail: format!(
                    "Tool '{}' not found in host-tools or allowed system binaries.",
                    tool_name
                ),
            },
        ))
    }

    pub async fn find_image_file(&self, image_tag: &str) -> Option<PathBuf> {
        let images_dir = self.request.base_path.join("images");
        if tokio::fs::metadata(&images_dir).await.is_ok() {
            let candidates = vec![
                images_dir.join(image_tag),
                images_dir.join(format!("{}.gz", image_tag)),
                images_dir.join(format!("{}.tar", image_tag)),
                images_dir.join(format!("{}.tar.gz", image_tag)),
            ];
            for candidate in candidates {
                if tokio::fs::metadata(&candidate).await.is_ok() {
                    return Some(candidate);
                }
            }
        }

        let artifacts = self.request.base_path.join("artifacts");
        let subdirs = ["images", "image"];

        for subdir in subdirs {
            let dir = artifacts.join(subdir);
            if tokio::fs::metadata(&dir).await.is_err() {
                continue;
            }

            let candidates = vec![
                dir.join(image_tag),
                dir.join(format!("{}.gz", image_tag)),
                dir.join(format!("{}.tar", image_tag)),
                dir.join(format!("{}.tar.gz", image_tag)),
            ];

            for candidate in candidates {
                if tokio::fs::metadata(&candidate).await.is_ok() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    pub async fn get_temp_path(&self) -> PathBuf {
        let temp_root = if let Some(local) = &self.request.node_local_path {
            local.join("repx").join("temp")
        } else {
            self.request.base_path.join("repx").join("temp")
        };

        let _ = tokio::fs::create_dir_all(&temp_root).await;
        temp_root
    }

    pub fn get_images_cache_dir(&self) -> PathBuf {
        if let Some(local) = &self.request.node_local_path {
            local.join("repx").join("cache").join("images")
        } else {
            self.request.base_path.join("cache").join("images")
        }
    }

    pub fn get_capabilities_cache_dir(&self) -> PathBuf {
        if let Some(local) = &self.request.node_local_path {
            local.join("repx").join("cache").join("capabilities")
        } else {
            self.request.base_path.join("cache").join("capabilities")
        }
    }

    pub async fn calculate_restricted_path(
        &self,
        required_system_binaries: &[&str],
    ) -> std::ffi::OsString {
        let mut new_paths = Vec::new();

        if let Some(host_tools) = &self.request.host_tools_bin_dir {
            new_paths.push(host_tools.clone());
        }

        if !required_system_binaries.is_empty() {
            let mut added_dirs = HashSet::new();
            for &binary in required_system_binaries {
                if ALLOWED_SYSTEM_BINARIES.contains(&binary) {
                    if let Some(dir) = self.find_system_binary_dir(binary).await {
                        if added_dirs.insert(dir.clone()) {
                            new_paths.push(dir);
                        }
                    } else {
                        tracing::debug!(
                            "Warning: Allowed system tool '{}' not found in system PATH.",
                            binary
                        );
                    }
                } else {
                    tracing::info!(
                        "[SECURITY] Blocked attempt to allowlist system binary '{}'. It is not in the allowed list.",
                        binary
                    );
                }
            }
        }

        std::env::join_paths(new_paths).unwrap_or_else(|_| std::ffi::OsString::from(""))
    }

    pub async fn restrict_command_environment(
        &self,
        cmd: &mut TokioCommand,
        required_system_binaries: &[&str],
    ) {
        let path = self
            .calculate_restricted_path(required_system_binaries)
            .await;
        cmd.env("PATH", path);
    }
}

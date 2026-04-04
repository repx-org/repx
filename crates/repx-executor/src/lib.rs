mod context;
mod error;
mod runtime;
mod util;

pub use context::RuntimeContext;
pub use error::{ExecutorError, IoContext, Result};
pub use runtime::{BwrapRuntime, ContainerRuntime, NativeRuntime, Runtime};
pub use util::{extract_image_hash, is_binary_allowed, ImageTag, ALLOWED_SYSTEM_BINARIES};

use repx_core::{
    constants::logs,
    model::{JobId, MountPolicy},
};
use std::path::{Path, PathBuf};
use tokio::fs::{File, OpenOptions};
use tokio::process::Command as TokioCommand;
pub use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    pub job_id: JobId,
    pub runtime: Runtime,
    pub base_path: PathBuf,
    pub node_local_path: Option<PathBuf>,
    pub local_artifacts_path: Option<PathBuf>,
    pub job_package_path: PathBuf,
    pub inputs_json_path: PathBuf,
    pub user_out_dir: PathBuf,
    pub repx_out_dir: PathBuf,
    pub host_tools_bin_dir: Option<PathBuf>,
    pub mount_policy: MountPolicy,
    pub inputs_data: Option<Vec<u8>>,
    pub parameters_data: Option<Vec<u8>>,
}

pub struct Executor {
    pub request: ExecutionRequest,
    local_log_dir: Option<PathBuf>,
}

impl Executor {
    pub fn new(request: ExecutionRequest) -> Self {
        Self {
            request,
            local_log_dir: None,
        }
    }

    fn context(&self) -> RuntimeContext<'_> {
        RuntimeContext::new(&self.request)
    }

    pub async fn execute_script(
        &mut self,
        script_path: &Path,
        args: &[String],
        cancel: &CancellationToken,
    ) -> Result<()> {
        if cancel.is_cancelled() {
            return Err(ExecutorError::Cancelled {
                job_id: self.request.job_id.to_string(),
            });
        }

        let (stdout_log, stderr_log) = self.create_log_files().await?;

        let stderr_path = if let Some(ref local_dir) = self.local_log_dir {
            local_dir.join(logs::STDERR)
        } else {
            self.request.repx_out_dir.join(logs::STDERR)
        };

        let mut cmd = self.build_command_for_script(script_path, args).await?;

        tracing::info!(
            "Executing command for job '{}': {:?}",
            self.request.job_id,
            cmd
        );

        let mut child = cmd
            .stdout(stdout_log.into_std().await)
            .stderr(stderr_log.into_std().await)
            .spawn()
            .map_err(|e| ExecutorError::CommandFailed {
                command: format!("{:?}", cmd.as_std().get_program()),
                source: e,
            })?;

        let status = tokio::select! {
            result = child.wait() => {
                result.map_err(|e| ExecutorError::CommandFailed {
                    command: format!("{:?}", cmd.as_std().get_program()),
                    source: e,
                })?
            }
            _ = cancel.cancelled() => {
                tracing::warn!(
                    "Cancellation requested for job '{}', killing child process...",
                    self.request.job_id,
                );
                let _ = child.kill().await;
                let _ = self.sync_logs_to_nfs().await;
                return Err(ExecutorError::Cancelled {
                    job_id: self.request.job_id.to_string(),
                });
            }
        };

        self.sync_logs_to_nfs().await?;

        if !status.success() {
            let nfs_stderr = self.request.repx_out_dir.join(logs::STDERR);
            let stderr_content = tokio::fs::read_to_string(&nfs_stderr)
                .await
                .or_else(|_| std::fs::read_to_string(&stderr_path))
                .unwrap_or_else(|e| format!("<failed to read stderr.log: {}>", e));
            return Err(ExecutorError::ScriptFailed {
                script: script_path.display().to_string(),
                code: status.code().unwrap_or(1),
                stderr: stderr_content,
            });
        }
        Ok(())
    }

    pub async fn build_command_for_script(
        &self,
        script_path: &Path,
        args: &[String],
    ) -> Result<TokioCommand> {
        let ctx = self.context();
        let resolved_script = ctx.resolve_to_local(script_path).await;
        let script_path = resolved_script.as_path();

        let cmd = match &self.request.runtime {
            Runtime::Native => NativeRuntime::build_command(&self.request, script_path, args)?,
            Runtime::Podman { .. } | Runtime::Docker { .. } => {
                ContainerRuntime::ensure_image_loaded(&ctx, &self.request.runtime).await?;
                ContainerRuntime::build_command(&ctx, &self.request.runtime, script_path, args)
                    .await?
            }
            Runtime::Bwrap { image_tag } => {
                let rootfs_path =
                    BwrapRuntime::ensure_rootfs_extracted(&ctx, image_tag.as_str()).await?;
                BwrapRuntime::build_command(&ctx, &rootfs_path, script_path, args).await?
            }
        };
        Ok(cmd)
    }

    async fn create_log_files(&mut self) -> Result<(File, File)> {
        let log_dir = if let Ok(tmpdir) = std::env::var("TMPDIR") {
            let local_dir = PathBuf::from(tmpdir).join("repx-logs");
            if tokio::fs::create_dir_all(&local_dir).await.is_ok() {
                self.local_log_dir = Some(local_dir.clone());
                local_dir
            } else {
                self.request.repx_out_dir.clone()
            }
        } else {
            self.request.repx_out_dir.clone()
        };

        let stdout_path = log_dir.join(logs::STDOUT);
        let stderr_path = log_dir.join(logs::STDERR);

        let stdout_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stdout_path)
            .await
            .io_ctx("open (create/append)", &stdout_path)?;
        let stderr_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stderr_path)
            .await
            .io_ctx("open (create/append)", &stderr_path)?;
        Ok((stdout_file, stderr_file))
    }

    pub async fn sync_logs_to_nfs(&self) -> Result<()> {
        if let Some(ref local_dir) = self.local_log_dir {
            let local_stdout = local_dir.join(logs::STDOUT);
            let local_stderr = local_dir.join(logs::STDERR);
            let nfs_stdout = self.request.repx_out_dir.join(logs::STDOUT);
            let nfs_stderr = self.request.repx_out_dir.join(logs::STDERR);

            tokio::fs::create_dir_all(&self.request.repx_out_dir)
                .await
                .io_ctx("create_dir_all", &self.request.repx_out_dir)?;

            if local_stdout.exists() {
                tokio::fs::copy(&local_stdout, &nfs_stdout)
                    .await
                    .io_ctx("copy stdout.log to NFS", &nfs_stdout)?;
            }
            if local_stderr.exists() {
                tokio::fs::copy(&local_stderr, &nfs_stderr)
                    .await
                    .io_ctx("copy stderr.log to NFS", &nfs_stderr)?;
            }
        }
        Ok(())
    }

    pub fn build_native_command(
        &self,
        script_path: &Path,
        args: &[String],
    ) -> Result<TokioCommand> {
        NativeRuntime::build_command(&self.request, script_path, args)
    }

    pub async fn find_image_file(&self, image_tag: &str) -> Option<PathBuf> {
        self.context().find_image_file(image_tag).await
    }

    pub async fn get_host_tool_path(&self, tool_name: &str) -> Result<PathBuf> {
        self.context().get_host_tool_path(tool_name).await
    }

    pub async fn calculate_restricted_path(
        &self,
        required_system_binaries: &[&str],
    ) -> std::ffi::OsString {
        self.context()
            .calculate_restricted_path(required_system_binaries)
            .await
    }

    pub async fn ensure_bwrap_rootfs_extracted(&self, image_tag: &str) -> Result<PathBuf> {
        BwrapRuntime::ensure_rootfs_extracted(&self.context(), image_tag).await
    }
}

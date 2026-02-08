mod context;
mod error;
mod runtime;
mod util;

pub use context::RuntimeContext;
pub use error::{ExecutorError, Result};
pub use runtime::{BwrapRuntime, ContainerRuntime, NativeRuntime, Runtime};
pub use util::{
    allowed_system_binaries, extract_image_hash, is_binary_allowed, ALLOWED_SYSTEM_BINARIES,
};

use repx_core::{constants::logs, model::JobId};
use std::path::{Path, PathBuf};
use tokio::fs::{File, OpenOptions};
use tokio::process::Command as TokioCommand;

#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    pub job_id: JobId,
    pub runtime: Runtime,
    pub base_path: PathBuf,
    pub node_local_path: Option<PathBuf>,
    pub job_package_path: PathBuf,
    pub inputs_json_path: PathBuf,
    pub user_out_dir: PathBuf,
    pub repx_out_dir: PathBuf,
    pub host_tools_bin_dir: Option<PathBuf>,
    pub mount_host_paths: bool,
    pub mount_paths: Vec<String>,
}

pub struct Executor {
    pub request: ExecutionRequest,
}

impl Executor {
    pub fn new(request: ExecutionRequest) -> Self {
        Self { request }
    }

    fn context(&self) -> RuntimeContext<'_> {
        RuntimeContext::new(&self.request)
    }

    pub async fn execute_script(&self, script_path: &Path, args: &[String]) -> Result<()> {
        let (stdout_log, stderr_log) = self.create_log_files().await?;
        let stderr_path = self.request.repx_out_dir.join(logs::STDERR);

        let mut cmd = self.build_command_for_script(script_path, args).await?;

        tracing::info!(
            "Executing command for job '{}': {:?}",
            self.request.job_id,
            cmd
        );

        let status = cmd
            .stdout(stdout_log.into_std().await)
            .stderr(stderr_log.into_std().await)
            .status()
            .await
            .map_err(|e| ExecutorError::CommandFailed {
                command: format!("{:?}", cmd.as_std().get_program()),
                source: e,
            })?;

        if !status.success() {
            let stderr_content = tokio::fs::read_to_string(&stderr_path)
                .await
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

        let cmd = match &self.request.runtime {
            Runtime::Native => NativeRuntime::build_command(&self.request, script_path, args),
            Runtime::Podman { .. } | Runtime::Docker { .. } => {
                ContainerRuntime::ensure_image_loaded(&ctx, &self.request.runtime).await?;
                ContainerRuntime::build_command(&ctx, &self.request.runtime, script_path, args)
                    .await?
            }
            Runtime::Bwrap { image_tag } => {
                let rootfs_path = BwrapRuntime::ensure_rootfs_extracted(&ctx, image_tag).await?;
                BwrapRuntime::build_command(&ctx, &rootfs_path, script_path, args).await?
            }
        };
        Ok(cmd)
    }

    async fn create_log_files(&self) -> Result<(File, File)> {
        let stdout_path = self.request.repx_out_dir.join(logs::STDOUT);
        let stderr_path = self.request.repx_out_dir.join(logs::STDERR);

        let stdout_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stdout_path)
            .await?;
        let stderr_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stderr_path)
            .await?;
        Ok((stdout_file, stderr_file))
    }

    pub fn build_native_command(&self, script_path: &Path, args: &[String]) -> TokioCommand {
        NativeRuntime::build_command(&self.request, script_path, args)
    }

    pub fn find_image_file(&self, image_tag: &str) -> Option<PathBuf> {
        self.context().find_image_file(image_tag)
    }

    pub fn get_host_tool_path(&self, tool_name: &str) -> Result<PathBuf> {
        self.context().get_host_tool_path(tool_name)
    }

    pub fn calculate_restricted_path(
        &self,
        required_system_binaries: &[&str],
    ) -> std::ffi::OsString {
        self.context()
            .calculate_restricted_path(required_system_binaries)
    }

    pub async fn ensure_bwrap_rootfs_extracted(&self, image_tag: &str) -> Result<PathBuf> {
        BwrapRuntime::ensure_rootfs_extracted(&self.context(), image_tag).await
    }
}

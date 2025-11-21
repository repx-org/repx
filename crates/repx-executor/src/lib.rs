use nix::fcntl::{Flock, FlockArg};
use repx_core::{log_debug, log_info, model::JobId};
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use tokio::fs::{File, OpenOptions};
use tokio::process::Command as TokioCommand;

#[derive(Error, Debug)]
pub enum ExecutorError {
    #[error(transparent)]
    Core(#[from] repx_core::error::AppError),

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
}

pub type Result<T> = std::result::Result<T, ExecutorError>;

#[derive(Debug, Clone)]
pub enum Runtime {
    Native,
    Podman { image_tag: String },
    Docker { image_tag: String },
    Bwrap,
}

#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    pub job_id: JobId,
    pub runtime: Runtime,
    pub base_path: PathBuf,
    pub job_package_path: PathBuf,
    pub inputs_json_path: PathBuf,
    pub user_out_dir: PathBuf,
    pub repx_out_dir: PathBuf,
}

pub struct Executor {
    request: ExecutionRequest,
}

impl Executor {
    pub fn new(request: ExecutionRequest) -> Self {
        Self { request }
    }

    async fn create_log_files(&self) -> Result<(File, File)> {
        let stdout_path = self.request.repx_out_dir.join("stdout.log");
        let stderr_path = self.request.repx_out_dir.join("stderr.log");

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

    pub async fn execute_script(&self, script_path: &Path, args: &[String]) -> Result<()> {
        let (stdout_log, stderr_log) = self.create_log_files().await?;
        let stderr_path = self.request.repx_out_dir.join("stderr.log");

        let mut cmd = self.build_command_for_script(script_path, args).await?;

        log_info!(
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
        let cmd = match &self.request.runtime {
            Runtime::Native => self.build_native_command(script_path, args),
            Runtime::Podman { image_tag } => {
                self.ensure_image_loaded("podman", image_tag).await?;
                self.build_container_command("podman", image_tag, script_path, args)?
            }
            Runtime::Docker { image_tag } => {
                self.ensure_image_loaded("docker", image_tag).await?;
                self.build_container_command("docker", image_tag, script_path, args)?
            }
            Runtime::Bwrap => self.build_bwrap_command(script_path, args)?,
        };
        Ok(cmd)
    }

    fn build_native_command(&self, script_path: &Path, args: &[String]) -> TokioCommand {
        let mut cmd = TokioCommand::new(script_path);
        cmd.args(args);
        cmd
    }

    fn build_bwrap_command(&self, script_path: &Path, args: &[String]) -> Result<TokioCommand> {
        let mut cmd = TokioCommand::new("bwrap");

        cmd.arg("--dev-bind")
            .arg("/")
            .arg("/")
            .arg("--proc")
            .arg("/proc")
            .arg("--tmpfs")
            .arg("/tmp")
            .arg("--ro-bind")
            .arg("/nix/store")
            .arg("/nix/store")
            .arg("--ro-bind")
            .arg(&self.request.base_path)
            .arg(&self.request.base_path)
            .arg("--ro-bind")
            .arg(&self.request.job_package_path)
            .arg(&self.request.job_package_path)
            .arg("--bind")
            .arg(&self.request.user_out_dir)
            .arg(&self.request.user_out_dir);

        cmd.arg(script_path);
        cmd.args(args);

        Ok(cmd)
    }
    async fn ensure_image_loaded(&self, runtime: &str, image_tag: &str) -> Result<()> {
        let image_hash = image_tag.split(':').next_back().unwrap_or(image_tag);
        let lock_path = std::env::temp_dir().join(format!("repx-load-{}.lock", image_hash));
        let lock_file = std::fs::File::create(&lock_path)?;
        let _lock = Flock::lock(lock_file, FlockArg::LockExclusive).map_err(|(_file, e)| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to acquire file lock: {}", e),
            )
        })?;
        log_debug!("Acquired lock for image '{}'", image_tag);

        let check_output = Command::new(runtime)
            .args(["images", "-q", image_tag])
            .output()?;

        if check_output.stdout.is_empty() {
            log_info!(
                "Image '{}' not found in cache. Loading from tarball...",
                image_tag
            );
            let image_filename_stem = image_hash;
            let image_full_path = self
                .request
                .base_path
                .join("artifacts")
                .join("images")
                .join(format!("{}.tar", image_filename_stem));

            if !image_full_path.exists() {
                return Err(ExecutorError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Image tarball not found at {}", image_full_path.display()),
                )));
            }

            let mut load_cmd = Command::new(runtime);
            load_cmd.arg("load").arg("-i").arg(&image_full_path);

            let load_output = load_cmd.output()?;
            if !load_output.status.success() {
                let stderr = String::from_utf8_lossy(&load_output.stderr);
                return Err(ExecutorError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("'{} load' failed: {}", runtime, stderr),
                )));
            }

            let output_str = String::from_utf8_lossy(&load_output.stdout);
            let loaded_image_id = output_str
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Loaded image ID: ")
                        .or_else(|| line.strip_prefix("Loaded image: "))
                })
                .map(|s| s.trim().to_string());

            if let Some(id) = loaded_image_id {
                Command::new(runtime)
                    .args(["tag", &id, image_tag])
                    .status()?;
                log_info!("Successfully loaded and tagged image '{}'", image_tag);
            } else {
                log_info!("Could not parse image ID from load output. Assuming tag is correct.");
            }
        } else {
            log_debug!("Image '{}' found in cache. Skipping load.", image_tag);
        }

        log_debug!("Released lock for image '{}'", image_tag);
        Ok(())
    }

    fn build_container_command(
        &self,
        runtime: &str,
        image_tag: &str,
        script_path: &Path,
        args: &[String],
    ) -> Result<TokioCommand> {
        let mut cmd = TokioCommand::new(runtime);
        let slurm_job_id = std::env::var("SLURM_JOB_ID").unwrap_or_else(|_| "local".to_string());
        let xdg_runtime_dir = format!("/tmp/podman-runtime-{}", slurm_job_id);

        cmd.arg("run")
            .arg("--rm")
            .env("XDG_RUNTIME_DIR", &xdg_runtime_dir)
            .arg("--volume")
            .arg(format!(
                "{}:{}",
                self.request.base_path.display(),
                self.request.base_path.display()
            ))
            .arg("--workdir")
            .arg(self.request.user_out_dir.display().to_string())
            .arg(image_tag)
            .arg(script_path);

        cmd.args(args);

        Ok(cmd)
    }
}

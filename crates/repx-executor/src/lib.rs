use nix::fcntl::{Flock, FlockArg};
use repx_core::{log_debug, log_info, model::JobId};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use thiserror::Error;
use tokio::fs::{File, OpenOptions};
use tokio::process::Command as TokioCommand;

const ALLOWED_SYSTEM_BINARIES: &[&str] = &[
    "docker", "podman", "sbatch", "squeue", "sinfo", "sacct", "scancel",
];

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

    #[error(
        "Security violation: Attempted to use system binary '{0}' which is not in the allowlist."
    )]
    SecurityViolation(String),
}

pub type Result<T> = std::result::Result<T, ExecutorError>;

#[derive(Debug, Clone)]
pub enum Runtime {
    Native,
    Podman { image_tag: String },
    Docker { image_tag: String },
    Bwrap { image_tag: String },
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
    pub host_tools_bin_dir: Option<PathBuf>,
}

pub struct Executor {
    request: ExecutionRequest,
}

impl Executor {
    pub fn new(request: ExecutionRequest) -> Self {
        Self { request }
    }

    fn find_system_binary_dir(&self, binary_name: &str) -> Option<PathBuf> {
        if let Some(path_var) = std::env::var_os("PATH") {
            for path in std::env::split_paths(&path_var) {
                let candidate = path.join(binary_name);
                if candidate.is_file() {
                    return Some(path);
                }
            }
        }
        None
    }

    fn get_host_tool_path(&self, tool_name: &str) -> Result<PathBuf> {
        let host_tools = self.request.host_tools_bin_dir.as_ref().ok_or_else(|| {
            ExecutorError::Core(repx_core::error::AppError::ConfigurationError(format!(
                "Host tools directory not configured. Cannot resolve '{}'.",
                tool_name
            )))
        })?;

        let tool_path = host_tools.join(tool_name);
        if tool_path.exists() {
            return Ok(tool_path);
        }

        Err(ExecutorError::Core(
            repx_core::error::AppError::ConfigurationError(format!(
                "Required host tool '{}' not found in host-tools bin directory ({:?}).",
                tool_name, host_tools
            )),
        ))
    }

    fn resolve_tool(&self, tool_name: &str) -> Result<PathBuf> {
        if let Ok(path) = self.get_host_tool_path(tool_name) {
            return Ok(path);
        }

        if ALLOWED_SYSTEM_BINARIES.contains(&tool_name) {
            if let Some(dir) = self.find_system_binary_dir(tool_name) {
                let path = dir.join(tool_name);
                if path.exists() {
                    return Ok(path);
                }
            }
        }

        Err(ExecutorError::Core(
            repx_core::error::AppError::ConfigurationError(format!(
                "Tool '{}' not found in host-tools or allowed system binaries.",
                tool_name
            )),
        ))
    }

    fn find_image_file(&self, image_tag: &str) -> Option<PathBuf> {
        let artifacts = self.request.base_path.join("artifacts");
        let subdirs = ["images", "image"];

        for subdir in subdirs {
            let dir = artifacts.join(subdir);
            if !dir.exists() {
                continue;
            }

            let candidates = vec![
                dir.join(image_tag),
                dir.join(format!("{}.gz", image_tag)),
                dir.join(format!("{}.tar", image_tag)),
                dir.join(format!("{}.tar.gz", image_tag)),
            ];

            for candidate in candidates {
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    fn calculate_restricted_path(&self, required_system_binaries: &[&str]) -> std::ffi::OsString {
        let mut new_paths = Vec::new();

        if let Some(host_tools) = &self.request.host_tools_bin_dir {
            new_paths.push(host_tools.clone());
        }

        if !required_system_binaries.is_empty() {
            let mut added_dirs = HashSet::new();
            for &binary in required_system_binaries {
                if ALLOWED_SYSTEM_BINARIES.contains(&binary) {
                    if let Some(dir) = self.find_system_binary_dir(binary) {
                        if added_dirs.insert(dir.clone()) {
                            new_paths.push(dir);
                        }
                    } else {
                        log_debug!(
                            "Warning: Allowed system tool '{}' not found in system PATH.",
                            binary
                        );
                    }
                } else {
                    log_info!(
                        "[SECURITY] Blocked attempt to allowlist system binary '{}'. It is not in the allowed list.",
                        binary
                    );
                }
            }
        }

        std::env::join_paths(new_paths).unwrap_or_else(|_| std::ffi::OsString::from(""))
    }

    fn restrict_command_environment(
        &self,
        cmd: &mut TokioCommand,
        required_system_binaries: &[&str],
    ) {
        let path = self.calculate_restricted_path(required_system_binaries);
        cmd.env("PATH", path);
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
            Runtime::Bwrap { image_tag } => {
                let rootfs_path = self.ensure_bwrap_rootfs_extracted(image_tag).await?;
                self.build_bwrap_command(&rootfs_path, script_path, args)?
            }
        };
        Ok(cmd)
    }

    fn build_native_command(&self, script_path: &Path, args: &[String]) -> TokioCommand {
        let mut cmd = TokioCommand::new(script_path);
        cmd.args(args);

        if let Some(host_tools) = &self.request.host_tools_bin_dir {
            if let Some(system_path) = std::env::var_os("PATH") {
                let mut paths = std::env::split_paths(&system_path).collect::<Vec<_>>();
                paths.insert(0, host_tools.clone());
                if let Ok(new_path) = std::env::join_paths(paths) {
                    cmd.env("PATH", new_path);
                }
            } else {
                cmd.env("PATH", host_tools);
            }
        }

        cmd
    }

    async fn ensure_bwrap_rootfs_extracted(&self, image_tag: &str) -> Result<PathBuf> {
        let image_hash = image_tag.split(':').next_back().unwrap_or(image_tag);

        let images_cache_dir = self.request.base_path.join("cache").join("images");
        let extract_dir = images_cache_dir.join(image_hash).join("rootfs");
        let lock_path = std::env::temp_dir().join(format!("repx-extract-{}.lock", image_hash));

        tokio::fs::create_dir_all(&images_cache_dir).await?;

        let mut lock_file = std::fs::File::create(&lock_path)?;
        let _lock = loop {
            match Flock::lock(lock_file, FlockArg::LockExclusiveNonblock) {
                Ok(lock) => break lock,
                Err((f, errno))
                    if errno == nix::errno::Errno::EWOULDBLOCK
                        || errno == nix::errno::Errno::EAGAIN =>
                {
                    lock_file = f;
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                Err((_, e)) => {
                    return Err(ExecutorError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to acquire extraction lock: {}", e),
                    )))
                }
            }
        };

        if extract_dir.exists() {
            return Ok(extract_dir);
        }

        log_info!(
            "Extracting rootfs for image '{}' to {:?}",
            image_tag,
            extract_dir
        );

        let image_tar_path = self.find_image_file(image_tag).ok_or_else(|| {
            ExecutorError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Image file for tag '{}' not found in artifacts/images or artifacts/image",
                    image_tag
                ),
            ))
        })?;

        let temp_extract_dir = tempfile::tempdir()?;
        let tar_path = self.resolve_tool("tar")?;
        let mut cmd = TokioCommand::new(&tar_path);
        cmd.arg("-xf")
            .arg(&image_tar_path)
            .arg("-C")
            .arg(temp_extract_dir.path())
            .arg("--mode=0755")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        self.restrict_command_environment(&mut cmd, &["tar", "gzip"]);
        let output = cmd.output().await?;
        if !output.status.success() {
            return Err(ExecutorError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "Failed to extract outer image tarball. Stderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            )));
        }

        let layer_tar_path = walkdir::WalkDir::new(temp_extract_dir.path())
            .into_iter()
            .filter_map(|e| e.ok())
            .find(|e| e.file_name() == "layer.tar")
            .map(|e| e.path().to_path_buf())
            .ok_or_else(|| {
                ExecutorError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not find 'layer.tar' inside the extracted image archive.",
                ))
            })?;

        tokio::fs::create_dir_all(&extract_dir).await?;
        let mut cmd2 = TokioCommand::new(&tar_path);
        cmd2.arg("-xf")
            .arg(&layer_tar_path)
            .arg("-C")
            .arg(&extract_dir)
            .arg("--no-same-owner")
            .arg("--no-same-permissions")
            .arg("--mode=0755")
            .arg("--delay-directory-restore")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        self.restrict_command_environment(&mut cmd2, &["tar", "gzip"]);

        let output2 = cmd2.output().await?;
        if !output2.status.success() {
            let _ = tokio::fs::remove_dir_all(&extract_dir).await;
            return Err(ExecutorError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "Failed to extract inner 'layer.tar'. Stderr: {}",
                    String::from_utf8_lossy(&output2.stderr)
                ),
            )));
        }

        for dir in &["dev", "proc", "tmp"] {
            let p = extract_dir.join(dir);
            if !p.exists() {
                tokio::fs::create_dir(&p).await?;
            }
        }

        log_info!("Successfully extracted rootfs for '{}'", image_tag);
        Ok(extract_dir)
    }

    fn build_bwrap_command(
        &self,
        rootfs_path: &Path,
        script_path: &Path,
        args: &[String],
    ) -> Result<TokioCommand> {
        let bwrap_path = self.get_host_tool_path("bwrap")?;
        let mut cmd = TokioCommand::new(bwrap_path);

        cmd.arg("--unshare-all")
            .arg("--overlay-src")
            .arg(rootfs_path)
            .arg("--tmp-overlay")
            .arg("/")
            .arg("--dev")
            .arg("/dev")
            .arg("--proc")
            .arg("/proc")
            .arg("--tmpfs")
            .arg("/tmp")
            .arg("--dir")
            .arg(&self.request.base_path)
            .arg("--ro-bind")
            .arg(&self.request.base_path)
            .arg(&self.request.base_path)
            .arg("--dir")
            .arg(&self.request.user_out_dir)
            .arg("--bind")
            .arg(&self.request.user_out_dir)
            .arg(&self.request.user_out_dir)
            .arg("--dir")
            .arg(&self.request.repx_out_dir)
            .arg("--bind")
            .arg(&self.request.repx_out_dir)
            .arg(&self.request.repx_out_dir)
            .arg("--dir")
            .arg(&self.request.job_package_path)
            .arg("--ro-bind")
            .arg(&self.request.job_package_path)
            .arg(&self.request.job_package_path)
            .arg("--chdir")
            .arg(&self.request.user_out_dir)
            .arg(script_path);
        cmd.args(args);

        self.restrict_command_environment(&mut cmd, &[]);

        Ok(cmd)
    }

    async fn ensure_image_loaded(&self, runtime: &str, image_tag: &str) -> Result<()> {
        let image_hash = image_tag.split(':').next_back().unwrap_or(image_tag);
        let lock_path = std::env::temp_dir().join(format!("repx-load-{}.lock", image_hash));

        let mut lock_file = std::fs::File::create(&lock_path)?;
        let _lock = loop {
            match Flock::lock(lock_file, FlockArg::LockExclusiveNonblock) {
                Ok(lock) => break lock,
                Err((f, errno))
                    if errno == nix::errno::Errno::EWOULDBLOCK
                        || errno == nix::errno::Errno::EAGAIN =>
                {
                    lock_file = f;
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                Err((_, e)) => {
                    return Err(ExecutorError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to acquire file lock: {}", e),
                    )))
                }
            }
        };

        log_debug!("Acquired lock for image '{}'", image_tag);

        let mut check_cmd = TokioCommand::new(runtime);
        check_cmd.args(["images", "-q", image_tag]);
        self.restrict_command_environment(&mut check_cmd, &[runtime]);

        let check_output = check_cmd.output().await?;

        if check_output.stdout.is_empty() {
            log_info!(
                "Image '{}' not found in cache. Loading from tarball...",
                image_tag
            );

            let image_full_path = self.find_image_file(image_tag).ok_or_else(|| {
                ExecutorError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Image file for tag '{}' not found", image_tag),
                ))
            })?;

            let mut load_cmd = TokioCommand::new(runtime);
            load_cmd.arg("load").arg("-i").arg(&image_full_path);
            self.restrict_command_environment(&mut load_cmd, &[runtime]);

            let load_output = load_cmd.output().await?;
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
                let mut tag_cmd = TokioCommand::new(runtime);
                tag_cmd.args(["tag", &id, image_tag]);
                self.restrict_command_environment(&mut tag_cmd, &[runtime]);
                tag_cmd.output().await?;
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
        self.restrict_command_environment(&mut cmd, &[runtime]);
        Ok(cmd)
    }
}

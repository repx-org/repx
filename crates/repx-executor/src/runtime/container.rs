use crate::context::RuntimeContext;
use crate::error::{ExecutorError, Result};
use nix::fcntl::{Flock, FlockArg};
use std::path::Path;
use std::process::Stdio;
use tokio::fs::File;
use tokio::process::Command as TokioCommand;

pub struct ContainerRuntime;

use super::Runtime;

impl ContainerRuntime {
    fn get_runtime_details(runtime: &Runtime) -> Result<(&str, &str)> {
        match runtime {
            Runtime::Docker { image_tag } => Ok(("docker", image_tag)),
            Runtime::Podman { image_tag } => Ok(("podman", image_tag)),
            _ => Err(ExecutorError::Config(
                repx_core::errors::ConfigError::General(
                    "Invalid runtime for container execution. Must be Docker or Podman."
                        .to_string(),
                ),
            )),
        }
    }

    pub async fn ensure_image_loaded(ctx: &RuntimeContext<'_>, runtime: &Runtime) -> Result<()> {
        let (binary, image_tag) = Self::get_runtime_details(runtime)?;
        let image_hash = image_tag.split(':').next_back().unwrap_or(image_tag);

        let temp_path = ctx.get_temp_path();
        let lock_path = temp_path.join(format!("repx-load-{}.lock", image_hash));

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
                    return Err(ExecutorError::Io(std::io::Error::other(format!(
                        "Failed to acquire file lock: {}",
                        e
                    ))))
                }
            }
        };

        tracing::debug!("Acquired lock for image '{}'", image_tag);

        let mut check_cmd = TokioCommand::new(binary);
        check_cmd.args(["images", "-q", image_tag]);
        ctx.restrict_command_environment(&mut check_cmd, &[binary]);

        let check_output = check_cmd.output().await?;

        if check_output.stdout.is_empty() {
            tracing::info!("Image '{}' not found in cache. Loading...", image_tag);

            let image_full_path = ctx.find_image_file(image_tag).ok_or_else(|| {
                ExecutorError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Image file for tag '{}' not found", image_tag),
                ))
            })?;

            let mut load_cmd = TokioCommand::new(binary);
            load_cmd.arg("load");
            load_cmd.stdin(Stdio::piped());
            load_cmd.stdout(Stdio::piped());
            load_cmd.stderr(Stdio::piped());
            ctx.restrict_command_environment(&mut load_cmd, &[binary]);

            let mut child = load_cmd.spawn()?;
            let mut load_stdin = child.stdin.take().ok_or_else(|| {
                ExecutorError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "Failed to open stdin for docker load",
                ))
            })?;

            if image_full_path.is_dir() {
                tracing::debug!("Streaming image directory from {:?}...", image_full_path);
                let tar_path = ctx.resolve_tool("tar")?;
                let mut tar_cmd = TokioCommand::new(tar_path);
                tar_cmd
                    .arg("-C")
                    .arg(&image_full_path)
                    .arg("-h")
                    .arg("-c")
                    .arg(".");
                tar_cmd.stdout(Stdio::piped());

                let mut tar_child = tar_cmd.spawn()?;
                let mut tar_stdout = tar_child.stdout.take().ok_or_else(|| {
                    ExecutorError::Io(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "Failed to open stdout for tar",
                    ))
                })?;

                let copy_task = tokio::spawn(async move {
                    let res = tokio::io::copy(&mut tar_stdout, &mut load_stdin).await;
                    drop(load_stdin);
                    res
                });

                let tar_status = tar_child.wait().await?;
                if !tar_status.success() {
                    tracing::debug!("tar command failed with status {}", tar_status);
                }

                if let Err(e) = copy_task.await.unwrap() {
                    tracing::debug!("Copying tar output to {} load failed: {}", binary, e);
                }
            } else {
                tracing::debug!("Loading image tarball from {:?}...", image_full_path);
                let mut file = File::open(&image_full_path).await?;
                tokio::io::copy(&mut file, &mut load_stdin).await?;
                drop(load_stdin);
            }

            let load_output = child.wait_with_output().await?;
            if !load_output.status.success() {
                let stderr = String::from_utf8_lossy(&load_output.stderr);
                let stdout = String::from_utf8_lossy(&load_output.stdout);
                return Err(ExecutorError::Io(std::io::Error::other(format!(
                    "'{} load' failed with status {}. Stderr:\n{}\nStdout:\n{}",
                    binary, load_output.status, stderr, stdout
                ))));
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
                let mut tag_cmd = TokioCommand::new(binary);
                tag_cmd.args(["tag", &id, image_tag]);
                ctx.restrict_command_environment(&mut tag_cmd, &[binary]);
                tag_cmd.output().await?;
                tracing::info!("Successfully loaded and tagged image '{}'", image_tag);
            } else {
                tracing::info!(
                    "Could not parse image ID from load output. Assuming tag is correct."
                );
            }
        } else {
            tracing::debug!("Image '{}' found in cache. Skipping load.", image_tag);
        }

        tracing::debug!("Released lock for image '{}'", image_tag);
        Ok(())
    }

    pub async fn build_command(
        ctx: &RuntimeContext<'_>,
        runtime: &Runtime,
        script_path: &Path,
        args: &[String],
    ) -> Result<TokioCommand> {
        let (binary, image_tag) = Self::get_runtime_details(runtime)?;
        let request = ctx.request;
        let mut cmd = TokioCommand::new(binary);

        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        request.repx_out_dir.hash(&mut hasher);
        let unique_id = hasher.finish();

        let xdg_runtime_dir = request
            .base_path
            .join("repx")
            .join("runtime")
            .join(format!("podman-{:x}", unique_id));

        if !xdg_runtime_dir.exists() {
            std::fs::create_dir_all(&xdg_runtime_dir).map_err(ExecutorError::Io)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o700);
                std::fs::set_permissions(&xdg_runtime_dir, perms).map_err(ExecutorError::Io)?;
            }
        }

        cmd.arg("run")
            .arg("--rm")
            .arg("--hostname")
            .arg("repx-container")
            .arg("--env")
            .arg("TERM=xterm");

        if matches!(runtime, Runtime::Podman { .. }) {
            cmd.arg("--unsetenv").arg("container");
        }

        cmd.env("XDG_RUNTIME_DIR", &xdg_runtime_dir)
            .arg("--volume")
            .arg(format!(
                "{}:{}",
                request.base_path.display(),
                request.base_path.display()
            ))
            .arg("--workdir")
            .arg(request.user_out_dir.display().to_string());

        if request.mount_host_paths {
            tracing::info!("[IMPURE] mount_host_paths is enabled. Container is not isolated.");
            for dir in ["/home", "/tmp", "/var", "/opt", "/run", "/media", "/mnt"] {
                if Path::new(dir).exists() {
                    cmd.arg("-v").arg(format!("{}:{}", dir, dir));
                }
            }
            if Path::new("/nix").exists() {
                cmd.arg("-v").arg("/nix:/nix");
            }
        } else if !request.mount_paths.is_empty() {
            tracing::info!(
                "[IMPURE] Specific host paths mounted: {:?}",
                request.mount_paths
            );
            for path in &request.mount_paths {
                cmd.arg("-v").arg(format!("{}:{}", path, path));
            }
        }

        cmd.arg(image_tag).arg(script_path);

        cmd.args(args);
        ctx.restrict_command_environment(&mut cmd, &[binary]);
        Ok(cmd)
    }
}

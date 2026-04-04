use crate::context::RuntimeContext;
use crate::error::{ExecutorError, IoContext, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs::File;
use tokio::process::Command as TokioCommand;

pub struct ContainerRuntime;

use super::{Runtime, CONTAINER_HOSTNAME};

impl ContainerRuntime {
    fn get_runtime_details(runtime: &Runtime) -> Result<(&str, &str)> {
        match runtime {
            Runtime::Docker { image_tag } => Ok(("docker", image_tag.as_str())),
            Runtime::Podman { image_tag } => Ok(("podman", image_tag.as_str())),
            _ => Err(ExecutorError::Config(
                repx_core::errors::CoreError::UnsupportedValue {
                    kind: "runtime".to_string(),
                    value: "Must be Docker or Podman for container execution".to_string(),
                },
            )),
        }
    }

    pub async fn ensure_image_loaded(ctx: &RuntimeContext<'_>, runtime: &Runtime) -> Result<()> {
        let (binary, image_tag) = Self::get_runtime_details(runtime)?;
        let image_hash = crate::util::extract_image_hash(image_tag)?;

        let temp_path = ctx.get_temp_path().await;
        let lock_path = temp_path.join(format!("repx-load-{}.lock", image_hash));

        let _lock = super::acquire_flock(&lock_path, "image load").await?;

        tracing::debug!("Acquired lock for image '{}'", image_tag);

        let mut check_cmd = TokioCommand::new(binary);
        check_cmd.args(["images", "-q", image_tag]);
        ctx.restrict_command_environment(&mut check_cmd, &[binary])
            .await;

        let check_output = check_cmd
            .output()
            .await
            .map_err(|e| ExecutorError::CommandFailed {
                command: format!("{} images -q {}", binary, image_tag),
                source: e,
            })?;

        if check_output.stdout.is_empty() {
            tracing::info!("Image '{}' not found in cache. Loading...", image_tag);

            let image_full_path = ctx.find_image_file(image_tag).await.ok_or_else(|| {
                ExecutorError::ImageNotFound(format!(
                    "Image file for tag '{}' not found",
                    image_tag
                ))
            })?;

            let mut load_cmd = TokioCommand::new(binary);
            load_cmd.arg("load");
            load_cmd.stdin(Stdio::piped());
            load_cmd.stdout(Stdio::piped());
            load_cmd.stderr(Stdio::piped());
            ctx.restrict_command_environment(&mut load_cmd, &[binary])
                .await;

            let mut child = load_cmd.spawn().map_err(|e| ExecutorError::CommandFailed {
                command: format!("{} load", binary),
                source: e,
            })?;
            let mut load_stdin = child.stdin.take().ok_or_else(|| ExecutorError::Io {
                source: std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "Failed to open stdin for docker load",
                ),
                operation: "spawn",
                path: image_full_path.clone(),
            })?;

            if image_full_path.is_dir() {
                tracing::debug!("Streaming image directory from {:?}...", image_full_path);
                let tar_path = ctx.resolve_tool("tar").await?;
                let mut tar_cmd = TokioCommand::new(tar_path);
                tar_cmd
                    .arg("-C")
                    .arg(&image_full_path)
                    .arg("-h")
                    .arg("-c")
                    .arg(".");
                tar_cmd.stdout(Stdio::piped());

                let mut tar_child = tar_cmd.spawn().map_err(|e| ExecutorError::CommandFailed {
                    command: "tar".to_string(),
                    source: e,
                })?;
                let mut tar_stdout = tar_child.stdout.take().ok_or_else(|| ExecutorError::Io {
                    source: std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "Failed to open stdout for tar",
                    ),
                    operation: "spawn",
                    path: image_full_path.clone(),
                })?;

                let copy_task = tokio::spawn(async move {
                    let res = tokio::io::copy(&mut tar_stdout, &mut load_stdin).await;
                    drop(load_stdin);
                    res
                });

                let copy_result = copy_task.await;
                let tar_status =
                    tar_child
                        .wait()
                        .await
                        .map_err(|e| ExecutorError::CommandFailed {
                            command: "tar".to_string(),
                            source: e,
                        })?;

                let tar_failed = !tar_status.success();
                let tar_error_msg = if tar_failed {
                    Some(format!("tar failed with status {}", tar_status))
                } else {
                    None
                };

                let copy_error_msg = match copy_result {
                    Ok(Ok(_)) => None,
                    Ok(Err(e)) => Some(format!(
                        "Copying tar output to {} load failed: {}",
                        binary, e
                    )),
                    Err(join_err) => Some(format!("tar-to-load copy task panicked: {}", join_err)),
                };

                let load_output =
                    child
                        .wait_with_output()
                        .await
                        .map_err(|e| ExecutorError::CommandFailed {
                            command: format!("{} load", binary),
                            source: e,
                        })?;
                if !load_output.status.success() {
                    let stderr = String::from_utf8_lossy(&load_output.stderr);
                    let stdout = String::from_utf8_lossy(&load_output.stdout);
                    return Err(ExecutorError::Io {
                        source: std::io::Error::other(format!(
                            "'{} load' failed with status {}. Stderr:\n{}\nStdout:\n{}",
                            binary, load_output.status, stderr, stdout
                        )),
                        operation: "load",
                        path: image_full_path.clone(),
                    });
                }

                if let Some(ref copy_err) = copy_error_msg {
                    let is_broken_pipe = copy_err.contains("Broken pipe")
                        || copy_err.contains("os error 32")
                        || copy_err.contains("BrokenPipe");
                    if !is_broken_pipe {
                        return Err(ExecutorError::Io {
                            source: std::io::Error::other(copy_err.clone()),
                            operation: "load",
                            path: image_full_path.clone(),
                        });
                    } else {
                        tracing::warn!(
                            "Copy task got broken pipe but {} load succeeded. Continuing.",
                            binary
                        );
                    }
                }
                if let Some(tar_err) = tar_error_msg {
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        if tar_status.signal() == Some(13) {
                            tracing::warn!(
                                "tar received SIGPIPE but {} load succeeded. Continuing.",
                                binary
                            );
                        } else {
                            return Err(ExecutorError::Io {
                                source: std::io::Error::other(tar_err),
                                operation: "load",
                                path: image_full_path.clone(),
                            });
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        return Err(ExecutorError::Io {
                            source: std::io::Error::other(tar_err),
                            operation: "load",
                            path: image_full_path.clone(),
                        });
                    }
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
                    ctx.restrict_command_environment(&mut tag_cmd, &[binary])
                        .await;
                    let tag_output =
                        tag_cmd
                            .output()
                            .await
                            .map_err(|e| ExecutorError::CommandFailed {
                                command: format!("{} tag {} {}", binary, id, image_tag),
                                source: e,
                            })?;
                    if !tag_output.status.success() {
                        let stderr = String::from_utf8_lossy(&tag_output.stderr);
                        return Err(ExecutorError::Io {
                            source: std::io::Error::other(format!(
                                "'{} tag {} {}' failed with status {}. Stderr:\n{}",
                                binary, id, image_tag, tag_output.status, stderr
                            )),
                            operation: "tag",
                            path: image_full_path.clone(),
                        });
                    }
                    tracing::info!("Successfully loaded and tagged image '{}'", image_tag);
                } else {
                    tracing::info!(
                        "Could not parse image ID from load output. Assuming tag is correct."
                    );
                }
            } else {
                tracing::debug!("Loading image tarball from {:?}...", image_full_path);
                let mut file = File::open(&image_full_path)
                    .await
                    .io_ctx("open", &image_full_path)?;
                tokio::io::copy(&mut file, &mut load_stdin)
                    .await
                    .io_ctx("read", &image_full_path)?;
                drop(load_stdin);

                let load_output =
                    child
                        .wait_with_output()
                        .await
                        .map_err(|e| ExecutorError::CommandFailed {
                            command: format!("{} load", binary),
                            source: e,
                        })?;
                if !load_output.status.success() {
                    let stderr = String::from_utf8_lossy(&load_output.stderr);
                    let stdout = String::from_utf8_lossy(&load_output.stdout);
                    return Err(ExecutorError::Io {
                        source: std::io::Error::other(format!(
                            "'{} load' failed with status {}. Stderr:\n{}\nStdout:\n{}",
                            binary, load_output.status, stderr, stdout
                        )),
                        operation: "load",
                        path: image_full_path.clone(),
                    });
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
                    ctx.restrict_command_environment(&mut tag_cmd, &[binary])
                        .await;
                    let tag_output =
                        tag_cmd
                            .output()
                            .await
                            .map_err(|e| ExecutorError::CommandFailed {
                                command: format!("{} tag {} {}", binary, id, image_tag),
                                source: e,
                            })?;
                    if !tag_output.status.success() {
                        let stderr = String::from_utf8_lossy(&tag_output.stderr);
                        return Err(ExecutorError::Io {
                            source: std::io::Error::other(format!(
                                "'{} tag {} {}' failed with status {}. Stderr:\n{}",
                                binary, id, image_tag, tag_output.status, stderr
                            )),
                            operation: "tag",
                            path: image_full_path.clone(),
                        });
                    }
                    tracing::info!("Successfully loaded and tagged image '{}'", image_tag);
                } else {
                    tracing::info!(
                        "Could not parse image ID from load output. Assuming tag is correct."
                    );
                }
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
            tokio::fs::create_dir_all(&xdg_runtime_dir)
                .await
                .io_ctx("create_dir_all", &xdg_runtime_dir)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o700);
                tokio::fs::set_permissions(&xdg_runtime_dir, perms)
                    .await
                    .io_ctx("set_permissions", &xdg_runtime_dir)?;
            }
        }

        cmd.arg("run")
            .arg("--rm")
            .arg("--hostname")
            .arg(CONTAINER_HOSTNAME)
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

        match &request.mount_policy {
            repx_core::model::MountPolicy::AllHostPaths => {
                tracing::info!("[IMPURE] mount_host_paths is enabled. Container is not isolated.");
                for dir in ["/home", "/tmp", "/var", "/opt", "/run", "/media", "/mnt"] {
                    if Path::new(dir).exists() {
                        cmd.arg("-v").arg(format!("{}:{}", dir, dir));
                    }
                }
                if Path::new("/nix").exists() {
                    cmd.arg("-v").arg("/nix:/nix");
                }
            }
            repx_core::model::MountPolicy::SpecificPaths(paths) => {
                tracing::info!("[IMPURE] Specific host paths mounted: {:?}", paths);
                for path in paths {
                    cmd.arg("-v").arg(format!("{}:{}", path, path));
                }
            }
            repx_core::model::MountPolicy::Isolated => {}
        }

        let mut rewritten_args: Vec<String> = args.to_vec();
        let mut shm_cleanup: Vec<PathBuf> = Vec::new();

        if let Some(ref data) = request.inputs_data {
            let shm_path =
                PathBuf::from(format!("/dev/shm/repx-inputs-{}.json", std::process::id()));
            std::fs::write(&shm_path, data).io_ctx("write inputs to /dev/shm", &shm_path)?;
            let container_path = "/tmp/repx-inputs.json";
            cmd.arg("-v")
                .arg(format!("{}:{}:ro", shm_path.display(), container_path));
            if rewritten_args.len() > 1 {
                rewritten_args[1] = container_path.to_string();
            }
            shm_cleanup.push(shm_path);
        }
        if let Some(ref data) = request.parameters_data {
            let shm_path =
                PathBuf::from(format!("/dev/shm/repx-params-{}.json", std::process::id()));
            std::fs::write(&shm_path, data).io_ctx("write params to /dev/shm", &shm_path)?;
            let container_path = "/tmp/repx-parameters.json";
            cmd.arg("-v")
                .arg(format!("{}:{}:ro", shm_path.display(), container_path));
            if rewritten_args.len() > 2 {
                rewritten_args[2] = container_path.to_string();
            }
            shm_cleanup.push(shm_path);
        }

        cmd.arg(image_tag).arg(script_path);
        cmd.args(&rewritten_args);

        ctx.restrict_command_environment(&mut cmd, &[binary]).await;
        Ok(cmd)
    }
}

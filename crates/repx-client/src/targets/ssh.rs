use super::common::shell_quote;
use super::{
    ArtifactSync, CommandRunner, FileOps, GcOps, JobRunner, RemoteCommand, SlurmOps, TargetInfo,
};
use crate::error::{ClientError, Result};
use repx_core::{config, constants::dirs, errors::ConfigError, logging, model::JobId};
use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::Sender,
};

pub struct SshTarget {
    pub(crate) name: String,
    pub(crate) address: String,
    pub(crate) config: config::Target,
    pub(crate) local_tools_path: PathBuf,
    pub(crate) local_temp_path: PathBuf,
    pub(crate) host_tools_dir_name: String,
}

impl SshTarget {
    fn local_tool(&self, name: &str) -> PathBuf {
        let tool_path = self.local_tools_path.join(name);
        if tool_path.exists() {
            tool_path
        } else {
            PathBuf::from(name)
        }
    }

    fn remote_tool(&self, name: &str) -> String {
        if ["sbatch", "scancel", "squeue", "sacct", "sh"].contains(&name) {
            return name.to_string();
        }

        self.artifacts_base_path()
            .join("host-tools")
            .join(&self.host_tools_dir_name)
            .join("bin")
            .join(name)
            .to_string_lossy()
            .to_string()
    }

    fn deploy_rsync_binary(&self) -> Result<String> {
        let rsync_local_path = self.local_tool("rsync");
        let hash = super::compute_file_hash(&rsync_local_path)?;

        let remote_versioned_dir = self.base_path().join("bin").join(&hash);
        let remote_dest_path = remote_versioned_dir.join("rsync");
        let remote_dest_str = remote_dest_path.to_string_lossy().to_string();

        let check_cmd = RemoteCommand::new("test")
            .arg("-f")
            .arg(&remote_dest_str)
            .and(RemoteCommand::new("echo").arg("exists"));

        if let Ok(output) = self.run_command("sh", &["-c", &check_cmd.to_shell_string()]) {
            if output.trim() == "exists" {
                return Ok(remote_dest_str);
            }
        }

        let mkdir_cmd = RemoteCommand::new("mkdir")
            .arg("-p")
            .arg(&remote_versioned_dir.to_string_lossy());

        self.run_command("sh", &["-c", &mkdir_cmd.to_shell_string()])?;

        let mut scp_cmd = Command::new(self.local_tool("scp"));
        scp_cmd.arg(&rsync_local_path).arg(format!(
            "{}:{}",
            self.address,
            remote_dest_path.display()
        ));

        logging::log_and_print_command(&scp_cmd);
        let output = scp_cmd
            .output()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::Config(ConfigError::General(format!(
                "scp failed for rsync binary to {}: {}",
                self.address, stderr
            ))));
        }

        let chmod_cmd = RemoteCommand::new("chmod").arg("755").arg(&remote_dest_str);
        let _ = self.run_command("sh", &["-c", &chmod_cmd.to_shell_string()]);

        Ok(remote_dest_str)
    }
}

impl TargetInfo for SshTarget {
    fn name(&self) -> &str {
        &self.name
    }

    fn base_path(&self) -> &Path {
        &self.config.base_path
    }

    fn config(&self) -> &config::Target {
        &self.config
    }

    fn get_remote_path_str(&self, job_id: &JobId) -> String {
        format!(
            "{}:{}",
            self.address,
            self.base_path()
                .join(dirs::OUTPUTS)
                .join(&job_id.0)
                .join(dirs::OUT)
                .display()
        )
    }
}

impl CommandRunner for SshTarget {
    fn run_command(&self, command: &str, args: &[&str]) -> Result<String> {
        let remote_cmd_exe = self.remote_tool(command);

        let remote_command_string = if command == "sh" && args.len() == 2 && args[0] == "-c" {
            format!("sh -c {}", shell_quote(args[1]))
        } else {
            let mut parts = vec![remote_cmd_exe.as_str()];
            parts.extend_from_slice(args);
            parts.join(" ")
        };

        let mut cmd = Command::new(self.local_tool("ssh"));
        cmd.arg(&self.address).arg(&remote_command_string);

        logging::log_and_print_command(&cmd);
        let output = cmd
            .output()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::TargetCommandFailed {
                target: self.name.clone(),
                source: ConfigError::General(format!(
                    "Command '{}' failed on target '{}': {}",
                    remote_command_string, self.name, stderr
                )),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

impl SlurmOps for SshTarget {
    fn scancel(&self, slurm_id: u32) -> Result<()> {
        self.run_command("scancel", &[&slurm_id.to_string()])?;
        Ok(())
    }
}

impl SshTarget {
    fn sync_directory_impl(
        &self,
        local_path: &Path,
        remote_path: &Path,
        follow_symlinks: bool,
    ) -> Result<()> {
        let remote_rsync_path = self.deploy_rsync_binary()?;

        let mut rsync_cmd = Command::new(self.local_tool("rsync"));
        let flags = if follow_symlinks { "-rLtpz" } else { "-rltpz" };
        rsync_cmd
            .arg(flags)
            .arg("--chmod=Du+w")
            .arg("--mkpath")
            .arg(format!("--rsync-path={}", remote_rsync_path))
            .arg(format!("{}/", local_path.display()))
            .arg(format!("{}:{}", self.address, remote_path.display()));

        logging::log_and_print_command(&rsync_cmd);
        let output = rsync_cmd
            .output()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::Config(ConfigError::General(format!(
                "rsync directory sync failed: {}",
                stderr
            ))));
        }

        Ok(())
    }
}

impl ArtifactSync for SshTarget {
    fn get_missing_artifacts(&self, artifacts: &HashSet<PathBuf>) -> Result<HashSet<PathBuf>> {
        if artifacts.is_empty() {
            return Ok(HashSet::new());
        }

        let artifacts_base = self.artifacts_base_path();
        let find_bin = self.remote_tool("find");
        let mkdir_bin = self.remote_tool("mkdir");

        let find_cmd = RemoteCommand::new(&mkdir_bin)
            .arg("-p")
            .arg(&artifacts_base.to_string_lossy())
            .and(
                RemoteCommand::new("cd")
                    .arg(&artifacts_base.to_string_lossy())
                    .and(RemoteCommand::new(&find_bin).arg(".").arg("-type").arg("f")),
            )
            .or(RemoteCommand::new("true"));

        let output = self.run_command("sh", &["-c", &find_cmd.to_shell_string()])?;

        let existing: HashSet<PathBuf> = output
            .lines()
            .filter_map(|s| s.strip_prefix("./"))
            .map(PathBuf::from)
            .collect();

        let missing = artifacts
            .iter()
            .filter(|p| !existing.contains(*p))
            .cloned()
            .collect();

        Ok(missing)
    }

    fn sync_artifacts_batch(
        &self,
        local_lab_path: &Path,
        artifacts: &HashSet<PathBuf>,
        _event_sender: Option<&Sender<super::super::ClientEvent>>,
    ) -> Result<()> {
        if artifacts.is_empty() {
            return Ok(());
        }

        let _ = fs_err::create_dir_all(&self.local_temp_path);
        let mut temp_file = tempfile::Builder::new()
            .prefix("repx-sync-list-")
            .tempfile_in(&self.local_temp_path)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        for path in artifacts {
            writeln!(temp_file, "{}", path.to_string_lossy())
                .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        }
        temp_file
            .flush()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        let remote_rsync_path = self.deploy_rsync_binary()?;

        let mut rsync_cmd = Command::new(self.local_tool("rsync"));
        rsync_cmd
            .arg("-rLtpz")
            .arg("--chmod=Du+w")
            .arg(format!("--rsync-path={}", remote_rsync_path))
            .arg("--files-from")
            .arg(temp_file.path())
            .arg("./")
            .arg(format!(
                "{}:{}",
                self.address,
                self.artifacts_base_path().display()
            ))
            .current_dir(local_lab_path);

        logging::log_and_print_command(&rsync_cmd);
        let output = rsync_cmd
            .output()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::Config(ConfigError::General(format!(
                "rsync batch sync failed: {}",
                stderr
            ))));
        }

        Ok(())
    }

    fn sync_artifact(&self, local_path: &Path, relative_path: &Path) -> Result<()> {
        let dest = self.artifacts_base_path().join(relative_path);
        let dest_str = format!("{}:{}", self.address, dest.display());

        if let Some(parent) = dest.parent() {
            let mkdir_cmd = RemoteCommand::new("mkdir")
                .arg("-p")
                .arg(&parent.to_string_lossy());
            self.run_command("sh", &["-c", &mkdir_cmd.to_shell_string()])?;
        }

        let mut scp_cmd = Command::new(self.local_tool("scp"));
        if local_path.is_dir() {
            scp_cmd.arg("-r");
        }
        scp_cmd.arg(local_path).arg(&dest_str);

        logging::log_and_print_command(&scp_cmd);
        let output = scp_cmd
            .output()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::Config(ConfigError::General(format!(
                "scp failed for {}: {}",
                relative_path.display(),
                stderr
            ))));
        }

        if !local_path.is_dir() {
            if let Ok(meta) = std::fs::metadata(local_path) {
                use std::os::unix::fs::MetadataExt;
                let is_executable = (meta.mode() & 0o111) != 0;
                if is_executable {
                    let chmod_cmd = RemoteCommand::new("chmod")
                        .arg("755")
                        .arg(&dest.to_string_lossy());
                    let _ = self.run_command("sh", &["-c", &chmod_cmd.to_shell_string()]);
                }
            }
        }

        Ok(())
    }

    fn sync_lab_root(&self, local_lab_path: &Path) -> Result<()> {
        let remote_artifacts_base = self.artifacts_base_path();
        self.sync_directory(local_lab_path, &remote_artifacts_base)?;

        let chmod_bin = self.remote_tool("chmod");
        let cmd = RemoteCommand::new(&chmod_bin)
            .arg("u+w")
            .arg(&remote_artifacts_base.to_string_lossy());
        self.run_command("sh", &["-c", &cmd.to_shell_string()])?;

        Ok(())
    }

    fn sync_directory(&self, local_path: &Path, remote_path: &Path) -> Result<()> {
        self.sync_directory_impl(local_path, remote_path, false)
    }

    fn sync_image_incrementally(
        &self,
        image_path: &Path,
        image_tag: &str,
        local_cache_root: &Path,
    ) -> Result<()> {
        let store_cache = local_cache_root.join("store");

        fs_err::create_dir_all(&store_cache)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        let tar_tool = self.local_tool("tar");

        if image_path.is_dir() {
            let parent = image_path.parent();
            let store_dir = parent.and_then(|p| p.parent()).map(|p| p.join("store"));

            if let Some(store_path) = store_dir {
                if store_path.exists() {
                    let remote_store = self.artifacts_base_path().join("store");
                    let remote_images = self.artifacts_base_path().join("images");

                    let mkdir_cmd = RemoteCommand::new("mkdir")
                        .arg("-p")
                        .arg(&remote_store.to_string_lossy())
                        .arg(&remote_images.to_string_lossy());
                    self.run_command("sh", &["-c", &mkdir_cmd.to_shell_string()])?;

                    self.sync_directory(&store_path, &remote_store)?;

                    let image_dir_name = image_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("image");
                    let remote_image_dir = remote_images.join(image_dir_name);
                    self.sync_directory(image_path, &remote_image_dir)?;

                    let ln_cmd = RemoteCommand::new("cd")
                        .arg(&remote_images.to_string_lossy())
                        .and(RemoteCommand::new("rm").arg("-f").arg(image_tag))
                        .and(
                            RemoteCommand::new("ln")
                                .arg("-sfn")
                                .arg(image_dir_name)
                                .arg(image_tag),
                        );
                    self.run_command("sh", &["-c", &ln_cmd.to_shell_string()])?;

                    return Ok(());
                }
            }

            let remote_images = self.artifacts_base_path().join("images");

            let mkdir_cmd = RemoteCommand::new("mkdir")
                .arg("-p")
                .arg(&remote_images.to_string_lossy());
            self.run_command("sh", &["-c", &mkdir_cmd.to_shell_string()])?;

            let image_dir_name = image_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("image");

            let remote_image_dir = remote_images.join(image_dir_name);

            self.sync_directory_impl(image_path, &remote_image_dir, true)?;

            let ln_cmd = RemoteCommand::new("cd")
                .arg(&remote_images.to_string_lossy())
                .and(
                    RemoteCommand::new("ln")
                        .arg("-sfn")
                        .arg(image_dir_name)
                        .arg(image_tag),
                );
            self.run_command("sh", &["-c", &ln_cmd.to_shell_string()])?;

            return Ok(());
        }

        let layers = super::common::get_image_manifest(image_path, &tar_tool)?;

        let remote_store = self.artifacts_base_path().join("store");
        let remote_images = self.artifacts_base_path().join("images");

        let mkdir_cmd = RemoteCommand::new("mkdir")
            .arg("-p")
            .arg(&remote_images.to_string_lossy())
            .arg(&remote_store.to_string_lossy());
        self.run_command("sh", &["-c", &mkdir_cmd.to_shell_string()])?;

        let check_cmd = RemoteCommand::new("ls")
            .arg("-1")
            .arg(&remote_store.to_string_lossy());

        let remote_store_list_str = self
            .run_command("sh", &["-c", &check_cmd.to_shell_string()])
            .unwrap_or_default();

        let existing_remote_items: HashSet<String> = remote_store_list_str
            .lines()
            .map(|s| s.trim().to_string())
            .collect();

        let mut layers_to_sync = Vec::new();

        for layer in &layers {
            let layer_hash = Path::new(layer)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or(layer);

            let flat_layer_name = format!("{}-layer.tar", layer_hash);

            if !existing_remote_items.contains(&flat_layer_name) {
                super::common::extract_layer_to_flat_store(
                    image_path,
                    layer,
                    layer_hash,
                    &store_cache,
                    &tar_tool,
                )?;
                layers_to_sync.push((store_cache.join(&flat_layer_name), flat_layer_name));
            }
        }

        if !layers_to_sync.is_empty() {
            let remote_rsync_path = self.deploy_rsync_binary()?;

            for (local_layer_path, layer_name) in &layers_to_sync {
                let remote_dest = remote_store.join(layer_name);

                let mut rsync_cmd = Command::new(self.local_tool("rsync"));
                rsync_cmd
                    .arg("-tpz")
                    .arg(format!("--rsync-path={}", remote_rsync_path))
                    .arg(local_layer_path)
                    .arg(format!("{}:{}", self.address, remote_dest.display()));

                logging::log_and_print_command(&rsync_cmd);
                let output = rsync_cmd
                    .output()
                    .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(ClientError::Config(ConfigError::General(format!(
                        "rsync failed for layer {}: {}",
                        layer_name, stderr
                    ))));
                }
            }
        }

        let image_filename = image_path.file_name().unwrap().to_str().unwrap();
        let image_hash_name = super::common::parse_image_hash(image_filename);
        let remote_image_dir = remote_images.join(image_hash_name);

        let manifest_content = serde_json::to_string(&vec![super::common::ManifestEntry {
            layers: layers.clone(),
        }])
        .map_err(|e| ClientError::Config(ConfigError::Json(e)))?;

        self.write_remote_file(&remote_image_dir.join("manifest.json"), &manifest_content)?;

        let mut link_script = String::from("set -e\n");
        for layer in &layers {
            let layer_hash = Path::new(layer)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or(layer);

            let link_dir = remote_image_dir.join(layer_hash);
            link_script.push_str(&format!("mkdir -p {}\n", link_dir.to_string_lossy()));

            let target_path = format!("../../store/{}-layer.tar", layer_hash);
            let link_path = link_dir.join("layer.tar");

            link_script.push_str(&format!(
                "ln -sfn {} {}\n",
                target_path,
                link_path.to_string_lossy()
            ));
        }

        self.run_command("sh", &["-c", &link_script])?;

        let ln_cmd = RemoteCommand::new("cd")
            .arg(&remote_images.to_string_lossy())
            .and(
                RemoteCommand::new("ln")
                    .arg("-sfn")
                    .arg(image_hash_name)
                    .arg(image_tag),
            );
        self.run_command("sh", &["-c", &ln_cmd.to_shell_string()])?;

        Ok(())
    }
}

impl FileOps for SshTarget {
    fn read_remote_file_tail(&self, path: &Path, line_count: u32) -> Result<Vec<String>> {
        let quoted_path = path.to_string_lossy();
        let tail_bin = self.remote_tool("tail");

        let cmd = RemoteCommand::new("[")
            .arg("-f")
            .arg(&quoted_path)
            .arg("]")
            .and(
                RemoteCommand::new(&tail_bin)
                    .arg("-n")
                    .arg(&line_count.to_string())
                    .arg(&quoted_path),
            )
            .or(RemoteCommand::new("true"));

        let output = self.run_command("sh", &["-c", &cmd.to_shell_string()])?;
        Ok(output.lines().map(String::from).collect())
    }

    fn write_remote_file(&self, path: &Path, content: &str) -> Result<()> {
        let parent = path.parent().ok_or_else(|| ClientError::InvalidPath {
            path: path.to_path_buf(),
            reason: "Path has no parent directory".to_string(),
        })?;

        let mkdir_bin = self.remote_tool("mkdir");
        let cat_bin = self.remote_tool("cat");

        let remote_command = RemoteCommand::new(&mkdir_bin)
            .arg("-p")
            .arg(&parent.to_string_lossy())
            .and(RemoteCommand::new(&cat_bin).redirect_out(&path.to_string_lossy()));

        let mut cmd = Command::new(self.local_tool("ssh"));
        cmd.arg(&self.address)
            .arg(remote_command.to_shell_string())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        logging::log_and_print_command(&cmd);

        let mut child = cmd
            .spawn()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        let mut stdin = child.stdin.take().expect("Failed to open stdin");
        let content_bytes = content.as_bytes().to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&content_bytes);
        });

        let output = child.wait_with_output().map_err(|e| {
            ClientError::Config(ConfigError::General(format!(
                "Failed to wait for remote write: {}",
                e
            )))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::TargetCommandFailed {
                target: self.name.clone(),
                source: ConfigError::General(format!(
                    "Failed to write '{}': {}",
                    path.display(),
                    stderr
                )),
            });
        }

        Ok(())
    }
}

impl JobRunner for SshTarget {
    fn deploy_repx_binary(&self) -> Result<PathBuf> {
        let runner_exe_path = super::find_local_runner_binary()?;
        let hash = super::compute_file_hash(&runner_exe_path)?;

        let remote_versioned_dir = self.base_path().join("bin").join(&hash);
        let remote_dest_path = remote_versioned_dir.join("repx");

        let verify = || -> Result<()> {
            let cmd = RemoteCommand::new(&remote_dest_path.to_string_lossy()).arg("--version");
            match self.run_command("sh", &["-c", &cmd.to_shell_string()]) {
                Ok(_) => Ok(()),
                Err(e) => Err(ClientError::TargetCommandFailed {
                    target: self.name.clone(),
                    source: ConfigError::General(format!(
                        "Binary verification failed. The deployed binary failed to execute. Check architecture compatibility.\nError: {}",
                        e
                    )),
                }),
            }
        };

        let check_cmd = RemoteCommand::new("test")
            .arg("-f")
            .arg(&remote_dest_path.to_string_lossy())
            .and(RemoteCommand::new("echo").arg("exists"));

        if let Ok(output) = self.run_command("sh", &["-c", &check_cmd.to_shell_string()]) {
            if output.trim() == "exists" {
                verify()?;
                return Ok(remote_dest_path);
            }
        }

        let mkdir_cmd = RemoteCommand::new("mkdir")
            .arg("-p")
            .arg(&remote_versioned_dir.to_string_lossy());
        self.run_command("sh", &["-c", &mkdir_cmd.to_shell_string()])?;

        let mut scp_cmd = Command::new(self.local_tool("scp"));
        scp_cmd.arg(&runner_exe_path).arg(format!(
            "{}:{}",
            self.address,
            remote_dest_path.display()
        ));

        logging::log_and_print_command(&scp_cmd);
        let output = scp_cmd
            .output()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::Config(ConfigError::General(format!(
                "scp failed for repx binary to {}: {}",
                self.address, stderr
            ))));
        }

        let chmod_cmd = RemoteCommand::new("chmod")
            .arg("755")
            .arg(&remote_dest_path.to_string_lossy());
        self.run_command("sh", &["-c", &chmod_cmd.to_shell_string()])?;

        verify()?;
        Ok(remote_dest_path)
    }

    fn spawn_repx_job(
        &self,
        repx_binary_path: &Path,
        args: &[String],
    ) -> Result<std::process::Child> {
        let remote_cmd = RemoteCommand::new(&repx_binary_path.to_string_lossy()).args(args);

        let mut cmd = Command::new(self.local_tool("ssh"));
        cmd.arg(&self.address)
            .arg(remote_cmd.to_shell_string())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        logging::log_and_print_command(&cmd);

        cmd.spawn().map_err(|e| {
            ClientError::Config(ConfigError::General(format!(
                "Failed to spawn SSH process: {}",
                e
            )))
        })
    }
}

impl GcOps for SshTarget {
    fn register_gc_root(&self, project_id: &str, lab_hash: &str) -> Result<()> {
        let gcroots_dir = self
            .base_path()
            .join("gcroots")
            .join("auto")
            .join(project_id);

        let link_name = super::common::generate_gc_link_name(lab_hash);
        let link_path = gcroots_dir.join(&link_name);

        let artifacts_base = self.artifacts_base_path();
        let lab_dir = artifacts_base.join("lab");

        let find_manifest_cmd = RemoteCommand::new("find")
            .arg(&lab_dir.to_string_lossy())
            .arg("-name")
            .arg(&format!("*{}*-lab-metadata.json", lab_hash))
            .pipe(RemoteCommand::new("head").arg("-n").arg("1"));

        let manifest_output =
            self.run_command("sh", &["-c", &find_manifest_cmd.to_shell_string()])?;
        let manifest_path_str = manifest_output.trim();

        let target_path_str = if !manifest_path_str.is_empty() {
            manifest_path_str.to_string()
        } else {
            artifacts_base.join(lab_hash).to_string_lossy().to_string()
        };

        let script = format!(
            r#"
            mkdir -p {0}
            ln -sfn {1} {2}
            cd {0}
            ls -1 | sort -r | tail -n +6 | xargs -r rm
            "#,
            shell_quote(&gcroots_dir.to_string_lossy()),
            shell_quote(&target_path_str),
            shell_quote(&link_path.to_string_lossy())
        );

        self.run_command("sh", &["-c", &script])?;
        Ok(())
    }

    fn garbage_collect(&self) -> Result<String> {
        let repx_bin = self.deploy_repx_binary()?;
        let cmd = RemoteCommand::new(&repx_bin.to_string_lossy())
            .arg("internal-gc")
            .arg("--base-path")
            .arg(&self.base_path().to_string_lossy());

        self.run_command("sh", &["-c", &cmd.to_shell_string()])
    }
}

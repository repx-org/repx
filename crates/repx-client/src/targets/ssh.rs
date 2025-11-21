use super::Target;
use crate::error::{ClientError, Result};
use repx_core::{error::AppError, log_info, logging, model::JobId};
use std::{
    collections::HashSet,
    io::Write,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::Sender,
};

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub struct SshTarget {
    pub(crate) name: String,
    pub(crate) address: String,
    pub(crate) config: repx_core::config::Target,
}

impl Target for SshTarget {
    fn name(&self) -> &str {
        &self.name
    }
    fn base_path(&self) -> &Path {
        &self.config.base_path
    }
    fn config(&self) -> &repx_core::config::Target {
        &self.config
    }
    fn get_remote_path_str(&self, job_id: &JobId) -> String {
        format!(
            "{}:{}",
            self.address,
            self.base_path()
                .join("outputs")
                .join(&job_id.0)
                .join("out")
                .display()
        )
    }

    fn run_command(&self, command: &str, args: &[&str]) -> Result<String> {
        let remote_command_string = if command == "sh" && args.len() == 2 && args[0] == "-c" {
            format!("sh -c {}", shell_quote(args[1]))
        } else {
            let mut all_parts = vec![command];
            all_parts.extend_from_slice(args);
            all_parts.join(" ")
        };

        let mut cmd = Command::new("ssh");
        cmd.arg(&self.address).arg(&remote_command_string);

        logging::log_and_print_command(&cmd);
        let output = cmd.output().map_err(|e| AppError::ProcessLaunchFailed {
            command_name: "ssh".to_string(),
            source: e,
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::TargetCommandFailed {
                target: self.name.clone(),
                source: AppError::ExecutionFailed {
                    message: format!(
                        "Command '{}' failed on target '{}'",
                        remote_command_string, self.name
                    ),
                    log_path: None,
                    log_summary: stderr.to_string(),
                },
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn scancel(&self, slurm_id: u32) -> Result<()> {
        self.run_command("scancel", &[&slurm_id.to_string()])?;
        Ok(())
    }

    fn get_missing_artifacts(&self, artifacts: &HashSet<PathBuf>) -> Result<HashSet<PathBuf>> {
        if artifacts.is_empty() {
            return Ok(HashSet::new());
        }

        let artifacts_base = self.artifacts_base_path();

        let find_cmd = format!(
            "mkdir -p {} && (cd {} && find . -type f) || true",
            shell_quote(&artifacts_base.to_string_lossy()),
            shell_quote(&artifacts_base.to_string_lossy())
        );

        let output = self.run_command("sh", &["-c", &find_cmd])?;

        let existing_artifacts: HashSet<PathBuf> = output
            .lines()
            .filter_map(|s| s.strip_prefix("./"))
            .map(PathBuf::from)
            .collect();

        let missing = artifacts
            .iter()
            .filter(|required| !existing_artifacts.contains(*required))
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

        let mut temp_file = tempfile::Builder::new()
            .prefix("repx-sync-list-")
            .tempfile()
            .map_err(AppError::from)?;

        for path in artifacts {
            writeln!(temp_file, "{}", path.to_string_lossy()).map_err(AppError::from)?;
        }

        let mut rsync_cmd = Command::new("rsync");
        rsync_cmd
            .arg("-rltpz")
            .arg("./")
            .arg(format!(
                "{}:{}",
                self.address,
                self.artifacts_base_path().display()
            ))
            .current_dir(local_lab_path);
        log_info!("[CMD] Syncing artifact batch with rsync: {:?}", rsync_cmd);
        let rsync_output = rsync_cmd.output().map_err(AppError::from)?;

        if !rsync_output.status.success() {
            let stderr = String::from_utf8_lossy(&rsync_output.stderr);
            return Err(ClientError::Core(AppError::ExecutionFailed {
                message: "rsync failed".to_string(),
                log_path: None,
                log_summary: format!(
                    "rsync exited with status {}. Stderr:\n{}",
                    rsync_output.status, stderr
                ),
            }));
        }

        let remote_artifacts_base = self.artifacts_base_path();
        let chmod_cmd_str = format!(
            "chmod -R a-w,a+rX {}",
            shell_quote(&remote_artifacts_base.to_string_lossy())
        );

        self.run_command("sh", &["-c", &chmod_cmd_str])?;

        Ok(())
    }

    fn sync_artifact(&self, local_path: &Path, relative_path: &Path) -> Result<()> {
        let remote_dest = self.artifacts_base_path().join(relative_path);
        let remote_parent = remote_dest.parent().unwrap();

        let mkdir_cmd = format!("mkdir -p {}", shell_quote(&remote_parent.to_string_lossy()));
        self.run_command("sh", &["-c", &mkdir_cmd])?;

        let mut scp_cmd = Command::new("scp");
        if local_path.is_dir() {
            scp_cmd.arg("-r");
        }
        scp_cmd
            .arg(local_path)
            .arg(format!("{}:{}", self.address, remote_dest.display()));
        log_info!("[CMD] Syncing artifact with scp: {:?}", scp_cmd);
        let scp_output = scp_cmd.output().map_err(AppError::from)?;

        if !scp_output.status.success() {
            let stderr = String::from_utf8_lossy(&scp_output.stderr);
            return Err(ClientError::Core(AppError::ExecutionFailed {
                message: format!("scp failed for {}", local_path.display()),
                log_path: None,
                log_summary: format!(
                    "scp exited with status {}. Stderr:\n{}",
                    scp_output.status, stderr
                ),
            }));
        }

        let mut chmod_cmds = Vec::new();
        if local_path.is_dir() {
            chmod_cmds.push(format!(
                "chmod -R a-w,a+rX {}",
                shell_quote(&remote_dest.to_string_lossy())
            ));

            for entry in walkdir::WalkDir::new(local_path).into_iter().flatten() {
                if entry.file_type().is_file() {
                    if let Ok(metadata) = entry.metadata() {
                        if (metadata.mode() & 0o111) != 0 {
                            let rel_path = entry.path().strip_prefix(local_path).unwrap();
                            let remote_file_path = remote_dest.join(rel_path);
                            chmod_cmds.push(format!(
                                "chmod a+x {}",
                                shell_quote(&remote_file_path.to_string_lossy())
                            ));
                        }
                    }
                }
            }
        } else {
            let is_executable = local_path.metadata().is_ok_and(|m| (m.mode() & 0o111) != 0);
            let mode = if is_executable { "555" } else { "444" };
            chmod_cmds.push(format!(
                "chmod {} {}",
                mode,
                shell_quote(&remote_dest.to_string_lossy())
            ));
        }

        if !chmod_cmds.is_empty() {
            let final_chmod_cmd = chmod_cmds.join(" && ");
            self.run_command("sh", &["-c", &final_chmod_cmd])?;
        }

        Ok(())
    }

    fn read_remote_file_tail(&self, path: &Path, line_count: u32) -> Result<Vec<String>> {
        let quoted_path = shell_quote(&path.to_string_lossy());
        let cmd_str = format!(
            "[ -f {} ] && tail -n {} {} || true",
            quoted_path, line_count, quoted_path
        );
        let output = self.run_command("sh", &["-c", &cmd_str])?;
        Ok(output.lines().map(String::from).collect())
    }

    fn write_remote_file(&self, path: &Path, content: &str) -> Result<()> {
        let parent = path.parent().ok_or_else(|| {
            ClientError::Core(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Path has no parent",
            )))
        })?;

        let remote_command = format!(
            "mkdir -p {} && cat > {}",
            shell_quote(&parent.to_string_lossy()),
            shell_quote(&path.to_string_lossy())
        );

        let mut cmd = Command::new("ssh");
        cmd.arg(&self.address)
            .arg(&remote_command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        logging::log_and_print_command(&cmd);

        let mut child = cmd.spawn().map_err(|e| AppError::ProcessLaunchFailed {
            command_name: "ssh".to_string(),
            source: e,
        })?;

        let mut stdin = child.stdin.take().expect("Failed to open stdin for ssh");
        let content_bytes = content.as_bytes().to_vec();
        std::thread::spawn(move || {
            let _ = std::io::Write::write_all(&mut stdin, &content_bytes);
        });

        let output = child.wait_with_output().map_err(|e| {
            ClientError::Core(AppError::ExecutionFailed {
                message: "Failed to wait for remote write command".to_string(),
                log_path: None,
                log_summary: e.to_string(),
            })
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::TargetCommandFailed {
                target: self.name.clone(),
                source: AppError::ExecutionFailed {
                    message: format!("Failed to write remote file '{}'", path.display()),
                    log_path: None,
                    log_summary: stderr.to_string(),
                },
            });
        }
        Ok(())
    }

    fn sync_directory(&self, local_path: &Path, remote_path: &Path) -> Result<()> {
        let mkdir_cmd = format!("mkdir -p {}", shell_quote(&remote_path.to_string_lossy()));
        self.run_command("sh", &["-c", &mkdir_cmd])?;

        let mut rsync_cmd = Command::new("rsync");
        rsync_cmd
            .arg("-rltpz")
            .arg(format!("{}/", local_path.display()))
            .arg(format!("{}:{}", self.address, remote_path.display()));

        logging::log_and_print_command(&rsync_cmd);
        let rsync_output = rsync_cmd.output().map_err(AppError::from)?;

        if !rsync_output.status.success() {
            let stderr = String::from_utf8_lossy(&rsync_output.stderr);
            return Err(ClientError::Core(AppError::ExecutionFailed {
                message: "rsync failed for directory sync".to_string(),
                log_path: None,
                log_summary: format!(
                    "rsync exited with status {}. Stderr:\n{}",
                    rsync_output.status, stderr
                ),
            }));
        }
        Ok(())
    }

    fn sync_lab_root(&self, local_lab_path: &Path) -> Result<()> {
        let remote_artifacts_base = self.artifacts_base_path();
        self.sync_directory(local_lab_path, &remote_artifacts_base)
    }
    fn deploy_repx_binary(&self) -> Result<PathBuf> {
        let current_exe = std::env::current_exe().map_err(AppError::from)?;
        let mut exe_dir = current_exe.parent().ok_or_else(|| {
            AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not find parent directory of the current executable",
            ))
        })?;

        if exe_dir.file_name().and_then(|s| s.to_str()) == Some("deps") {
            if let Some(parent) = exe_dir.parent() {
                exe_dir = parent;
            }
        }

        let runner_exe_path = exe_dir.join("repx-runner");

        if !runner_exe_path.exists() {
            return Err(ClientError::Core(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "repx-runner executable not found at expected path: {}. Please ensure it is built and in the same directory as the TUI.",
                    runner_exe_path.display()
                ),
            ))));
        }
        let remote_bin_dir = self.base_path().join("bin");

        let mkdir_cmd = format!(
            "mkdir -p {}",
            shell_quote(&remote_bin_dir.to_string_lossy())
        );
        self.run_command("sh", &["-c", &mkdir_cmd])?;

        let remote_dest_path = remote_bin_dir.join("repx");

        let mut scp_cmd = Command::new("scp");
        scp_cmd.arg(&runner_exe_path).arg(format!(
            "{}:{}",
            self.address,
            remote_dest_path.display()
        ));
        logging::log_and_print_command(&scp_cmd);
        let scp_output = scp_cmd.output().map_err(AppError::from)?;

        if !scp_output.status.success() {
            let stderr = String::from_utf8_lossy(&scp_output.stderr);
            return Err(ClientError::Core(AppError::ExecutionFailed {
                message: format!("scp failed for repx binary to {}", self.address),
                log_path: None,
                log_summary: format!(
                    "scp exited with status {}. Stderr:\n{}",
                    scp_output.status, stderr
                ),
            }));
        }

        let chmod_cmd = format!(
            "chmod 755 {}",
            shell_quote(&remote_dest_path.to_string_lossy())
        );
        self.run_command("sh", &["-c", &chmod_cmd])?;

        Ok(remote_dest_path)
    }
}

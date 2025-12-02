use super::Target;
use crate::error::{ClientError, Result};
use fs_err;
use repx_core::{error::AppError, model::JobId};
use std::{
    collections::HashSet,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};
use walkdir::WalkDir;

pub struct LocalTarget {
    pub(crate) name: String,
    pub(crate) config: repx_core::config::Target,
    pub(crate) local_tools_path: PathBuf,
}

impl LocalTarget {
    fn tool(&self, name: &str) -> PathBuf {
        let tool_path = self.local_tools_path.join(name);
        if tool_path.exists() {
            tool_path
        } else {
            PathBuf::from(name)
        }
    }
}

impl Target for LocalTarget {
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
        self.base_path()
            .join("outputs")
            .join(&job_id.0)
            .join("out")
            .to_string_lossy()
            .to_string()
    }

    fn run_command(&self, command: &str, args: &[&str]) -> Result<String> {
        let cmd_path = self.tool(command);
        let mut cmd = Command::new(cmd_path);
        cmd.args(args);
        repx_core::logging::log_and_print_command(&cmd);
        let output = cmd.output().map_err(|e| AppError::ProcessLaunchFailed {
            command_name: command.to_string(),
            source: e,
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::TargetCommandFailed {
                target: self.name.clone(),
                source: AppError::ExecutionFailed {
                    message: format!("Command '{}' failed on target '{}'", command, self.name),
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
        let artifacts_path = self.artifacts_base_path();
        let missing = artifacts
            .iter()
            .filter(|&p| !artifacts_path.join(p).exists())
            .cloned()
            .collect();
        Ok(missing)
    }

    fn sync_artifact(&self, local_path: &Path, relative_path: &Path) -> Result<()> {
        let dest_path = self.artifacts_base_path().join(relative_path);
        if let Some(parent) = dest_path.parent() {
            fs_err::create_dir_all(parent).map_err(AppError::from)?;
        }

        if local_path.is_dir() {
            for entry in WalkDir::new(local_path) {
                let entry = entry?;
                let path = entry.path();
                let relative_to_source = path.strip_prefix(local_path).unwrap();
                let dest_entry_path = dest_path.join(relative_to_source);

                if path.is_dir() {
                    fs_err::create_dir_all(&dest_entry_path).map_err(AppError::from)?;
                } else {
                    fs_err::copy(path, &dest_entry_path).map_err(AppError::from)?;

                    let source_meta = fs_err::metadata(path).map_err(AppError::from)?;
                    let is_executable = (source_meta.mode() & 0o111) != 0;

                    let perms = if is_executable {
                        PermissionsExt::from_mode(0o555)
                    } else {
                        PermissionsExt::from_mode(0o444)
                    };

                    fs_err::set_permissions(&dest_entry_path, perms).map_err(AppError::from)?;
                }
            }
            for entry in WalkDir::new(&dest_path) {
                let entry = entry?;
                if entry.file_type().is_dir() {
                    fs_err::set_permissions(entry.path(), PermissionsExt::from_mode(0o555))
                        .map_err(AppError::from)?;
                }
            }
        } else {
            fs_err::copy(local_path, &dest_path).map_err(AppError::from)?;
            let source_meta = fs_err::metadata(local_path).map_err(AppError::from)?;
            let is_executable = (source_meta.mode() & 0o111) != 0;
            let perms = if is_executable {
                PermissionsExt::from_mode(0o555)
            } else {
                PermissionsExt::from_mode(0o444)
            };
            fs_err::set_permissions(&dest_path, perms).map_err(AppError::from)?;
        }
        Ok(())
    }

    fn write_remote_file(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent).map_err(AppError::from)?;
        }
        fs_err::write(path, content).map_err(AppError::from)?;
        Ok(())
    }
    fn sync_directory(&self, local_path: &Path, remote_path: &Path) -> Result<()> {
        fs_err::create_dir_all(remote_path).map_err(AppError::from)?;

        let mut rsync_cmd = Command::new(self.tool("rsync"));
        rsync_cmd
            .arg("-rltp")
            .arg(format!("{}/", local_path.display()))
            .arg(remote_path);

        repx_core::logging::log_and_print_command(&rsync_cmd);
        let rsync_output = rsync_cmd.output().map_err(AppError::from)?;

        if !rsync_output.status.success() {
            let stderr = String::from_utf8_lossy(&rsync_output.stderr);
            return Err(ClientError::Core(AppError::ExecutionFailed {
                message: "rsync failed for directory sync on local target".to_string(),
                log_path: None,
                log_summary: format!(
                    "rsync exited with status {}. Stderr:\n{}",
                    rsync_output.status, stderr
                ),
            }));
        }
        Ok(())
    }
    fn spawn_repx_job(
        &self,
        repx_binary_path: &Path,
        args: &[String],
    ) -> Result<std::process::Child> {
        let mut cmd = Command::new(repx_binary_path);
        cmd.args(args);
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        repx_core::logging::log_and_print_command(&cmd);

        cmd.spawn().map_err(|e| {
            ClientError::Core(AppError::ProcessLaunchFailed {
                command_name: repx_binary_path.to_string_lossy().to_string(),
                source: e,
            })
        })
    }
    fn read_remote_file_tail(&self, path: &Path, line_count: u32) -> Result<Vec<String>> {
        if !path.exists() {
            return Ok(vec![]);
        }
        let mut cmd = Command::new(self.tool("tail"));
        cmd.arg("-n").arg(line_count.to_string()).arg(path);
        repx_core::logging::log_and_print_command(&cmd);
        let output = cmd.output().map_err(|e| AppError::ProcessLaunchFailed {
            command_name: "tail".to_string(),
            source: e,
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such file or directory") {
                return Ok(vec![]);
            }
            return Err(ClientError::TargetCommandFailed {
                target: self.name.clone(),
                source: AppError::ExecutionFailed {
                    message: format!("Command 'tail' failed on target '{}'", self.name),
                    log_path: None,
                    log_summary: stderr.to_string(),
                },
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect())
    }

    fn sync_lab_root(&self, local_lab_path: &Path) -> Result<()> {
        let dest_path = self.artifacts_base_path();
        fs_err::create_dir_all(&dest_path).map_err(AppError::from)?;

        let mut rsync_cmd = Command::new(self.tool("rsync"));
        rsync_cmd
            .arg("-rltp")
            .arg(format!("{}/", local_lab_path.display()))
            .arg(&dest_path);

        repx_core::logging::log_and_print_command(&rsync_cmd);
        let rsync_output = rsync_cmd.output().map_err(AppError::from)?;

        if !rsync_output.status.success() {
            let stderr = String::from_utf8_lossy(&rsync_output.stderr);
            return Err(ClientError::Core(AppError::ExecutionFailed {
                message: "rsync failed for full lab sync on local target".to_string(),
                log_path: None,
                log_summary: format!(
                    "rsync exited with status {}. Stderr:\n{}",
                    rsync_output.status, stderr
                ),
            }));
        }
        Ok(())
    }
    fn deploy_repx_binary(&self) -> Result<PathBuf> {
        let runner_exe_path = super::find_local_runner_binary()?;
        let hash = super::compute_file_hash(&runner_exe_path)?;

        let versioned_bin_dir = self.base_path().join("bin").join(&hash);
        let dest_path = versioned_bin_dir.join("repx-runner");

        if dest_path.exists() {
            return Ok(dest_path);
        }

        fs_err::create_dir_all(&versioned_bin_dir).map_err(AppError::from)?;
        fs_err::copy(&runner_exe_path, &dest_path).map_err(AppError::from)?;
        let perms = std::os::unix::fs::PermissionsExt::from_mode(0o755);
        fs_err::set_permissions(&dest_path, perms).map_err(AppError::from)?;

        Ok(dest_path)
    }

    fn register_gc_root(&self, project_id: &str, lab_hash: &str) -> Result<()> {
        let gcroots = self
            .base_path()
            .join("gcroots")
            .join("auto")
            .join(project_id);
        fs_err::create_dir_all(&gcroots).map_err(AppError::from)?;

        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let link_name = format!("{}_{}", timestamp, lab_hash);
        let link_path = gcroots.join(&link_name);

        let artifacts_base = self.artifacts_base_path();
        let lab_dir = artifacts_base.join("lab");
        let manifest_path = if lab_dir.exists() {
            fs_err::read_dir(&lab_dir)
                .map_err(AppError::from)?
                .filter_map(std::result::Result::ok)
                .find(|e| {
                    e.file_name().to_string_lossy().contains(lab_hash)
                        && e.file_name()
                            .to_string_lossy()
                            .ends_with("-lab-metadata.json")
                })
                .map(|e| e.path())
        } else {
            None
        };

        let target_path = manifest_path.unwrap_or_else(|| artifacts_base.join(lab_hash));

        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(&target_path, &link_path);
        }

        let mut entries = fs_err::read_dir(&gcroots)
            .map_err(AppError::from)?
            .filter_map(|e| e.ok())
            .collect::<Vec<_>>();

        entries.sort_by_key(|e| e.file_name());

        if entries.len() > 5 {
            let to_remove = entries.len() - 5;
            for entry in entries.iter().take(to_remove) {
                let _ = fs_err::remove_file(entry.path());
            }
        }

        Ok(())
    }

    fn garbage_collect(&self) -> Result<String> {
        let repx_bin = self.deploy_repx_binary()?;
        let args = vec![
            "internal-gc".to_string(),
            "--base-path".to_string(),
            self.base_path().to_string_lossy().to_string(),
        ];

        let output = std::process::Command::new(repx_bin)
            .args(args)
            .output()
            .map_err(AppError::from)?;

        if !output.status.success() {
            return Err(ClientError::Core(AppError::ExecutionFailed {
                message: "GC failed".into(),
                log_path: None,
                log_summary: String::from_utf8_lossy(&output.stderr).to_string(),
            }));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

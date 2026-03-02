use super::{ArtifactSync, CommandRunner, FileOps, GcOps, JobRunner, SlurmOps, TargetInfo};
use crate::error::{ClientError, Result};
use repx_core::{
    config,
    constants::{dirs, markers},
    errors::ConfigError,
    model::JobId,
};
use std::{
    collections::HashSet,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};
use walkdir::WalkDir;

pub struct LocalTarget {
    pub(crate) name: String,
    pub(crate) config: config::Target,
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

impl TargetInfo for LocalTarget {
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
        self.base_path()
            .join(dirs::OUTPUTS)
            .join(&job_id.0)
            .join(dirs::OUT)
            .to_string_lossy()
            .to_string()
    }
}

impl CommandRunner for LocalTarget {
    fn run_command(&self, command: &str, args: &[&str]) -> Result<String> {
        let cmd_path = self.tool(command);
        let mut cmd = Command::new(&cmd_path);
        cmd.args(args);

        repx_core::logging::log_and_print_command(&cmd);

        let output = cmd
            .output()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::TargetCommandFailed {
                target: self.name.clone(),
                source: ConfigError::General(format!(
                    "Command '{}' failed on target '{}': {}",
                    command, self.name, stderr
                )),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

impl SlurmOps for LocalTarget {
    fn scancel(&self, slurm_id: u32) -> Result<()> {
        self.run_command("scancel", &[&slurm_id.to_string()])?;
        Ok(())
    }
}

impl ArtifactSync for LocalTarget {
    fn get_missing_artifacts(&self, artifacts: &HashSet<PathBuf>) -> Result<HashSet<PathBuf>> {
        let artifacts_path = self.artifacts_base_path();
        let missing = artifacts
            .iter()
            .filter(|p| !artifacts_path.join(p).exists())
            .cloned()
            .collect();
        Ok(missing)
    }

    fn sync_artifact(&self, local_path: &Path, relative_path: &Path) -> Result<()> {
        let dest_path = self.artifacts_base_path().join(relative_path);

        if let Some(parent) = dest_path.parent() {
            fs_err::create_dir_all(parent).map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        }

        if local_path.is_dir() {
            self.copy_directory_with_permissions(local_path, &dest_path)?;
        } else {
            self.copy_file_with_permissions(local_path, &dest_path)?;
        }

        Ok(())
    }

    fn sync_lab_root(&self, local_lab_path: &Path) -> Result<()> {
        let dest_path = self.artifacts_base_path();
        fs_err::create_dir_all(&dest_path).map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        let mut cmd = Command::new(self.tool("rsync"));
        cmd.arg("-rltp")
            .arg(format!("{}/", local_lab_path.display()))
            .arg(&dest_path);

        repx_core::logging::log_and_print_command(&cmd);
        let output = cmd
            .output()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::Config(ConfigError::General(format!(
                "rsync failed for lab sync: {}",
                stderr
            ))));
        }

        Ok(())
    }

    fn sync_directory(&self, local_path: &Path, remote_path: &Path) -> Result<()> {
        fs_err::create_dir_all(remote_path).map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        let mut cmd = Command::new(self.tool("rsync"));
        cmd.arg("-rltp")
            .arg(format!("{}/", local_path.display()))
            .arg(remote_path);

        repx_core::logging::log_and_print_command(&cmd);
        let output = cmd
            .output()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClientError::Config(ConfigError::General(format!(
                "rsync failed for directory sync: {}",
                stderr
            ))));
        }

        Ok(())
    }

    fn sync_image_incrementally(
        &self,
        image_path: &Path,
        image_tag: &str,
        local_cache_root: &Path,
    ) -> Result<()> {
        let dest_images_dir = self.base_path().join("images");
        let dest_store_dir = self.base_path().join("store");
        fs_err::create_dir_all(&dest_images_dir)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        fs_err::create_dir_all(&dest_store_dir)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        let dest_image_path = dest_images_dir.join(image_tag);

        let store_cache = local_cache_root.join("store");
        let images_cache = local_cache_root.join("images");
        fs_err::create_dir_all(&store_cache)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        fs_err::create_dir_all(&images_cache)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if image_path.is_dir() {
            let final_source = image_path.to_path_buf();
            if dest_image_path.exists() || dest_image_path.is_symlink() {
                if dest_image_path.is_dir() && !dest_image_path.is_symlink() {
                    let _ = fs_err::remove_dir_all(&dest_image_path);
                } else {
                    let _ = fs_err::remove_file(&dest_image_path);
                }
            }
            std::os::unix::fs::symlink(&final_source, &dest_image_path)
                .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
            return Ok(());
        }

        let tar_tool = self.tool("tar");
        let layers = super::common::get_image_manifest(image_path, &tar_tool)?;

        let image_filename = image_path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| ClientError::InvalidPath {
                path: image_path.to_path_buf(),
                reason: "Image path has no filename".to_string(),
            })?;
        let image_hash_name = super::common::parse_image_hash(image_filename);
        let image_cache_dir = images_cache.join(image_hash_name);

        fs_err::create_dir_all(&image_cache_dir)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        let manifest_content = serde_json::to_string(&vec![super::common::ManifestEntry {
            layers: layers.clone(),
        }])
        .map_err(|e| ClientError::Config(ConfigError::Json(e)))?;
        fs_err::write(image_cache_dir.join("manifest.json"), manifest_content)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        for layer in &layers {
            let layer_hash = Path::new(layer)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or(layer);

            super::common::extract_layer_to_flat_store(
                image_path,
                layer,
                layer_hash,
                &store_cache,
                &tar_tool,
            )?;

            let image_layer_dir = image_cache_dir.join(layer_hash);
            fs_err::create_dir_all(&image_layer_dir)
                .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

            let flat_layer_name = format!("{}-layer.tar", layer_hash);
            let target_layer_tar = store_cache.join(&flat_layer_name);
            let link_path = image_layer_dir.join("layer.tar");

            if link_path.exists() || link_path.is_symlink() {
                let _ = fs_err::remove_file(&link_path);
            }
            std::os::unix::fs::symlink(&target_layer_tar, &link_path)
                .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        }

        let final_source = image_cache_dir;

        if dest_image_path.exists() || dest_image_path.is_symlink() {
            if dest_image_path.is_dir() && !dest_image_path.is_symlink() {
                let _ = fs_err::remove_dir_all(&dest_image_path);
            } else {
                let _ = fs_err::remove_file(&dest_image_path);
            }
        }

        std::os::unix::fs::symlink(&final_source, &dest_image_path)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        Ok(())
    }
}

impl FileOps for LocalTarget {
    fn write_remote_file(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent).map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        }
        fs_err::write(path, content).map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        Ok(())
    }

    fn read_remote_file(&self, path: &Path) -> Result<String> {
        fs_err::read_to_string(path).map_err(|e| ClientError::Config(ConfigError::Io(e)))
    }

    fn read_remote_file_tail(&self, path: &Path, line_count: u32) -> Result<Vec<String>> {
        if !path.exists() {
            return Ok(vec![]);
        }

        let mut cmd = Command::new(self.tool("tail"));
        cmd.arg("-n").arg(line_count.to_string()).arg(path);

        repx_core::logging::log_and_print_command(&cmd);
        let output = cmd
            .output()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such file or directory") {
                return Ok(vec![]);
            }
            return Err(ClientError::TargetCommandFailed {
                target: self.name.clone(),
                source: ConfigError::General(format!(
                    "tail failed on '{}': {}",
                    path.display(),
                    stderr
                )),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(String::from)
            .collect())
    }
}

impl JobRunner for LocalTarget {
    fn deploy_repx_binary(&self) -> Result<PathBuf> {
        let runner_exe_path = super::find_local_runner_binary()?;
        let hash = super::compute_file_hash(&runner_exe_path)?;

        let versioned_bin_dir = self.base_path().join("bin").join(&hash);
        let dest_path = versioned_bin_dir.join("repx");

        if dest_path.exists() {
            return Ok(dest_path);
        }

        fs_err::create_dir_all(&versioned_bin_dir)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        fs_err::copy(&runner_exe_path, &dest_path)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        fs_err::set_permissions(&dest_path, PermissionsExt::from_mode(0o755))
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        Ok(dest_path)
    }

    fn spawn_repx_job(
        &self,
        repx_binary_path: &Path,
        args: &[String],
    ) -> Result<std::process::Child> {
        let mut cmd = Command::new(repx_binary_path);
        cmd.args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        repx_core::logging::log_and_print_command(&cmd);

        cmd.spawn()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))
    }

    fn check_outcome_markers(
        &self,
    ) -> Result<std::collections::HashMap<JobId, repx_core::engine::JobStatus>> {
        let outputs_path = self.base_path().join(dirs::OUTPUTS);
        let mut outcomes = std::collections::HashMap::new();

        if !outputs_path.exists() {
            return Ok(outcomes);
        }

        for entry in WalkDir::new(&outputs_path)
            .min_depth(3)
            .max_depth(3)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if file_name != markers::SUCCESS && file_name != markers::FAIL {
                continue;
            }

            if let Some(repx_dir) = path.parent() {
                if repx_dir.file_name().and_then(|s| s.to_str()) != Some(dirs::REPX) {
                    continue;
                }

                if let Some(job_dir) = repx_dir.parent() {
                    let job_id_str = job_dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    let job_id = JobId(job_id_str.to_string());
                    let location = self.name().to_string();

                    let status = if file_name == markers::SUCCESS {
                        repx_core::engine::JobStatus::Succeeded { location }
                    } else {
                        repx_core::engine::JobStatus::Failed { location }
                    };
                    outcomes.insert(job_id, status);
                }
            }
        }

        Ok(outcomes)
    }
}

impl GcOps for LocalTarget {
    fn register_gc_root(&self, project_id: &str, lab_hash: &str) -> Result<()> {
        let gcroots = self
            .base_path()
            .join(repx_core::constants::dirs::GCROOTS)
            .join("auto")
            .join(project_id);
        fs_err::create_dir_all(&gcroots).map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        let link_name = super::common::generate_gc_link_name(lab_hash);
        let link_path = gcroots.join(&link_name);

        let target_path = self
            .find_lab_manifest(lab_hash)
            .unwrap_or_else(|_| self.artifacts_base_path().join(lab_hash));

        let _ = std::os::unix::fs::symlink(&target_path, &link_path);

        self.cleanup_old_gc_roots(&gcroots, 5)?;

        Ok(())
    }

    fn pin_gc_root(&self, lab_hash: &str, name: &str) -> Result<()> {
        let pinned_dir = self
            .base_path()
            .join(repx_core::constants::dirs::GCROOTS)
            .join("pinned");
        fs_err::create_dir_all(&pinned_dir).map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        let link_path = pinned_dir.join(name);
        if link_path.exists() || link_path.symlink_metadata().is_ok() {
            let _ = fs_err::remove_file(&link_path);
        }

        let target_path = self.find_lab_manifest(lab_hash)?;
        std::os::unix::fs::symlink(&target_path, &link_path)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        Ok(())
    }

    fn unpin_gc_root(&self, name: &str) -> Result<()> {
        let link_path = self
            .base_path()
            .join(repx_core::constants::dirs::GCROOTS)
            .join("pinned")
            .join(name);

        if !link_path.exists() && link_path.symlink_metadata().is_err() {
            return Err(ClientError::Config(ConfigError::General(format!(
                "No pinned GC root named '{}'",
                name
            ))));
        }

        fs_err::remove_file(&link_path).map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        Ok(())
    }

    fn list_gc_roots(&self) -> Result<Vec<super::GcRootEntry>> {
        let gcroots_dir = self.base_path().join(repx_core::constants::dirs::GCROOTS);
        let mut entries = Vec::new();

        let pinned_dir = gcroots_dir.join("pinned");
        if pinned_dir.exists() {
            for entry in fs_err::read_dir(&pinned_dir)
                .map_err(|e| ClientError::Config(ConfigError::Io(e)))?
            {
                let entry = entry.map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
                let target_path = std::fs::read_link(entry.path())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "???".to_string());
                entries.push(super::GcRootEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    kind: super::GcRootKind::Pinned,
                    target_path,
                    project_id: None,
                });
            }
        }

        let auto_dir = gcroots_dir.join("auto");
        if auto_dir.exists() {
            for project_entry in
                fs_err::read_dir(&auto_dir).map_err(|e| ClientError::Config(ConfigError::Io(e)))?
            {
                let project_entry =
                    project_entry.map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
                if !project_entry.path().is_dir() {
                    continue;
                }
                let project_id = project_entry.file_name().to_string_lossy().to_string();
                for link_entry in fs_err::read_dir(project_entry.path())
                    .map_err(|e| ClientError::Config(ConfigError::Io(e)))?
                {
                    let link_entry =
                        link_entry.map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
                    let target_path = std::fs::read_link(link_entry.path())
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "???".to_string());
                    entries.push(super::GcRootEntry {
                        name: link_entry.file_name().to_string_lossy().to_string(),
                        kind: super::GcRootKind::Auto,
                        target_path,
                        project_id: Some(project_id.clone()),
                    });
                }
            }
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn garbage_collect(&self) -> Result<String> {
        let repx_bin = self.deploy_repx_binary()?;

        let output = Command::new(&repx_bin)
            .arg("internal-gc")
            .arg("--base-path")
            .arg(self.base_path())
            .output()
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        if !output.status.success() {
            return Err(ClientError::Config(ConfigError::General(format!(
                "GC failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

impl LocalTarget {
    fn copy_file_with_permissions(&self, src: &Path, dest: &Path) -> Result<()> {
        fs_err::copy(src, dest).map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        let meta = fs_err::metadata(src).map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        let is_executable = (meta.mode() & 0o111) != 0;
        let perms = if is_executable {
            PermissionsExt::from_mode(0o555)
        } else {
            PermissionsExt::from_mode(0o444)
        };
        fs_err::set_permissions(dest, perms)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        Ok(())
    }

    fn copy_directory_with_permissions(&self, src: &Path, dest: &Path) -> Result<()> {
        for entry in WalkDir::new(src) {
            let entry = entry?;
            let path = entry.path();
            let relative = path.strip_prefix(src).unwrap();
            let dest_path = dest.join(relative);

            if path.is_dir() {
                fs_err::create_dir_all(&dest_path)
                    .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
            } else {
                self.copy_file_with_permissions(path, &dest_path)?;
            }
        }

        for entry in WalkDir::new(dest) {
            let entry = entry?;
            if entry.file_type().is_dir() {
                fs_err::set_permissions(entry.path(), PermissionsExt::from_mode(0o555))
                    .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
            }
        }

        Ok(())
    }

    fn find_lab_manifest(&self, lab_hash: &str) -> Result<PathBuf> {
        let artifacts_base = self.artifacts_base_path();
        let lab_dir = artifacts_base.join("lab");

        if lab_dir.exists() {
            if let Some(entry) = fs_err::read_dir(&lab_dir)
                .map_err(|e| ClientError::Config(ConfigError::Io(e)))?
                .filter_map(|e| e.ok())
                .find(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    name.contains(lab_hash) && name.ends_with("-lab-metadata.json")
                })
            {
                return Ok(entry.path());
            }

            for entry in fs_err::read_dir(&lab_dir)
                .map_err(|e| ClientError::Config(ConfigError::Io(e)))?
                .filter_map(|e| e.ok())
            {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.ends_with("-lab-metadata.json") {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        if json.get("labId").and_then(|v| v.as_str()) == Some(lab_hash) {
                            return Ok(entry.path());
                        }
                    }
                }
            }
        }

        let fallback = artifacts_base.join(lab_hash);
        if fallback.exists() {
            return Ok(fallback);
        }

        Err(ClientError::Config(ConfigError::General(format!(
            "No lab manifest found for hash '{}'",
            lab_hash
        ))))
    }

    fn cleanup_old_gc_roots(&self, gcroots_dir: &Path, keep: usize) -> Result<()> {
        let mut entries: Vec<_> = fs_err::read_dir(gcroots_dir)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?
            .filter_map(|e| e.ok())
            .collect();

        entries.sort_by_key(|e| e.file_name());

        if entries.len() > keep {
            for entry in entries.iter().take(entries.len() - keep) {
                let _ = fs_err::remove_file(entry.path());
            }
        }

        Ok(())
    }
}

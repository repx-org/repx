use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct TestContext {
    pub _temp_dir: tempfile::TempDir,
    pub test_root: PathBuf,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub lab_path: PathBuf,
    pub metadata: Value,
}

impl TestContext {
    pub fn new() -> Self {
        Self::with_execution_type("native")
    }
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TestContext {
    pub fn with_execution_type(exec_type: &str) -> Self {
        Self::with_execution_type_and_lab(exec_type, "REFERENCE_LAB_PATH")
    }

    pub fn with_execution_type_and_lab(exec_type: &str, lab_env_var: &str) -> Self {
        let temp_dir = tempfile::Builder::new()
            .prefix("repx-test-")
            .tempdir()
            .expect("Failed to create temp dir");
        let test_root = temp_dir.path().to_path_buf();

        let config_dir = test_root.join("config");
        let cache_dir = test_root.join("cache");
        fs::create_dir_all(&config_dir).expect("Failed to create config dir");
        fs::create_dir_all(&cache_dir).expect("Failed to create cache dir");

        let repx_config_subdir = config_dir.join("repx");
        fs::create_dir(&repx_config_subdir).expect("Failed to create repx config subdir");

        let config_content = format!(
            r#"
submission_target = "local"
[targets.local]
base_path = "{}"
default_scheduler = "local"
default_execution_type = "{}"

[targets.local.local]
execution_types = ["native", "bwrap"]
local_concurrency = 2
"#,
            cache_dir.display(),
            exec_type
        );
        fs::write(repx_config_subdir.join("config.toml"), config_content)
            .expect("Failed to write temp config");

        let lab_path = PathBuf::from(
            env::var(lab_env_var).unwrap_or_else(|_| panic!("{} must be set", lab_env_var)),
        );
        assert!(
            lab_path.exists(),
            "{} path does not exist: {}",
            lab_env_var,
            lab_path.display()
        );

        let metadata = Self::load_metadata(&lab_path);

        let harness = Self {
            _temp_dir: temp_dir,
            test_root,
            config_dir,
            cache_dir,
            lab_path,
            metadata,
        };
        harness.stage_lab();
        harness
    }

    pub fn stage_lab(&self) {
        let dest = self.cache_dir.join("artifacts");
        fs::create_dir_all(&dest).unwrap();

        let host_tools_dir = self.get_host_tools_dir_name();
        let bin_dir = self
            .lab_path
            .join("host-tools")
            .join(host_tools_dir)
            .join("bin");

        let rsync_path = bin_dir.join("rsync");
        let output = Command::new(&rsync_path)
            .arg("-a")
            .arg("--no-o")
            .arg("--no-g")
            .arg("--delete")
            .arg(format!("{}/", self.lab_path.display()))
            .arg(&dest)
            .output()
            .expect("rsync command failed");

        if !output.status.success() {
            eprintln!("Rsync failed using path: {:?}", rsync_path);
            eprintln!("Rsync stderr: {}", String::from_utf8_lossy(&output.stderr));
            eprintln!("Rsync stdout: {}", String::from_utf8_lossy(&output.stdout));
        }
        assert!(output.status.success(), "rsync of lab to cache failed");

        let chmod_path = bin_dir.join("chmod");
        let status_chmod = Command::new(chmod_path)
            .arg("-R")
            .arg("u+w")
            .arg(&dest)
            .status()
            .expect("chmod command failed");
        assert!(status_chmod.success(), "chmod of artifacts failed");

        let images_store = self.cache_dir.join("images_store");
        fs::create_dir_all(&images_store).expect("Failed to create images_store");

        let mut image_mapping = std::collections::HashMap::new();

        let image_dirs = ["image", "images"];
        for img_dir_name in image_dirs {
            let img_dir = dest.join(img_dir_name);
            if img_dir.exists() {
                let entries: Vec<_> = fs::read_dir(&img_dir)
                    .expect("Failed to read image dir")
                    .filter_map(Result::ok)
                    .collect();

                for entry in entries {
                    let path = entry.path();
                    if path.is_file() && path.extension().is_some_and(|e| e == "tar") {
                        let file_stem = path
                            .file_stem()
                            .expect("No file stem")
                            .to_string_lossy()
                            .to_string();
                        let extract_dir = images_store.join(&file_stem);

                        if !extract_dir.exists() {
                            fs::create_dir(&extract_dir).expect("Failed to create extract dir");

                            let tar_bin = bin_dir.join("tar");
                            let tar_cmd_path = if tar_bin.exists() {
                                tar_bin
                            } else {
                                PathBuf::from("tar")
                            };

                            let status = Command::new(tar_cmd_path)
                                .arg("-xf")
                                .arg(&path)
                                .arg("-C")
                                .arg(&extract_dir)
                                .status()
                                .expect("Failed to execute tar extraction");

                            assert!(
                                status.success(),
                                "Failed to extract image tarball: {:?}",
                                path
                            );
                        }

                        let rel_path = path.strip_prefix(&dest).unwrap().to_path_buf();
                        image_mapping.insert(rel_path, extract_dir);
                    }
                }
            }
        }

        let base_images_dir = self.cache_dir.join("images");
        fs::create_dir_all(&base_images_dir).expect("Failed to create base_path/images");

        for (rel_path, extract_path) in &image_mapping {
            if let Some(stem) = rel_path.file_stem().and_then(|s| s.to_str()) {
                let symlink_path = base_images_dir.join(stem);
                let _ = fs::remove_file(&symlink_path);
                let _ = fs::remove_dir_all(&symlink_path);

                std::os::unix::fs::symlink(extract_path, &symlink_path)
                    .expect("Failed to create symlink");
            }
        }
    }
    pub fn stage_job_dirs(&self, job_id: &str) {
        let job_out_path = self.get_job_output_path(job_id);
        fs::create_dir_all(job_out_path.join("out")).unwrap();
        fs::create_dir_all(job_out_path.join("repx")).unwrap();
    }

    pub fn get_job_id_by_name(&self, name_substring: &str) -> String {
        let jobs = self.metadata["jobs"]
            .as_object()
            .expect("metadata.json has no 'jobs' object");

        let (job_id, _) = jobs
            .iter()
            .find(|(id, job_data)| {
                id.contains(name_substring)
                    || job_data["name"]
                        .as_str()
                        .unwrap_or("")
                        .contains(name_substring)
            })
            .unwrap_or_else(|| {
                panic!(
                    "Could not find job with name/id containing '{}'",
                    name_substring
                )
            });
        job_id.clone()
    }

    pub fn get_job_package_path(&self, job_id: &str) -> PathBuf {
        let path_in_lab = PathBuf::from("jobs").join(job_id);
        self.cache_dir.join("artifacts").join(path_in_lab)
    }

    pub fn get_job_output_path(&self, job_id: &str) -> PathBuf {
        self.cache_dir.join("outputs").join(job_id)
    }

    fn load_metadata(lab_path: &Path) -> Value {
        let lab_subdir = lab_path.join("lab");
        let entries = fs::read_dir(&lab_subdir).expect("Could not read lab/ subdirectory");

        let manifest_path = entries
            .filter_map(|e| e.ok())
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .ends_with("lab-metadata.json")
            })
            .map(|e| e.path())
            .expect("Could not find *-lab-metadata.json in lab/");

        let manifest_content = fs::read_to_string(&manifest_path).expect("Failed to read manifest");
        let manifest: Value =
            serde_json::from_str(&manifest_content).expect("Failed to parse manifest");

        let root_meta_rel_path = manifest["metadata"]
            .as_str()
            .expect("Manifest missing metadata path");
        let root_meta_path = lab_path.join(root_meta_rel_path);

        let root_content = fs::read_to_string(&root_meta_path).unwrap_or_else(|e| {
            panic!(
                "Could not read root metadata at '{}': {}",
                root_meta_path.display(),
                e
            )
        });
        let root_meta: Value =
            serde_json::from_str(&root_content).expect("Could not parse root metadata");
        let mut all_jobs = serde_json::Map::new();
        let mut all_runs = serde_json::Map::new();
        let mut combined_metadata = root_meta
            .as_object()
            .expect("Root metadata is not a JSON object")
            .clone();

        if let Some(run_paths) = root_meta.get("runs").and_then(|r| r.as_array()) {
            for run_path_val in run_paths {
                if let Some(run_rel_path) = run_path_val.as_str() {
                    let run_meta_path = lab_path.join(run_rel_path);
                    let run_content =
                        fs::read_to_string(&run_meta_path).expect("Could not read run metadata");
                    let run_meta: Value =
                        serde_json::from_str(&run_content).expect("Could not parse run metadata");

                    if let Some(name) = run_meta.get("name").and_then(|n| n.as_str()) {
                        all_runs.insert(name.to_string(), run_meta.clone());
                    }

                    if let Some(jobs) = run_meta.get("jobs").and_then(|j| j.as_object()) {
                        all_jobs.extend(jobs.clone());
                    }
                }
            }
        }

        combined_metadata.remove("runs");
        combined_metadata.insert("jobs".to_string(), Value::Object(all_jobs));
        combined_metadata.insert("runs_data".to_string(), Value::Object(all_runs));
        Value::Object(combined_metadata)
    }
    pub fn get_lab_content_hash(&self) -> String {
        let lab_subdir = self.cache_dir.join("artifacts").join("lab");
        let manifest_path = fs::read_dir(&lab_subdir)
            .expect("Could not read lab/ subdirectory in artifacts")
            .filter_map(|e| e.ok())
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .ends_with("-lab-metadata.json")
            })
            .map(|e| e.path())
            .expect("Could not find *-lab-metadata.json in artifacts/lab/");

        let content = fs::read_to_string(&manifest_path).expect("Failed to read manifest");
        let manifest: serde_json::Value =
            serde_json::from_str(&content).expect("Failed to parse manifest");
        manifest["labId"]
            .as_str()
            .expect("Manifest missing labId field")
            .to_string()
    }

    pub fn get_staged_executable_path(&self, job_id: &str) -> PathBuf {
        let job_data = &self.metadata["jobs"][job_id];
        let path_in_lab_str = job_data["executables"]["main"]["path"]
            .as_str()
            .expect("Job has no main executable path in metadata");
        self.cache_dir.join("artifacts").join(path_in_lab_str)
    }
    pub fn get_host_tools_dir_name(&self) -> String {
        let host_tools_path = self.lab_path.join("host-tools");
        let entry = fs::read_dir(host_tools_path)
            .expect("Could not read host-tools dir")
            .filter_map(Result::ok)
            .find(|e| e.path().is_dir())
            .expect("No directory found in host-tools");
        entry.file_name().to_string_lossy().to_string()
    }

    pub fn get_any_image_tag(&self) -> Option<String> {
        self.metadata
            .get("runs_data")?
            .as_object()?
            .values()
            .find_map(|run| {
                let path_str = run.get("image")?.as_str()?;
                let path = Path::new(path_str);
                let file_name = path.file_name()?.to_str()?;

                if let Some(stem) = file_name.strip_suffix(".tar.gz") {
                    Some(stem.to_string())
                } else if let Some(stem) = file_name.strip_suffix(".tar") {
                    Some(stem.to_string())
                } else if let Some(stem) = file_name.strip_suffix(".gz") {
                    Some(stem.to_string())
                } else {
                    Some(file_name.to_string())
                }
            })
    }
}

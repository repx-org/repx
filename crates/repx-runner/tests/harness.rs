use assert_cmd::Command as AssertCommand;
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::{tempdir, TempDir};

pub struct TestHarness {
    _config_dir: TempDir,
    pub cache_dir: TempDir,
    pub lab_path: PathBuf,
    pub metadata: Value,
}

impl TestHarness {
    pub fn new() -> Self {
        let config_dir = tempdir().expect("Failed to create temp config dir");
        let cache_dir = tempdir().expect("Failed to create temp cache dir");

        let repx_config_subdir = config_dir.path().join("repx");
        fs::create_dir(&repx_config_subdir).expect("Failed to create repx config subdir");

        let config_content = format!(
            r#"
submission_target = "local"
[targets.local]
base_path = "{}"
default_scheduler = "local"
default_execution_type = "native"

[targets.local.local]
execution_types = ["native"]
local_concurrency = 2
"#,
            cache_dir.path().display()
        );
        fs::write(repx_config_subdir.join("config.toml"), config_content)
            .expect("Failed to write temp config");

        let lab_path =
            PathBuf::from(env::var("EXAMPLE_REPX_LAB").expect("EXAMPLE_REPX_LAB must be set"));
        assert!(
            lab_path.exists(),
            "EXAMPLE_REPX_LAB path does not exist: {}",
            lab_path.display()
        );

        let metadata = Self::load_metadata(&lab_path);

        Self {
            _config_dir: config_dir,
            cache_dir,
            lab_path,
            metadata,
        }
    }

    pub fn cmd(&self) -> AssertCommand {
        let mut cmd = AssertCommand::new(env!("CARGO_BIN_EXE_repx-runner"));
        cmd.env("XDG_CONFIG_HOME", self._config_dir.path());
        cmd.env("RUST_BACKTRACE", "1");
        cmd.arg("--lab").arg(&self.lab_path);
        cmd.env("REPX_TEST_LOG_TEE", "1");
        cmd.env("REPX_LOG_LEVEL", "DEBUG");
        cmd
    }

    pub fn stage_lab(&self) {
        let dest = self.cache_dir.path().join("artifacts");
        fs::create_dir_all(&dest).unwrap();

        let status = Command::new("rsync")
            .arg("-a")
            .arg("--delete")
            .arg(format!("{}/", self.lab_path.display()))
            .arg(&dest)
            .status()
            .expect("rsync command failed");
        assert!(status.success(), "rsync of lab to cache failed");
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
        self.cache_dir.path().join("artifacts").join(path_in_lab)
    }

    pub fn get_job_output_path(&self, job_id: &str) -> PathBuf {
        self.cache_dir.path().join("outputs").join(job_id)
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

                    if let Some(jobs) = run_meta.get("jobs").and_then(|j| j.as_object()) {
                        all_jobs.extend(jobs.clone());
                    }
                }
            }
        }

        combined_metadata.remove("runs");
        combined_metadata.insert("jobs".to_string(), Value::Object(all_jobs));

        Value::Object(combined_metadata)
    }

    pub fn get_staged_executable_path(&self, job_id: &str) -> PathBuf {
        let job_data = &self.metadata["jobs"][job_id];
        let path_in_lab_str = job_data["executables"]["main"]["path"]
            .as_str()
            .expect("Job has no main executable path in metadata");
        self.cache_dir
            .path()
            .join("artifacts")
            .join(path_in_lab_str)
    }
}

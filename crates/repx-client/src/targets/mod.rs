use crate::error::Result;
use repx_core::{engine, model::JobId};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc::Sender,
};
use whoami;
pub mod local;
pub mod ssh;

pub(crate) fn find_local_runner_binary() -> Result<PathBuf> {
    use repx_core::error::AppError;
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
        return Err(crate::error::ClientError::Core(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "repx-runner executable not found at expected path: {}. Please ensure it is built and in the same directory as the TUI.",
                runner_exe_path.display()
            ),
        ))));
    }
    Ok(runner_exe_path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlurmState {
    Pending,
    Running,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct SlurmJobInfo {
    pub slurm_id: u32,
    pub repx_id: JobId,
    pub state: SlurmState,
}

pub trait Target: Send + Sync {
    fn name(&self) -> &str;
    fn base_path(&self) -> &Path;
    fn config(&self) -> &repx_core::config::Target;
    fn run_command(&self, command: &str, args: &[&str]) -> Result<String>;
    fn scancel(&self, slurm_id: u32) -> Result<()>;
    fn get_missing_artifacts(&self, artifacts: &HashSet<PathBuf>) -> Result<HashSet<PathBuf>>;
    fn sync_artifact(&self, local_path: &Path, relative_path: &Path) -> Result<()>;
    fn sync_lab_root(&self, local_lab_path: &Path) -> Result<()>;
    fn write_remote_file(&self, path: &Path, content: &str) -> Result<()>;
    fn deploy_repx_binary(&self) -> Result<PathBuf>;
    fn sync_directory(&self, local_path: &Path, remote_path: &Path) -> Result<()>;
    fn read_remote_file_tail(&self, path: &Path, line_count: u32) -> Result<Vec<String>>;

    fn sync_artifacts_batch(
        &self,
        local_lab_path: &Path,
        artifacts: &HashSet<PathBuf>,
        event_sender: Option<&Sender<super::ClientEvent>>,
    ) -> Result<()> {
        for relative_path in artifacts {
            if let Some(sender) = event_sender {
                let _ = sender.send(super::ClientEvent::SyncingArtifactProgress {
                    path: relative_path.clone(),
                });
            }
            let local_path = local_lab_path.join(relative_path);
            self.sync_artifact(&local_path, relative_path)?;
        }
        Ok(())
    }

    fn artifacts_base_path(&self) -> PathBuf {
        self.base_path().join("artifacts")
    }

    fn squeue(&self) -> Result<HashMap<JobId, SlurmJobInfo>> {
        let user = if self.config().address.is_some() {
            self.run_command("whoami", &[])?.trim().to_string()
        } else {
            whoami::username()
        };

        let squeue_command = format!("squeue -h -o '%i %j %t' -u '{}'", user);
        let output = self.run_command("sh", &["-c", &squeue_command])?;
        Ok(parse_squeue(&output))
    }

    fn check_outcome_markers(&self) -> Result<HashMap<JobId, engine::JobStatus>> {
        let outputs_path = self.base_path().join("outputs");
        let find_cmd = format!(
            "find {} -mindepth 3 -maxdepth 3 \\( -name SUCCESS -o -name FAIL \\) -path '*/repx/*'",
            outputs_path.display()
        );
        let output = self
            .run_command("sh", &["-c", &find_cmd])
            .unwrap_or_default();

        let mut outcomes = HashMap::new();
        for line in output.lines() {
            let path = Path::new(line);
            let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if let Some(repx_dir) = path.parent() {
                if let Some(job_dir) = repx_dir.parent() {
                    let job_id_str = job_dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    let job_id = JobId(job_id_str.to_string());
                    let location = self.name().to_string();

                    let status = if file_name == "SUCCESS" {
                        engine::JobStatus::Succeeded { location }
                    } else if file_name == "FAIL" {
                        engine::JobStatus::Failed { location }
                    } else {
                        continue;
                    };
                    outcomes.insert(job_id, status);
                }
            }
        }
        Ok(outcomes)
    }

    fn get_remote_path_str(&self, job_id: &JobId) -> String;
}

fn parse_squeue(output: &str) -> HashMap<JobId, SlurmJobInfo> {
    let mut jobs = HashMap::new();
    for line in output.lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        if let Ok(slurm_id) = parts[0].parse::<u32>() {
            let repx_id = JobId(parts[1].to_string());
            let state = match parts[2] {
                "PD" => SlurmState::Pending,
                "R" => SlurmState::Running,
                s => SlurmState::Other(s.to_string()),
            };
            jobs.insert(
                repx_id.clone(),
                SlurmJobInfo {
                    slurm_id,
                    repx_id,
                    state,
                },
            );
        }
    }
    jobs
}

#[cfg(test)]
mod tests {
    use super::*;
    use repx_core::model::JobId;

    #[test]
    fn test_parse_squeue_output() {
        let squeue_output = r#"
12345   job-one-running    R
12346   job-two-pending    PD
12347   job-three-other    CG
garbage line to ignore
12348   job-four-running   R
"#;
        let parsed = parse_squeue(squeue_output);
        assert_eq!(parsed.len(), 4);

        let job_one = parsed.get(&JobId("job-one-running".into())).unwrap();
        assert_eq!(job_one.slurm_id, 12345);
        assert_eq!(job_one.state, SlurmState::Running);

        let job_two = parsed.get(&JobId("job-two-pending".into())).unwrap();
        assert_eq!(job_two.slurm_id, 12346);
        assert_eq!(job_two.state, SlurmState::Pending);

        let job_three = parsed.get(&JobId("job-three-other".into())).unwrap();
        assert_eq!(job_three.slurm_id, 12347);
        assert_eq!(job_three.state, SlurmState::Other("CG".into()));
    }

    #[test]
    fn test_parse_squeue_empty_output() {
        let parsed = parse_squeue("");
        assert!(parsed.is_empty());
    }
}

use repx_core::model::{Job, JobId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobPlan {
    pub script_hash: String,
    pub dependencies: Vec<JobId>,
    #[serde(default)]
    pub job_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationPlan {
    pub submissions_dir: PathBuf,
    pub jobs: HashMap<JobId, JobPlan>,
}

impl OrchestrationPlan {
    pub fn new(base_path: &Path, lab_content_hash: &str) -> Self {
        Self {
            submissions_dir: base_path.join("submissions").join(lab_content_hash),
            jobs: HashMap::new(),
        }
    }

    pub fn add_job(
        &mut self,
        job_id: JobId,
        job_def: &Job,
        script_hash: String,
        batch_jobs: &HashSet<JobId>,
    ) {
        let entrypoint_exe = job_def
            .executables
            .get("main")
            .or_else(|| job_def.executables.get("scatter"))
            .unwrap();

        let dependencies = entrypoint_exe
            .inputs
            .iter()
            .filter_map(|m| m.job_id.clone())
            .filter(|dep_id| batch_jobs.contains(dep_id))
            .collect();

        self.jobs.insert(
            job_id,
            JobPlan {
                script_hash,
                dependencies,
                job_type: job_def.stage_type.clone(),
            },
        );
    }
}

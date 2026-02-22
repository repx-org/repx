use crate::error::Result;
use crate::targets::Target;
use repx_core::{
    engine,
    model::{Job, JobId, Lab, RunId, StageType},
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

pub fn generate_project_id(lab_path: &Path) -> String {
    let lab_path_abs = fs_err::canonicalize(lab_path).unwrap_or_else(|_| lab_path.to_path_buf());
    let abs_hash = format!(
        "{:x}",
        Sha256::digest(lab_path_abs.to_string_lossy().as_bytes())
    );

    let remote_hash = match Command::new("git")
        .arg("remote")
        .arg("get-url")
        .arg("origin")
        .current_dir(lab_path)
        .output()
    {
        Ok(output) if output.status.success() => {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            format!("{:x}", Sha256::digest(s.as_bytes()))
        }
        Ok(_) => "no_remote".to_string(),
        Err(_) => "no_git".to_string(),
    };

    format!("{}_{}", remote_hash, abs_hash)
}

pub fn resolve_dependency_graph(lab: &Lab, run_specs: &[String]) -> Result<HashSet<JobId>> {
    let mut full_dependency_set = HashSet::new();

    let expanded_run_ids: Vec<RunId> = run_specs
        .iter()
        .map(|spec| repx_core::resolver::resolve_run_spec(lab, spec))
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();

    for run_id in &expanded_run_ids {
        let final_job_ids = repx_core::resolver::resolve_all_final_job_ids(lab, run_id)?;
        for final_job_id in final_job_ids {
            let graph = engine::build_dependency_graph(lab, final_job_id);
            full_dependency_set.extend(graph);
        }
    }

    Ok(full_dependency_set)
}

pub fn filter_jobs_to_run<'a>(
    lab: &'a Lab,
    dependency_set: &HashSet<JobId>,
    job_statuses: &HashMap<JobId, engine::JobStatus>,
) -> HashMap<JobId, &'a Job> {
    dependency_set
        .iter()
        .filter(|job_id| {
            !matches!(
                job_statuses.get(*job_id),
                Some(engine::JobStatus::Succeeded { .. })
            )
        })
        .filter_map(|job_id| lab.jobs.get(job_id).map(|job| (job_id.clone(), job)))
        .collect()
}

pub fn collect_images_to_sync(
    lab: &Lab,
    job_ids: &HashSet<JobId>,
) -> HashSet<(std::path::PathBuf, String)> {
    let mut images_to_sync = HashSet::new();

    for job_id in job_ids {
        if let Some(run) = lab.runs.values().find(|r| r.jobs.contains(job_id)) {
            if let Some(image_path) = &run.image {
                if let Some(stem) = image_path.file_stem().and_then(|s| s.to_str()) {
                    images_to_sync.insert((image_path.clone(), stem.to_string()));
                }
            }
        }
    }

    images_to_sync
}

pub fn sync_images(
    lab_path: &Path,
    target: &Arc<dyn Target>,
    local_target: &Arc<dyn Target>,
    images_to_sync: &HashSet<(std::path::PathBuf, String)>,
) -> Result<()> {
    if images_to_sync.is_empty() {
        return Ok(());
    }

    let lab_path_abs = fs_err::canonicalize(lab_path).unwrap_or_else(|_| lab_path.to_path_buf());
    let local_cache_root = local_target.base_path().join("cache");

    for (relative_path, tag) in images_to_sync {
        let full_path = if relative_path.is_absolute() {
            relative_path.clone()
        } else {
            lab_path_abs.join(relative_path)
        };
        target.sync_image_incrementally(&full_path, tag, &local_cache_root)?;
    }

    Ok(())
}

pub fn generate_inputs_for_jobs(
    lab: &Lab,
    lab_path: &Path,
    jobs_to_run: &HashMap<JobId, &Job>,
    target: Arc<dyn Target>,
) -> Result<()> {
    for (job_id, job) in jobs_to_run {
        let exe_name = if job.stage_type == StageType::ScatterGather {
            "scatter"
        } else {
            "main"
        };
        crate::inputs::generate_and_write_inputs_json(
            lab,
            lab_path,
            job,
            job_id,
            target.clone(),
            exe_name,
        )?;
    }
    Ok(())
}

pub fn filter_jobs_for_local_submission<'a>(
    jobs_to_run: &HashMap<JobId, &'a Job>,
    jobs_to_run_ids: &HashSet<JobId>,
) -> Result<HashMap<JobId, &'a Job>> {
    jobs_to_run
        .iter()
        .filter(|(_job_id, job)| {
            let entrypoint_exe = job
                .executables
                .get("main")
                .or_else(|| job.executables.get("scatter"));

            match entrypoint_exe {
                Some(exe) => {
                    let has_deps_in_batch = exe
                        .inputs
                        .iter()
                        .filter_map(|m| m.job_id.as_ref())
                        .any(|job_id| jobs_to_run_ids.contains(job_id));
                    !has_deps_in_batch
                }
                None => false,
            }
        })
        .map(|(id, job)| Ok((id.clone(), *job)))
        .collect()
}

use super::{Client, ClientEvent, SubmitOptions};
use crate::error::{ClientError, Result};
use crate::targets::Target;
use num_cpus;
use repx_core::{
    engine,
    error::AppError,
    model::{Job, JobId},
};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn submit_local_batch_run(
    client: &Client,
    jobs_in_batch: HashMap<JobId, &Job>,
    target: Arc<dyn Target>,
    _target_name: &str,
    repx_binary_path: &Path,
    options: &SubmitOptions,
    send: impl Fn(ClientEvent),
) -> Result<String> {
    send(ClientEvent::SubmittingJobs {
        total: jobs_in_batch.len(),
    });

    let all_deps: HashSet<JobId> = jobs_in_batch
        .values()
        .flat_map(|job| {
            job.executables
                .values()
                .flat_map(|exe| exe.inputs.iter().filter_map(|m| m.job_id.as_ref()))
        })
        .cloned()
        .collect();
    let raw_statuses = client.get_statuses_for_active_target(target.name(), Some("local"))?;
    let all_job_statuses = engine::determine_job_statuses(&client.lab, &raw_statuses);
    let mut completed_jobs: HashSet<JobId> = all_job_statuses
        .into_iter()
        .filter(|(id, status)| {
            matches!(status, repx_core::engine::JobStatus::Succeeded { .. })
                && (all_deps.contains(id) || jobs_in_batch.contains_key(id))
        })
        .map(|(id, _)| id)
        .collect();

    let mut jobs_left: HashSet<JobId> = jobs_in_batch.keys().cloned().collect();
    let total_to_submit = jobs_in_batch.len();
    let mut submitted_count = 0;
    let mut active_handles: Vec<(
        JobId,
        std::thread::JoinHandle<std::io::Result<std::process::Output>>,
    )> = vec![];
    let concurrency = options.num_jobs.unwrap_or_else(num_cpus::get);

    loop {
        let mut finished_indices = Vec::new();
        for (i, (_id, handle)) in active_handles.iter().enumerate() {
            if handle.is_finished() {
                finished_indices.push(i);
            }
        }

        for i in finished_indices.into_iter().rev() {
            let (job_id, handle) = active_handles.remove(i);
            let join_res = handle.join();
            match join_res {
                Ok(output_res) => {
                    let output = output_res.map_err(AppError::from)?;

                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        return Err(ClientError::Core(AppError::ExecutionFailed {
                            message: format!("Local execution of job '{}' failed.", job_id),
                            log_path: Some(
                                target
                                    .base_path()
                                    .join("outputs")
                                    .join(job_id.0)
                                    .join("repx"),
                            ),
                            log_summary: format!(
                                "Process exited with status {}.\n--- STDERR ---\n{}",
                                output.status, stderr
                            ),
                        }));
                    }
                    completed_jobs.insert(job_id.clone());
                }
                Err(e) => {
                    return Err(ClientError::Core(AppError::ExecutionFailed {
                        message: format!("Local execution thread for job '{}' panicked.", job_id),
                        log_path: None,
                        log_summary: format!("{:?}", e),
                    }));
                }
            }
        }

        if jobs_left.is_empty() && active_handles.is_empty() {
            break;
        }

        let slots_available = concurrency.saturating_sub(active_handles.len());

        if slots_available > 0 && !jobs_left.is_empty() {
            let mut ready_candidates: Vec<JobId> = jobs_left
                .iter()
                .filter(|job_id| {
                    let job = jobs_in_batch.get(job_id).unwrap();
                    let is_schedulable_type =
                        job.stage_type != "worker" && job.stage_type != "gather";

                    let entrypoint_exe = job
                        .executables
                        .get("main")
                        .or_else(|| job.executables.get("scatter"))
                        .unwrap();

                    let deps_are_met = entrypoint_exe
                        .inputs
                        .iter()
                        .filter_map(|m| m.job_id.as_ref())
                        .all(|dep_id| completed_jobs.contains(dep_id));

                    is_schedulable_type && deps_are_met
                })
                .cloned()
                .collect();

            ready_candidates.sort();

            if ready_candidates.is_empty() && active_handles.is_empty() {
                return Err(ClientError::Core(AppError::ConfigurationError(
                    "Cycle detected in job dependency graph or missing dependency.".to_string(),
                )));
            }

            for job_id in ready_candidates.into_iter().take(slots_available) {
                jobs_left.remove(&job_id);
                let job = jobs_in_batch.get(&job_id).unwrap();

                let image_path_opt = client
                    .lab
                    .runs
                    .values()
                    .find(|r| r.jobs.contains(&job_id))
                    .and_then(|r| r.image.as_deref());
                let image_tag = image_path_opt
                    .and_then(|p| p.file_stem())
                    .and_then(|s| s.to_str());

                let stage_type = &job.stage_type;
                let execution_type = if options.execution_type.is_none() && image_tag.is_none() {
                    "native"
                } else {
                    options.execution_type.as_deref().unwrap_or_else(|| {
                        let scheduler_config = target.config().local.as_ref().unwrap();
                        target
                            .config()
                            .default_execution_type
                            .as_deref()
                            .filter(|&et| {
                                scheduler_config.execution_types.contains(&et.to_string())
                            })
                            .or_else(|| {
                                scheduler_config.execution_types.first().map(|s| s.as_str())
                            })
                            .unwrap_or("native")
                    })
                };
                let mut args = Vec::new();

                if stage_type == "scatter-gather" {
                    args.push("internal-scatter-gather".to_string());
                } else {
                    args.push("internal-execute".to_string());
                };

                args.push("--job-id".to_string());
                args.push(job_id.0.clone());

                args.push("--runtime".to_string());
                args.push(execution_type.to_string());

                if let Some(tag) = image_tag {
                    args.push("--image-tag".to_string());
                    args.push(tag.to_string());
                }

                args.push("--base-path".to_string());
                args.push(target.base_path().to_string_lossy().to_string());

                if let Some(local_path) = &target.config().node_local_path {
                    args.push("--node-local-path".to_string());
                    args.push(local_path.to_string_lossy().to_string());
                }

                args.push("--host-tools-dir".to_string());
                args.push(client.lab.host_tools_dir_name.clone());

                if stage_type == "scatter-gather" {
                    let scatter_exe = job.executables.get("scatter").unwrap();
                    let worker_exe = job.executables.get("worker").unwrap();
                    let gather_exe = job.executables.get("gather").unwrap();

                    let artifacts_base = target.artifacts_base_path();
                    let job_package_path_on_target =
                        artifacts_base.join(format!("jobs/{}", job_id));
                    let scatter_exe_path = artifacts_base.join(&scatter_exe.path);
                    let worker_exe_path = artifacts_base.join(&worker_exe.path);
                    let gather_exe_path = artifacts_base.join(&gather_exe.path);

                    let worker_outputs_json =
                        serde_json::to_string(&worker_exe.outputs).map_err(AppError::from)?;

                    args.push("--job-package-path".to_string());
                    args.push(job_package_path_on_target.to_string_lossy().to_string());

                    args.push("--scatter-exe-path".to_string());
                    args.push(scatter_exe_path.to_string_lossy().to_string());

                    args.push("--worker-exe-path".to_string());
                    args.push(worker_exe_path.to_string_lossy().to_string());

                    args.push("--gather-exe-path".to_string());
                    args.push(gather_exe_path.to_string_lossy().to_string());

                    args.push("--worker-outputs-json".to_string());
                    args.push(worker_outputs_json);

                    args.push("--scheduler".to_string());
                    args.push("local".to_string());

                    args.push("--worker-sbatch-opts".to_string());
                    args.push("".to_string());
                } else {
                    let main_exe = job.executables.get("main").unwrap();
                    let executable_path_on_target =
                        target.artifacts_base_path().join(&main_exe.path);
                    args.push("--executable-path".to_string());
                    args.push(executable_path_on_target.to_string_lossy().to_string());
                }

                let child = target.spawn_repx_job(repx_binary_path, &args)?;
                submitted_count += 1;
                let pid = child.id();

                send(ClientEvent::JobStarted {
                    job_id: job_id.clone(),
                    pid,
                    total: total_to_submit,
                    current: submitted_count,
                });

                let handle = thread::spawn(move || child.wait_with_output());
                active_handles.push((job_id, handle));
            }
        }

        if !active_handles.is_empty() {
            thread::sleep(Duration::from_millis(50));
        }
    }

    Ok(format!(
        "Successfully executed {} jobs locally.",
        submitted_count
    ))
}

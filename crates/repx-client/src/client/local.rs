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
use std::process::{Child, Command};
use std::sync::Arc;

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
    let raw_statuses = client.get_statuses_for_active_target(target.name())?;
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
    let mut wave_num = 0;

    while !jobs_left.is_empty() {
        wave_num += 1;
        let mut current_wave: Vec<JobId> = jobs_left
            .iter()
            .filter(|job_id| {
                let job = jobs_in_batch.get(job_id).unwrap();
                let is_schedulable_type = job.stage_type != "worker" && job.stage_type != "gather";
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
        current_wave.sort();

        if current_wave.is_empty() && !jobs_left.is_empty() {
            return Err(ClientError::Core(AppError::ConfigurationError(
                "Cycle detected in job dependency graph or missing dependency.".to_string(),
            )));
        }

        let wave_job_count = current_wave.len();
        let mut jobs_to_spawn = current_wave.into_iter();
        let mut active_handles: Vec<(JobId, Child)> = vec![];
        let mut finished_jobs_in_wave = 0;
        let concurrency = options.num_jobs.unwrap_or_else(num_cpus::get);
        while finished_jobs_in_wave < wave_job_count {
            while active_handles.len() < concurrency {
                if let Some(job_id) = jobs_to_spawn.next() {
                    jobs_left.remove(&job_id);
                    let job = jobs_in_batch.get(&job_id).unwrap();

                    let stage_type = &job.stage_type;
                    let execution_type = options.execution_type.as_deref().unwrap_or_else(|| {
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
                    });
                    let image_path_opt = client
                        .lab
                        .runs
                        .values()
                        .find(|r| r.jobs.contains(&job_id))
                        .and_then(|r| r.image.as_deref());
                    let image_tag = image_path_opt
                        .and_then(|p| p.file_stem())
                        .and_then(|s| s.to_str());

                    let mut cmd = Command::new(repx_binary_path);

                    if stage_type == "scatter-gather" {
                        cmd.arg("internal-scatter-gather");
                    } else {
                        cmd.arg("internal-execute");
                    };

                    cmd.arg("--job-id").arg(job_id.0.as_str());
                    cmd.arg("--runtime").arg(execution_type);
                    if let Some(tag) = image_tag {
                        cmd.arg("--image-tag").arg(tag);
                    }
                    cmd.arg("--base-path").arg(target.base_path());
                    cmd.arg("--host-tools-dir")
                        .arg(&client.lab.host_tools_dir_name);

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

                        cmd.arg("--job-package-path")
                            .arg(job_package_path_on_target);
                        cmd.arg("--scatter-exe-path").arg(scatter_exe_path);
                        cmd.arg("--worker-exe-path").arg(worker_exe_path);
                        cmd.arg("--gather-exe-path").arg(gather_exe_path);
                        cmd.arg("--worker-outputs-json").arg(worker_outputs_json);

                        cmd.arg("--scheduler").arg("local");
                        cmd.arg("--worker-sbatch-opts").arg("");
                    } else {
                        let main_exe = job.executables.get("main").unwrap();
                        let executable_path_on_target =
                            target.artifacts_base_path().join(&main_exe.path);
                        cmd.arg("--executable-path").arg(executable_path_on_target);
                    }

                    cmd.stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped());

                    let child = cmd.spawn().map_err(|e| {
                        ClientError::Core(AppError::ProcessLaunchFailed {
                            command_name: repx_binary_path.to_string_lossy().to_string(),
                            source: e,
                        })
                    })?;

                    submitted_count += 1;
                    send(ClientEvent::JobStarted {
                        job_id: job_id.clone(),
                        pid: child.id(),
                        total: total_to_submit,
                        current: submitted_count,
                    });

                    active_handles.push((job_id, child));
                } else {
                    break;
                }
            }

            if active_handles.is_empty() {
                break;
            }

            let (job_id, handle) = active_handles.remove(0);
            let output = handle.wait_with_output().map_err(AppError::from)?;
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
            finished_jobs_in_wave += 1;
        }

        send(ClientEvent::WaveCompleted {
            wave: wave_num,
            num_jobs: wave_job_count,
        });
    }

    Ok(format!(
        "Successfully executed {} jobs locally.",
        submitted_count
    ))
}

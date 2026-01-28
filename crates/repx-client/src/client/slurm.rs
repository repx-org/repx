use super::{Client, ClientEvent, SubmitOptions};
use crate::error::{ClientError, Result};
use crate::orchestration::OrchestrationPlan;
use crate::resources::{self, SbatchDirectives};
use crate::targets::Target;
use fs_err;
use repx_core::{
    error::AppError,
    log_debug,
    model::{Job, JobId},
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use xdg;

fn generate_repx_invoker_script(
    job_id: &JobId,
    job_root_on_target: &Path,
    directives: &SbatchDirectives,
    repx_command_to_wrap: String,
) -> Result<String> {
    let mut s = String::from("#!/usr/bin/env bash\n");
    s.push_str(&format!("#SBATCH --job-name={}\n", job_id.0));
    s.push_str(&format!(
        "#SBATCH --chdir={}\n",
        job_root_on_target.display()
    ));
    let log_path = job_root_on_target.join("repx").join("slurm-%j.out");
    s.push_str(&format!("#SBATCH --output={}\n", log_path.display()));
    s.push_str(&format!("#SBATCH --error={}\n", log_path.display()));

    if let Some(p) = &directives.partition {
        s.push_str(&format!("#SBATCH --partition={}\n", p));
    }
    if let Some(c) = directives.cpus_per_task {
        s.push_str(&format!("#SBATCH --cpus-per-task={}\n", c));
    }
    if let Some(m) = &directives.mem {
        s.push_str(&format!("#SBATCH --mem={}\n", m));
    }
    if let Some(t) = &directives.time {
        s.push_str(&format!("#SBATCH --time={}\n", t));
    }
    for opt in &directives.sbatch_opts {
        s.push_str(&format!("#SBATCH {}\n", opt));
    }

    s.push_str("\nset -e\n\n");
    s.push_str("# This script invokes the repx binary to handle execution.\n");
    s.push_str(&repx_command_to_wrap);
    s.push('\n');

    Ok(s)
}

pub fn submit_slurm_batch_run(
    client: &Client,
    jobs_to_submit: HashMap<JobId, &Job>,
    target: Arc<dyn Target>,
    target_name: &str,
    remote_repx_binary_path: &Path,
    options: &SubmitOptions,
    send: impl Fn(ClientEvent),
) -> Result<String> {
    let remote_repx_command = remote_repx_binary_path.to_string_lossy();
    send(ClientEvent::GeneratingSlurmScripts {
        num_jobs: jobs_to_submit.len(),
    });
    let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
    let cache_home = xdg_dirs.get_cache_home().ok_or_else(|| {
        ClientError::Core(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not find cache home directory",
        )))
    })?;
    let local_batch_dir = cache_home
        .join("submissions")
        .join(&client.lab.content_hash);
    fs_err::create_dir_all(&local_batch_dir).map_err(AppError::from)?;

    let mut plan = OrchestrationPlan::new(target.base_path(), &client.lab.content_hash);
    let job_ids_in_batch: HashSet<JobId> = jobs_to_submit.keys().cloned().collect();

    for (job_id, job) in &jobs_to_submit {
        let job_root_on_target = target.base_path().join("outputs").join(&job_id.0);
        let image_path_opt = client
            .lab
            .runs
            .values()
            .find(|r| r.jobs.contains(job_id))
            .and_then(|r| r.image.as_deref());
        let image_tag = image_path_opt
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str());

        let execution_type = if options.execution_type.is_none() && image_tag.is_none() {
            "native"
        } else {
            options.execution_type.as_deref().unwrap_or_else(|| {
                let scheduler_config = target.config().slurm.as_ref().unwrap();
                target
                    .config()
                    .default_execution_type
                    .as_deref()
                    .filter(|&et| scheduler_config.execution_types.contains(&et.to_string()))
                    .or_else(|| scheduler_config.execution_types.first().map(|s| s.as_str()))
                    .unwrap_or("native")
            })
        };
        let mut repx_args = format!(
            "--job-id {} --runtime {} {} --base-path {} --host-tools-dir {}",
            job_id,
            execution_type,
            image_tag
                .map(|t| format!("--image-tag {}", t))
                .unwrap_or_default(),
            target.base_path().display(),
            client.lab.host_tools_dir_name
        );
        if let Some(local_path) = &target.config().node_local_path {
            repx_args.push_str(&format!(" --node-local-path {}", local_path.display()));
        }
        if target.config().mount_host_paths {
            if !target.config().mount_paths.is_empty() {
                return Err(ClientError::Core(AppError::ConfigurationError(
                    "Cannot specify both 'mount_host_paths = true' and 'mount_paths'.".into(),
                )));
            }
            repx_args.push_str(" --mount-host-paths");
        } else {
            for path in &target.config().mount_paths {
                repx_args.push_str(&format!(" --mount-paths {}", path));
            }
        }
        let (repx_command_to_wrap, directives) = if job.stage_type == "scatter-gather" {
            let scatter_exe = job.executables.get("scatter").ok_or_else(|| {
                AppError::ConfigurationError(
                    "Scatter-gather job missing 'scatter' executable".into(),
                )
            })?;
            let worker_exe = job.executables.get("worker").ok_or_else(|| {
                AppError::ConfigurationError(
                    "Scatter-gather job missing 'worker' executable".into(),
                )
            })?;
            let gather_exe = job.executables.get("gather").ok_or_else(|| {
                AppError::ConfigurationError(
                    "Scatter-gather job missing 'gather' executable".into(),
                )
            })?;

            let artifacts_base = target.artifacts_base_path();
            let scatter_exe_path = artifacts_base.join(&scatter_exe.path);
            let worker_exe_path = artifacts_base.join(&worker_exe.path);
            let gather_exe_path = artifacts_base.join(&gather_exe.path);

            let worker_outputs_json =
                serde_json::to_string(&worker_exe.outputs).map_err(AppError::from)?;

            let scatter_gather_args = format!(
                "--job-package-path {} --scatter-exe-path {} --worker-exe-path {} --gather-exe-path {} --worker-outputs-json '{}' {}",
                target.artifacts_base_path().join(format!("jobs/{}", job_id)).display(),
                scatter_exe_path.display(),
                worker_exe_path.display(),
                gather_exe_path.display(),
                worker_outputs_json,
                if target.config().mount_host_paths {
                    "--mount-host-paths".to_string()
                } else {
                    target
                        .config()
                        .mount_paths
                        .iter()
                        .map(|p| format!("--mount-paths {}", p))
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            );
            let main_directives =
                resources::resolve_for_job(job_id, target_name, &options.resources);
            let worker_directives =
                resources::resolve_worker_resources(job_id, target_name, &options.resources);
            let worker_opts_str = worker_directives.to_shell_string();

            let command = format!(
                "{} internal-scatter-gather {} {} --worker-sbatch-opts='{}' --scheduler slurm --anchor-id $REPX_ANCHOR_ID",
                remote_repx_command, repx_args, scatter_gather_args, worker_opts_str
            );
            (command, main_directives)
        } else {
            let main_exe = job.executables.get("main").ok_or_else(|| {
                AppError::ConfigurationError(format!(
                    "Simple job '{}' missing 'main' executable",
                    job_id
                ))
            })?;
            let executable_path_on_target = target.artifacts_base_path().join(&main_exe.path);

            let repx_args = format!(
                "{} --executable-path {}",
                repx_args,
                executable_path_on_target.display()
            );
            let directives = resources::resolve_for_job(job_id, target_name, &options.resources);
            let command = format!("{} internal-execute {}", remote_repx_command, repx_args);
            (command, directives)
        };

        let script_content = generate_repx_invoker_script(
            job_id,
            &job_root_on_target,
            &directives,
            repx_command_to_wrap,
        )?;

        let mut hasher = Sha256::new();
        hasher.update(&script_content);
        let hash_bytes = hasher.finalize();
        let script_hash = format!("{:x}", hash_bytes);

        let script_path = local_batch_dir.join(format!("{}.sbatch", script_hash));
        let mut file = fs_err::File::create(script_path).map_err(AppError::from)?;
        file.write_all(script_content.as_bytes())
            .map_err(AppError::from)?;

        plan.add_job(job_id.clone(), job, script_hash, &job_ids_in_batch);
    }
    let plan_filename = "plan.json";
    let plan_content = serde_json::to_string_pretty(&plan).map_err(AppError::from)?;
    fs_err::write(local_batch_dir.join(plan_filename), plan_content).map_err(AppError::from)?;

    send(ClientEvent::ExecutingOrchestrator);
    send(ClientEvent::SubmittingJobs {
        total: jobs_to_submit.len(),
    });

    let submission_dir_on_target = target
        .base_path()
        .join("submissions")
        .join(&client.lab.content_hash);
    target.sync_directory(&local_batch_dir, &submission_dir_on_target)?;

    let orchestrator_command = format!(
        "{} internal-orchestrate {}",
        remote_repx_command,
        submission_dir_on_target.join(plan_filename).display()
    );

    let orchestrator_output = target.run_command("sh", &["-c", &orchestrator_command])?;

    log_debug!(
        "Orchestrator raw output on target '{}':\n---\n{}\n---",
        target.name(),
        orchestrator_output
    );

    let mut submitted_count = 0;
    let total_to_submit = jobs_to_submit.len();
    for line in orchestrator_output.lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() == 2 {
            if let (Ok(repx_id), Ok(slurm_id)) =
                (JobId::from_str(parts[0]), parts[1].parse::<u32>())
            {
                client
                    .slurm_map
                    .lock()
                    .unwrap()
                    .insert(repx_id.clone(), (target_name.to_string(), slurm_id));
                submitted_count += 1;
                send(ClientEvent::JobSubmitted {
                    job_id: repx_id,
                    slurm_id,
                    total: total_to_submit,
                    current: submitted_count,
                });
            }
        }
    }

    client.save_slurm_map()?;
    Ok(format!(
        "Successfully submitted {} jobs via SLURM orchestrator.",
        submitted_count
    ))
}

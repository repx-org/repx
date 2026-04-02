use super::{Client, ClientEvent, SubmitOptions};
use crate::error::{ClientError, Result};
use crate::orchestration::OrchestrationPlan;
use crate::resources::{self, SbatchDirectives};
use crate::targets::common::shell_quote;
use fs_err;
use repx_core::{
    constants::{dirs, targets},
    errors::CoreError,
    model::{Job, JobId},
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::str::FromStr;

fn generate_repx_invoker_script(
    job_id: &JobId,
    job_root_on_target: &Path,
    directives: &SbatchDirectives,
    repx_command_to_wrap: String,
    lab_tar_info: Option<&super::LabTarInfo>,
) -> Result<String> {
    let mut s = String::from("#!/usr/bin/env bash\n");
    s.push_str(&format!("#SBATCH --job-name={}\n", job_id.as_str()));
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
        s.push_str(&format!("#SBATCH --mem={}\n", m.as_str()));
    }
    if let Some(t) = &directives.time {
        s.push_str(&format!("#SBATCH --time={}\n", t.as_str()));
    }
    for opt in &directives.sbatch_opts {
        s.push_str(&format!("#SBATCH {}\n", opt));
    }

    s.push_str("\nset -e\n\n");

    if let Some(info) = lab_tar_info {
        s.push_str("# -- Node-local lab tar bootstrap --\n");
        s.push_str(&format!("LAB_TAR=\"{}\"\n", info.remote_tar_path.display()));
        s.push_str(&format!(
            "LOCAL_BASE=\"{}\"\n",
            info.node_local_base.display()
        ));
        s.push_str(&format!(
            "MARKER=\"$LOCAL_BASE/.extracted-{}\"\n",
            info.content_hash
        ));
        s.push_str("LOCK_FILE=\"$LOCAL_BASE/.lock\"\n");
        s.push_str("mkdir -p \"$LOCAL_BASE\"\n");
        s.push_str("(\n");
        s.push_str("  flock -x 200\n");
        s.push_str("  if [ ! -f \"$MARKER\" ]; then\n");
        s.push_str("    echo \"[repx] Extracting lab tar to node-local storage...\"\n");
        s.push_str("    tar xf \"$LAB_TAR\" -C \"$LOCAL_BASE/\"\n");
        s.push_str("    touch \"$MARKER\"\n");
        s.push_str("    echo \"[repx] Lab tar extraction complete.\"\n");
        s.push_str("  fi\n");
        s.push_str(") 200>\"$LOCK_FILE\"\n");
        s.push_str(&format!(
            "export REPX_LOCAL_ARTIFACTS=\"$LOCAL_BASE/{}\"\n",
            info.lab_dir_name
        ));
        s.push('\n');
    }

    s.push_str("# This script invokes the repx binary to handle execution.\n");
    s.push_str(&repx_command_to_wrap);
    s.push('\n');

    Ok(s)
}

#[allow(clippy::expect_used)]
pub fn submit_slurm_batch_run(
    client: &Client,
    jobs_to_submit: HashMap<JobId, &Job>,
    sub_target: &super::SubmissionTarget,
    options: &SubmitOptions,
    lab_tar_info: Option<&super::LabTarInfo>,
    send: impl Fn(ClientEvent),
) -> Result<String> {
    let target = &sub_target.target;
    let target_name = &sub_target.target_name;
    let remote_repx_binary = sub_target.repx_binary_path.to_string_lossy();
    let verbose_flags = options.verbose.as_flag_str();
    let remote_repx_command = format!("{} {}", remote_repx_binary, verbose_flags);
    let remote_repx_command = remote_repx_command.trim_end();
    send(ClientEvent::GeneratingSlurmScripts {
        num_jobs: jobs_to_submit.len(),
    });
    let local_target = client
        .get_target(targets::LOCAL)
        .ok_or(ClientError::Config(CoreError::MissingLocalTarget))?;
    let client_temp_dir = local_target.base_path().join("repx").join("temp");
    let local_batch_dir = client_temp_dir.join("slurm_batch");
    if local_batch_dir.exists() {
        if let Err(e) = fs_err::remove_dir_all(&local_batch_dir) {
            tracing::debug!(
                "Failed to remove old batch dir '{}': {}",
                local_batch_dir.display(),
                e
            );
        }
    }
    fs_err::create_dir_all(&local_batch_dir).map_err(|e| ClientError::Config(CoreError::Io(e)))?;

    let mut plan = OrchestrationPlan::new(target.base_path(), &client.lab.content_hash);
    let job_ids_in_batch: HashSet<JobId> = jobs_to_submit.keys().cloned().collect();

    for (job_id, job) in &jobs_to_submit {
        let job_root_on_target = target.base_path().join(dirs::OUTPUTS).join(job_id.as_str());
        let image_path_opt = client
            .lab
            .runs
            .values()
            .find(|r| r.jobs.contains(job_id))
            .and_then(|r| r.image.as_deref());
        let image_tag = image_path_opt
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str());

        let execution_type = super::resolve_execution_type(
            image_tag,
            options.execution_type.as_deref(),
            target.config(),
            target.config().slurm.as_ref(),
        );
        let mut repx_args = format!(
            "--job-id {} --runtime {} {} --base-path {} --host-tools-dir {}",
            shell_quote(job_id.as_str()),
            shell_quote(&execution_type),
            image_tag
                .map(|t| format!("--image-tag {}", shell_quote(t)))
                .unwrap_or_default(),
            shell_quote(&target.base_path().to_string_lossy()),
            shell_quote(&client.lab.host_tools_dir_name)
        );
        if let Some(local_path) = &target.config().node_local_path {
            repx_args.push_str(&format!(
                " --node-local-path {}",
                shell_quote(&local_path.to_string_lossy())
            ));
        }
        if lab_tar_info.is_some() {
            repx_args.push_str(" --local-artifacts-path \"$REPX_LOCAL_ARTIFACTS\"");
        }
        match target.config().mount_policy() {
            repx_core::model::MountPolicy::AllHostPaths => {
                repx_args.push_str(" --mount-host-paths");
            }
            repx_core::model::MountPolicy::SpecificPaths(ref paths) => {
                for path in paths {
                    repx_args.push_str(&format!(" --mount-paths {}", shell_quote(path)));
                }
            }
            repx_core::model::MountPolicy::Isolated => {}
        }
        let (repx_command_to_wrap, directives) = if job.stage_type
            == repx_core::model::StageType::ScatterGather
        {
            let scatter_exe = job.executables.get("scatter").ok_or_else(|| {
                ClientError::Config(CoreError::MissingExecutable {
                    job_id: job_id.to_string(),
                    executable: "scatter".to_string(),
                })
            })?;
            let gather_exe = job.executables.get("gather").ok_or_else(|| {
                ClientError::Config(CoreError::MissingExecutable {
                    job_id: job_id.to_string(),
                    executable: "gather".to_string(),
                })
            })?;

            let artifacts_base = target.artifacts_base_path();
            let scatter_exe_path = artifacts_base.join(&scatter_exe.path);
            let gather_exe_path = artifacts_base.join(&gather_exe.path);

            let (steps_json, last_step_outputs_json) =
                super::local::build_steps_json(job, &artifacts_base)?;

            let sink_step_key = {
                let all_deps: HashSet<String> = job
                    .executables
                    .iter()
                    .filter(|(k, _)| k.starts_with("step-"))
                    .flat_map(|(_, exe)| exe.deps.iter().cloned())
                    .collect();
                let sink_candidates: Vec<&String> = job
                    .executables
                    .keys()
                    .filter(|k| k.starts_with("step-"))
                    .filter(|k| {
                        let name = k
                            .strip_prefix("step-")
                            .expect("prefix guaranteed by starts_with filter");
                        !all_deps.contains(name)
                    })
                    .collect();
                sink_candidates.first().cloned().cloned().ok_or_else(|| {
                    ClientError::Config(CoreError::InconsistentMetadata {
                        detail: format!(
                            "Scatter-gather job '{}' has no sink step in its step DAG",
                            job_id
                        ),
                    })
                })?
            };
            let sink_step_hints = job
                .executables
                .get(&sink_step_key)
                .and_then(|e| e.resource_hints.as_ref());

            let scatter_gather_args = format!(
                "--job-package-path {} --scatter-exe-path {} --gather-exe-path {} --steps-json '{}' --last-step-outputs-json '{}' {}",
                target.artifacts_base_path().join(format!("jobs/{}", job_id)).display(),
                scatter_exe_path.display(),
                gather_exe_path.display(),
                steps_json.replace('\'', "'\\''"),
                last_step_outputs_json.replace('\'', "'\\''"),
                match target.config().mount_policy() {
                    repx_core::model::MountPolicy::AllHostPaths => "--mount-host-paths".to_string(),
                    repx_core::model::MountPolicy::SpecificPaths(ref paths) => paths
                        .iter()
                        .map(|p| format!("--mount-paths {}", p))
                        .collect::<Vec<_>>()
                        .join(" "),
                    repx_core::model::MountPolicy::Isolated => String::new(),
                }
            );

            let orchestrator_hints = job.resource_hints.as_ref();

            let main_directives = resources::resolve_for_job(
                job_id,
                target_name,
                &options.resources,
                orchestrator_hints,
            );
            let step_directives = resources::resolve_worker_resources(
                job_id,
                target_name,
                &options.resources,
                orchestrator_hints,
                sink_step_hints,
            );
            let step_opts_str = step_directives.to_shell_string();

            let lab_tar_flag = lab_tar_info
                .map(|info| {
                    format!(
                        " --lab-tar-path {}",
                        shell_quote(&info.remote_tar_path.to_string_lossy())
                    )
                })
                .unwrap_or_default();
            let command = format!(
                "{} internal-scatter-gather {} {}{} --step-sbatch-opts='{}' --scheduler slurm --anchor-id $REPX_ANCHOR_ID",
                remote_repx_command, repx_args, scatter_gather_args, lab_tar_flag, step_opts_str
            );
            (command, main_directives)
        } else {
            let main_exe = job.executables.get("main").ok_or_else(|| {
                ClientError::Config(CoreError::MissingExecutable {
                    job_id: job_id.to_string(),
                    executable: "main".to_string(),
                })
            })?;
            let executable_path_on_target = target.artifacts_base_path().join(&main_exe.path);

            let repx_args = format!(
                "{} --executable-path {}",
                repx_args,
                executable_path_on_target.display()
            );

            let hints = job.resource_hints.as_ref();
            let directives =
                resources::resolve_for_job(job_id, target_name, &options.resources, hints);
            let command = format!("{} internal-execute {}", remote_repx_command, repx_args);
            (command, directives)
        };

        let script_content = generate_repx_invoker_script(
            job_id,
            &job_root_on_target,
            &directives,
            repx_command_to_wrap,
            lab_tar_info,
        )?;

        let mut hasher = Sha256::new();
        hasher.update(&script_content);
        let hash_bytes = hasher.finalize();
        let script_hash = format!("{:x}", hash_bytes);

        let script_path = local_batch_dir.join(format!("{}.sbatch", script_hash));
        let mut file =
            fs_err::File::create(script_path).map_err(|e| ClientError::Config(CoreError::Io(e)))?;
        file.write_all(script_content.as_bytes())
            .map_err(|e| ClientError::Config(CoreError::Io(e)))?;

        plan.add_job(job_id.clone(), job, script_hash, &job_ids_in_batch)?;
    }
    let plan_filename = "plan.json";
    let plan_content =
        serde_json::to_string_pretty(&plan).map_err(|e| ClientError::Config(CoreError::Json(e)))?;
    fs_err::write(local_batch_dir.join(plan_filename), plan_content)
        .map_err(|e| ClientError::Config(CoreError::Io(e)))?;

    send(ClientEvent::ExecutingOrchestrator);
    send(ClientEvent::SubmittingJobs {
        total: jobs_to_submit.len(),
        concurrency: None,
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

    tracing::debug!(
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
                super::lock_slurm_map(&client.slurm_map).insert(
                    repx_id.clone(),
                    super::SlurmJobEntry {
                        target_name: target_name.to_string(),
                        slurm_id,
                    },
                );
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

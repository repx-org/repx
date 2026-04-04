use super::{Client, ClientEvent, SubmitOptions};
use crate::error::{ClientError, Result};
use crate::inputs;
use crate::resources::{self, SbatchDirectives};
use crate::targets::common::shell_quote;
use repx_core::{
    constants::dirs,
    errors::CoreError,
    model::{Job, JobId, StageType},
    protocol::{self, StreamJob, StreamJobResult, StreamJobType},
};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::str::FromStr;

fn generate_repx_invoker_script(
    job_id: &JobId,
    _job_root_on_target: &Path,
    directives: &SbatchDirectives,
    repx_command_to_wrap: String,
    lab_tar_info: Option<&super::LabTarInfo>,
    inputs_json: &str,
    parameters_json: &str,
) -> Result<String> {
    let mut s = String::with_capacity(4096);
    s.push_str("#!/usr/bin/env bash\n");
    s.push_str(&format!("#SBATCH --job-name={}\n", job_id.as_str()));
    s.push_str("#SBATCH --chdir=/tmp\n");
    s.push_str("#SBATCH --output=/dev/null\n");
    s.push_str("#SBATCH --error=/dev/null\n");

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

    s.push_str("exec 3<<'__REPX_INPUTS_EOF__'\n");
    s.push_str(inputs_json);
    if !inputs_json.ends_with('\n') {
        s.push('\n');
    }
    s.push_str("__REPX_INPUTS_EOF__\n\n");
    s.push_str("exec 4<<'__REPX_PARAMS_EOF__'\n");
    s.push_str(parameters_json);
    if !parameters_json.ends_with('\n') {
        s.push('\n');
    }
    s.push_str("__REPX_PARAMS_EOF__\n\n");

    if let Some(info) = lab_tar_info {
        s.push_str("# -- Node-local lab tar bootstrap --\n");
        s.push_str(&format!(
            "export LAB_TAR=\"{}\"\n",
            info.remote_tar_path.display()
        ));
        s.push_str(&format!(
            "export LOCAL_BASE=\"{}\"\n",
            info.node_local_base.display()
        ));
        s.push_str(&format!(
            "export MARKER=\"$LOCAL_BASE/.extracted-{}\"\n",
            info.content_hash
        ));
        s.push_str("mkdir -p \"$LOCAL_BASE\"\n");
        s.push_str("flock -x \"$LOCAL_BASE/.lock\" sh -c '\n");
        s.push_str("  if [ ! -f \"$MARKER\" ]; then\n");
        s.push_str("    echo \"[repx] Cleaning stale extraction (if any)...\"\n");
        s.push_str("    chmod -R u+w \"$LOCAL_BASE\" 2>/dev/null || true\n");
        s.push_str(
            "    find \"$LOCAL_BASE\" -mindepth 1 -not -name .lock -delete 2>/dev/null || true\n",
        );
        s.push_str("    echo \"[repx] Extracting lab tar to node-local storage...\"\n");
        s.push_str("    tar xf \"$LAB_TAR\" -C \"$LOCAL_BASE/\"\n");
        s.push_str("    touch \"$MARKER\"\n");
        s.push_str("    echo \"[repx] Lab tar extraction complete.\"\n");
        s.push_str("  fi\n");
        s.push_str("'\n");
        s.push_str(&format!(
            "export REPX_LOCAL_ARTIFACTS=\"$LOCAL_BASE/{}\"\n",
            info.lab_dir_name
        ));
        s.push('\n');
    }

    s.push_str(&repx_command_to_wrap);
    s.push_str(" --inputs-json-path /dev/fd/3");
    s.push_str(" --parameters-json-path /dev/fd/4");
    s.push('\n');

    Ok(s)
}

fn compute_waves(
    jobs: &HashMap<JobId, &Job>,
    batch_job_ids: &HashSet<JobId>,
) -> Result<Vec<Vec<JobId>>> {
    let mut deps: HashMap<JobId, Vec<JobId>> = HashMap::new();
    for (job_id, job) in jobs {
        let in_batch_deps: Vec<JobId> = job
            .all_dependencies()
            .filter(|dep_id| batch_job_ids.contains(*dep_id))
            .cloned()
            .collect();
        deps.insert(job_id.clone(), in_batch_deps);
    }

    let mut waves: Vec<Vec<JobId>> = Vec::new();
    let mut assigned: HashSet<JobId> = HashSet::new();
    let mut remaining: HashSet<JobId> = jobs.keys().cloned().collect();

    while !remaining.is_empty() {
        let mut wave: Vec<JobId> = Vec::new();
        for job_id in &remaining {
            let job_deps = deps.get(job_id).map(|d| d.as_slice()).unwrap_or(&[]);
            if job_deps.iter().all(|dep| assigned.contains(dep)) {
                wave.push(job_id.clone());
            }
        }
        if wave.is_empty() {
            return Err(ClientError::Config(CoreError::CommandFailed(
                "Cycle detected in job dependency graph".to_string(),
            )));
        }
        wave.sort();
        for id in &wave {
            remaining.remove(id);
            assigned.insert(id.clone());
        }
        waves.push(wave);
    }

    Ok(waves)
}

#[allow(clippy::too_many_arguments)]
fn build_job_command_and_directives(
    client: &Client,
    job_id: &JobId,
    job: &Job,
    target: &dyn crate::targets::Target,
    target_name: &str,
    remote_repx_command: &str,
    options: &SubmitOptions,
    lab_tar_info: Option<&super::LabTarInfo>,
) -> Result<(String, SbatchDirectives)> {
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

    if job.stage_type == StageType::ScatterGather {
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
                .filter(|k| match k.strip_prefix("step-") {
                    Some(name) => !all_deps.contains(name),
                    None => false,
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

        let main_directives =
            resources::resolve_for_job(job_id, target_name, &options.resources, orchestrator_hints);
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
        Ok((command, main_directives))
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
        let directives = resources::resolve_for_job(job_id, target_name, &options.resources, hints);
        let command = format!("{} internal-execute {}", remote_repx_command, repx_args);
        Ok((command, directives))
    }
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

    let total_to_submit = jobs_to_submit.len();

    send(ClientEvent::GeneratingSlurmScripts {
        num_jobs: total_to_submit,
    });

    let job_ids_in_batch: HashSet<JobId> = jobs_to_submit.keys().cloned().collect();
    let waves = compute_waves(&jobs_to_submit, &job_ids_in_batch)?;

    tracing::info!(
        "Computed {} waves for {} jobs",
        waves.len(),
        total_to_submit
    );

    send(ClientEvent::ExecutingOrchestrator);
    send(ClientEvent::SubmittingJobs {
        total: total_to_submit,
        concurrency: None,
    });

    let orchestrator_command = format!("{} internal-orchestrate --stream", remote_repx_command);
    let mut child = target.spawn_command("sh", &["-c", &orchestrator_command])?;

    let child_stdin = child.stdin.take().ok_or_else(|| {
        ClientError::Config(CoreError::CommandFailed(
            "Failed to capture orchestrator stdin".to_string(),
        ))
    })?;
    let child_stdout = child.stdout.take().ok_or_else(|| {
        ClientError::Config(CoreError::CommandFailed(
            "Failed to capture orchestrator stdout".to_string(),
        ))
    })?;

    let mut writer = BufWriter::new(child_stdin);
    let mut reader = BufReader::new(child_stdout);

    let mut slurm_ids: HashMap<JobId, u32> = HashMap::new();
    let mut submitted_count = 0;

    let exe_name_for_job = |job: &Job| -> &str {
        if job.stage_type == StageType::ScatterGather {
            "scatter"
        } else {
            "main"
        }
    };

    for (wave_idx, wave) in waves.iter().enumerate() {
        tracing::info!(
            "Streaming wave {}/{} with {} jobs",
            wave_idx + 1,
            waves.len(),
            wave.len()
        );

        for job_id in wave {
            let job = jobs_to_submit.get(job_id).expect("job must exist in batch");

            let inputs_json = inputs::generate_inputs_json_content(
                &client.lab,
                &client.lab_source,
                job,
                job_id,
                target.base_path(),
                &target.artifacts_base_path(),
                exe_name_for_job(job),
            )?;
            let parameters_json = inputs::generate_parameters_json_content(job)?;

            let job_root_on_target = target.base_path().join(dirs::OUTPUTS).join(job_id.as_str());
            let (repx_command, directives) = build_job_command_and_directives(
                client,
                job_id,
                job,
                target.as_ref(),
                target_name,
                remote_repx_command,
                options,
                lab_tar_info,
            )?;

            let script_content = generate_repx_invoker_script(
                job_id,
                &job_root_on_target,
                &directives,
                repx_command,
                lab_tar_info,
                &inputs_json,
                &parameters_json,
            )?;

            let deps: Vec<u32> = job
                .all_dependencies()
                .filter(|dep_id| job_ids_in_batch.contains(*dep_id))
                .filter_map(|dep_id| slurm_ids.get(dep_id))
                .copied()
                .collect();

            let stream_job = StreamJob {
                id: job_id.to_string(),
                job_type: if job.stage_type == StageType::ScatterGather {
                    StreamJobType::ScatterGather
                } else {
                    StreamJobType::Simple
                },
                script: script_content,
                deps,
            };

            let line = serde_json::to_string(&stream_job)
                .map_err(|e| ClientError::Config(CoreError::Json(e)))?;
            writeln!(writer, "{}", line).map_err(|e| ClientError::Config(CoreError::Io(e)))?;
        }

        writeln!(writer, "{}", protocol::WAVE_BOUNDARY)
            .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
        writer
            .flush()
            .map_err(|e| ClientError::Config(CoreError::Io(e)))?;

        let mut line_buf = String::new();
        loop {
            line_buf.clear();
            let bytes_read = reader
                .read_line(&mut line_buf)
                .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
            if bytes_read == 0 {
                let status = child.wait().ok();
                let stderr = child
                    .stderr
                    .take()
                    .map(|mut s| {
                        let mut buf = String::new();
                        std::io::Read::read_to_string(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();
                return Err(ClientError::Config(CoreError::CommandFailed(format!(
                    "Orchestrator died unexpectedly (exit={:?}). stderr:\n{}",
                    status, stderr
                ))));
            }

            let trimmed = line_buf.trim();
            if trimmed == protocol::WAVE_DONE {
                break;
            }

            let result: StreamJobResult = serde_json::from_str(trimmed).map_err(|e| {
                ClientError::Config(CoreError::CommandFailed(format!(
                    "Failed to parse orchestrator response '{}': {}",
                    trimmed, e
                )))
            })?;

            let repx_id = JobId::from_str(&result.id).map_err(|_| {
                ClientError::Config(CoreError::CommandFailed(format!(
                    "Invalid job ID in orchestrator response: {}",
                    result.id
                )))
            })?;

            slurm_ids.insert(repx_id.clone(), result.slurm_id);
            super::lock_slurm_map(&client.slurm_map).insert(
                repx_id.clone(),
                super::SlurmJobEntry {
                    target_name: target_name.to_string(),
                    slurm_id: result.slurm_id,
                },
            );
            submitted_count += 1;
            send(ClientEvent::JobSubmitted {
                job_id: repx_id,
                slurm_id: result.slurm_id,
                total: total_to_submit,
                current: submitted_count,
            });
        }
    }

    writeln!(writer, "{}", protocol::STREAM_END)
        .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
    writer
        .flush()
        .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
    drop(writer);

    let status = child
        .wait()
        .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
    if !status.success() {
        let stderr = child
            .stderr
            .take()
            .map(|mut s| {
                let mut buf = String::new();
                std::io::Read::read_to_string(&mut s, &mut buf).ok();
                buf
            })
            .unwrap_or_default();
        return Err(ClientError::Config(CoreError::CommandFailed(format!(
            "Orchestrator exited with {}: {}",
            status, stderr
        ))));
    }

    client.save_slurm_map()?;
    Ok(format!(
        "Successfully submitted {} jobs via SLURM orchestrator.",
        submitted_count
    ))
}

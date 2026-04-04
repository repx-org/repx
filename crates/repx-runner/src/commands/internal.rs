use crate::{cli::InternalOrchestrateArgs, error::CliError};
use repx_core::{
    errors::CoreError,
    model::{JobId, StageType},
    protocol::{self, StreamJob, StreamJobResult, StreamJobType},
};
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::process::{Command, Stdio};

fn submit_via_sbatch_stdin(
    script: &str,
    deps: &[u32],
    anchor_id: Option<u32>,
) -> Result<u32, CliError> {
    let mut sbatch_cmd = Command::new("sbatch");
    sbatch_cmd.arg("--parsable");

    if !deps.is_empty() {
        let dep_str: Vec<String> = deps.iter().map(|d| d.to_string()).collect();
        sbatch_cmd.arg(format!("--dependency=afterok:{}", dep_str.join(":")));
        sbatch_cmd.arg("--kill-on-invalid-dep=yes");
    }

    if let Some(aid) = anchor_id {
        sbatch_cmd.arg(format!("--export=ALL,REPX_ANCHOR_ID={}", aid));
    }

    sbatch_cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = sbatch_cmd.spawn().map_err(|e| CliError::ExecutionFailed {
        message: "Failed to spawn sbatch".to_string(),
        log_path: None,
        log_summary: e.to_string(),
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| CliError::ExecutionFailed {
                message: "Failed to write script to sbatch stdin".to_string(),
                log_path: None,
                log_summary: e.to_string(),
            })?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| CliError::ExecutionFailed {
            message: "sbatch process failed".to_string(),
            log_path: None,
            log_summary: e.to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CliError::ExecutionFailed {
            message: "sbatch command failed".to_string(),
            log_path: None,
            log_summary: stderr.to_string(),
        });
    }

    let id_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    id_str
        .parse::<u32>()
        .map_err(|_| CliError::ExecutionFailed {
            message: "Failed to parse SLURM ID from sbatch output".to_string(),
            log_path: None,
            log_summary: format!("sbatch output was: '{}'", id_str),
        })
}

fn submit_anchor(job_id: &str) -> Result<u32, CliError> {
    let mut cmd = Command::new("sbatch");
    cmd.arg("--parsable")
        .arg("--hold")
        .arg(format!("--job-name=anchor-{}", job_id))
        .arg("--time=00:01:00")
        .arg("--output=/dev/null")
        .arg("--error=/dev/null")
        .arg("--wrap=exit 0");

    let output = cmd.output().map_err(|e| CliError::ExecutionFailed {
        message: "Failed to submit anchor job".to_string(),
        log_path: None,
        log_summary: e.to_string(),
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CliError::ExecutionFailed {
            message: format!("Failed to submit anchor for job '{}'", job_id),
            log_path: None,
            log_summary: stderr.to_string(),
        });
    }

    let id_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    id_str
        .parse::<u32>()
        .map_err(|_| CliError::ExecutionFailed {
            message: format!("Failed to parse Anchor ID for job '{}'", job_id),
            log_path: None,
            log_summary: id_str,
        })
}

fn handle_stream_orchestrate() -> Result<(), CliError> {
    let stdin = io::stdin();
    let reader = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());

    eprintln!("[REPX-ORCH] Streaming orchestrator started.");

    let mut wave_num = 0;
    let mut wave_count = 0;

    for line_result in reader.lines() {
        let line =
            line_result.map_err(|e| CliError::Config(CoreError::CommandFailed(e.to_string())))?;
        let trimmed = line.trim();

        if trimmed == protocol::WAVE_BOUNDARY {
            eprintln!(
                "[REPX-ORCH] Wave {} complete ({} jobs submitted).",
                wave_num, wave_count
            );
            writeln!(writer, "{}", protocol::WAVE_DONE)
                .map_err(|e| CliError::Config(CoreError::CommandFailed(e.to_string())))?;
            writer
                .flush()
                .map_err(|e| CliError::Config(CoreError::CommandFailed(e.to_string())))?;
            wave_num += 1;
            wave_count = 0;
            continue;
        }

        if trimmed == protocol::STREAM_END {
            eprintln!("[REPX-ORCH] Stream ended. All jobs submitted successfully.");
            break;
        }

        if trimmed.is_empty() {
            continue;
        }

        let job: StreamJob = serde_json::from_str(trimmed).map_err(|e| {
            CliError::Config(CoreError::CommandFailed(format!(
                "Failed to parse job record: {}",
                e
            )))
        })?;

        let anchor_id = if job.job_type == StreamJobType::ScatterGather {
            Some(submit_anchor(&job.id)?)
        } else {
            None
        };

        let slurm_id = submit_via_sbatch_stdin(&job.script, &job.deps, anchor_id)?;
        let track_id = anchor_id.unwrap_or(slurm_id);

        let result = StreamJobResult {
            id: job.id,
            slurm_id: track_id,
        };
        let result_line = serde_json::to_string(&result)
            .map_err(|e| CliError::Config(CoreError::CommandFailed(e.to_string())))?;
        writeln!(writer, "{}", result_line)
            .map_err(|e| CliError::Config(CoreError::CommandFailed(e.to_string())))?;

        wave_count += 1;
    }

    Ok(())
}

#[allow(clippy::expect_used)]
fn handle_plan_file_orchestrate(plan_file: &std::path::Path) -> Result<(), CliError> {
    let plan_content = std::fs::read_to_string(plan_file)
        .map_err(|e| CliError::Config(CoreError::path_io(plan_file, e)))?;
    let plan: repx_client::orchestration::OrchestrationPlan = serde_json::from_str(&plan_content)?;

    let mut submitted_slurm_ids: HashMap<JobId, u32> = HashMap::new();
    let mut jobs_left: HashSet<JobId> = plan.jobs.keys().cloned().collect();
    let mut wave_num = 0;

    while !jobs_left.is_empty() {
        let mut current_wave: Vec<JobId> = Vec::new();

        for job_id in &jobs_left {
            let job_plan = plan
                .jobs
                .get(job_id)
                .expect("job_id comes from plan.jobs.keys() iteration");
            let all_deps_met = job_plan
                .dependencies
                .iter()
                .all(|dep_id| submitted_slurm_ids.contains_key(dep_id));
            if all_deps_met {
                current_wave.push(job_id.clone());
            }
        }
        current_wave.sort();

        if current_wave.is_empty() {
            return Err(CliError::Config(CoreError::CycleDetected {
                context: "job dependency graph".to_string(),
            }));
        }

        eprintln!(
            "[REPX-ORCH] Submitting wave {} with {} jobs...",
            wave_num,
            current_wave.len()
        );

        for job_id in current_wave {
            jobs_left.remove(&job_id);
            let job_plan = plan
                .jobs
                .get(&job_id)
                .expect("job_id was just removed from jobs_left which came from plan.jobs");
            let script_path = plan
                .submissions_dir
                .join(format!("{}.sbatch", job_plan.script_hash));

            let dep_ids: Vec<String> = job_plan
                .dependencies
                .iter()
                .filter_map(|dep_id| submitted_slurm_ids.get(dep_id))
                .map(|id| id.to_string())
                .collect();

            let mut anchor_id = None;

            if job_plan.job_type == StageType::ScatterGather {
                anchor_id = Some(submit_anchor(job_id.as_str())?);
            }

            let mut sbatch_cmd = Command::new("sbatch");
            sbatch_cmd.arg("--parsable");

            if !dep_ids.is_empty() {
                sbatch_cmd.arg(format!("--dependency=afterok:{}", dep_ids.join(":")));
                sbatch_cmd.arg("--kill-on-invalid-dep=yes");
            }

            if let Some(aid) = anchor_id {
                sbatch_cmd.arg(format!("--export=ALL,REPX_ANCHOR_ID={}", aid));
            }

            sbatch_cmd.arg(&script_path);

            let output = sbatch_cmd.output().map_err(|e| CliError::ExecutionFailed {
                message: "Process launch failed".to_string(),
                log_path: None,
                log_summary: e.to_string(),
            })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(CliError::ExecutionFailed {
                    message: format!("sbatch command failed for job '{}'", job_id),
                    log_path: Some(script_path),
                    log_summary: stderr.to_string(),
                });
            }

            let slurm_id_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let slurm_id = slurm_id_str
                .parse::<u32>()
                .map_err(|_| CliError::ExecutionFailed {
                    message: format!(
                        "Failed to parse SLURM ID from sbatch output for job '{}'",
                        job_id
                    ),
                    log_path: Some(script_path),
                    log_summary: format!("sbatch output was: '{}'", slurm_id_str),
                })?;

            let track_id = anchor_id.unwrap_or(slurm_id);
            submitted_slurm_ids.insert(job_id.clone(), track_id);

            println!("{} {}", job_id, track_id);
        }
        wave_num += 1;
    }

    eprintln!("[REPX-ORCH] All jobs submitted successfully.");
    Ok(())
}

pub fn handle_internal_orchestrate(args: InternalOrchestrateArgs) -> Result<(), CliError> {
    if args.stream {
        handle_stream_orchestrate()
    } else if let Some(ref plan_file) = args.plan_file {
        handle_plan_file_orchestrate(plan_file)
    } else {
        Err(CliError::Config(CoreError::CommandFailed(
            "Either --stream or a plan file path must be provided".to_string(),
        )))
    }
}

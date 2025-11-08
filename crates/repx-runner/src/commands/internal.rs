use crate::cli::InternalOrchestrateArgs;
use repx_client::orchestration::OrchestrationPlan;
use repx_core::{error::AppError, model::JobId};
use std::collections::{HashMap, HashSet};
use std::process::Command;

pub fn handle_internal_orchestrate(args: InternalOrchestrateArgs) -> Result<(), AppError> {
    let plan_content = std::fs::read_to_string(&args.plan_file).map_err(|e| AppError::PathIo {
        path: args.plan_file.clone(),
        source: e,
    })?;
    let plan: OrchestrationPlan = serde_json::from_str(&plan_content)?;

    let mut submitted_slurm_ids: HashMap<JobId, u32> = HashMap::new();
    let mut jobs_left: HashSet<JobId> = plan.jobs.keys().cloned().collect();
    let mut wave_num = 0;

    while !jobs_left.is_empty() {
        let mut current_wave: Vec<JobId> = Vec::new();

        for job_id in &jobs_left {
            let job_plan = plan.jobs.get(job_id).unwrap();
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
            return Err(AppError::ConfigurationError(
                "Cycle detected in job dependency graph.".to_string(),
            ));
        }

        eprintln!(
            "[REPX-ORCH] Submitting wave {} with {} jobs...",
            wave_num,
            current_wave.len()
        );

        for job_id in current_wave {
            jobs_left.remove(&job_id);
            let job_plan = plan.jobs.get(&job_id).unwrap();
            let script_path = plan
                .submissions_dir
                .join(format!("{}.sbatch", job_plan.script_hash));

            let dep_ids: Vec<String> = job_plan
                .dependencies
                .iter()
                .filter_map(|dep_id| submitted_slurm_ids.get(dep_id))
                .map(|id| id.to_string())
                .collect();

            let mut sbatch_cmd = Command::new("sbatch");
            sbatch_cmd.arg("--parsable");

            if !dep_ids.is_empty() {
                sbatch_cmd.arg(format!("--dependency=afterok:{}", dep_ids.join(":")));
                sbatch_cmd.arg("--kill-on-invalid-dep=yes");
            }
            sbatch_cmd.arg(&script_path);

            let output = sbatch_cmd
                .output()
                .map_err(|e| AppError::ProcessLaunchFailed {
                    command_name: "sbatch".to_string(),
                    source: e,
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(AppError::ExecutionFailed {
                    message: format!("sbatch command failed for job '{}'", job_id),
                    log_path: Some(script_path),
                    log_summary: stderr.to_string(),
                });
            }

            let slurm_id_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let slurm_id = slurm_id_str
                .parse::<u32>()
                .map_err(|_| AppError::ExecutionFailed {
                    message: format!(
                        "Failed to parse SLURM ID from sbatch output for job '{}'",
                        job_id
                    ),
                    log_path: Some(script_path),
                    log_summary: format!("sbatch output was: '{}'", slurm_id_str),
                })?;

            submitted_slurm_ids.insert(job_id.clone(), slurm_id);

            println!("{} {}", job_id, slurm_id);
        }
        wave_num += 1;
    }

    eprintln!("[REPX-ORCH] All jobs submitted successfully.");
    Ok(())
}

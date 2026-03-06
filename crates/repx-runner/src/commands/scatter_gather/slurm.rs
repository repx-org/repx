use crate::error::CliError;
use repx_core::{constants::dirs, errors::ConfigError};
use std::{collections::HashMap, fs};
use tokio::process::Command as TokioCommand;

use super::{inputs::resolve_step_inputs, ScatterGatherOrchestrator, StepsMetadata};
use crate::cli::InternalScatterGatherArgs;
use repx_core::constants::manifests;
use serde_json::Value;

pub(crate) fn command_to_shell_string(cmd: &TokioCommand) -> String {
    let program = cmd.as_std().get_program().to_string_lossy();
    let args: Vec<String> = cmd
        .as_std()
        .get_args()
        .map(|arg| format!("'{}'", arg.to_string_lossy().replace('\'', "'\\''")))
        .collect();
    format!("{} {}", program, args.join(" "))
}

pub(crate) async fn cancel_workers_from_manifest(repx_dir: &std::path::Path) {
    let manifest_path = repx_dir.join(manifests::WORKER_SLURM_IDS);
    if let Ok(content) = fs::read_to_string(&manifest_path) {
        if let Ok(worker_ids) = serde_json::from_str::<Vec<u32>>(&content) {
            if !worker_ids.is_empty() {
                let id_strs: Vec<String> = worker_ids.iter().map(|id| id.to_string()).collect();
                tracing::info!(
                    "Cancelling {} worker SLURM jobs: {:?}",
                    worker_ids.len(),
                    &id_strs
                );
                let _ = TokioCommand::new("scancel").args(&id_strs).output().await;
            }
        }
    }
}

pub(crate) async fn submit_slurm_gather_job(
    orch: &ScatterGatherOrchestrator,
    args: &InternalScatterGatherArgs,
    last_step_slurm_ids: &[String],
    verbose: repx_core::logging::Verbosity,
) -> Result<(), CliError> {
    let current_exe = std::env::current_exe()?;
    let current_exe_str = current_exe.to_string_lossy();

    let steps_json_escaped = args.steps_json.replace('\'', "'\\''");

    let mut gather_cmd_parts = vec![current_exe_str.to_string()];
    gather_cmd_parts.extend(verbose.as_args());
    gather_cmd_parts.extend_from_slice(&[
        "internal-scatter-gather".to_string(),
        "--phase".to_string(),
        "gather".to_string(),
        "--job-id".to_string(),
        args.job_id.clone(),
        "--runtime".to_string(),
        args.runtime.to_string(),
        "--base-path".to_string(),
        args.base_path.to_string_lossy().to_string(),
        "--host-tools-dir".to_string(),
        args.host_tools_dir.clone(),
        "--scheduler".to_string(),
        "slurm".to_string(),
        "--step-sbatch-opts".to_string(),
        "''".to_string(),
        "--job-package-path".to_string(),
        args.job_package_path.to_string_lossy().to_string(),
        "--scatter-exe-path".to_string(),
        args.scatter_exe_path.to_string_lossy().to_string(),
        "--gather-exe-path".to_string(),
        args.gather_exe_path.to_string_lossy().to_string(),
        "--steps-json".to_string(),
        format!("'{}'", steps_json_escaped),
        "--last-step-outputs-json".to_string(),
        format!("'{}'", args.last_step_outputs_json),
    ]);

    {
        let policy = repx_core::model::MountPolicy::from_flags(
            args.mount_host_paths,
            args.mount_paths.clone(),
        );
        match &policy {
            repx_core::model::MountPolicy::AllHostPaths => {
                gather_cmd_parts.push("--mount-host-paths".to_string());
            }
            repx_core::model::MountPolicy::SpecificPaths(paths) => {
                for path in paths {
                    gather_cmd_parts.push("--mount-paths".to_string());
                    gather_cmd_parts.push(path.clone());
                }
            }
            repx_core::model::MountPolicy::Isolated => {}
        }
    }

    if let Some(tag) = &args.image_tag {
        gather_cmd_parts.push("--image-tag".to_string());
        gather_cmd_parts.push(tag.clone());
    }
    if let Some(local) = &args.node_local_path {
        gather_cmd_parts.push("--node-local-path".to_string());
        gather_cmd_parts.push(local.to_string_lossy().to_string());
    }
    if let Some(anchor) = args.anchor_id {
        gather_cmd_parts.push("--anchor-id".to_string());
        gather_cmd_parts.push(anchor.to_string());
    }

    let cmd_str = gather_cmd_parts.join(" ");

    let mut sbatch = TokioCommand::new("sbatch");
    sbatch.arg("--parsable");
    if !last_step_slurm_ids.is_empty() {
        sbatch.arg(format!(
            "--dependency=afterany:{}",
            last_step_slurm_ids.join(":")
        ));
    }
    sbatch
        .arg(format!("--job-name={}-gather", orch.job_id.as_str()))
        .arg(format!(
            "--output={}/gather/repx/slurm-%j.out",
            orch.job_root.display()
        ))
        .arg("--wrap")
        .arg(cmd_str);

    let output = sbatch.output().await?;
    if !output.status.success() {
        return Err(CliError::ExecutionFailed {
            message: "Failed to submit Gather job".to_string(),
            log_path: None,
            log_summary: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    Ok(())
}

#[allow(clippy::expect_used)]
pub(crate) async fn submit_slurm_branches(
    orch: &ScatterGatherOrchestrator,
    work_items: &[Value],
    steps_meta: &StepsMetadata,
    topo_order: &[String],
    sbatch_opts: &str,
) -> Result<(Vec<String>, Vec<u32>), CliError> {
    let mut last_step_slurm_ids = Vec::new();
    let mut all_worker_slurm_ids: Vec<u32> = Vec::new();

    for (branch_idx, item) in work_items.iter().enumerate() {
        let branch_root = orch.job_root.join(format!("branch-{}", branch_idx));
        let branch_repx = branch_root.join(dirs::REPX);
        fs::create_dir_all(&branch_repx)?;

        let work_item_path = branch_repx.join("work_item.json");
        fs::write(&work_item_path, serde_json::to_string(item)?)?;

        let mut step_slurm_ids: HashMap<String, String> = HashMap::new();

        for step_name in topo_order {
            let step_meta = steps_meta
                .steps
                .get(step_name)
                .expect("step_name comes from topo_order which was derived from steps");
            let step_root = branch_root.join(format!("step-{}", step_name));
            let step_out = step_root.join(dirs::OUT);
            let step_repx = step_root.join(dirs::REPX);
            fs::create_dir_all(&step_out)?;
            fs::create_dir_all(&step_repx)?;

            let inputs = resolve_step_inputs(
                step_meta,
                &branch_root,
                &work_item_path,
                &orch.static_inputs,
                &steps_meta.steps,
            )?;
            let inputs_path = step_repx.join("inputs.json");
            fs::write(&inputs_path, serde_json::to_string_pretty(&inputs)?)?;

            let executor = orch.create_executor(step_out.clone(), step_repx.clone());
            let exe_args = vec![
                step_out.to_string_lossy().to_string(),
                inputs_path.to_string_lossy().to_string(),
            ];
            let cmd = executor
                .build_command_for_script(&step_meta.exe_path, &exe_args)
                .await
                .map_err(|e| CliError::ExecutionFailed {
                    message: format!(
                        "Failed to build command for branch #{} step '{}'",
                        branch_idx, step_name
                    ),
                    log_path: None,
                    log_summary: e.to_string(),
                })?;
            let cmd_str = command_to_shell_string(&cmd);

            let wrapped_cmd = format!(
                "( {} && touch {}/{} ) || ( touch {}/{}; exit 1 )",
                cmd_str,
                step_repx.display(),
                repx_core::constants::markers::SUCCESS,
                step_repx.display(),
                repx_core::constants::markers::FAIL
            );

            let mut sbatch = TokioCommand::new("sbatch");
            sbatch
                .arg("--parsable")
                .args(sbatch_opts.split_whitespace());

            let dep_slurm_ids: Vec<&String> = step_meta
                .deps
                .iter()
                .filter_map(|dep| step_slurm_ids.get(dep))
                .collect();

            if !dep_slurm_ids.is_empty() {
                let dep_ids_str: Vec<&str> = dep_slurm_ids.iter().map(|s| s.as_str()).collect();
                sbatch.arg(format!("--dependency=afterok:{}", dep_ids_str.join(":")));
            }

            sbatch
                .arg(format!(
                    "--job-name={}-b{}-{}",
                    orch.job_id.as_str(),
                    branch_idx,
                    step_name
                ))
                .arg(format!("--output={}/slurm-%j.out", step_repx.display()))
                .arg("--wrap")
                .arg(wrapped_cmd);

            let output = sbatch.output().await?;
            if !output.status.success() {
                return Err(CliError::ExecutionFailed {
                    message: format!(
                        "sbatch submission for branch #{} step '{}' failed",
                        branch_idx, step_name
                    ),
                    log_path: None,
                    log_summary: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            let slurm_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Ok(id) = slurm_id.parse::<u32>() {
                all_worker_slurm_ids.push(id);
            }
            step_slurm_ids.insert(step_name.clone(), slurm_id);
        }

        if let Some(sink_slurm_id) = step_slurm_ids.get(&steps_meta.sink_step) {
            last_step_slurm_ids.push(sink_slurm_id.clone());
        } else {
            return Err(CliError::Config(ConfigError::InconsistentMetadata {
                detail: format!(
                    "Sink step '{}' was not submitted for branch #{}",
                    steps_meta.sink_step, branch_idx
                ),
            }));
        }
    }

    tracing::info!(
        "Submitted {} branches ({} steps each, {} total worker jobs) to Slurm.",
        work_items.len(),
        topo_order.len(),
        all_worker_slurm_ids.len()
    );
    Ok((last_step_slurm_ids, all_worker_slurm_ids))
}

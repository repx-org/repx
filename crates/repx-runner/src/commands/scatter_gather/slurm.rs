use crate::error::CliError;
use repx_core::{constants::dirs, errors::CoreError, fs_utils::path_to_string};
use std::collections::HashMap;
use std::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;

use super::{inputs::resolve_step_inputs, ScatterGatherOrchestrator, StepsMetadata};
use crate::cli::InternalScatterGatherArgs;
use repx_core::constants::manifests;
use serde_json::Value;

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
        path_to_string(&args.base_path),
        "--host-tools-dir".to_string(),
        args.host_tools_dir.clone(),
        "--scheduler".to_string(),
        "slurm".to_string(),
        "--step-sbatch-opts".to_string(),
        "''".to_string(),
        "--job-package-path".to_string(),
        path_to_string(&args.job_package_path),
        "--scatter-exe-path".to_string(),
        path_to_string(&args.scatter_exe_path),
        "--gather-exe-path".to_string(),
        path_to_string(&args.gather_exe_path),
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
        gather_cmd_parts.push(path_to_string(local));
    }
    if let Some(local_art) = &args.local_artifacts_path {
        gather_cmd_parts.push("--local-artifacts-path".to_string());
        gather_cmd_parts.push(path_to_string(local_art));
    }
    if let Some(tar) = &args.lab_tar_path {
        gather_cmd_parts.push("--lab-tar-path".to_string());
        gather_cmd_parts.push(path_to_string(tar));
    }
    if let Some(anchor) = args.anchor_id {
        gather_cmd_parts.push("--anchor-id".to_string());
        gather_cmd_parts.push(anchor.to_string());
    }

    let gather_bootstrap = match (&orch.local_artifacts_path, &orch.lab_tar_path) {
        (Some(local_artifacts), Some(tar_path)) => {
            let local_base = local_artifacts.parent().unwrap_or(local_artifacts);
            let content_hash = local_base
                .file_name()
                .unwrap_or(std::ffi::OsStr::new("unknown"))
                .to_string_lossy();
            format!(
                "export LOCAL_BASE='{local_base}' && \
                 export MARKER=\"$LOCAL_BASE/.extracted-{hash}\" && \
                 export LAB_TAR='{tar}' && \
                 mkdir -p \"$LOCAL_BASE\" && \
                 flock -x \"$LOCAL_BASE/.lock\" sh -c \
                   'if [ ! -f \"$MARKER\" ]; then tar xf \"$LAB_TAR\" -C \"$LOCAL_BASE/\" && touch \"$MARKER\"; fi' && ",
                local_base = local_base.display(),
                hash = content_hash,
                tar = tar_path.display(),
            )
        }
        _ => String::new(),
    };
    let cmd_str = format!("{}{}", gather_bootstrap, gather_cmd_parts.join(" "));

    let gather_repx_dir = orch.job_root.join("gather").join(dirs::REPX);
    fs::create_dir_all(&gather_repx_dir)?;

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
        return Err(CliError::execution_failed(
            "Failed to submit Gather job",
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

#[allow(clippy::expect_used)]
pub(crate) async fn submit_slurm_branches(
    orch: &ScatterGatherOrchestrator,
    args: &InternalScatterGatherArgs,
    work_items: &[Value],
    steps_meta: &StepsMetadata,
    topo_order: &[String],
    sbatch_opts: &str,
) -> Result<(Vec<String>, Vec<u32>), CliError> {
    let mut last_step_slurm_ids = Vec::new();
    let mut all_worker_slurm_ids: Vec<u32> = Vec::new();

    let repx_binary = std::env::current_exe()?;
    let repx_binary_str = repx_binary.to_string_lossy();
    let runtime_str = args.runtime.to_string();
    let base_path_str = orch.base_path.to_string_lossy();
    let image_tag_flag = args
        .image_tag
        .as_ref()
        .map(|t| format!("--image-tag '{}'", t))
        .unwrap_or_default();
    let node_local_flag = orch
        .node_local_path
        .as_ref()
        .map(|p| format!("--node-local-path '{}'", p.to_string_lossy()))
        .unwrap_or_default();
    let local_artifacts_flag = orch
        .local_artifacts_path
        .as_ref()
        .map(|p| format!("--local-artifacts-path '{}'", p.to_string_lossy()))
        .unwrap_or_default();

    let worker_bootstrap = match (&orch.local_artifacts_path, &orch.lab_tar_path) {
        (Some(local_artifacts), Some(tar_path)) => {
            let local_base = local_artifacts.parent().unwrap_or(local_artifacts);
            let content_hash = local_base
                .file_name()
                .unwrap_or(std::ffi::OsStr::new("unknown"))
                .to_string_lossy();
            format!(
                r#"export LOCAL_BASE='{local_base}'
export MARKER="$LOCAL_BASE/.extracted-{hash}"
export LAB_TAR='{tar}'
mkdir -p "$LOCAL_BASE"
flock -x "$LOCAL_BASE/.lock" sh -c \
  'if [ ! -f "$MARKER" ]; then tar xf "$LAB_TAR" -C "$LOCAL_BASE/" && touch "$MARKER"; fi'
"#,
                local_base = local_base.display(),
                hash = content_hash,
                tar = tar_path.display(),
            )
        }
        _ => String::new(),
    };
    let mount_flags = match &orch.mount_policy {
        repx_core::model::MountPolicy::AllHostPaths => "--mount-host-paths".to_string(),
        repx_core::model::MountPolicy::SpecificPaths(paths) => paths
            .iter()
            .map(|p| format!("--mount-paths '{}'", p))
            .collect::<Vec<_>>()
            .join(" "),
        repx_core::model::MountPolicy::Isolated => String::new(),
    };

    for (branch_idx, item) in work_items.iter().enumerate() {
        let branch_root = orch.job_root.join(format!("branch-{}", branch_idx));

        let work_item_json = serde_json::to_string(item)?;

        let mut step_slurm_ids: HashMap<String, String> = HashMap::new();

        for step_name in topo_order {
            let step_meta = steps_meta
                .steps
                .get(step_name)
                .expect("step_name comes from topo_order which was derived from steps");
            let step_root = branch_root.join(format!("step-{}", step_name));
            let step_out = step_root.join(dirs::OUT);
            let step_repx = step_root.join(dirs::REPX);

            let work_item_fd_path = std::path::PathBuf::from("/dev/fd/4");
            let inputs = resolve_step_inputs(
                step_meta,
                &branch_root,
                &work_item_fd_path,
                &orch.static_inputs,
                &steps_meta.steps,
            )?;
            let inputs_json = serde_json::to_string_pretty(&inputs)?;

            #[allow(clippy::format_in_format_args)]
            let script = format!(
                r#"#!/usr/bin/env bash
#SBATCH --job-name={job_name}
#SBATCH --output=/dev/null
#SBATCH --error=/dev/null
{sbatch_directives}
set -e

# Inputs, parameters, and work_item are passed via file descriptors.
# Zero filesystem writes — data flows: heredoc -> kernel buffer -> fd -> child.
exec 3<<'__REPX_INPUTS_EOF__'
{inputs_json}
__REPX_INPUTS_EOF__

exec 4<<'__REPX_WI_EOF__'
{work_item_json}
__REPX_WI_EOF__

{worker_bootstrap}
exec {repx} internal-execute \
  --job-id '{job_id}' \
  --runtime '{runtime}' \
  {image_tag} \
  --base-path '{base_path}' \
  --host-tools-dir '{host_tools_dir}' \
  {node_local} \
  {local_artifacts} \
  {mount} \
  --executable-path '{exe_path}' \
  --user-out-dir '{user_out}' \
  --repx-out-dir '{repx_out}' \
  --inputs-json-path /dev/fd/3 \
  --parameters-json-path '{params_json}' \
  --job-package-path '{job_pkg}'
"#,
                job_name = format!("{}-b{}-{}", orch.job_id.as_str(), branch_idx, step_name),
                sbatch_directives = format_sbatch_opts(sbatch_opts),
                work_item_json = work_item_json,
                inputs_json = inputs_json,
                worker_bootstrap = worker_bootstrap,
                repx = repx_binary_str,
                job_id = orch.job_id.as_str(),
                runtime = runtime_str,
                image_tag = image_tag_flag,
                base_path = base_path_str,
                host_tools_dir = args.host_tools_dir,
                node_local = node_local_flag,
                local_artifacts = local_artifacts_flag,
                mount = mount_flags,
                exe_path = step_meta.exe_path.display(),
                user_out = step_out.display(),
                repx_out = step_repx.display(),
                params_json = orch.parameters_json_path.display(),
                job_pkg = orch.job_package_path.display(),
            );

            let mut sbatch = TokioCommand::new("sbatch");
            sbatch
                .arg("--parsable")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            let dep_slurm_ids: Vec<&String> = step_meta
                .deps
                .iter()
                .filter_map(|dep| step_slurm_ids.get(dep))
                .collect();

            if !dep_slurm_ids.is_empty() {
                let dep_ids_str: Vec<&str> = dep_slurm_ids.iter().map(|s| s.as_str()).collect();
                sbatch.arg(format!("--dependency=afterok:{}", dep_ids_str.join(":")));
            }

            let mut child = sbatch.spawn().map_err(|e| {
                CliError::Config(CoreError::CommandFailed(format!(
                    "Failed to spawn sbatch for branch #{} step '{}': {}",
                    branch_idx, step_name, e
                )))
            })?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(script.as_bytes()).await.map_err(|e| {
                    CliError::Config(CoreError::CommandFailed(format!(
                        "Failed to write script to sbatch stdin for branch #{} step '{}': {}",
                        branch_idx, step_name, e
                    )))
                })?;
                drop(stdin);
            }

            let output = child.wait_with_output().await?;
            if !output.status.success() {
                return Err(CliError::execution_failed(
                    format!(
                        "sbatch submission for branch #{} step '{}' failed",
                        branch_idx, step_name
                    ),
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ));
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
            return Err(CliError::Config(CoreError::InconsistentMetadata {
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

fn format_sbatch_opts(opts: &str) -> String {
    let trimmed = opts.trim();
    if trimmed.is_empty() || trimmed == "''" {
        return String::new();
    }
    trimmed
        .split_whitespace()
        .filter(|s| !s.is_empty())
        .map(|opt| format!("#SBATCH {}", opt))
        .collect::<Vec<_>>()
        .join("\n")
}

use crate::{
    cli::{InternalScatterGatherArgs, ScatterGatherPhase},
    error::CliError,
};
use repx_core::{
    constants::{dirs, manifests, markers},
    errors::CoreError,
    model::{JobId, Memory, MountPolicy, SlurmTime},
    store::completion_log,
};
use repx_executor::{CancellationToken, ExecutionRequest, Executor, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use tokio::process::Command as TokioCommand;

use super::write_marker;

pub(crate) mod inputs;
pub(crate) mod slurm;
pub(crate) mod toposort;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepMeta {
    pub exe_path: PathBuf,
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub outputs: HashMap<String, String>,
    #[serde(default)]
    pub inputs: Vec<StepInputMapping>,
    #[serde(default)]
    pub resource_hints: Option<StepResourceHints>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepInputMapping {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_output: Option<String>,
    pub target_input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub mapping_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResourceHints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem: Option<Memory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<SlurmTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepsMetadata {
    pub steps: HashMap<String, StepMeta>,
    pub sink_step: String,
}

pub fn handle_scatter_gather(
    args: InternalScatterGatherArgs,
    verbose: repx_core::logging::Verbosity,
) -> Result<(), CliError> {
    let rt = super::create_tokio_runtime()?;
    rt.block_on(async_handle_scatter_gather(args, verbose))
}

pub(crate) struct ScatterGatherOrchestrator {
    pub(crate) job_id: JobId,
    pub(crate) base_path: PathBuf,
    pub(crate) job_root: PathBuf,
    pub(crate) user_out_dir: PathBuf,
    pub(crate) repx_dir: PathBuf,
    pub(crate) scatter_out_dir: PathBuf,
    pub(crate) scatter_repx_dir: PathBuf,
    pub(crate) inputs_json_path: PathBuf,
    pub(crate) parameters_json_path: PathBuf,
    pub(crate) runtime: Runtime,
    pub(crate) job_package_path: PathBuf,
    pub(crate) static_inputs: Value,
    pub(crate) host_tools_bin_dir: Option<PathBuf>,
    pub(crate) node_local_path: Option<PathBuf>,
    pub(crate) local_artifacts_path: Option<PathBuf>,
    pub(crate) lab_tar_path: Option<PathBuf>,
    pub(crate) mount_policy: MountPolicy,
}

impl ScatterGatherOrchestrator {
    pub(crate) fn new(args: &InternalScatterGatherArgs) -> Result<Self, CliError> {
        let job_id = JobId::from(args.job_id.clone());
        let job_root = args.base_path.join(dirs::OUTPUTS).join(job_id.as_str());
        let user_out_dir = job_root.join(dirs::OUT);
        let repx_dir = job_root.join(dirs::REPX);
        let scatter_root = job_root.join("scatter");
        let scatter_out_dir = scatter_root.join(dirs::OUT);
        let scatter_repx_dir = scatter_root.join(dirs::REPX);
        let inputs_json_path = repx_dir.join("inputs.json");
        let parameters_json_path = repx_dir.join("parameters.json");

        let runtime = super::parse_runtime(args.runtime, args.image_tag.clone())?;

        let host_tools_bin_dir = if let Some(ref local) = args.local_artifacts_path {
            let local_tools = local
                .join("host-tools")
                .join(&args.host_tools_dir)
                .join("bin");
            if local_tools.exists() {
                Some(local_tools)
            } else {
                let host_tools_root = args.base_path.join("artifacts").join("host-tools");
                Some(host_tools_root.join(&args.host_tools_dir).join("bin"))
            }
        } else {
            let host_tools_root = args.base_path.join("artifacts").join("host-tools");
            Some(host_tools_root.join(&args.host_tools_dir).join("bin"))
        };

        let resolve = |p: &std::path::Path| {
            crate::commands::resolve_to_local_artifacts(
                p,
                &args.base_path,
                &args.local_artifacts_path,
            )
        };

        Ok(Self {
            job_id,
            base_path: args.base_path.clone(),
            job_root,
            user_out_dir,
            repx_dir,
            scatter_out_dir,
            scatter_repx_dir,
            inputs_json_path,
            parameters_json_path,
            runtime,
            job_package_path: resolve(&args.job_package_path),
            static_inputs: Value::Object(Default::default()),
            host_tools_bin_dir,
            node_local_path: args.node_local_path.clone(),
            local_artifacts_path: args.local_artifacts_path.clone(),
            lab_tar_path: args.lab_tar_path.clone(),
            mount_policy: MountPolicy::from_flags(args.mount_host_paths, args.mount_paths.clone()),
        })
    }

    pub(crate) fn init_dirs(&mut self) -> Result<(), CliError> {
        for dir in [
            &self.user_out_dir,
            &self.repx_dir,
            &self.scatter_out_dir,
            &self.scatter_repx_dir,
        ] {
            fs::create_dir_all(dir)?;
        }
        let _ = fs::remove_file(self.repx_dir.join(markers::SUCCESS));
        let _ = fs::remove_file(self.repx_dir.join(markers::FAIL));

        self.load_static_inputs()?;
        Ok(())
    }

    pub(crate) fn load_static_inputs(&mut self) -> Result<(), CliError> {
        if self.inputs_json_path.exists() {
            self.static_inputs =
                serde_json::from_str(&fs::read_to_string(&self.inputs_json_path)?)?;
        }
        Ok(())
    }

    pub(crate) fn create_executor(&self, user_out: PathBuf, repx_out: PathBuf) -> Executor {
        Executor::new(ExecutionRequest {
            job_id: self.job_id.clone(),
            runtime: self.runtime.clone(),
            base_path: self.base_path.clone(),
            node_local_path: self.node_local_path.clone(),
            local_artifacts_path: self.local_artifacts_path.clone(),
            job_package_path: self.job_package_path.clone(),
            inputs_json_path: self.inputs_json_path.clone(),
            user_out_dir: user_out,
            repx_out_dir: repx_out,
            host_tools_bin_dir: self.host_tools_bin_dir.clone(),
            mount_policy: self.mount_policy.clone(),
            inputs_data: None,
            parameters_data: None,
        })
    }

    async fn run_scatter(&self, exe_path: &Path) -> Result<(), CliError> {
        tracing::info!("[1/4] Starting scatter phase for job '{}'...", self.job_id);
        let mut executor =
            self.create_executor(self.scatter_out_dir.clone(), self.scatter_repx_dir.clone());
        let args = vec![
            self.scatter_out_dir.to_string_lossy().to_string(),
            self.inputs_json_path.to_string_lossy().to_string(),
            self.parameters_json_path.to_string_lossy().to_string(),
        ];
        let cancel = CancellationToken::new();
        executor
            .execute_script(exe_path, &args, &cancel)
            .await
            .map_err(|e| CliError::ExecutionFailed {
                message: "Scatter script failed".to_string(),
                log_path: Some(self.scatter_repx_dir.clone()),
                log_summary: e.to_string(),
            })?;
        Ok(())
    }

    async fn run_gather(
        &self,
        exe_path: &Path,
        branch_sink_out_dirs: &[PathBuf],
        last_step_outputs_template_json: &str,
    ) -> Result<(), CliError> {
        tracing::info!("[4/4] All branches completed. Starting gather phase...");

        let mut worker_outs_manifest = Vec::new();
        let last_step_outputs: HashMap<String, Value> =
            serde_json::from_str(last_step_outputs_template_json)?;

        for sink_out_dir in branch_sink_out_dirs {
            let mut outputs = HashMap::new();
            for (name, template) in &last_step_outputs {
                let template_str = template.as_str().ok_or_else(|| {
                    CliError::Config(CoreError::StepError {
                        detail: format!(
                            "Last step output template for '{}' must be a string.",
                            name
                        ),
                    })
                })?;
                let path = template_str.replace("$out", &sink_out_dir.to_string_lossy());
                outputs.insert(name.clone(), path);
            }
            worker_outs_manifest.push(outputs);
        }

        let worker_manifest_path = self.repx_dir.join("worker_outs_manifest.json");
        fs::write(
            &worker_manifest_path,
            serde_json::to_string_pretty(&worker_outs_manifest)?,
        )?;

        let mut gather_inputs = self.static_inputs.as_object().cloned().unwrap_or_default();
        gather_inputs.insert(
            "worker__outs".to_string(),
            Value::String(worker_manifest_path.to_string_lossy().to_string()),
        );

        let gather_inputs_json_path = self.repx_dir.join("gather_inputs.json");
        fs::write(
            &gather_inputs_json_path,
            serde_json::to_string_pretty(&gather_inputs)?,
        )?;

        let mut executor = self.create_executor(self.user_out_dir.clone(), self.repx_dir.clone());
        let args = vec![
            self.user_out_dir.to_string_lossy().to_string(),
            gather_inputs_json_path.to_string_lossy().to_string(),
            self.parameters_json_path.to_string_lossy().to_string(),
        ];

        let cancel = CancellationToken::new();
        executor
            .execute_script(exe_path, &args, &cancel)
            .await
            .map_err(|e| CliError::ExecutionFailed {
                message: "Gather script failed".to_string(),
                log_path: Some(self.repx_dir.clone()),
                log_summary: e.to_string(),
            })?;
        Ok(())
    }
}

async fn run_scatter_if_needed(
    orch: &ScatterGatherOrchestrator,
    scatter_exe_path: &Path,
) -> Result<bool, CliError> {
    let already_succeeded = orch.scatter_repx_dir.join(markers::SUCCESS).exists()
        && orch.scatter_out_dir.join("work_items.json").exists();

    if already_succeeded {
        return Ok(true);
    }

    if orch.scatter_out_dir.exists() {
        let _ = fs::remove_dir_all(&orch.scatter_out_dir);
        fs::create_dir_all(&orch.scatter_out_dir)?;
    }

    if let Err(e) = orch.run_scatter(scatter_exe_path).await {
        write_marker(&orch.scatter_repx_dir.join(markers::FAIL))?;
        write_marker(&orch.repx_dir.join(markers::FAIL))?;
        tracing::error!("Scatter failed: {}", e);
        return Err(e);
    }
    write_marker(&orch.scatter_repx_dir.join(markers::SUCCESS))?;
    Ok(false)
}

async fn handle_phase_scatter_only(
    orch: &mut ScatterGatherOrchestrator,
    args: &InternalScatterGatherArgs,
) -> Result<(), CliError> {
    orch.init_dirs()?;

    let skipped = run_scatter_if_needed(orch, &args.scatter_exe_path).await?;
    if skipped {
        tracing::info!("Scatter already succeeded (SUCCESS marker exists), skipping re-execution.");
    }
    Ok(())
}

fn invalidate_stale_step_markers(
    branch_root: &Path,
    work_item_path: &Path,
    new_work_item_json: &str,
    steps: &HashMap<String, StepMeta>,
    branch_idx: usize,
) -> Result<(), CliError> {
    if work_item_path.exists() {
        let old = match fs::read_to_string(work_item_path) {
            Ok(content) => content,
            Err(e) => {
                tracing::warn!(
                    "Failed to read previous work item at '{}': {}. Treating as changed.",
                    work_item_path.display(),
                    e
                );
                String::new()
            }
        };
        if old != new_work_item_json {
            tracing::info!(
                "Branch {} work item changed, invalidating step markers",
                branch_idx
            );
            let topo_order = toposort::toposort_steps(steps)?;
            for s in &topo_order {
                let sr = branch_root.join(format!("step-{}", s)).join(dirs::REPX);
                let _ = fs::remove_file(sr.join(markers::SUCCESS));
                let _ = fs::remove_file(sr.join(markers::FAIL));
            }
        }
    }
    Ok(())
}

fn clear_step_markers(step_repx: &Path) {
    let _ = fs::remove_file(step_repx.join(markers::SUCCESS));
    let _ = fs::remove_file(step_repx.join(markers::FAIL));

    if let Some(step_root) = step_repx.parent() {
        let step_out = step_root.join(dirs::OUT);
        if step_out.exists() {
            let _ = fs::remove_dir_all(&step_out);
        }
    }
}

async fn handle_phase_step(
    orch: &mut ScatterGatherOrchestrator,
    args: &InternalScatterGatherArgs,
    steps_meta: &StepsMetadata,
) -> Result<(), CliError> {
    orch.load_static_inputs()?;
    let branch_idx = args.branch_idx.ok_or_else(|| {
        CliError::Config(CoreError::MissingArgument {
            argument: "--branch-idx".to_string(),
            context: "required for --phase step".to_string(),
        })
    })?;
    let step_name = args.step_name.as_ref().ok_or_else(|| {
        CliError::Config(CoreError::MissingArgument {
            argument: "--step-name".to_string(),
            context: "required for --phase step".to_string(),
        })
    })?;
    let step_meta = steps_meta.steps.get(step_name).ok_or_else(|| {
        CliError::Config(CoreError::StepError {
            detail: format!("Step '{}' not found in steps metadata", step_name),
        })
    })?;

    let branch_root = orch.job_root.join(format!("branch-{}", branch_idx));
    let branch_repx = branch_root.join(dirs::REPX);
    fs::create_dir_all(&branch_repx)?;

    let work_items_str = fs::read_to_string(orch.scatter_out_dir.join("work_items.json"))?;
    let work_items: Vec<Value> = serde_json::from_str(&work_items_str)?;
    let item = work_items.get(branch_idx).ok_or_else(|| {
        CliError::Config(CoreError::InvalidConfig {
            detail: format!(
                "Branch index {} out of range (only {} work items)",
                branch_idx,
                work_items.len()
            ),
        })
    })?;
    let new_work_item_json = serde_json::to_string(item)?;

    let work_item_path = branch_repx.join("work_item.json");
    invalidate_stale_step_markers(
        &branch_root,
        &work_item_path,
        &new_work_item_json,
        &steps_meta.steps,
        branch_idx,
    )?;
    fs::write(&work_item_path, &new_work_item_json)?;

    let step_out = branch_root
        .join(format!("step-{}", step_name))
        .join(dirs::OUT);
    let step_repx = branch_root
        .join(format!("step-{}", step_name))
        .join(dirs::REPX);
    fs::create_dir_all(&step_repx)?;
    clear_step_markers(&step_repx);
    fs::create_dir_all(&step_out)?;

    let inputs = inputs::resolve_step_inputs(
        step_meta,
        &branch_root,
        &work_item_path,
        &orch.static_inputs,
        &steps_meta.steps,
    )?;
    let step_inputs_path = step_repx.join("inputs.json");
    fs::write(&step_inputs_path, serde_json::to_string_pretty(&inputs)?)?;

    let mut executor = orch.create_executor(step_out.clone(), step_repx.clone());
    let exec_args = vec![
        step_out.to_string_lossy().to_string(),
        step_inputs_path.to_string_lossy().to_string(),
        orch.parameters_json_path.to_string_lossy().to_string(),
    ];

    let cancel = CancellationToken::new();
    match executor
        .execute_script(&step_meta.exe_path, &exec_args, &cancel)
        .await
    {
        Ok(_) => {
            write_marker(&step_repx.join(markers::SUCCESS))?;
            tracing::info!(
                "Branch #{} step '{}' completed successfully.",
                branch_idx,
                step_name
            );
        }
        Err(e) => {
            let _ = write_marker(&step_repx.join(markers::FAIL));
            return Err(CliError::ExecutionFailed {
                message: format!("Branch #{} step '{}' failed", branch_idx, step_name),
                log_path: Some(step_repx),
                log_summary: e.to_string(),
            });
        }
    }
    Ok(())
}

async fn handle_phase_gather(
    orch: &mut ScatterGatherOrchestrator,
    args: &InternalScatterGatherArgs,
    sink_step: &str,
) -> Result<(), CliError> {
    orch.init_dirs()?;
    let work_items_str = fs::read_to_string(orch.scatter_out_dir.join("work_items.json"))?;
    let work_items: Vec<Value> = serde_json::from_str(&work_items_str)?;

    let mut branch_sink_out_dirs = Vec::new();
    for i in 0..work_items.len() {
        let branch_root = orch.job_root.join(format!("branch-{}", i));
        let sink_step_repx = branch_root
            .join(format!("step-{}", sink_step))
            .join(dirs::REPX);
        if !sink_step_repx.join(markers::SUCCESS).exists() {
            let msg = format!(
                "Branch #{} sink step '{}' SUCCESS marker not found.",
                i, sink_step
            );
            tracing::error!("{}", msg);
            write_marker(&orch.repx_dir.join(markers::FAIL))?;
            slurm::cancel_workers_from_manifest(&orch.repx_dir).await;
            if let Some(anchor) = args.anchor_id {
                let _ = TokioCommand::new("scancel")
                    .arg(anchor.to_string())
                    .output()
                    .await;
            }
            return Err(CliError::ExecutionFailed {
                message: msg,
                log_path: Some(sink_step_repx),
                log_summary: "Branch did not complete all steps successfully".into(),
            });
        }
        branch_sink_out_dirs.push(
            branch_root
                .join(format!("step-{}", sink_step))
                .join(dirs::OUT),
        );
    }

    match orch
        .run_gather(
            &args.gather_exe_path,
            &branch_sink_out_dirs,
            &args.last_step_outputs_json,
        )
        .await
    {
        Ok(_) => {
            write_marker(&orch.repx_dir.join(markers::SUCCESS))?;
            if let Err(e) = completion_log::append_completion(&orch.base_path, &orch.job_id, true) {
                tracing::debug!("Failed to append to completion log: {}", e);
            }
            if let Some(anchor) = args.anchor_id {
                tracing::info!("Releasing anchor job {}", anchor);
                let _ = TokioCommand::new("scontrol")
                    .arg("release")
                    .arg(anchor.to_string())
                    .output()
                    .await;
            }
        }
        Err(e) => {
            write_marker(&orch.repx_dir.join(markers::FAIL))?;
            if let Err(err) =
                completion_log::append_completion(&orch.base_path, &orch.job_id, false)
            {
                tracing::debug!("Failed to append to completion log: {}", err);
            }
            slurm::cancel_workers_from_manifest(&orch.repx_dir).await;
            if let Some(anchor) = args.anchor_id {
                let _ = TokioCommand::new("scancel")
                    .arg(anchor.to_string())
                    .output()
                    .await;
            }
            return Err(e);
        }
    }
    Ok(())
}

async fn async_handle_scatter_gather(
    mut args: InternalScatterGatherArgs,
    verbose: repx_core::logging::Verbosity,
) -> Result<(), CliError> {
    tracing::debug!(
        "INTERNAL SCATTER-GATHER (Phase: {}) starting for job '{}'",
        args.phase,
        args.job_id
    );

    let resolve = |p: &std::path::Path| -> std::path::PathBuf {
        crate::commands::resolve_to_local_artifacts(p, &args.base_path, &args.local_artifacts_path)
    };
    args.scatter_exe_path = resolve(&args.scatter_exe_path);
    args.gather_exe_path = resolve(&args.gather_exe_path);
    args.job_package_path = resolve(&args.job_package_path);

    let steps_meta: StepsMetadata = serde_json::from_str(&args.steps_json).map_err(|e| {
        CliError::Config(CoreError::SerializationError(format!(
            "Failed to parse --steps-json: {}",
            e
        )))
    })?;

    if steps_meta.steps.is_empty() {
        return Err(CliError::Config(CoreError::InvalidConfig {
            detail: "No steps defined in --steps-json. At least one step is required.".to_string(),
        }));
    }

    let topo_order = toposort::toposort_steps(&steps_meta.steps)?;
    let sink_step = &steps_meta.sink_step;

    if !steps_meta.steps.contains_key(sink_step) {
        return Err(CliError::Config(CoreError::StepError {
            detail: format!("Sink step '{}' not found in steps metadata", sink_step),
        }));
    }

    let mut orch = ScatterGatherOrchestrator::new(&args)?;

    match args.phase {
        ScatterGatherPhase::ScatterOnly => {
            return handle_phase_scatter_only(&mut orch, &args).await;
        }
        ScatterGatherPhase::Step => {
            return handle_phase_step(&mut orch, &args, &steps_meta).await;
        }
        ScatterGatherPhase::Gather => {
            return handle_phase_gather(&mut orch, &args, sink_step).await;
        }
        ScatterGatherPhase::All => {}
    }

    orch.init_dirs()?;
    tracing::info!(
        "Orchestrating scatter-gather stage '{}' with {} step(s) in DAG order: {:?}",
        orch.job_id,
        steps_meta.steps.len(),
        topo_order
    );

    match run_scatter_if_needed(&orch, &args.scatter_exe_path).await {
        Ok(true) => {
            tracing::info!(
                "[1/4] Scatter already succeeded (SUCCESS marker exists), skipping re-execution."
            );
        }
        Ok(false) => {}
        Err(e) => {
            slurm::cancel_workers_from_manifest(&orch.repx_dir).await;
            if let Some(anchor) = args.anchor_id {
                let _ = TokioCommand::new("scancel")
                    .arg(anchor.to_string())
                    .output()
                    .await;
            }
            return Err(e);
        }
    }

    tracing::info!("[2/4] Scatter finished. Reading work items...");
    let work_items_str = fs::read_to_string(orch.scatter_out_dir.join("work_items.json"))?;
    let work_items: Vec<Value> = serde_json::from_str(&work_items_str)?;

    match args.scheduler {
        repx_core::model::SchedulerType::Slurm => {
            let (last_step_slurm_ids, all_worker_slurm_ids) = slurm::submit_slurm_branches(
                &orch,
                &args,
                &work_items,
                &steps_meta,
                &topo_order,
                &args.step_sbatch_opts,
            )
            .await?;

            let manifest_path = orch.repx_dir.join(manifests::WORKER_SLURM_IDS);
            let manifest_json = serde_json::to_string(&all_worker_slurm_ids)?;
            fs::write(&manifest_path, manifest_json)?;
            tracing::info!(
                "Wrote {} worker SLURM IDs to {}",
                all_worker_slurm_ids.len(),
                manifest_path.display()
            );

            slurm::submit_slurm_gather_job(&orch, &args, &last_step_slurm_ids, verbose).await?;

            tracing::info!(
                "Orchestrator finished submitting branches and gather job. Exiting to free slot."
            );
        }
        other => {
            return Err(CliError::Config(CoreError::UnsupportedValue {
                kind: "scheduler".to_string(),
                value: other.to_string(),
            }));
        }
    }

    Ok(())
}

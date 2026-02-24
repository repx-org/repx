use crate::{cli::InternalScatterGatherArgs, error::CliError};
use futures::future::join_all;
use repx_core::{
    constants::{dirs, markers},
    errors::ConfigError,
    model::JobId,
};
use repx_executor::{ExecutionRequest, Executor, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use tokio::{process::Command as TokioCommand, runtime::Runtime as TokioRuntime};

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
    pub mem: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepsMetadata {
    pub steps: HashMap<String, StepMeta>,
    pub sink_step: String,
}

fn toposort_steps(steps: &HashMap<String, StepMeta>) -> Result<Vec<String>, CliError> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for (name, meta) in steps {
        in_degree.entry(name.as_str()).or_insert(0);
        for dep in &meta.deps {
            if !steps.contains_key(dep) {
                return Err(CliError::Config(ConfigError::General(format!(
                    "Step '{}' depends on unknown step '{}'",
                    name, dep
                ))));
            }
            *in_degree.entry(name.as_str()).or_insert(0) += 1;
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(name.as_str());
        }
    }

    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&name, _)| name)
        .collect();
    queue.sort();

    let mut result = Vec::new();
    while let Some(name) = queue.pop() {
        result.push(name.to_string());
        if let Some(deps) = dependents.get(name) {
            let mut newly_ready = Vec::new();
            for &dep_name in deps {
                let deg = in_degree.get_mut(dep_name).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    newly_ready.push(dep_name);
                }
            }
            newly_ready.sort();
            newly_ready.reverse();
            queue.extend(newly_ready);
        }
    }

    if result.len() != steps.len() {
        return Err(CliError::Config(ConfigError::General(
            "Cycle detected in step dependency graph".into(),
        )));
    }

    Ok(result)
}

pub fn handle_scatter_gather(args: InternalScatterGatherArgs) -> Result<(), CliError> {
    let rt = TokioRuntime::new().unwrap();
    rt.block_on(async_handle_scatter_gather(args))
}

struct ScatterGatherOrchestrator {
    job_id: JobId,
    base_path: PathBuf,
    job_root: PathBuf,
    user_out_dir: PathBuf,
    repx_dir: PathBuf,
    scatter_out_dir: PathBuf,
    scatter_repx_dir: PathBuf,
    inputs_json_path: PathBuf,
    runtime: Runtime,
    job_package_path: PathBuf,
    static_inputs: Value,
    host_tools_bin_dir: Option<PathBuf>,
    node_local_path: Option<PathBuf>,
    mount_host_paths: bool,
    mount_paths: Vec<String>,
}

impl ScatterGatherOrchestrator {
    fn new(args: &InternalScatterGatherArgs) -> Result<Self, CliError> {
        let job_id = JobId(args.job_id.clone());
        let job_root = args.base_path.join(dirs::OUTPUTS).join(&job_id.0);
        let user_out_dir = job_root.join(dirs::OUT);
        let repx_dir = job_root.join(dirs::REPX);
        let scatter_root = job_root.join("scatter");
        let scatter_out_dir = scatter_root.join(dirs::OUT);
        let scatter_repx_dir = scatter_root.join(dirs::REPX);
        let inputs_json_path = repx_dir.join("inputs.json");

        let runtime = match args.runtime.as_str() {
            "native" => Runtime::Native,
            "podman" => Runtime::Podman {
                image_tag: args.image_tag.clone().ok_or_else(|| {
                    CliError::Config(ConfigError::General(
                        "Podman runtime requires --image-tag".into(),
                    ))
                })?,
            },
            "docker" => Runtime::Docker {
                image_tag: args.image_tag.clone().ok_or_else(|| {
                    CliError::Config(ConfigError::General(
                        "Docker runtime requires --image-tag".into(),
                    ))
                })?,
            },
            "bwrap" => Runtime::Bwrap {
                image_tag: args.image_tag.clone().ok_or_else(|| {
                    CliError::Config(ConfigError::General(
                        "Bwrap runtime requires --image-tag".into(),
                    ))
                })?,
            },
            _ => {
                return Err(CliError::Config(ConfigError::General(format!(
                    "Unsupported runtime: {}",
                    args.runtime
                ))))
            }
        };
        let host_tools_root = args.base_path.join("artifacts").join("host-tools");
        let host_tools_bin_dir = Some(host_tools_root.join(&args.host_tools_dir).join("bin"));

        Ok(Self {
            job_id,
            base_path: args.base_path.clone(),
            job_root,
            user_out_dir,
            repx_dir,
            scatter_out_dir,
            scatter_repx_dir,
            inputs_json_path,
            runtime,
            job_package_path: args.job_package_path.clone(),
            static_inputs: Value::Object(Default::default()),
            host_tools_bin_dir,
            node_local_path: args.node_local_path.clone(),
            mount_host_paths: args.mount_host_paths,
            mount_paths: args.mount_paths.clone(),
        })
    }

    fn init_dirs(&mut self) -> Result<(), CliError> {
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

        if self.inputs_json_path.exists() {
            self.static_inputs =
                serde_json::from_str(&fs::read_to_string(&self.inputs_json_path)?)?;
        }
        Ok(())
    }

    fn create_executor(&self, user_out: PathBuf, repx_out: PathBuf) -> Executor {
        Executor::new(ExecutionRequest {
            job_id: self.job_id.clone(),
            runtime: self.runtime.clone(),
            base_path: self.base_path.clone(),
            node_local_path: self.node_local_path.clone(),
            job_package_path: self.job_package_path.clone(),
            inputs_json_path: self.inputs_json_path.clone(),
            user_out_dir: user_out,
            repx_out_dir: repx_out,
            host_tools_bin_dir: self.host_tools_bin_dir.clone(),
            mount_host_paths: self.mount_host_paths,
            mount_paths: self.mount_paths.clone(),
        })
    }

    async fn run_scatter(&self, exe_path: &Path) -> Result<(), CliError> {
        tracing::info!("[1/4] Starting scatter phase for job '{}'...", self.job_id);
        let executor =
            self.create_executor(self.scatter_out_dir.clone(), self.scatter_repx_dir.clone());
        let args = vec![
            self.scatter_out_dir.to_string_lossy().to_string(),
            self.inputs_json_path.to_string_lossy().to_string(),
        ];
        executor
            .execute_script(exe_path, &args)
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
                    CliError::Config(ConfigError::General(format!(
                        "Last step output template for '{}' must be a string.",
                        name
                    )))
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

        let executor = self.create_executor(self.user_out_dir.clone(), self.repx_dir.clone());
        let args = vec![
            self.user_out_dir.to_string_lossy().to_string(),
            gather_inputs_json_path.to_string_lossy().to_string(),
        ];

        executor
            .execute_script(exe_path, &args)
            .await
            .map_err(|e| CliError::ExecutionFailed {
                message: "Gather script failed".to_string(),
                log_path: Some(self.repx_dir.clone()),
                log_summary: e.to_string(),
            })?;
        Ok(())
    }
}

fn resolve_step_inputs(
    step_meta: &StepMeta,
    branch_root: &Path,
    work_item_path: &Path,
    static_inputs: &Value,
    steps_meta: &HashMap<String, StepMeta>,
) -> Result<serde_json::Map<String, Value>, CliError> {
    let mut inputs = serde_json::Map::new();

    for mapping in &step_meta.inputs {
        let target = &mapping.target_input;

        if let Some(source) = &mapping.source {
            if source == "scatter:work_item" {
                inputs.insert(
                    target.clone(),
                    Value::String(work_item_path.to_string_lossy().to_string()),
                );
            } else if let Some(dep_name) = source.strip_prefix("step:") {
                let source_output = mapping.source_output.as_ref().ok_or_else(|| {
                    CliError::Config(ConfigError::General(format!(
                        "Step input mapping with source '{}' missing source_output",
                        source
                    )))
                })?;

                let dep_meta = steps_meta.get(dep_name).ok_or_else(|| {
                    CliError::Config(ConfigError::General(format!(
                        "Step input references unknown step '{}'",
                        dep_name
                    )))
                })?;

                let template = dep_meta.outputs.get(source_output).ok_or_else(|| {
                    CliError::Config(ConfigError::General(format!(
                        "Step '{}' does not have output '{}'",
                        dep_name, source_output
                    )))
                })?;

                let dep_out_dir = branch_root
                    .join(format!("step-{}", dep_name))
                    .join(dirs::OUT);
                let resolved_path = template.replace("$out", &dep_out_dir.to_string_lossy());
                inputs.insert(target.clone(), Value::String(resolved_path));
            } else {
                tracing::warn!(
                    "Unknown source type '{}' for input '{}', skipping",
                    source,
                    target
                );
            }
        } else if mapping.job_id.is_some() {
            if let Some(static_obj) = static_inputs.as_object() {
                if let Some(val) = static_obj.get(target) {
                    inputs.insert(target.clone(), val.clone());
                } else {
                    tracing::warn!(
                        "External input '{}' not found in static_inputs, skipping",
                        target
                    );
                }
            }
        }
    }

    Ok(inputs)
}

async fn async_handle_scatter_gather(args: InternalScatterGatherArgs) -> Result<(), CliError> {
    tracing::debug!(
        "INTERNAL SCATTER-GATHER (Phase: {}) starting for job '{}'",
        args.phase,
        args.job_id
    );

    let steps_meta: StepsMetadata = serde_json::from_str(&args.steps_json).map_err(|e| {
        CliError::Config(ConfigError::General(format!(
            "Failed to parse --steps-json: {}",
            e
        )))
    })?;

    if steps_meta.steps.is_empty() {
        return Err(CliError::Config(ConfigError::General(
            "No steps defined in --steps-json. At least one step is required.".into(),
        )));
    }

    let topo_order = toposort_steps(&steps_meta.steps)?;
    let sink_step = &steps_meta.sink_step;

    if !steps_meta.steps.contains_key(sink_step) {
        return Err(CliError::Config(ConfigError::General(format!(
            "Sink step '{}' not found in steps metadata",
            sink_step
        ))));
    }

    let mut orch = ScatterGatherOrchestrator::new(&args)?;

    if args.phase == "gather" {
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
                fs::File::create(orch.repx_dir.join(markers::FAIL))?;
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
                fs::File::create(orch.repx_dir.join(markers::SUCCESS))?;
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
                fs::File::create(orch.repx_dir.join(markers::FAIL))?;
                if let Some(anchor) = args.anchor_id {
                    let _ = TokioCommand::new("scancel")
                        .arg(anchor.to_string())
                        .output()
                        .await;
                }
                return Err(e);
            }
        }
        return Ok(());
    }

    orch.init_dirs()?;
    tracing::info!(
        "Orchestrating scatter-gather stage '{}' with {} step(s) in DAG order: {:?}",
        orch.job_id,
        steps_meta.steps.len(),
        topo_order
    );

    if let Err(e) = orch.run_scatter(&args.scatter_exe_path).await {
        let _ = fs::File::create(orch.scatter_repx_dir.join(markers::FAIL));
        fs::File::create(orch.repx_dir.join(markers::FAIL))?;
        if let Some(anchor) = args.anchor_id {
            let _ = TokioCommand::new("scancel")
                .arg(anchor.to_string())
                .output()
                .await;
        }
        tracing::error!("Scatter failed: {}", e);
        return Err(e);
    }
    let _ = fs::File::create(orch.scatter_repx_dir.join(markers::SUCCESS));

    tracing::info!("[2/4] Scatter finished. Reading work items...");
    let work_items_str = fs::read_to_string(orch.scatter_out_dir.join("work_items.json"))?;
    let work_items: Vec<Value> = serde_json::from_str(&work_items_str)?;

    if args.scheduler == "local" {
        let branch_sink_out_dirs =
            run_local_branches(&orch, &work_items, &steps_meta, &topo_order).await?;

        if let Err(e) = orch
            .run_gather(
                &args.gather_exe_path,
                &branch_sink_out_dirs,
                &args.last_step_outputs_json,
            )
            .await
        {
            fs::File::create(orch.repx_dir.join(markers::FAIL))?;
            return Err(e);
        }
        fs::File::create(orch.repx_dir.join(markers::SUCCESS))?;
    } else if args.scheduler == "slurm" {
        let last_step_slurm_ids = submit_slurm_branches(
            &orch,
            &work_items,
            &steps_meta,
            &topo_order,
            &args.step_sbatch_opts,
        )
        .await?;

        submit_slurm_gather_job(&orch, &args, &last_step_slurm_ids).await?;

        tracing::info!(
            "Orchestrator finished submitting branches and gather job. Exiting to free slot."
        );
    } else {
        return Err(CliError::Config(ConfigError::General(format!(
            "Unknown scheduler: {}",
            args.scheduler
        ))));
    }

    Ok(())
}

async fn run_local_branches(
    orch: &ScatterGatherOrchestrator,
    work_items: &[Value],
    steps_meta: &StepsMetadata,
    topo_order: &[String],
) -> Result<Vec<PathBuf>, CliError> {
    let num_branches = work_items.len();
    let num_steps = steps_meta.steps.len();

    tracing::info!(
        "[3/4] Running {} branches x {} steps locally...",
        num_branches,
        num_steps
    );

    let mut tasks = Vec::new();

    for (branch_idx, work_item) in work_items.iter().enumerate() {
        let orch_job_root = orch.job_root.clone();
        let orch_base_path = orch.base_path.clone();
        let orch_job_id = orch.job_id.clone();
        let orch_runtime = orch.runtime.clone();
        let orch_job_package_path = orch.job_package_path.clone();
        let orch_inputs_json_path = orch.inputs_json_path.clone();
        let orch_host_tools_bin_dir = orch.host_tools_bin_dir.clone();
        let orch_node_local_path = orch.node_local_path.clone();
        let orch_mount_host_paths = orch.mount_host_paths;
        let orch_mount_paths = orch.mount_paths.clone();
        let orch_static_inputs = orch.static_inputs.clone();
        let steps = steps_meta.clone();
        let topo = topo_order.to_vec();
        let item = work_item.clone();

        tasks.push(tokio::spawn(async move {
            run_single_branch(
                branch_idx,
                &item,
                &steps,
                &topo,
                &orch_job_root,
                &orch_base_path,
                &orch_job_id,
                &orch_runtime,
                &orch_job_package_path,
                &orch_inputs_json_path,
                orch_host_tools_bin_dir.as_deref(),
                orch_node_local_path.as_deref(),
                orch_mount_host_paths,
                &orch_mount_paths,
                &orch_static_inputs,
            )
            .await
        }));
    }

    let results = join_all(tasks).await;
    let mut branch_sink_out_dirs = Vec::new();
    let mut first_failure: Option<CliError> = None;

    for (i, res) in results.into_iter().enumerate() {
        match res {
            Ok(Ok(sink_out)) => {
                branch_sink_out_dirs.push(sink_out);
            }
            Ok(Err(e)) => {
                tracing::error!("Branch #{} failed: {}", i, e);
                if first_failure.is_none() {
                    first_failure = Some(e);
                }
                let placeholder = orch
                    .job_root
                    .join(format!("branch-{}", i))
                    .join(format!("step-{}", steps_meta.sink_step))
                    .join(dirs::OUT);
                branch_sink_out_dirs.push(placeholder);
            }
            Err(e) => {
                tracing::error!("Branch #{} panicked: {}", i, e);
                if first_failure.is_none() {
                    first_failure = Some(CliError::ExecutionFailed {
                        message: format!("Branch #{} panicked", i),
                        log_path: None,
                        log_summary: e.to_string(),
                    });
                }
                let placeholder = orch
                    .job_root
                    .join(format!("branch-{}", i))
                    .join(format!("step-{}", steps_meta.sink_step))
                    .join(dirs::OUT);
                branch_sink_out_dirs.push(placeholder);
            }
        }
    }

    if let Some(e) = first_failure {
        fs::File::create(orch.repx_dir.join(markers::FAIL))?;
        return Err(e);
    }

    Ok(branch_sink_out_dirs)
}

#[allow(clippy::too_many_arguments)]
async fn run_single_branch(
    branch_idx: usize,
    work_item: &Value,
    steps_meta: &StepsMetadata,
    topo_order: &[String],
    job_root: &Path,
    base_path: &Path,
    job_id: &JobId,
    runtime: &Runtime,
    job_package_path: &Path,
    inputs_json_path: &Path,
    host_tools_bin_dir: Option<&Path>,
    node_local_path: Option<&Path>,
    mount_host_paths: bool,
    mount_paths: &[String],
    static_inputs: &Value,
) -> Result<PathBuf, CliError> {
    let branch_root = job_root.join(format!("branch-{}", branch_idx));

    let branch_repx = branch_root.join(dirs::REPX);
    fs::create_dir_all(&branch_repx)?;
    let work_item_path = branch_repx.join("work_item.json");
    fs::write(&work_item_path, serde_json::to_string(work_item)?)?;

    for step_name in topo_order {
        let step_meta = steps_meta
            .steps
            .get(step_name)
            .expect("toposort returned unknown step name");

        let step_root = branch_root.join(format!("step-{}", step_name));
        let step_out = step_root.join(dirs::OUT);
        let step_repx = step_root.join(dirs::REPX);

        if step_repx.join(markers::SUCCESS).exists() {
            tracing::debug!(
                "Branch {} step '{}' already succeeded, skipping",
                branch_idx,
                step_name
            );
            continue;
        }

        fs::create_dir_all(&step_out)?;
        fs::create_dir_all(&step_repx)?;
        let _ = fs::remove_file(step_repx.join(markers::SUCCESS));
        let _ = fs::remove_file(step_repx.join(markers::FAIL));

        let inputs = resolve_step_inputs(
            step_meta,
            &branch_root,
            &work_item_path,
            static_inputs,
            &steps_meta.steps,
        )?;

        let step_inputs_path = step_repx.join("inputs.json");
        fs::write(&step_inputs_path, serde_json::to_string_pretty(&inputs)?)?;

        let executor = Executor::new(ExecutionRequest {
            job_id: job_id.clone(),
            runtime: runtime.clone(),
            base_path: base_path.to_path_buf(),
            node_local_path: node_local_path.map(|p| p.to_path_buf()),
            job_package_path: job_package_path.to_path_buf(),
            inputs_json_path: inputs_json_path.to_path_buf(),
            user_out_dir: step_out.clone(),
            repx_out_dir: step_repx.clone(),
            host_tools_bin_dir: host_tools_bin_dir.map(|p| p.to_path_buf()),
            mount_host_paths,
            mount_paths: mount_paths.to_vec(),
        });

        let exe_args = vec![
            step_out.to_string_lossy().to_string(),
            step_inputs_path.to_string_lossy().to_string(),
        ];

        let result = executor
            .execute_script(&step_meta.exe_path, &exe_args)
            .await;

        match result {
            Ok(_) => {
                let _ = fs::File::create(step_repx.join(markers::SUCCESS));
                tracing::debug!(
                    "Branch {} step '{}' completed successfully",
                    branch_idx,
                    step_name
                );
            }
            Err(e) => {
                let _ = fs::File::create(step_repx.join(markers::FAIL));
                return Err(CliError::ExecutionFailed {
                    message: format!("Branch #{} step '{}' failed", branch_idx, step_name),
                    log_path: Some(step_repx),
                    log_summary: e.to_string(),
                });
            }
        }
    }

    let sink_out = branch_root
        .join(format!("step-{}", steps_meta.sink_step))
        .join(dirs::OUT);
    Ok(sink_out)
}

async fn submit_slurm_gather_job(
    orch: &ScatterGatherOrchestrator,
    args: &InternalScatterGatherArgs,
    last_step_slurm_ids: &[String],
) -> Result<(), CliError> {
    let current_exe = std::env::current_exe()?;
    let current_exe_str = current_exe.to_string_lossy();

    let steps_json_escaped = args.steps_json.replace('\'', "'\\''");

    let mut gather_cmd_parts = vec![
        current_exe_str.to_string(),
        "internal-scatter-gather".to_string(),
        "--phase".to_string(),
        "gather".to_string(),
        "--job-id".to_string(),
        args.job_id.clone(),
        "--runtime".to_string(),
        args.runtime.clone(),
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
    ];

    if args.mount_host_paths {
        gather_cmd_parts.push("--mount-host-paths".to_string());
    }

    for path in &args.mount_paths {
        gather_cmd_parts.push("--mount-paths".to_string());
        gather_cmd_parts.push(path.clone());
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
        .arg(format!("--job-name={}-gather", orch.job_id.0))
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

async fn submit_slurm_branches(
    orch: &ScatterGatherOrchestrator,
    work_items: &[Value],
    steps_meta: &StepsMetadata,
    topo_order: &[String],
    sbatch_opts: &str,
) -> Result<Vec<String>, CliError> {
    let mut last_step_slurm_ids = Vec::new();

    for (branch_idx, item) in work_items.iter().enumerate() {
        let branch_root = orch.job_root.join(format!("branch-{}", branch_idx));
        let branch_repx = branch_root.join(dirs::REPX);
        fs::create_dir_all(&branch_repx)?;

        let work_item_path = branch_repx.join("work_item.json");
        fs::write(&work_item_path, serde_json::to_string(item)?)?;

        let mut step_slurm_ids: HashMap<String, String> = HashMap::new();

        for step_name in topo_order {
            let step_meta = steps_meta.steps.get(step_name).unwrap();
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
                markers::SUCCESS,
                step_repx.display(),
                markers::FAIL
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
                    orch.job_id.0, branch_idx, step_name
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
            step_slurm_ids.insert(step_name.clone(), slurm_id);
        }

        if let Some(sink_slurm_id) = step_slurm_ids.get(&steps_meta.sink_step) {
            last_step_slurm_ids.push(sink_slurm_id.clone());
        } else {
            return Err(CliError::Config(ConfigError::General(format!(
                "Sink step '{}' was not submitted for branch #{}",
                steps_meta.sink_step, branch_idx
            ))));
        }
    }

    tracing::info!(
        "Submitted {} branches ({} steps each) to Slurm.",
        work_items.len(),
        topo_order.len()
    );
    Ok(last_step_slurm_ids)
}

fn command_to_shell_string(cmd: &TokioCommand) -> String {
    let program = cmd.as_std().get_program().to_string_lossy();
    let args: Vec<String> = cmd
        .as_std()
        .get_args()
        .map(|arg| format!("'{}'", arg.to_string_lossy().replace('\'', "'\\''")))
        .collect();
    format!("{} {}", program, args.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_toposort_single_step() {
        let mut steps = HashMap::new();
        steps.insert(
            "compute".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/compute"),
                deps: vec![],
                outputs: HashMap::new(),
                inputs: vec![],
                resource_hints: None,
            },
        );
        let order = toposort_steps(&steps).unwrap();
        assert_eq!(order, vec!["compute"]);
    }

    #[test]
    fn test_toposort_linear_chain() {
        let mut steps = HashMap::new();
        steps.insert(
            "a".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/a"),
                deps: vec![],
                outputs: HashMap::new(),
                inputs: vec![],
                resource_hints: None,
            },
        );
        steps.insert(
            "b".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/b"),
                deps: vec!["a".to_string()],
                outputs: HashMap::new(),
                inputs: vec![],
                resource_hints: None,
            },
        );
        steps.insert(
            "c".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/c"),
                deps: vec!["b".to_string()],
                outputs: HashMap::new(),
                inputs: vec![],
                resource_hints: None,
            },
        );
        let order = toposort_steps(&steps).unwrap();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_toposort_diamond() {
        let mut steps = HashMap::new();
        steps.insert(
            "root".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/root"),
                deps: vec![],
                outputs: HashMap::new(),
                inputs: vec![],
                resource_hints: None,
            },
        );
        steps.insert(
            "left".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/left"),
                deps: vec!["root".to_string()],
                outputs: HashMap::new(),
                inputs: vec![],
                resource_hints: None,
            },
        );
        steps.insert(
            "right".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/right"),
                deps: vec!["root".to_string()],
                outputs: HashMap::new(),
                inputs: vec![],
                resource_hints: None,
            },
        );
        steps.insert(
            "sink".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/sink"),
                deps: vec!["left".to_string(), "right".to_string()],
                outputs: HashMap::new(),
                inputs: vec![],
                resource_hints: None,
            },
        );
        let order = toposort_steps(&steps).unwrap();
        assert_eq!(order[0], "root");
        assert_eq!(order[3], "sink");
        let middle: HashSet<&str> = order[1..3].iter().map(|s| s.as_str()).collect();
        assert!(middle.contains("left"));
        assert!(middle.contains("right"));
    }

    #[test]
    fn test_toposort_cycle_detection() {
        let mut steps = HashMap::new();
        steps.insert(
            "a".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/a"),
                deps: vec!["b".to_string()],
                outputs: HashMap::new(),
                inputs: vec![],
                resource_hints: None,
            },
        );
        steps.insert(
            "b".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/b"),
                deps: vec!["a".to_string()],
                outputs: HashMap::new(),
                inputs: vec![],
                resource_hints: None,
            },
        );
        let result = toposort_steps(&steps);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cycle detected"));
    }

    #[test]
    fn test_toposort_unknown_dep() {
        let mut steps = HashMap::new();
        steps.insert(
            "a".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/a"),
                deps: vec!["nonexistent".to_string()],
                outputs: HashMap::new(),
                inputs: vec![],
                resource_hints: None,
            },
        );
        let result = toposort_steps(&steps);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown step"));
    }

    #[test]
    fn test_steps_metadata_deserialize() {
        let json = r#"{
            "steps": {
                "compute": {
                    "exe_path": "/nix/store/abc/bin/step-compute",
                    "deps": [],
                    "outputs": {"partial_sum": "$out/worker-result.txt"},
                    "inputs": [
                        {"source": "scatter:work_item", "target_input": "worker__item"},
                        {"job_id": "xyz-stage-C-1.1", "source_output": "combined_list", "target_input": "number_list_file", "type": "intra-pipeline"}
                    ]
                }
            },
            "sink_step": "compute"
        }"#;
        let meta: StepsMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.steps.len(), 1);
        assert_eq!(meta.sink_step, "compute");
        let compute = &meta.steps["compute"];
        assert!(compute.deps.is_empty());
        assert_eq!(compute.outputs.len(), 1);
        assert_eq!(compute.inputs.len(), 2);
    }

    #[test]
    fn test_steps_metadata_diamond_deserialize() {
        let json = r#"{
            "steps": {
                "trace_gen": {
                    "exe_path": "/bin/trace-gen",
                    "deps": [],
                    "outputs": {"trace": "$out/trace.bin"},
                    "inputs": [
                        {"source": "scatter:work_item", "target_input": "worker__item"}
                    ]
                },
                "trace_align": {
                    "exe_path": "/bin/trace-align",
                    "deps": ["trace_gen"],
                    "outputs": {"aligned": "$out/aligned.bin"},
                    "inputs": [
                        {"source": "step:trace_gen", "source_output": "trace", "target_input": "trace"}
                    ]
                },
                "trace_analyze": {
                    "exe_path": "/bin/trace-analyze",
                    "deps": ["trace_gen"],
                    "outputs": {"analysis": "$out/analysis.json"},
                    "inputs": [
                        {"source": "step:trace_gen", "source_output": "trace", "target_input": "trace"}
                    ]
                },
                "foldability": {
                    "exe_path": "/bin/foldability",
                    "deps": ["trace_align", "trace_analyze"],
                    "outputs": {"result": "$out/fold.json"},
                    "inputs": [
                        {"source": "step:trace_align", "source_output": "aligned", "target_input": "aligned"},
                        {"source": "step:trace_analyze", "source_output": "analysis", "target_input": "analysis"}
                    ]
                }
            },
            "sink_step": "foldability"
        }"#;
        let meta: StepsMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.steps.len(), 4);
        assert_eq!(meta.sink_step, "foldability");

        let order = toposort_steps(&meta.steps).unwrap();
        assert_eq!(order[0], "trace_gen");
        assert_eq!(order[3], "foldability");
    }

    #[test]
    fn test_resolve_step_inputs_scatter_source() {
        let mut steps = HashMap::new();
        let step = StepMeta {
            exe_path: PathBuf::from("/bin/step"),
            deps: vec![],
            outputs: HashMap::from([("out1".to_string(), "$out/result.txt".to_string())]),
            inputs: vec![StepInputMapping {
                source: Some("scatter:work_item".to_string()),
                source_output: None,
                target_input: "worker__item".to_string(),
                job_id: None,
                mapping_type: None,
            }],
            resource_hints: None,
        };
        steps.insert("compute".to_string(), step.clone());

        let branch_root = PathBuf::from("/tmp/job/branch-0");
        let work_item_path = PathBuf::from("/tmp/job/branch-0/repx/work_item.json");
        let static_inputs = Value::Object(Default::default());

        let result =
            resolve_step_inputs(&step, &branch_root, &work_item_path, &static_inputs, &steps)
                .unwrap();
        assert_eq!(
            result["worker__item"],
            "/tmp/job/branch-0/repx/work_item.json"
        );
    }

    #[test]
    fn test_resolve_step_inputs_step_dep() {
        let mut steps = HashMap::new();
        steps.insert(
            "gen".to_string(),
            StepMeta {
                exe_path: PathBuf::from("/bin/gen"),
                deps: vec![],
                outputs: HashMap::from([("trace".to_string(), "$out/trace.bin".to_string())]),
                inputs: vec![],
                resource_hints: None,
            },
        );

        let consumer = StepMeta {
            exe_path: PathBuf::from("/bin/analyze"),
            deps: vec!["gen".to_string()],
            outputs: HashMap::new(),
            inputs: vec![StepInputMapping {
                source: Some("step:gen".to_string()),
                source_output: Some("trace".to_string()),
                target_input: "input_trace".to_string(),
                job_id: None,
                mapping_type: None,
            }],
            resource_hints: None,
        };
        steps.insert("analyze".to_string(), consumer.clone());

        let branch_root = PathBuf::from("/tmp/job/branch-0");
        let work_item_path = PathBuf::from("/tmp/job/branch-0/repx/work_item.json");
        let static_inputs = Value::Object(Default::default());

        let result = resolve_step_inputs(
            &consumer,
            &branch_root,
            &work_item_path,
            &static_inputs,
            &steps,
        )
        .unwrap();
        assert_eq!(
            result["input_trace"],
            "/tmp/job/branch-0/step-gen/out/trace.bin"
        );
    }

    #[test]
    fn test_resolve_step_inputs_external() {
        let steps = HashMap::new();
        let step = StepMeta {
            exe_path: PathBuf::from("/bin/step"),
            deps: vec![],
            outputs: HashMap::new(),
            inputs: vec![StepInputMapping {
                source: None,
                source_output: Some("combined_list".to_string()),
                target_input: "number_list_file".to_string(),
                job_id: Some("xyz-stage-C-1.1".to_string()),
                mapping_type: Some("intra-pipeline".to_string()),
            }],
            resource_hints: None,
        };

        let branch_root = PathBuf::from("/tmp/job/branch-0");
        let work_item_path = PathBuf::from("/tmp/job/branch-0/repx/work_item.json");
        let static_inputs = serde_json::json!({
            "number_list_file": "/outputs/xyz-stage-C-1.1/out/combined_list.txt"
        });

        let result =
            resolve_step_inputs(&step, &branch_root, &work_item_path, &static_inputs, &steps)
                .unwrap();
        assert_eq!(
            result["number_list_file"],
            "/outputs/xyz-stage-C-1.1/out/combined_list.txt"
        );
    }
}

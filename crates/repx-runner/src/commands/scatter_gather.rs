use crate::cli::InternalScatterGatherArgs;
use futures::future::join_all;
use repx_core::{error::AppError, log_debug, log_error, log_info, model::JobId};
use repx_executor::{ExecutionRequest, Executor, Runtime};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tokio::{process::Command as TokioCommand, runtime::Runtime as TokioRuntime};

pub fn handle_scatter_gather(args: InternalScatterGatherArgs) -> Result<(), AppError> {
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
    fn new(args: &InternalScatterGatherArgs) -> Result<Self, AppError> {
        let job_id = JobId(args.job_id.clone());
        let job_root = args.base_path.join("outputs").join(&job_id.0);
        let user_out_dir = job_root.join("out");
        let repx_dir = job_root.join("repx");
        let scatter_root = job_root.join("scatter");
        let scatter_out_dir = scatter_root.join("out");
        let scatter_repx_dir = scatter_root.join("repx");
        let inputs_json_path = repx_dir.join("inputs.json");

        let runtime = match args.runtime.as_str() {
            "native" => Runtime::Native,
            "podman" => Runtime::Podman {
                image_tag: args.image_tag.clone().ok_or_else(|| {
                    AppError::ConfigurationError("Podman runtime requires --image-tag".into())
                })?,
            },
            "docker" => Runtime::Docker {
                image_tag: args.image_tag.clone().ok_or_else(|| {
                    AppError::ConfigurationError("Docker runtime requires --image-tag".into())
                })?,
            },
            "bwrap" => Runtime::Bwrap {
                image_tag: args.image_tag.clone().ok_or_else(|| {
                    AppError::ConfigurationError("Bwrap runtime requires --image-tag".into())
                })?,
            },
            other => {
                return Err(AppError::ConfigurationError(format!(
                    "Unsupported runtime: {}",
                    other
                )))
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
    fn init_dirs(&mut self) -> Result<(), AppError> {
        for dir in [
            &self.user_out_dir,
            &self.repx_dir,
            &self.scatter_out_dir,
            &self.scatter_repx_dir,
        ] {
            fs::create_dir_all(dir)?;
        }
        let _ = fs::remove_file(self.repx_dir.join("SUCCESS"));
        let _ = fs::remove_file(self.repx_dir.join("FAIL"));

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
    async fn run_scatter(&self, exe_path: &Path) -> Result<(), AppError> {
        log_info!("[1/4] Starting scatter phase for job '{}'...", self.job_id);
        let executor =
            self.create_executor(self.scatter_out_dir.clone(), self.scatter_repx_dir.clone());
        let args = vec![
            self.scatter_out_dir.to_string_lossy().to_string(),
            self.inputs_json_path.to_string_lossy().to_string(),
        ];
        executor
            .execute_script(exe_path, &args)
            .await
            .map_err(|e| AppError::ExecutionFailed {
                message: format!("Scatter phase failed for job {}", self.job_id),
                log_path: Some(self.scatter_repx_dir.clone()),
                log_summary: e.to_string(),
            })
    }
    async fn run_gather(
        &self,
        exe_path: &Path,
        worker_output_dirs: &[PathBuf],
        worker_outputs_template_json: &str,
    ) -> Result<(), AppError> {
        log_info!("[4/4] All workers completed. Starting gather phase...");

        let mut worker_outs_manifest = Vec::new();
        let worker_job_outputs: HashMap<String, Value> =
            serde_json::from_str(worker_outputs_template_json)?;

        for worker_out_dir in worker_output_dirs {
            let mut worker_outputs = HashMap::new();
            for (name, template) in &worker_job_outputs {
                let template_str = template.as_str().ok_or_else(|| {
                    AppError::ConfigurationError(format!(
                        "Worker output template for '{}' must be a string.",
                        name
                    ))
                })?;
                let path = template_str.replace("$out", &worker_out_dir.to_string_lossy());
                worker_outputs.insert(name.clone(), path);
            }
            worker_outs_manifest.push(worker_outputs);
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
            .map_err(|e| AppError::ExecutionFailed {
                message: format!("Gather phase failed for job {}", self.job_id),
                log_path: Some(self.repx_dir.clone()),
                log_summary: e.to_string(),
            })
    }

    fn prepare_worker(
        &self,
        idx: usize,
        work_item: &Value,
    ) -> Result<(PathBuf, PathBuf, PathBuf), AppError> {
        let worker_root = self.job_root.join(format!("worker-{}", idx));
        let worker_out = worker_root.join("out");
        let worker_repx = worker_root.join("repx");
        fs::create_dir_all(&worker_out)?;
        fs::create_dir_all(&worker_repx)?;

        let mut inputs = self.static_inputs.as_object().cloned().unwrap_or_default();
        let item_path = worker_repx.join("work_item.json");
        fs::write(&item_path, serde_json::to_string(work_item)?)?;

        inputs.insert(
            "worker__item".to_string(),
            Value::String(item_path.to_string_lossy().to_string()),
        );

        let inputs_path = worker_repx.join("inputs.json");
        fs::write(&inputs_path, serde_json::to_string_pretty(&inputs)?)?;

        Ok((worker_out, worker_repx, inputs_path))
    }
}

async fn async_handle_scatter_gather(args: InternalScatterGatherArgs) -> Result<(), AppError> {
    log_debug!(
        "INTERNAL SCATTER-GATHER (Phase: {}) starting for job '{}'",
        args.phase,
        args.job_id
    );

    let mut orch = ScatterGatherOrchestrator::new(&args)?;

    if args.phase == "gather" {
        orch.init_dirs()?;
        let work_items_str = fs::read_to_string(orch.scatter_out_dir.join("work_items.json"))?;
        let work_items: Vec<Value> = serde_json::from_str(&work_items_str)?;

        let mut worker_out_dirs = Vec::new();
        for i in 0..work_items.len() {
            let worker_root = orch.job_root.join(format!("worker-{}", i));
            let worker_repx = worker_root.join("repx");
            if !worker_repx.join("SUCCESS").exists() {
                let msg = format!("Worker #{} SUCCESS marker not found.", i);
                log_error!("{}", msg);
                fs::File::create(orch.repx_dir.join("FAIL"))?;
                if let Some(anchor) = args.anchor_id {
                    let _ = Command::new("scancel").arg(anchor.to_string()).output();
                }
                return Err(AppError::ExecutionFailed {
                    message: msg,
                    log_path: Some(worker_repx),
                    log_summary: "Worker did not complete successfully".into(),
                });
            }
            worker_out_dirs.push(worker_root.join("out"));
        }

        match orch
            .run_gather(
                &args.gather_exe_path,
                &worker_out_dirs,
                &args.worker_outputs_json,
            )
            .await
        {
            Ok(_) => {
                fs::File::create(orch.repx_dir.join("SUCCESS"))?;
                if let Some(anchor) = args.anchor_id {
                    log_info!("Releasing anchor job {}", anchor);
                    let _ = Command::new("scontrol")
                        .arg("release")
                        .arg(anchor.to_string())
                        .output();
                }
            }
            Err(e) => {
                fs::File::create(orch.repx_dir.join("FAIL"))?;
                if let Some(anchor) = args.anchor_id {
                    let _ = Command::new("scancel").arg(anchor.to_string()).output();
                }
                return Err(e);
            }
        }
        return Ok(());
    }

    orch.init_dirs()?;
    log_info!("Orchestrating scatter-gather stage '{}'", orch.job_id);

    if let Err(e) = orch.run_scatter(&args.scatter_exe_path).await {
        let _ = fs::File::create(orch.scatter_repx_dir.join("FAIL"));
        fs::File::create(orch.repx_dir.join("FAIL"))?;
        if let Some(anchor) = args.anchor_id {
            let _ = Command::new("scancel").arg(anchor.to_string()).output();
        }
        log_error!("Scatter failed: {}", e);
        return Err(e);
    }
    let _ = fs::File::create(orch.scatter_repx_dir.join("SUCCESS"));

    log_info!("[2/4] Scatter finished. Preparing workers...");
    let work_items_str = fs::read_to_string(orch.scatter_out_dir.join("work_items.json"))?;
    let work_items: Vec<Value> = serde_json::from_str(&work_items_str)?;

    let mut worker_out_dirs = Vec::new();
    let mut worker_repx_dirs = Vec::new();

    if args.scheduler == "local" {
        run_local_workers(
            &orch,
            &work_items,
            &args.worker_exe_path,
            &mut worker_out_dirs,
            &mut worker_repx_dirs,
        )
        .await?;

        for (i, repx_dir) in worker_repx_dirs.iter().enumerate() {
            if !repx_dir.join("SUCCESS").exists() {
                let _ = fs::File::create(orch.repx_dir.join("FAIL"));
                return Err(AppError::ExecutionFailed {
                    message: format!("Worker #{} failed", i),
                    log_path: Some(repx_dir.clone()),
                    log_summary: "Worker failure".into(),
                });
            }
        }
        if let Err(e) = orch
            .run_gather(
                &args.gather_exe_path,
                &worker_out_dirs,
                &args.worker_outputs_json,
            )
            .await
        {
            fs::File::create(orch.repx_dir.join("FAIL"))?;
            return Err(e);
        }
        fs::File::create(orch.repx_dir.join("SUCCESS"))?;
    } else if args.scheduler == "slurm" {
        let slurm_ids = submit_slurm_workers_async(
            &orch,
            &work_items,
            &args.worker_exe_path,
            &args.worker_sbatch_opts,
        )
        .await?;

        submit_slurm_gather_job(&orch, &args, &slurm_ids).await?;

        log_info!("Orchestrator finished submitting workers and gather job. Exiting to free slot.");
    } else {
        return Err(AppError::ConfigurationError(format!(
            "Unknown scheduler: {}",
            args.scheduler
        )));
    }

    Ok(())
}

async fn submit_slurm_gather_job(
    orch: &ScatterGatherOrchestrator,
    args: &InternalScatterGatherArgs,
    worker_ids: &[String],
) -> Result<(), AppError> {
    let current_exe = std::env::current_exe()?;
    let current_exe_str = current_exe.to_string_lossy();

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
        "--worker-sbatch-opts".to_string(),
        "''".to_string(),
        "--job-package-path".to_string(),
        args.job_package_path.to_string_lossy().to_string(),
        "--scatter-exe-path".to_string(),
        args.scatter_exe_path.to_string_lossy().to_string(),
        "--worker-exe-path".to_string(),
        args.worker_exe_path.to_string_lossy().to_string(),
        "--gather-exe-path".to_string(),
        args.gather_exe_path.to_string_lossy().to_string(),
        "--worker-outputs-json".to_string(),
        format!("'{}'", args.worker_outputs_json),
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

    let mut sbatch = Command::new("sbatch");
    sbatch.arg("--parsable");
    if !worker_ids.is_empty() {
        sbatch.arg(format!("--dependency=afterany:{}", worker_ids.join(":")));
    }
    sbatch
        .arg(format!("--job-name={}-gather", orch.job_id.0))
        .arg(format!(
            "--output={}/gather/repx/slurm-%j.out",
            orch.job_root.display()
        ))
        .arg("--wrap")
        .arg(cmd_str);

    let output = sbatch.output()?;
    if !output.status.success() {
        return Err(AppError::ExecutionFailed {
            message: "Failed to submit Gather job".to_string(),
            log_path: None,
            log_summary: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    Ok(())
}

async fn submit_slurm_workers_async(
    orch: &ScatterGatherOrchestrator,
    work_items: &[Value],
    worker_exe: &Path,
    sbatch_opts: &str,
) -> Result<Vec<String>, AppError> {
    let mut slurm_ids = Vec::new();

    for (i, item) in work_items.iter().enumerate() {
        let (w_out, w_repx, w_inputs) = orch.prepare_worker(i, item)?;

        let executor = orch.create_executor(w_out.clone(), w_repx.clone());
        let args = vec![
            w_out.to_string_lossy().to_string(),
            w_inputs.to_string_lossy().to_string(),
        ];
        let cmd = executor
            .build_command_for_script(worker_exe, &args)
            .await
            .map_err(|e| AppError::ExecutionFailed {
                message: format!("Failed to build command for worker #{}", i),
                log_path: None,
                log_summary: e.to_string(),
            })?;
        let cmd_str = command_to_shell_string(&cmd);
        let wrapped_cmd = format!(
            "( {} && touch {}/SUCCESS ) || ( touch {}/FAIL; exit 1 )",
            cmd_str,
            w_repx.display(),
            w_repx.display()
        );

        let mut sbatch = Command::new("sbatch");
        sbatch
            .arg("--parsable")
            .args(sbatch_opts.split_whitespace())
            .arg(format!("--job-name={}-w{}", orch.job_id.0, i))
            .arg(format!("--output={}/slurm-%j.out", w_repx.display()))
            .arg("--wrap")
            .arg(wrapped_cmd);

        let output = sbatch.output()?;
        if !output.status.success() {
            return Err(AppError::ExecutionFailed {
                message: format!("sbatch submission for worker #{} failed", i),
                log_path: None,
                log_summary: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        slurm_ids.push(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    log_info!("Submitted {} workers to Slurm.", slurm_ids.len());
    Ok(slurm_ids)
}
async fn run_local_workers(
    orch: &ScatterGatherOrchestrator,
    work_items: &[Value],
    worker_exe: &Path,
    out_dirs: &mut Vec<PathBuf>,
    repx_dirs: &mut Vec<PathBuf>,
) -> Result<(), AppError> {
    let mut tasks = Vec::new();

    for (i, item) in work_items.iter().enumerate() {
        let (w_out, w_repx, w_inputs) = orch.prepare_worker(i, item)?;
        out_dirs.push(w_out.clone());
        repx_dirs.push(w_repx.clone());

        let executor = orch.create_executor(w_out.clone(), w_repx.clone());
        let exe = worker_exe.to_path_buf();
        let args = vec![
            w_out.to_string_lossy().to_string(),
            w_inputs.to_string_lossy().to_string(),
        ];

        let repx_dir_for_task = w_repx;
        tasks.push(tokio::spawn(async move {
            let result = executor.execute_script(&exe, &args).await;
            if result.is_ok() {
                let _ = fs::File::create(repx_dir_for_task.join("SUCCESS"));
            } else {
                let _ = fs::File::create(repx_dir_for_task.join("FAIL"));
            }
            result
        }));
    }
    log_info!(
        "[3/4] Waiting for {} local worker jobs to complete...",
        tasks.len()
    );
    let results = join_all(tasks).await;
    for (i, res) in results.into_iter().enumerate() {
        match res {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                fs::File::create(orch.repx_dir.join("FAIL"))?;
                return Err(AppError::ExecutionFailed {
                    message: format!("Local worker #{} failed", i),
                    log_path: None,
                    log_summary: e.to_string(),
                });
            }
            Err(e) => {
                fs::File::create(orch.repx_dir.join("FAIL"))?;
                return Err(AppError::ExecutionFailed {
                    message: format!("Local worker #{} panicked", i),
                    log_path: None,
                    log_summary: e.to_string(),
                });
            }
        }
    }
    Ok(())
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

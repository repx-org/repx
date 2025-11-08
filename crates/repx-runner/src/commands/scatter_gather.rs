use crate::cli::InternalScatterGatherArgs;
use futures::future::join_all;
use itertools::Itertools;
use repx_core::{error::AppError, log_debug, log_error, log_info, model::JobId};
use repx_executor::{ExecutionRequest, Executor, Runtime};
use serde_json::Value;
use std::{collections::HashMap, fs, path::PathBuf, process::Command};
use tokio::{process::Command as TokioCommand, runtime::Runtime as TokioRuntime};

pub fn handle_scatter_gather(args: InternalScatterGatherArgs) -> Result<(), AppError> {
    let rt = TokioRuntime::new().unwrap();
    rt.block_on(async_handle_scatter_gather(args))
}

fn command_to_shell_string(cmd: &TokioCommand) -> String {
    let program = cmd.as_std().get_program().to_string_lossy();
    let mut args = cmd
        .as_std()
        .get_args()
        .map(|arg| format!("'{}'", arg.to_string_lossy().replace('\'', "'\\''")));
    format!("{} {}", program, args.join(" "))
}

async fn async_handle_scatter_gather(args: InternalScatterGatherArgs) -> Result<(), AppError> {
    log_debug!("INTERNAL SCATTER-GATHER starting for job '{}'", args.job_id,);

    let job_id = JobId(args.job_id);

    let job_root = args.base_path.join("outputs").join(&job_id.0);
    let user_out_dir = job_root.join("out");
    let repx_dir = job_root.join("repx");
    let scatter_temp_dir = job_root.join("scatter_temp");

    for dir in [&user_out_dir, &repx_dir, &scatter_temp_dir] {
        fs::create_dir_all(dir)?;
    }
    let _ = fs::remove_file(repx_dir.join("SUCCESS"));
    let _ = fs::remove_file(repx_dir.join("FAIL"));

    log_info!("Orchestrating scatter-gather stage '{}'", job_id);

    let scatter_exe = args.scatter_exe_path;
    let worker_exe = args.worker_exe_path;
    let gather_exe = args.gather_exe_path;

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
        "bwrap" => Runtime::Bwrap,
        other => {
            return Err(AppError::ConfigurationError(format!(
                "Unsupported runtime: {}",
                other
            )))
        }
    };
    let inputs_json_path = repx_dir.join("inputs.json");

    let executor_for_stage = |user_out_dir: PathBuf, repx_out_dir: PathBuf| -> Executor {
        Executor::new(ExecutionRequest {
            job_id: job_id.clone(),
            runtime: runtime.clone(),
            base_path: args.base_path.clone(),
            job_package_path: args.job_package_path.clone(),
            inputs_json_path: inputs_json_path.clone(),
            user_out_dir,
            repx_out_dir,
        })
    };

    log_info!("[1/4] Starting scatter phase for job '{}'...", job_id);
    let scatter_executor = executor_for_stage(scatter_temp_dir.clone(), repx_dir.clone());
    let scatter_args = vec![
        scatter_temp_dir.to_string_lossy().to_string(),
        inputs_json_path.to_string_lossy().to_string(),
    ];
    if let Err(e) = scatter_executor
        .execute_script(&scatter_exe, &scatter_args)
        .await
    {
        fs::File::create(repx_dir.join("FAIL"))?;
        log_error!("Scatter phase failed: {}", e);
        return Err(AppError::ExecutionFailed {
            message: format!("Scatter phase failed for job {}", job_id),
            log_path: Some(repx_dir.clone()),
            log_summary: e.to_string(),
        });
    }

    log_info!("[2/4] Scatter finished. Submitting worker jobs...");
    let work_items_str = fs::read_to_string(scatter_temp_dir.join("work_items.json"))?;
    let work_items: Vec<Value> = serde_json::from_str(&work_items_str)?;
    let static_inputs: Value = if inputs_json_path.exists() {
        serde_json::from_str(&fs::read_to_string(&inputs_json_path)?)?
    } else {
        Value::Object(Default::default())
    };

    let mut worker_output_dirs: Vec<PathBuf> = Vec::new();

    if args.scheduler == "local" {
        let mut worker_tasks = Vec::new();
        for (i, work_item) in work_items.into_iter().enumerate() {
            let worker_root = job_root.join(format!("worker-{}", i));
            let worker_out_dir = worker_root.join("out");
            let worker_repx_dir = worker_root.join("repx");
            fs::create_dir_all(&worker_out_dir)?;
            fs::create_dir_all(&worker_repx_dir)?;
            worker_output_dirs.push(worker_out_dir.clone());

            let mut final_worker_inputs = static_inputs.as_object().cloned().unwrap_or_default();

            let work_item_path = worker_repx_dir.join("work_item.json");
            fs::write(&work_item_path, serde_json::to_string(&work_item)?)?;

            final_worker_inputs.insert(
                "worker__item".to_string(),
                Value::String(work_item_path.to_string_lossy().to_string()),
            );

            let worker_inputs_json_path = worker_repx_dir.join("inputs.json");
            fs::write(
                &worker_inputs_json_path,
                serde_json::to_string_pretty(&final_worker_inputs)?,
            )?;

            let worker_executor =
                executor_for_stage(worker_out_dir.clone(), worker_repx_dir.clone());
            let worker_exe_clone = worker_exe.clone();
            let worker_args = vec![
                worker_out_dir.to_string_lossy().to_string(),
                worker_inputs_json_path.to_string_lossy().to_string(),
            ];

            worker_tasks.push(tokio::spawn(async move {
                worker_executor
                    .execute_script(&worker_exe_clone, &worker_args)
                    .await
            }));
        }

        log_info!(
            "[3/4] Waiting for {} local worker jobs to complete...",
            worker_tasks.len()
        );
        let results = join_all(worker_tasks).await;
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(Ok(_)) => (),
                Ok(Err(e)) => {
                    fs::File::create(repx_dir.join("FAIL"))?;
                    return Err(AppError::ExecutionFailed {
                        message: format!("Local worker #{} failed during execution", i),
                        log_path: None,
                        log_summary: e.to_string(),
                    });
                }
                Err(e) => {
                    fs::File::create(repx_dir.join("FAIL"))?;
                    return Err(AppError::ExecutionFailed {
                        message: format!("Local worker #{} task panicked", i),
                        log_path: None,
                        log_summary: e.to_string(),
                    });
                }
            }
        }
    } else if args.scheduler == "slurm" {
        let mut worker_slurm_ids: Vec<String> = Vec::new();
        for (i, work_item) in work_items.iter().enumerate() {
            let worker_root = job_root.join(format!("worker-{}", i));
            let worker_out_dir = worker_root.join("out");
            let worker_repx_dir = worker_root.join("repx");
            fs::create_dir_all(&worker_out_dir)?;
            fs::create_dir_all(&worker_repx_dir)?;
            worker_output_dirs.push(worker_out_dir.clone());

            let mut final_worker_inputs = static_inputs.as_object().cloned().unwrap_or_default();

            let work_item_path = worker_repx_dir.join("work_item.json");
            fs::write(&work_item_path, serde_json::to_string(&work_item)?)?;

            final_worker_inputs.insert(
                "worker__item".to_string(),
                Value::String(work_item_path.to_string_lossy().to_string()),
            );

            let worker_inputs_json_path = worker_repx_dir.join("inputs.json");
            fs::write(
                &worker_inputs_json_path,
                serde_json::to_string_pretty(&final_worker_inputs)?,
            )?;

            let worker_executor =
                executor_for_stage(worker_out_dir.clone(), worker_repx_dir.clone());
            let worker_args = vec![
                worker_out_dir.to_string_lossy().to_string(),
                worker_inputs_json_path.to_string_lossy().to_string(),
            ];
            let mut worker_cmd = worker_executor
                .build_command_for_script(&worker_exe, &worker_args)
                .await
                .map_err(|e| AppError::ExecutionFailed {
                    message: format!("Failed to build command for worker #{}", i),
                    log_path: None,
                    log_summary: e.to_string(),
                })?;
            let command_string = command_to_shell_string(&mut worker_cmd);

            let mut sbatch_cmd = Command::new("sbatch");
            sbatch_cmd
                .arg("--parsable")
                .args(args.worker_sbatch_opts.split_whitespace())
                .arg(format!("--job-name={}-w{}", job_id.0, i))
                .arg(format!(
                    "--output={}/slurm-%j.out",
                    worker_repx_dir.display()
                ))
                .arg("--wrap")
                .arg(&command_string);

            let output = sbatch_cmd.output()?;
            if !output.status.success() {
                return Err(AppError::ExecutionFailed {
                    message: format!("sbatch submission for worker #{} failed", i),
                    log_path: None,
                    log_summary: String::from_utf8_lossy(&output.stderr).to_string(),
                });
            }
            worker_slurm_ids.push(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }

        log_info!(
            "[3/4] Waiting for {} SLURM worker jobs to complete...",
            worker_slurm_ids.len()
        );
        if !worker_slurm_ids.is_empty() {
            let mut sacct_cmd = Command::new("sacct");
            sacct_cmd
                .arg("-n")
                .arg("-j")
                .arg(worker_slurm_ids.join(","))
                .arg("-o")
                .arg("State");
            loop {
                let output = sacct_cmd.output()?;
                let output_str = String::from_utf8_lossy(&output.stdout);
                let states: Vec<_> = output_str
                    .lines()
                    .map(|s| s.trim().to_lowercase())
                    .collect();
                if states
                    .iter()
                    .any(|s| s.contains("fail") || s.contains("cancel"))
                {
                    fs::File::create(repx_dir.join("FAIL"))?;
                    return Err(AppError::ExecutionFailed {
                        message: "One or more SLURM workers failed or were cancelled.".into(),
                        log_path: None,
                        log_summary: "Check sacct for details.".into(),
                    });
                }
                if states.iter().all(|s| s.contains("completed")) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    log_info!("[3/4] All workers completed. Starting gather phase...");
    let mut worker_outs_manifest = Vec::new();
    for worker_out_dir in &worker_output_dirs {
        let mut worker_outputs = HashMap::new();
        let worker_job_outputs: HashMap<String, Value> =
            serde_json::from_str(&args.worker_outputs_json)?;
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
    let worker_manifest_path = repx_dir.join("worker_outs_manifest.json");
    fs::write(
        &worker_manifest_path,
        serde_json::to_string_pretty(&worker_outs_manifest)?,
    )?;

    let mut gather_inputs = static_inputs.as_object().cloned().unwrap_or_default();
    gather_inputs.insert(
        "worker__outs".to_string(),
        Value::String(worker_manifest_path.to_string_lossy().to_string()),
    );
    let gather_inputs_json_path = repx_dir.join("gather_inputs.json");
    fs::write(
        &gather_inputs_json_path,
        serde_json::to_string_pretty(&gather_inputs)?,
    )?;

    let gather_executor = executor_for_stage(user_out_dir.clone(), repx_dir.clone());
    let gather_args = vec![
        user_out_dir.to_string_lossy().to_string(),
        gather_inputs_json_path.to_string_lossy().to_string(),
    ];
    if let Err(e) = gather_executor
        .execute_script(&gather_exe, &gather_args)
        .await
    {
        fs::File::create(repx_dir.join("FAIL"))?;
        log_error!("Gather phase failed: {}", e);
        return Err(AppError::ExecutionFailed {
            message: format!("Gather phase failed for job {}", job_id),
            log_path: Some(repx_dir.clone()),
            log_summary: e.to_string(),
        });
    }

    fs::File::create(repx_dir.join("SUCCESS"))?;
    log_info!(
        "[4/4] Scatter-gather stage '{}' completed successfully.",
        job_id
    );

    Ok(())
}

use crate::cli::InternalExecuteArgs;
use repx_core::{error::AppError, log_debug, model::JobId};
use repx_executor::{ExecutionRequest, Executor, Runtime};
use std::fs;
use tokio::runtime::Runtime as TokioRuntime;

pub fn handle_execute(args: InternalExecuteArgs) -> Result<(), AppError> {
    let rt = TokioRuntime::new().unwrap();
    rt.block_on(async_handle_execute(args))
}

async fn async_handle_execute(args: InternalExecuteArgs) -> Result<(), AppError> {
    log_debug!("INTERNAL EXECUTE starting for job '{}'", args.job_id,);

    let job_id = JobId(args.job_id);
    let job_root = args.base_path.join("outputs").join(&job_id.0);
    let user_out_dir = job_root.join("out");
    let repx_dir = job_root.join("repx");
    fs::create_dir_all(&user_out_dir)?;
    fs::create_dir_all(&repx_dir)?;

    let _ = fs::remove_file(repx_dir.join("SUCCESS"));
    let _ = fs::remove_file(repx_dir.join("FAIL"));

    let script_path = args.executable_path;
    let job_package_path = script_path
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| {
            AppError::ConfigurationError(
                "Could not determine job package path from executable path".into(),
            )
        })?
        .to_path_buf();
    let inputs_json_path = repx_dir.join("inputs.json");

    let runtime = match args.runtime.as_str() {
        "native" => Runtime::Native,
        "podman" => Runtime::Podman {
            image_tag: args.image_tag.ok_or_else(|| {
                AppError::ConfigurationError(
                    "Container execution with 'podman' requires an --image-tag.".to_string(),
                )
            })?,
        },
        "docker" => Runtime::Docker {
            image_tag: args.image_tag.ok_or_else(|| {
                AppError::ConfigurationError(
                    "Container execution with 'docker' requires an --image-tag.".to_string(),
                )
            })?,
        },
        "bwrap" => Runtime::Bwrap {
            image_tag: args.image_tag.ok_or_else(|| {
                AppError::ConfigurationError(
                    "Container execution with 'bwrap' requires an --image-tag.".to_string(),
                )
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

    let request = ExecutionRequest {
        job_id: job_id.clone(),
        runtime,
        base_path: args.base_path,
        job_package_path,
        inputs_json_path: inputs_json_path.clone(),
        user_out_dir,
        repx_out_dir: repx_dir.clone(),
        host_tools_bin_dir,
    };

    let executor = Executor::new(request);
    let exec_args = vec![
        job_root.join("out").to_string_lossy().to_string(),
        inputs_json_path.to_string_lossy().to_string(),
    ];

    let result = executor.execute_script(&script_path, &exec_args).await;

    match result {
        Ok(_) => {
            fs::File::create(repx_dir.join("SUCCESS"))?;
            repx_core::log_info!("Job '{}' completed successfully.", job_id);
        }
        Err(e) => {
            fs::File::create(repx_dir.join("FAIL"))?;
            let err_msg = format!("Job '{}' failed: {}", job_id, e);
            repx_core::log_error!("{}", err_msg);

            eprintln!("{}", err_msg);
            return Err(AppError::ExecutionFailed {
                message: format!("Execution failed for job {}", job_id),
                log_path: Some(repx_dir.join("stderr.log")),
                log_summary: e.to_string(),
            });
        }
    }

    Ok(())
}

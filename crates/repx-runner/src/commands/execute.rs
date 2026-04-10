use crate::{cli::InternalExecuteArgs, error::CliError};
use repx_core::{
    cache::{CacheKey, CacheMetadata, CacheStore, FsCache},
    constants::{dirs, logs, markers},
    errors::CoreError,
    fs_utils::path_to_string,
    model::{JobId, MountPolicy},
    store::completion_log,
};
use repx_executor::{CancellationToken, ExecutionRequest, Executor};
use std::fs;

use super::write_marker;

pub fn handle_execute(args: InternalExecuteArgs) -> Result<(), CliError> {
    let rt = super::create_tokio_runtime()?;
    rt.block_on(async_handle_execute(args))
}

async fn async_handle_execute(args: InternalExecuteArgs) -> Result<(), CliError> {
    tracing::debug!("INTERNAL EXECUTE starting for job '{}'", args.job_id,);

    let job_id = JobId::from(args.job_id);
    let job_root = args.base_path.join(dirs::OUTPUTS).join(job_id.as_str());

    let user_out_dir = args
        .user_out_dir
        .unwrap_or_else(|| job_root.join(dirs::OUT));
    let repx_dir = args
        .repx_out_dir
        .unwrap_or_else(|| job_root.join(dirs::REPX));

    fs::create_dir_all(&user_out_dir)?;
    fs::create_dir_all(&repx_dir)?;

    let _ = fs::remove_file(repx_dir.join(markers::SUCCESS));
    let _ = fs::remove_file(repx_dir.join(markers::FAIL));

    let script_path = super::resolve_to_local_artifacts(
        &args.executable_path,
        &args.base_path,
        &args.local_artifacts_path,
    );
    let job_package_path = if let Some(pkg_path) = args.job_package_path {
        super::resolve_to_local_artifacts(&pkg_path, &args.base_path, &args.local_artifacts_path)
    } else {
        script_path
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| {
                CliError::Config(CoreError::InvalidConfig {
                    detail: "Could not determine job package path from executable path".to_string(),
                })
            })?
            .to_path_buf()
    };
    let inputs_json_path_raw = args
        .inputs_json_path
        .unwrap_or_else(|| repx_dir.join("inputs.json"));
    let parameters_json_path_raw = args
        .parameters_json_path
        .unwrap_or_else(|| repx_dir.join("parameters.json"));

    let (inputs_json_path, inputs_data) = read_fd_path_to_memory(&inputs_json_path_raw)?;
    let (parameters_json_path, parameters_data) =
        read_fd_path_to_memory(&parameters_json_path_raw)?;

    let runtime = super::parse_runtime(args.runtime, args.image_tag)?;
    let host_tools_bin_dir = super::resolve_host_tools_dir(
        &args.base_path,
        &args.host_tools_dir,
        args.local_artifacts_path.as_deref(),
    );

    let exec_args = vec![
        path_to_string(&user_out_dir),
        path_to_string(&inputs_json_path),
        path_to_string(&parameters_json_path),
    ];

    let base_path = args.base_path;
    let request = ExecutionRequest {
        job_id: job_id.clone(),
        runtime,
        base_path: base_path.clone(),
        node_local_path: args.node_local_path,
        local_artifacts_path: args.local_artifacts_path,
        job_package_path,
        inputs_json_path,
        user_out_dir,
        repx_out_dir: repx_dir.clone(),
        host_tools_bin_dir,
        mount_policy: MountPolicy::from_flags(args.mount_host_paths, args.mount_paths),
        inputs_data,
        parameters_data,
    };

    let mut executor = Executor::new(request);

    let cancel = CancellationToken::new();
    let result = executor
        .execute_script(&script_path, &exec_args, &cancel)
        .await;

    let outcome_cache = FsCache::new(base_path.clone());
    let outcome_key = CacheKey::JobOutcome {
        job_id: job_id.as_str().to_string(),
    };

    match result {
        Ok(_) => {
            write_marker(&repx_dir.join(markers::SUCCESS))?;
            let meta = CacheMetadata::new(&outcome_key, format!("job '{}' succeeded", job_id));
            if let Err(e) = outcome_cache.mark_ready(&outcome_key, meta) {
                tracing::debug!("Failed to write cache metadata for job outcome: {}", e);
            }
            if let Err(e) = completion_log::append_completion(&base_path, &job_id, true) {
                tracing::debug!("Failed to append to completion log: {}", e);
            }
            tracing::info!("Job '{}' completed successfully.", job_id);
        }
        Err(e) => {
            write_marker(&repx_dir.join(markers::FAIL))?;
            let meta = CacheMetadata::new(&outcome_key, format!("job '{}' failed", job_id));
            if let Err(err) = outcome_cache.mark_ready(&outcome_key, meta) {
                tracing::debug!("Failed to write cache metadata for job outcome: {}", err);
            }
            if let Err(err) = completion_log::append_completion(&base_path, &job_id, false) {
                tracing::debug!("Failed to append to completion log: {}", err);
            }
            let err_msg = format!("Job '{}' failed: {}", job_id, e);
            tracing::error!("{}", err_msg);

            eprintln!("{}", err_msg);
            return Err(CliError::ExecutionFailed {
                message: "Execution failed".to_string(),
                log_path: Some(repx_dir.join(logs::STDERR)),
                log_summary: e.to_string(),
            });
        }
    }

    Ok(())
}

fn read_fd_path_to_memory(
    path: &std::path::Path,
) -> Result<(std::path::PathBuf, Option<Vec<u8>>), CliError> {
    let path_str = path.to_string_lossy();
    if path_str.starts_with("/dev/fd/") || path_str.starts_with("/proc/self/fd/") {
        let data = fs::read(path).map_err(|e| {
            CliError::Config(CoreError::CommandFailed(format!(
                "Failed to read fd-backed path '{}': {}",
                path.display(),
                e
            )))
        })?;
        Ok((path.to_path_buf(), Some(data)))
    } else {
        Ok((path.to_path_buf(), None))
    }
}

use crate::error::{ClientError, Result};
use repx_core::{
    constants::dirs,
    errors::ConfigError,
    model::{Job, JobId, Lab},
};
use std::path::Path;
use std::sync::Arc;

pub fn generate_and_write_inputs_json(
    lab: &Lab,
    local_lab_path: &Path,
    job: &Job,
    job_id: &JobId,
    target: Arc<dyn crate::targets::Target>,
    executable_name: &str,
) -> Result<()> {
    let mut inputs_map = serde_json::Map::new();

    let exe = job.executables.get(executable_name).ok_or_else(|| {
        ClientError::Config(ConfigError::General(format!(
            "Job '{}' missing required executable '{}'",
            job_id, executable_name
        )))
    })?;

    for mapping in &exe.inputs {
        if let (Some(dep_job_id), Some(source_output)) = (&mapping.job_id, &mapping.source_output) {
            let dep_job = lab.jobs.get(dep_job_id).ok_or_else(|| {
                ClientError::JobNotTracked(dep_job_id.clone(), "unknown".to_string())
            })?;

            let dep_exe = if dep_job.stage_type == repx_core::model::StageType::ScatterGather {
                dep_job.executables.get("gather")
            } else {
                dep_job.executables.get("main")
            }
            .ok_or_else(|| {
                ClientError::Config(ConfigError::General(format!(
                    "Could not find output executable for dependency job '{}'",
                    dep_job_id
                )))
            })?;

            let value_template_val = dep_exe.outputs.get(source_output).ok_or_else(|| {
                ClientError::Config(ConfigError::General(format!(
                            "Inconsistent metadata: job '{}' requires output '{}' from dependency '{}', but this output is not defined in the dependency's metadata.",
                            job_id, source_output, dep_job_id
                        )))
            })?;

            let value_template = value_template_val.as_str().ok_or_else(|| {
                ClientError::Config(ConfigError::General(format!(
                        "Inconsistent metadata: job '{}' requires output '{}' from dependency '{}', but this output is not a string path template.",
                        job_id, source_output, dep_job_id
                    )))
            })?;

            let dep_output_dir = target
                .base_path()
                .join(dirs::OUTPUTS)
                .join(&dep_job_id.0)
                .join(dirs::OUT);
            let final_path = value_template.replace("$out", &dep_output_dir.to_string_lossy());

            inputs_map.insert(
                mapping.target_input.clone(),
                serde_json::Value::String(final_path),
            );
        } else if mapping.mapping_type.as_deref() == Some("global")
            || mapping.target_input == "store__base"
        {
            let store_path = target.base_path().to_string_lossy().to_string();
            inputs_map.insert(
                mapping.target_input.clone(),
                serde_json::Value::String(store_path),
            );
        } else if let Some(run_id) = &mapping.source_run {
            let revision_dir = local_lab_path.join("revision");
            let suffix = format!("metadata-{}.json", run_id.0);

            let mut found_filename = None;

            if let Ok(entries) = fs_err::read_dir(&revision_dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.ends_with(&suffix) {
                            found_filename = Some(name.to_string());
                            break;
                        }
                    }
                }
            }

            if let Some(filename) = found_filename {
                let remote_path = target.artifacts_base_path().join("revision").join(filename);
                inputs_map.insert(
                    mapping.target_input.clone(),
                    serde_json::Value::String(remote_path.to_string_lossy().to_string()),
                );
            } else {
                tracing::info!(
                        "Warning: Could not resolve metadata file for run '{}' in revision directory. Input '{}' will be missing.",
                        run_id, mapping.target_input
                    );
            }
        }
    }

    let json_content = serde_json::to_string_pretty(&serde_json::Value::Object(inputs_map))
        .map_err(|e| ClientError::Config(ConfigError::Json(e)))?;

    let inputs_json_path_on_target = target
        .base_path()
        .join(dirs::OUTPUTS)
        .join(&job_id.0)
        .join(dirs::REPX)
        .join("inputs.json");

    tracing::info!(
        "Generating inputs.json for job '{}' on target '{}'",
        job_id,
        target.name()
    );
    tracing::debug!(
        "Writing inputs.json to '{}' with content:\n{}",
        inputs_json_path_on_target.display(),
        json_content
    );

    target.write_remote_file(&inputs_json_path_on_target, &json_content)
}

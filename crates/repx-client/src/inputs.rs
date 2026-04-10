use crate::error::{ClientError, Result};
use repx_core::{
    constants::dirs,
    errors::CoreError,
    fs_utils::path_to_string,
    lab::LabSource,
    model::{Job, JobId, Lab},
};
use std::path::Path;
use std::sync::Arc;

pub fn generate_parameters_json_content(job: &Job) -> Result<String> {
    serde_json::to_string_pretty(&job.params).map_err(ClientError::Json)
}

pub fn generate_inputs_json_content(
    lab: &Lab,
    source: &LabSource,
    job: &Job,
    job_id: &JobId,
    base_path: &Path,
    artifacts_base_path: &Path,
    executable_name: &str,
) -> Result<String> {
    let mut inputs_map = serde_json::Map::new();

    let exe = job.executables.get(executable_name).ok_or_else(|| {
        ClientError::Config(CoreError::MissingExecutable {
            job_id: job_id.to_string(),
            executable: executable_name.to_string(),
        })
    })?;

    for mapping in &exe.inputs {
        if let (Some(dep_job_id), Some(source_output)) = (&mapping.job_id, &mapping.source_output) {
            let dep_job = lab.jobs.get(dep_job_id).ok_or_else(|| {
                ClientError::Config(CoreError::InconsistentMetadata {
                    detail: format!(
                        "Dependency job '{}' not found in lab for job '{}'",
                        dep_job_id, job_id
                    ),
                })
            })?;

            let dep_exe = if dep_job.stage_type == repx_core::model::StageType::ScatterGather {
                dep_job.executables.get("gather")
            } else {
                dep_job.executables.get("main")
            }
            .ok_or_else(|| {
                ClientError::Config(CoreError::MissingExecutable {
                    job_id: dep_job_id.to_string(),
                    executable: "main/gather".to_string(),
                })
            })?;

            let value_template_val = dep_exe.outputs.get(source_output).ok_or_else(|| {
                ClientError::Config(CoreError::InconsistentMetadata {
                    detail: format!(
                        "Job '{}' requires output '{}' from dependency '{}', but this output is not defined in the dependency's metadata.",
                        job_id, source_output, dep_job_id
                    ),
                })
            })?;

            let value_template = value_template_val.as_str().ok_or_else(|| {
                ClientError::Config(CoreError::InconsistentMetadata {
                    detail: format!(
                        "Job '{}' requires output '{}' from dependency '{}', but this output is not a string path template.",
                        job_id, source_output, dep_job_id
                    ),
                })
            })?;

            let dep_output_dir = base_path
                .join(dirs::OUTPUTS)
                .join(dep_job_id.as_str())
                .join(dirs::OUT);
            let final_path = value_template.replace("$out", &dep_output_dir.to_string_lossy());

            inputs_map.insert(
                mapping.target_input.clone(),
                serde_json::Value::String(final_path),
            );
        } else if mapping.mapping_type == Some(repx_core::model::MappingType::Global)
            || mapping.target_input == "store__base"
        {
            let store_path = path_to_string(base_path);
            inputs_map.insert(
                mapping.target_input.clone(),
                serde_json::Value::String(store_path),
            );
        } else if let Some(run_id) = &mapping.source_run {
            let suffix = format!("metadata-{}.json", run_id.as_str());

            let found_filename = match source {
                LabSource::Directory(dir) => {
                    let revision_dir = dir.join("revision");
                    let mut found = None;
                    if let Ok(entries) = fs_err::read_dir(&revision_dir) {
                        for entry in entries.flatten() {
                            if let Some(name) = entry.file_name().to_str() {
                                if name.ends_with(&suffix) {
                                    found = Some(name.to_string());
                                    break;
                                }
                            }
                        }
                    }
                    found
                }
                LabSource::Tar(tar_path) => {
                    match repx_core::lab::list_tar_entries(tar_path, "revision/") {
                        Ok(entries) => entries
                            .into_iter()
                            .filter_map(|e| {
                                let filename =
                                    std::path::Path::new(&e).file_name()?.to_str()?.to_string();
                                if filename.ends_with(&suffix) {
                                    Some(filename)
                                } else {
                                    None
                                }
                            })
                            .next(),
                        Err(e) => {
                            tracing::warn!("Failed to list tar entries for revision/: {}", e);
                            None
                        }
                    }
                }
            };

            if let Some(filename) = found_filename {
                let remote_path = artifacts_base_path.join("revision").join(filename);
                inputs_map.insert(
                    mapping.target_input.clone(),
                    serde_json::Value::String(path_to_string(&remote_path)),
                );
            } else {
                tracing::warn!(
                        "Could not resolve metadata file for run '{}' in revision directory. Input '{}' will be missing.",
                        run_id, mapping.target_input
                    );
            }
        }
    }

    serde_json::to_string_pretty(&serde_json::Value::Object(inputs_map)).map_err(ClientError::Json)
}

pub fn generate_and_write_parameters_json(
    job: &Job,
    job_id: &JobId,
    target: Arc<dyn crate::targets::Target>,
) -> Result<()> {
    let json_content = generate_parameters_json_content(job)?;

    let parameters_json_path = target
        .base_path()
        .join(dirs::OUTPUTS)
        .join(job_id.as_str())
        .join(dirs::REPX)
        .join("parameters.json");

    tracing::info!(
        "Generating parameters.json for job '{}' on target '{}'",
        job_id,
        target.name()
    );
    tracing::debug!(
        "Writing parameters.json to '{}' with content:\n{}",
        parameters_json_path.display(),
        json_content
    );

    target.write_remote_file(&parameters_json_path, &json_content)
}

pub fn generate_and_write_inputs_json(
    lab: &Lab,
    source: &LabSource,
    job: &Job,
    job_id: &JobId,
    target: Arc<dyn crate::targets::Target>,
    executable_name: &str,
) -> Result<()> {
    let json_content = generate_inputs_json_content(
        lab,
        source,
        job,
        job_id,
        target.base_path(),
        &target.artifacts_base_path(),
        executable_name,
    )?;

    let inputs_json_path_on_target = target
        .base_path()
        .join(dirs::OUTPUTS)
        .join(job_id.as_str())
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

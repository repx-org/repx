use crate::error::CliError;
use repx_core::{constants::dirs, errors::CoreError, fs_utils::path_to_string};
use serde_json::Value;
use std::{collections::HashMap, path::Path};

use super::StepMeta;

pub(crate) fn resolve_step_inputs(
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
                    Value::String(path_to_string(work_item_path)),
                );
            } else if let Some(dep_name) = source.strip_prefix("step:") {
                let source_output = mapping.source_output.as_ref().ok_or_else(|| {
                    CliError::Config(CoreError::StepError {
                        detail: format!(
                            "Step input mapping with source '{}' missing source_output",
                            source
                        ),
                    })
                })?;

                let dep_meta = steps_meta.get(dep_name).ok_or_else(|| {
                    CliError::Config(CoreError::StepError {
                        detail: format!("Step input references unknown step '{}'", dep_name),
                    })
                })?;

                let template = dep_meta.outputs.get(source_output).ok_or_else(|| {
                    CliError::Config(CoreError::StepError {
                        detail: format!(
                            "Step '{}' does not have output '{}'",
                            dep_name, source_output
                        ),
                    })
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

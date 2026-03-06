use crate::error::CliError;
use repx_core::errors::CoreError;
use std::collections::HashMap;

use super::StepMeta;

#[allow(clippy::expect_used)]
pub(crate) fn toposort_steps(steps: &HashMap<String, StepMeta>) -> Result<Vec<String>, CliError> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for (name, meta) in steps {
        in_degree.entry(name.as_str()).or_insert(0);
        for dep in &meta.deps {
            if !steps.contains_key(dep) {
                return Err(CliError::Config(CoreError::StepError {
                    detail: format!("Step '{}' depends on unknown step '{}'", name, dep),
                }));
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
                let deg = in_degree
                    .get_mut(dep_name)
                    .expect("in_degree must contain all step names from initialization");
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
        return Err(CliError::Config(CoreError::CycleDetected {
            context: "step dependency graph".to_string(),
        }));
    }

    Ok(result)
}

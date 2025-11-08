use crate::{error::AppError, model::JobId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobOutcome {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoundJob {
    pub outcome: JobOutcome,
    pub path: PathBuf,
}

pub fn get_job_outcomes(
    store_path: &Path,
    job_ids_to_check: &[JobId],
) -> Result<HashMap<JobId, FoundJob>, AppError> {
    let outputs_dir = store_path.join("outputs");
    if !outputs_dir.exists() {
        return Ok(HashMap::new());
    }

    let mut outcomes = HashMap::new();
    for job_id in job_ids_to_check {
        let job_path = outputs_dir.join(&job_id.0);
        if !job_path.is_dir() {
            continue;
        }

        let success_marker = job_path.join("SUCCESS");
        let fail_marker = job_path.join("FAIL");

        if success_marker.exists() {
            outcomes.insert(
                job_id.clone(),
                FoundJob {
                    outcome: JobOutcome::Succeeded,
                    path: job_path,
                },
            );
        } else if fail_marker.exists() {
            outcomes.insert(
                job_id.clone(),
                FoundJob {
                    outcome: JobOutcome::Failed,
                    path: job_path,
                },
            );
        }
    }
    Ok(outcomes)
}

pub struct MergeProgress {
    pub total_entries: u64,
    pub processed_entries: u64,
    pub current_path: PathBuf,
}

pub fn merge_stores(
    sources: &[PathBuf],
    destination: &Path,
    mut on_progress: impl FnMut(MergeProgress),
) -> Result<(), AppError> {
    fs::create_dir_all(destination)?;

    let entries: Vec<_> = sources
        .iter()
        .flat_map(|path| WalkDir::new(path).into_iter().filter_map(Result::ok))
        .collect();

    let total_entries = entries.len() as u64;

    for (i, entry) in entries.into_iter().enumerate() {
        let path = entry.path();

        let source_root = sources
            .iter()
            .find(|s| path.starts_with(s))
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "Could not determine source root for path {}",
                        path.display()
                    ),
                )
            })?;

        let relative_path = path.strip_prefix(source_root).unwrap();
        let dest_path = destination.join(relative_path);

        on_progress(MergeProgress {
            total_entries,
            processed_entries: i as u64,
            current_path: relative_path.to_path_buf(),
        });

        if path.is_dir() {
            fs::create_dir_all(&dest_path)?;
        } else {
            if !dest_path.exists() {
                if let Some(p) = dest_path.parent() {
                    fs::create_dir_all(p)?;
                }
                fs::copy(path, &dest_path)?;
            }
        }
    }

    Ok(())
}

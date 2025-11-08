use crate::log_debug;
use crate::{error::AppError, model::Lab};
use std::fs;
use std::path::{Path, PathBuf};

pub fn find_metadata_path(lab_path: &Path) -> Option<PathBuf> {
    let direct_path = lab_path.join("metadata.json");
    if direct_path.is_file() {
        return Some(direct_path);
    }

    let revision_dir = lab_path.join("revision");
    if revision_dir.is_dir() {
        if let Some(Ok(entry)) = fs::read_dir(revision_dir).ok()?.next() {
            let revision_subdir = entry.path();
            let nested_path = revision_subdir.join("metadata.json");
            if nested_path.is_file() {
                return Some(nested_path);
            }
        }
    }

    None
}

pub fn load_from_path(lab_path: &Path) -> Result<Lab, AppError> {
    log_debug!(
        "Loading and validating lab from '{}'...",
        lab_path.display()
    );

    if !lab_path.is_dir() {
        return Err(AppError::LabNotFound(lab_path.to_path_buf()));
    }

    let metadata_path = find_metadata_path(lab_path)
        .ok_or_else(|| AppError::MetadataNotFound(lab_path.to_path_buf()))?;

    log_debug!("Found metadata file at '{}'", metadata_path.display());

    let file_content = fs::read_to_string(&metadata_path)?;
    let mut lab: Lab = serde_json::from_str(&file_content)?;

    for (job_id, job) in lab.jobs.iter_mut() {
        job.path_in_lab = PathBuf::from("jobs").join(&job_id.0);
    }

    log_debug!("Successfully parsed metadata.json.");

    let parent_dir = metadata_path.parent().ok_or_else(|| {
        AppError::ConfigurationError(format!(
            "Could not get parent directory of metadata.json: {}",
            metadata_path.display()
        ))
    })?;
    let parent_dir_name = parent_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            AppError::ConfigurationError(format!(
                "Could not get directory name for: {}",
                parent_dir.display()
            ))
        })?;

    const SUFFIX: &str = "-experiment-metadata-json";
    let hash = parent_dir_name.strip_suffix(SUFFIX).ok_or_else(|| AppError::ConfigurationError(format!(
        "Cannot determine unique lab hash. The directory containing metadata.json ('{}') does not follow the expected '<hash>{}' format.",
        parent_dir_name, SUFFIX
    )))?;

    if hash.is_empty() {
        return Err(AppError::ConfigurationError(format!(
            "Cannot determine unique lab hash. The directory name '{}' results in an empty hash.",
            parent_dir_name
        )));
    }
    lab.content_hash = hash.to_string();

    let jobs_dir = lab_path.join("jobs");
    if !jobs_dir.is_dir() {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "Lab integrity check failed: 'jobs' directory not found in lab at '{}'",
                lab_path.display()
            ),
        )));
    }

    for run in lab.runs.values() {
        if let Some(image_rel_path) = &run.image {
            let image_full_path = lab_path.join(image_rel_path);
            if !image_full_path.exists() {
                return Err(AppError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "Lab integrity check failed: image file '{}' not found.",
                        image_full_path.display()
                    ),
                )));
            }
        }
    }
    log_debug!("All container image files found.");

    for job in lab.jobs.values() {
        let job_pkg_path = lab_path.join(&job.path_in_lab);
        if !job_pkg_path.is_dir() {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Lab integrity check failed: job package directory '{}' not found.",
                    job_pkg_path.display()
                ),
            )));
        }
    }
    log_debug!("All job package directories found.");

    log_debug!("Lab validation successful.");
    Ok(lab)
}

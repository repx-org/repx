use crate::log_debug;
use crate::{
    error::AppError,
    model::{Lab, LabManifest, RootMetadata, Run, RunMetadataForLoading},
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn find_manifest_path(lab_path: &Path) -> Option<PathBuf> {
    let lab_subdir = lab_path.join("lab");
    if !lab_subdir.is_dir() {
        return None;
    }

    let entries = fs::read_dir(lab_subdir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with("lab-metadata.json") {
                return Some(path);
            }
        }
    }
    None
}

pub fn load_from_path(initial_path: &Path) -> Result<Lab, AppError> {
    log_debug!(
        "Attempting to load lab from initial path: '{}'",
        initial_path.display()
    );

    let lab_path = if initial_path.is_dir() {
        initial_path.to_path_buf()
    } else {
        initial_path
            .parent()
            .ok_or_else(|| AppError::LabNotFound(initial_path.to_path_buf()))?
            .to_path_buf()
    };

    log_debug!(
        "Loading and validating lab from resolved directory '{}'...",
        lab_path.display()
    );

    if !lab_path.is_dir() {
        return Err(AppError::LabNotFound(lab_path.to_path_buf()));
    }

    let manifest_path = find_manifest_path(&lab_path)
        .ok_or_else(|| AppError::MetadataNotFound(lab_path.to_path_buf()))?;

    log_debug!("Found lab manifest at: '{}'", manifest_path.display());

    let manifest_content = fs::read_to_string(&manifest_path)?;
    let manifest: LabManifest = serde_json::from_str(&manifest_content)?;
    let content_hash = manifest.lab_id;

    log_debug!("Lab Content Hash (ID): {}", content_hash);

    let root_metadata_path = lab_path.join(&manifest.metadata);
    if !root_metadata_path.is_file() {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "Root metadata file not found at '{}'",
                root_metadata_path.display()
            ),
        )));
    }

    log_debug!(
        "Loading root metadata from '{}'",
        root_metadata_path.display()
    );
    let root_metadata_content = fs::read_to_string(&root_metadata_path)?;
    let root_meta: RootMetadata = serde_json::from_str(&root_metadata_content)?;

    let mut lab = Lab {
        schema_version: root_meta.schema_version,
        git_hash: root_meta.git_hash,
        content_hash,
        runs: HashMap::new(),
        jobs: HashMap::new(),
    };

    for run_rel_path in root_meta.runs {
        let run_metadata_path = lab_path.join(&run_rel_path);
        log_debug!(
            "Loading run metadata from '{}'",
            run_metadata_path.display()
        );

        let run_meta_content = fs::read_to_string(&run_metadata_path).map_err(|e| {
            AppError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to read run metadata at {:?}: {}",
                    run_metadata_path, e
                ),
            ))
        })?;

        let mut run_meta: RunMetadataForLoading = serde_json::from_str(&run_meta_content)?;
        let run_id = run_meta.name.clone();

        let job_ids_for_run: Vec<_> = run_meta.jobs.keys().cloned().collect();

        let run = Run {
            image: run_meta.image,
            jobs: job_ids_for_run,
            dependencies: run_meta.dependencies,
        };

        lab.runs.insert(run_id, run);

        for (job_id, mut job) in run_meta.jobs.drain() {
            job.path_in_lab = PathBuf::from("jobs").join(&job_id.0);
            lab.jobs.insert(job_id, job);
        }
    }

    log_debug!(
        "Successfully parsed all metadata. Total runs: {}, Total jobs: {}",
        lab.runs.len(),
        lab.jobs.len()
    );

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
                        "Lab integrity check failed: image file '{}' not found for run.",
                        image_full_path.display()
                    ),
                )));
            }
        }
    }

    for (job_id, job) in &lab.jobs {
        let job_pkg_path = lab_path.join(&job.path_in_lab);
        if !job_pkg_path.is_dir() {
            return Err(AppError::JobPackageIoError {
                job_id: job_id.clone(),
                path: job_pkg_path,
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Job package directory not found",
                ),
            });
        }
    }

    log_debug!("Lab validation successful.");
    Ok(lab)
}

use crate::{
    errors::ConfigError,
    model::{FileEntry, Lab, LabManifest, RootMetadata, Run, RunId, RunMetadataForLoading},
};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

const EXPECTED_REPX_VERSION: &str = env!("CARGO_PKG_VERSION");

fn verify_file_integrity(lab_path: &Path, files: &[FileEntry]) -> Result<(), ConfigError> {
    tracing::debug!(
        "Verifying integrity of {} files in parallel...",
        files.len()
    );

    let result: Result<(), ConfigError> = files.par_iter().try_for_each(|entry| {
        let file_path = lab_path.join(&entry.path);

        if !file_path.exists() {
            return Err(ConfigError::IntegrityFileMissing(entry.path.clone()));
        }

        let mut file = File::open(&file_path).map_err(|e| {
            ConfigError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to open file for integrity check: {}", entry.path),
            ))
        })?;

        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let bytes_read = file.read(&mut buffer).map_err(|e| {
                ConfigError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to read file for integrity check: {}", entry.path),
                ))
            })?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let actual_hash = format!("{:x}", hasher.finalize());

        if actual_hash != entry.sha256 {
            return Err(ConfigError::IntegrityHashMismatch {
                path: entry.path.clone(),
                expected: entry.sha256.clone(),
                actual: actual_hash,
            });
        }

        Ok(())
    });

    result?;
    tracing::debug!("File integrity verification passed.");
    Ok(())
}

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

pub fn load_from_path(initial_path: &Path) -> Result<Lab, ConfigError> {
    tracing::debug!(
        "Attempting to load lab from initial path: '{}'",
        initial_path.display()
    );

    let (lab_path, specific_manifest) = if initial_path.is_file() {
        if let Some(parent) = initial_path.parent() {
            if parent.file_name().and_then(|s| s.to_str()) == Some("lab") {
                if let Some(root) = parent.parent() {
                    (root.to_path_buf(), Some(initial_path.to_path_buf()))
                } else {
                    (parent.to_path_buf(), None)
                }
            } else {
                (parent.to_path_buf(), None)
            }
        } else {
            (initial_path.parent().unwrap().to_path_buf(), None)
        }
    } else {
        (initial_path.to_path_buf(), None)
    };

    tracing::debug!(
        "Loading and validating lab from resolved directory '{}'...",
        lab_path.display()
    );

    if !lab_path.is_dir() {
        return Err(ConfigError::LabNotFound(lab_path.to_path_buf()));
    }

    let manifest_path = if let Some(p) = specific_manifest {
        p
    } else {
        find_manifest_path(&lab_path)
            .ok_or_else(|| ConfigError::MetadataNotFound(lab_path.to_path_buf()))?
    };

    tracing::debug!("Found lab manifest at: '{}'", manifest_path.display());

    let manifest_content = fs::read_to_string(&manifest_path)?;
    let manifest: LabManifest = serde_json::from_str(&manifest_content)?;
    let content_hash = manifest.lab_id.clone();
    let lab_version = manifest.lab_version.clone();

    tracing::debug!("Lab Content Hash (ID): {}", content_hash);
    tracing::debug!("Lab Version: {}", lab_version);

    verify_file_integrity(&lab_path, &manifest.files)?;

    let root_metadata_path = lab_path.join(&manifest.metadata);
    if !root_metadata_path.is_file() {
        return Err(ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "Root metadata file not found at '{}'",
                root_metadata_path.display()
            ),
        )));
    }

    tracing::debug!(
        "Loading root metadata from '{}'",
        root_metadata_path.display()
    );
    let root_metadata_content = fs::read_to_string(&root_metadata_path)?;
    let root_meta: RootMetadata = serde_json::from_str(&root_metadata_content)?;

    if root_meta.repx_version != EXPECTED_REPX_VERSION {
        tracing::warn!(
            "Lab version mismatch: binary expects '{}', lab has '{}'. Proceeding anyway.",
            EXPECTED_REPX_VERSION,
            root_meta.repx_version
        );
    } else {
        tracing::debug!("repx_version check passed: {}", root_meta.repx_version);
    }

    let host_tools_root = lab_path.join("host-tools");
    if !host_tools_root.is_dir() {
        return Err(ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "'host-tools' directory not found in lab at '{}'",
                host_tools_root.display()
            ),
        )));
    }

    let host_tools_entry = fs::read_dir(&host_tools_root)?
        .filter_map(Result::ok)
        .find(|e| e.path().is_dir())
        .ok_or_else(|| {
            ConfigError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No tool directory found inside host-tools",
            ))
        })?;

    let host_tools_dir_name = host_tools_entry.file_name().to_string_lossy().to_string();
    let host_tools_path = host_tools_entry.path().join("bin");

    let mut referenced_files = Vec::new();
    if let Ok(p) = manifest_path.strip_prefix(&lab_path) {
        referenced_files.push(p.to_path_buf());
    }
    if let Ok(p) = root_metadata_path.strip_prefix(&lab_path) {
        referenced_files.push(p.to_path_buf());
    }
    if let Ok(p) = host_tools_entry.path().strip_prefix(&lab_path) {
        referenced_files.push(p.to_path_buf());
    }

    let groups = root_meta
        .groups
        .into_iter()
        .map(|(name, run_names)| {
            let run_ids = run_names.into_iter().map(RunId).collect();
            (name, run_ids)
        })
        .collect();

    let mut lab = Lab {
        repx_version: root_meta.repx_version,
        lab_version,
        git_hash: root_meta.git_hash,
        content_hash,
        runs: HashMap::new(),
        jobs: HashMap::new(),
        groups,
        host_tools_path,
        host_tools_dir_name,
        referenced_files,
    };

    for run_rel_path in root_meta.runs {
        lab.referenced_files.push(PathBuf::from(&run_rel_path));
        let run_metadata_path = lab_path.join(&run_rel_path);
        tracing::debug!(
            "Loading run metadata from '{}'",
            run_metadata_path.display()
        );

        let run_meta_content = fs::read_to_string(&run_metadata_path).map_err(|e| {
            ConfigError::Io(std::io::Error::new(
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

        if let Some(img) = &run_meta.image {
            lab.referenced_files.push(img.clone());
        }

        let run = Run {
            image: run_meta.image,
            jobs: job_ids_for_run,
            dependencies: run_meta.dependencies,
        };

        lab.runs.insert(run_id, run);

        for (job_id, mut job) in run_meta.jobs.drain() {
            job.path_in_lab = PathBuf::from("jobs").join(&job_id.0);
            lab.referenced_files.push(job.path_in_lab.clone());
            lab.jobs.insert(job_id, job);
        }
    }

    tracing::debug!(
        "Successfully parsed all metadata. Total runs: {}, Total jobs: {}",
        lab.runs.len(),
        lab.jobs.len()
    );

    let jobs_dir = lab_path.join("jobs");
    if !jobs_dir.is_dir() {
        return Err(ConfigError::IntegrityError(format!(
            "'jobs' directory not found in lab at '{}'",
            lab_path.display()
        )));
    }

    for run in lab.runs.values() {
        if let Some(image_rel_path) = &run.image {
            let image_full_path = lab_path.join(image_rel_path);
            if !image_full_path.exists() {
                return Err(ConfigError::IntegrityError(format!(
                    "image file '{}' not found for run.",
                    image_full_path.display()
                )));
            }
        }
    }

    for (job_id, job) in &lab.jobs {
        let job_pkg_path = lab_path.join(&job.path_in_lab);
        if !job_pkg_path.is_dir() {
            return Err(ConfigError::IntegrityError(format!(
                "Job package directory not found for job '{}' at '{}'",
                job_id,
                job_pkg_path.display()
            )));
        }
    }

    tracing::debug!("Lab validation successful.");
    Ok(lab)
}

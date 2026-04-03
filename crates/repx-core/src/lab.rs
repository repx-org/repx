use crate::{
    errors::CoreError,
    model::{FileEntry, Lab, LabManifest, RootMetadata, Run, RunId, RunMetadataForLoading},
    path_safety::safe_join,
};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

const EXPECTED_REPX_VERSION: &str = env!("CARGO_PKG_VERSION");
const HASH_BUFFER_SIZE: usize = 8192;

#[derive(Debug, Clone)]
pub enum LabSource {
        Directory(PathBuf),
        Tar(PathBuf),
}

impl LabSource {
        pub fn from_path(path: &Path) -> Self {
        if path.is_file() {
            LabSource::Tar(path.to_path_buf())
        } else {
            LabSource::Directory(path.to_path_buf())
        }
    }

        pub fn path(&self) -> &Path {
        match self {
            LabSource::Directory(p) | LabSource::Tar(p) => p,
        }
    }

        pub fn is_tar(&self) -> bool {
        matches!(self, LabSource::Tar(_))
    }
}

impl std::fmt::Display for LabSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LabSource::Directory(p) => write!(f, "{}", p.display()),
            LabSource::Tar(p) => write!(f, "{}", p.display()),
        }
    }
}

pub fn load(source: &LabSource) -> Result<Lab, CoreError> {
    match source {
        LabSource::Directory(path) => load_from_path(path),
        LabSource::Tar(path) => load_from_tar(path),
    }
}

pub fn load_unchecked(source: &LabSource) -> Result<Lab, CoreError> {
    match source {
        LabSource::Directory(path) => load_from_path_unchecked(path),
        LabSource::Tar(path) => load_from_tar(path),
    }
}

fn reject_external_symlink(path: &Path, lab_root: &Path) -> Result<(), CoreError> {
    let meta = fs::symlink_metadata(path).map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!(
                "Failed to read symlink metadata for '{}': {}",
                path.display(),
                e
            ),
        ))
    })?;
    if meta.file_type().is_symlink() {
        let target = fs::read_link(path).map_err(|e| {
            CoreError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to read symlink target for '{}': {}",
                    path.display(),
                    e
                ),
            ))
        })?;
        let resolved = if target.is_absolute() {
            target.clone()
        } else {
            path.parent().unwrap_or(Path::new(".")).join(&target)
        };
        let canonical_target = resolved
            .canonicalize()
            .map_err(|_| CoreError::SymlinkEscape {
                link: path.to_path_buf(),
                target: target.clone(),
            })?;
        let canonical_root = lab_root.canonicalize().map_err(|e| {
            CoreError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to canonicalize lab root '{}': {}",
                    lab_root.display(),
                    e
                ),
            ))
        })?;
        if !canonical_target.starts_with(&canonical_root) {
            return Err(CoreError::SymlinkEscape {
                link: path.to_path_buf(),
                target: canonical_target,
            });
        }
    }
    Ok(())
}

fn verify_file_integrity(lab_path: &Path, files: &[FileEntry]) -> Result<(), CoreError> {
    tracing::debug!(
        "Verifying integrity of {} files in parallel...",
        files.len()
    );

    let result: Result<(), CoreError> = files.par_iter().try_for_each(|entry| {
        let file_path = safe_join(lab_path, &entry.path)?;

        if !file_path.exists() {
            return Err(CoreError::IntegrityFileMissing(entry.path.clone()));
        }

        let mut file = File::open(&file_path).map_err(|e| {
            CoreError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to open file for integrity check: {}", entry.path),
            ))
        })?;

        let mut hasher = Sha256::new();
        let mut buffer = [0u8; HASH_BUFFER_SIZE];
        loop {
            let bytes_read = file.read(&mut buffer).map_err(|e| {
                CoreError::Io(std::io::Error::new(
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

        if actual_hash != entry.sha256.as_str() {
            return Err(CoreError::IntegrityHashMismatch {
                path: entry.path.clone(),
                expected: entry.sha256.to_string(),
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

pub fn load_from_path_unchecked(initial_path: &Path) -> Result<Lab, CoreError> {
    load_from_path_inner(initial_path, false)
}

pub fn load_from_path(initial_path: &Path) -> Result<Lab, CoreError> {
    load_from_path_inner(initial_path, true)
}

fn load_from_path_inner(initial_path: &Path, verify_integrity: bool) -> Result<Lab, CoreError> {
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
            return Err(CoreError::InvalidConfig {
                detail: format!("Path '{}' has no parent directory", initial_path.display()),
            });
        }
    } else {
        (initial_path.to_path_buf(), None)
    };

    tracing::debug!(
        "Loading and validating lab from resolved directory '{}'...",
        lab_path.display()
    );

    if !lab_path.is_dir() {
        return Err(CoreError::LabNotFound(lab_path.to_path_buf()));
    }

    let manifest_path = if let Some(p) = specific_manifest {
        p
    } else {
        find_manifest_path(&lab_path)
            .ok_or_else(|| CoreError::MetadataNotFound(lab_path.to_path_buf()))?
    };

    tracing::debug!("Found lab manifest at: '{}'", manifest_path.display());
    reject_external_symlink(&manifest_path, &lab_path)?;

    let manifest_content =
        fs::read_to_string(&manifest_path).map_err(|e| CoreError::path_io(&manifest_path, e))?;
    let manifest: LabManifest = serde_json::from_str(&manifest_content)
        .map_err(|e| CoreError::json_path(&manifest_path, e))?;
    let content_hash = manifest.lab_id.clone();
    let lab_version = manifest.lab_version.clone();

    tracing::debug!("Lab Content Hash (ID): {}", content_hash);
    tracing::debug!("Lab Version: {}", lab_version);

    if verify_integrity {
        verify_file_integrity(&lab_path, &manifest.files)?;
    }

    let root_metadata_path = lab_path.join(&manifest.metadata);
    if !root_metadata_path.is_file() {
        return Err(CoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "Root metadata file not found at '{}'",
                root_metadata_path.display()
            ),
        )));
    }

    reject_external_symlink(&root_metadata_path, &lab_path)?;
    tracing::debug!(
        "Loading root metadata from '{}'",
        root_metadata_path.display()
    );
    let root_metadata_content = fs::read_to_string(&root_metadata_path)
        .map_err(|e| CoreError::path_io(&root_metadata_path, e))?;
    let root_meta: RootMetadata = serde_json::from_str(&root_metadata_content)
        .map_err(|e| CoreError::json_path(&root_metadata_path, e))?;

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
        return Err(CoreError::Io(std::io::Error::new(
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
            CoreError::Io(std::io::Error::new(
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

    {
        let mut seen = std::collections::HashSet::new();
        for entry in &manifest.files {
            let p = Path::new(&entry.path);
            let mut components = p.components();
            if let (Some(a), Some(b)) = (components.next(), components.next()) {
                let dir_entry = PathBuf::from(a.as_os_str()).join(b.as_os_str());
                if seen.insert(dir_entry.clone()) {
                    referenced_files.push(dir_entry);
                }
            }
        }
    }

    let groups = root_meta
        .groups
        .into_iter()
        .map(|(name, run_names)| {
            let run_ids = run_names.into_iter().map(RunId::from).collect();
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

        reject_external_symlink(&run_metadata_path, &lab_path)?;
        let run_meta_content = fs::read_to_string(&run_metadata_path).map_err(|e| {
            CoreError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to read run metadata at {:?}: {}",
                    run_metadata_path, e
                ),
            ))
        })?;

        let mut run_meta: RunMetadataForLoading = serde_json::from_str(&run_meta_content)
            .map_err(|e| CoreError::json_path(&run_metadata_path, e))?;
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
            job.path_in_lab = PathBuf::from("jobs").join(job_id.as_str());
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
        return Err(CoreError::IntegrityError(format!(
            "'jobs' directory not found in lab at '{}'",
            lab_path.display()
        )));
    }

    for run in lab.runs.values() {
        if let Some(image_rel_path) = &run.image {
            let image_full_path = lab_path.join(image_rel_path);
            if !image_full_path.exists() {
                return Err(CoreError::IntegrityError(format!(
                    "image file '{}' not found for run.",
                    image_full_path.display()
                )));
            }
            reject_external_symlink(&image_full_path, &lab_path)?;
        }
    }

    for (job_id, job) in &lab.jobs {
        let job_pkg_path = lab_path.join(&job.path_in_lab);
        if !job_pkg_path.is_dir() {
            return Err(CoreError::IntegrityError(format!(
                "Job package directory not found for job '{}' at '{}'",
                job_id,
                job_pkg_path.display()
            )));
        }
        reject_external_symlink(&job_pkg_path, &lab_path)?;
    }

    tracing::debug!("Lab validation successful.");
    Ok(lab)
}


fn strip_tar_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    path.strip_prefix(prefix)
        .map(|s| s.strip_prefix('/').unwrap_or(s))
}

pub fn load_from_tar(tar_path: &Path) -> Result<Lab, CoreError> {
    tracing::debug!("Loading lab from tar: '{}'", tar_path.display());

    let file = File::open(tar_path).map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to open lab tar '{}': {}", tar_path.display(), e),
        ))
    })?;
    let mut archive = tar::Archive::new(file);

    let mut file_hashes: HashMap<String, String> = HashMap::new(); 
    let mut file_contents: HashMap<String, Vec<u8>> = HashMap::new(); 
    let mut known_paths: HashSet<String> = HashSet::new(); 
    let mut dir_paths: HashSet<String> = HashSet::new(); 
    let mut symlinks: HashMap<String, PathBuf> = HashMap::new(); 

    let mut tar_prefix: Option<String> = None;

    let entries = archive.entries().map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to read tar entries from '{}': {}", tar_path.display(), e),
        ))
    })?;

    for entry_result in entries {
        let mut entry = entry_result.map_err(|e| {
            CoreError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to read tar entry: {}", e),
            ))
        })?;

        let raw_path = entry
            .path()
            .map_err(|e| {
                CoreError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Invalid path in tar entry: {}", e),
                ))
            })?
            .to_path_buf();
        let raw_path_str = raw_path.to_string_lossy().to_string();

        if tar_prefix.is_none() {
            if let Some(first_component) = raw_path.components().next() {
                let component_str = first_component.as_os_str().to_string_lossy().to_string();
                if raw_path.components().count() > 1 {
                    tar_prefix = Some(format!("{}/", component_str));
                } else if entry.header().entry_type().is_dir() {
                    tar_prefix = Some(format!("{}/", component_str.trim_end_matches('/')));
                } else {
                    tar_prefix = Some(String::new());
                }
            }
        }

        let prefix = tar_prefix.as_deref().unwrap_or("");
        let rel_path = match strip_tar_prefix(&raw_path_str, prefix) {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => {
                continue;
            }
        };

        let entry_type = entry.header().entry_type();

        if entry_type.is_dir() {
            let dir_name = rel_path.trim_end_matches('/').to_string();
            dir_paths.insert(dir_name.clone());
            known_paths.insert(dir_name);
            continue;
        }

        if entry_type.is_symlink() {
            if let Ok(Some(target)) = entry.link_name() {
                symlinks.insert(rel_path.clone(), target.to_path_buf());
            }
            known_paths.insert(rel_path);
            continue;
        }

        if !entry_type.is_file() {
            continue;
        }

        known_paths.insert(rel_path.clone());

        {
            let p = Path::new(&rel_path);
            let mut ancestor = p.parent();
            while let Some(dir) = ancestor {
                let dir_str = dir.to_string_lossy().to_string();
                if dir_str.is_empty() {
                    break;
                }
                dir_paths.insert(dir_str.clone());
                known_paths.insert(dir_str);
                ancestor = dir.parent();
            }
        }

        let mut hasher = Sha256::new();
        let mut buf = [0u8; HASH_BUFFER_SIZE];
        let mut content_buf: Option<Vec<u8>> = None;

        let should_buffer = rel_path.ends_with(".json")
            || rel_path.starts_with("lab/")
            || rel_path.starts_with("revision/");

        if should_buffer {
            content_buf = Some(Vec::new());
        }

        loop {
            let n = entry.read(&mut buf).map_err(|e| {
                CoreError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to read tar entry '{}': {}", rel_path, e),
                ))
            })?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            if let Some(ref mut cb) = content_buf {
                cb.extend_from_slice(&buf[..n]);
            }
        }

        let hash = format!("{:x}", hasher.finalize());
        file_hashes.insert(rel_path.clone(), hash);

        if let Some(cb) = content_buf {
            file_contents.insert(rel_path, cb);
        }
    }

    tracing::debug!(
        "Tar scan complete: {} files hashed, {} metadata files buffered, {} paths known",
        file_hashes.len(),
        file_contents.len(),
        known_paths.len()
    );

    let manifest_key = file_contents
        .keys()
        .find(|k| k.starts_with("lab/") && k.ends_with("lab-metadata.json"))
        .cloned()
        .ok_or_else(|| CoreError::MetadataNotFound(tar_path.to_path_buf()))?;

    let manifest_bytes = file_contents
        .get(&manifest_key)
        .ok_or_else(|| CoreError::MetadataNotFound(tar_path.to_path_buf()))?;
    let manifest: LabManifest = serde_json::from_slice(manifest_bytes)
        .map_err(CoreError::Json)?;
    let content_hash = manifest.lab_id.clone();
    let lab_version = manifest.lab_version.clone();

    tracing::debug!("Lab Content Hash (ID): {}", content_hash);
    tracing::debug!("Lab Version: {}", lab_version);

    tracing::debug!(
        "Verifying integrity of {} manifest files against tar hashes...",
        manifest.files.len()
    );
    for entry in &manifest.files {
        match file_hashes.get(&entry.path) {
            None => {
                return Err(CoreError::IntegrityFileMissing(entry.path.clone()));
            }
            Some(actual_hash) => {
                if actual_hash != entry.sha256.as_str() {
                    return Err(CoreError::IntegrityHashMismatch {
                        path: entry.path.clone(),
                        expected: entry.sha256.to_string(),
                        actual: actual_hash.clone(),
                    });
                }
            }
        }
    }
    tracing::debug!("File integrity verification passed.");

    let root_meta_bytes = file_contents
        .get(&manifest.metadata)
        .ok_or_else(|| {
            CoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Root metadata '{}' not found in tar", manifest.metadata),
            ))
        })?;
    let root_meta: RootMetadata = serde_json::from_slice(root_meta_bytes)
        .map_err(CoreError::Json)?;

    if root_meta.repx_version != EXPECTED_REPX_VERSION {
        tracing::warn!(
            "Lab version mismatch: binary expects '{}', lab has '{}'. Proceeding anyway.",
            EXPECTED_REPX_VERSION,
            root_meta.repx_version
        );
    } else {
        tracing::debug!("repx_version check passed: {}", root_meta.repx_version);
    }

    let host_tools_dir_name = dir_paths
        .iter()
        .filter_map(|p| {
            let path = Path::new(p);
            let mut components = path.components();
            let first = components.next()?;
            let second = components.next()?;
            if first.as_os_str() == "host-tools" && components.next().is_none() {
                Some(second.as_os_str().to_string_lossy().to_string())
            } else {
                None
            }
        })
        .next()
        .ok_or_else(|| {
            CoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "'host-tools' directory not found in tar",
            ))
        })?;

    let host_tools_path = PathBuf::from("host-tools")
        .join(&host_tools_dir_name)
        .join("bin");

    let mut referenced_files = Vec::new();
    referenced_files.push(PathBuf::from(&manifest_key));
    referenced_files.push(PathBuf::from(&manifest.metadata));
    referenced_files.push(
        PathBuf::from("host-tools").join(&host_tools_dir_name),
    );

    {
        let mut seen = HashSet::new();
        for entry in &manifest.files {
            let p = Path::new(&entry.path);
            let mut components = p.components();
            if let (Some(a), Some(b)) = (components.next(), components.next()) {
                let dir_entry = PathBuf::from(a.as_os_str()).join(b.as_os_str());
                if seen.insert(dir_entry.clone()) {
                    referenced_files.push(dir_entry);
                }
            }
        }
    }

    let groups = root_meta
        .groups
        .into_iter()
        .map(|(name, run_names)| {
            let run_ids = run_names.into_iter().map(RunId::from).collect();
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

        let run_meta_bytes = file_contents
            .get(&run_rel_path)
            .ok_or_else(|| {
                CoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Run metadata '{}' not found in tar", run_rel_path),
                ))
            })?;

        let mut run_meta: RunMetadataForLoading = serde_json::from_slice(run_meta_bytes)
            .map_err(CoreError::Json)?;
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
            job.path_in_lab = PathBuf::from("jobs").join(job_id.as_str());
            lab.referenced_files.push(job.path_in_lab.clone());
            lab.jobs.insert(job_id, job);
        }
    }

    tracing::debug!(
        "Successfully parsed all metadata from tar. Total runs: {}, Total jobs: {}",
        lab.runs.len(),
        lab.jobs.len()
    );

    if !dir_paths.contains("jobs") {
        return Err(CoreError::IntegrityError(
            "'jobs' directory not found in lab tar".to_string(),
        ));
    }

    for run in lab.runs.values() {
        if let Some(image_rel_path) = &run.image {
            let image_path_str = image_rel_path.to_string_lossy().to_string();
            if !known_paths.contains(&image_path_str) && !dir_paths.contains(&image_path_str) {
                return Err(CoreError::IntegrityError(format!(
                    "image file '{}' not found in tar.",
                    image_path_str
                )));
            }
        }
    }

    for (job_id, job) in &lab.jobs {
        let job_pkg_str = job.path_in_lab.to_string_lossy().to_string();
        if !dir_paths.contains(&job_pkg_str) {
            return Err(CoreError::IntegrityError(format!(
                "Job package directory not found for job '{}' at '{}' in tar",
                job_id, job_pkg_str
            )));
        }
    }

    for (link_path, target) in &symlinks {
        let link_parent = Path::new(link_path)
            .parent()
            .unwrap_or(Path::new(""));
        let resolved = if target.is_absolute() {
            return Err(CoreError::SymlinkEscape {
                link: PathBuf::from(link_path),
                target: target.clone(),
            });
        } else {
            link_parent.join(target)
        };
        let mut depth: i32 = 0;
        for component in resolved.components() {
            match component {
                std::path::Component::ParentDir => {
                    depth -= 1;
                    if depth < 0 {
                        return Err(CoreError::SymlinkEscape {
                            link: PathBuf::from(link_path),
                            target: target.clone(),
                        });
                    }
                }
                std::path::Component::Normal(_) => {
                    depth += 1;
                }
                _ => {}
            }
        }
    }

    tracing::debug!("Lab tar validation successful.");
    Ok(lab)
}

pub fn list_tar_entries(tar_path: &Path, prefix: &str) -> Result<Vec<String>, CoreError> {
    let file = File::open(tar_path).map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to open tar '{}': {}", tar_path.display(), e),
        ))
    })?;
    let mut archive = tar::Archive::new(file);
    let mut result = Vec::new();
    let mut tar_prefix: Option<String> = None;

    let entries = archive.entries().map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to read tar entries: {}", e),
        ))
    })?;

    for entry_result in entries {
        let entry = entry_result.map_err(|e| {
            CoreError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to read tar entry: {}", e),
            ))
        })?;

        let raw_path = entry
            .path()
            .map_err(|e| {
                CoreError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Invalid path in tar entry: {}", e),
                ))
            })?
            .to_path_buf();
        let raw_path_str = raw_path.to_string_lossy().to_string();

        if tar_prefix.is_none() {
            if let Some(first_component) = raw_path.components().next() {
                let component_str = first_component.as_os_str().to_string_lossy().to_string();
                if raw_path.components().count() > 1 {
                    tar_prefix = Some(format!("{}/", component_str));
                } else if entry.header().entry_type().is_dir() {
                    tar_prefix = Some(format!("{}/", component_str.trim_end_matches('/')));
                } else {
                    tar_prefix = Some(String::new());
                }
            }
        }

        let pfx = tar_prefix.as_deref().unwrap_or("");
        if let Some(rel) = strip_tar_prefix(&raw_path_str, pfx) {
            if !rel.is_empty() && rel.starts_with(prefix) {
                result.push(rel.to_string());
            }
        }
    }

    Ok(result)
}

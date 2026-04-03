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
        tar_dir_name: None,
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

fn detect_tar_prefix(tar_path: &Path) -> Result<String, CoreError> {
    let data = fs::read(tar_path).map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to read tar '{}': {}", tar_path.display(), e),
        ))
    })?;
    detect_tar_prefix_from_bytes(&data, tar_path)
}

fn detect_tar_prefix_from_bytes(data: &[u8], source: &Path) -> Result<String, CoreError> {
    let cursor = std::io::Cursor::new(data);
    let mut probe = tar::Archive::new(cursor);
    let detected = probe.entries().ok().and_then(|entries| {
        entries.flatten().find_map(|entry| {
            let path = entry.path().ok()?;
            let s = path.to_string_lossy();
            let needle = "lab/";
            let mut pos = 0;
            while pos < s.len() {
                let idx = s[pos..].find(needle)?;
                let abs_idx = pos + idx;
                if (abs_idx == 0 || s.as_bytes()[abs_idx - 1] == b'/')
                    && s[abs_idx + needle.len()..].ends_with("-lab-metadata.json")
                {
                    return Some(s[..abs_idx].to_string());
                }
                pos = abs_idx + 1;
            }
            None
        })
    });
    detected.ok_or_else(|| CoreError::MetadataNotFound(source.to_path_buf()))
}

struct TarProbe {
    prefix: String,
    manifest: LabManifest,
    files_to_verify: HashSet<String>,
    file_contents: HashMap<String, Vec<u8>>,
    known_paths: HashSet<String>,
    dir_paths: HashSet<String>,
    symlinks: HashMap<String, PathBuf>,
    hardlinks: Vec<(String, String)>,
}

fn probe_tar(data: &[u8], prefix: &str) -> Result<TarProbe, CoreError> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = tar::Archive::new(cursor);
    let entries = archive.entries().map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to read tar entries: {}", e),
        ))
    })?;

    let mut file_contents: HashMap<String, Vec<u8>> = HashMap::new();
    let mut known_paths: HashSet<String> = HashSet::new();
    let mut dir_paths: HashSet<String> = HashSet::new();
    let mut symlinks: HashMap<String, PathBuf> = HashMap::new();
    let mut hardlinks: Vec<(String, String)> = Vec::new();

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

        let rel_path = match strip_tar_prefix(&raw_path_str, prefix) {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => continue,
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

        if entry_type == tar::EntryType::Link {
            if let Ok(Some(link_target)) = entry.link_name() {
                let target_str = link_target.to_string_lossy().to_string();
                let target_rel = match strip_tar_prefix(&target_str, prefix) {
                    Some(p) if !p.is_empty() => p.to_string(),
                    _ => target_str,
                };
                hardlinks.push((rel_path.clone(), target_rel));
            }
            known_paths.insert(rel_path.clone());
            register_parent_dirs(&rel_path, &mut dir_paths, &mut known_paths);
            continue;
        }

        if !entry_type.is_file() {
            continue;
        }

        known_paths.insert(rel_path.clone());
        register_parent_dirs(&rel_path, &mut dir_paths, &mut known_paths);

        let should_buffer = rel_path.ends_with(".json")
            || rel_path.starts_with("lab/")
            || rel_path.starts_with("revision/");

        if should_buffer {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(|e| {
                CoreError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to read tar entry '{}': {}", rel_path, e),
                ))
            })?;
            file_contents.insert(rel_path, buf);
        }
    }

    let manifest_key = file_contents
        .keys()
        .find(|k| k.starts_with("lab/") && k.ends_with("lab-metadata.json"))
        .cloned()
        .ok_or_else(|| {
            CoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Lab manifest not found in tar",
            ))
        })?;

    let manifest_bytes = file_contents.get(&manifest_key).ok_or_else(|| {
        CoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Manifest key found but content missing from probe",
        ))
    })?;
    let manifest: LabManifest = serde_json::from_slice(manifest_bytes).map_err(CoreError::Json)?;

    let files_to_verify: HashSet<String> = manifest.files.iter().map(|f| f.path.clone()).collect();

    tracing::debug!(
        "Tar probe complete: {} metadata files buffered, {} paths known, {} files to verify",
        file_contents.len(),
        known_paths.len(),
        files_to_verify.len(),
    );

    Ok(TarProbe {
        prefix: prefix.to_string(),
        manifest,
        files_to_verify,
        file_contents,
        known_paths,
        dir_paths,
        symlinks,
        hardlinks,
    })
}

fn register_parent_dirs(
    rel_path: &str,
    dir_paths: &mut HashSet<String>,
    known_paths: &mut HashSet<String>,
) {
    let p = Path::new(rel_path);
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

pub fn load_from_tar(tar_path: &Path) -> Result<Lab, CoreError> {
    tracing::debug!("Loading lab from tar: '{}'", tar_path.display());

    let data = fs::read(tar_path).map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to read tar '{}': {}", tar_path.display(), e),
        ))
    })?;
    tracing::debug!(
        "Read {} bytes from tar '{}'",
        data.len(),
        tar_path.display()
    );

    let prefix = detect_tar_prefix_from_bytes(&data, tar_path)?;
    let probe = probe_tar(&data, &prefix)?;

    let content_hash = probe.manifest.lab_id.clone();
    let lab_version = probe.manifest.lab_version.clone();

    tracing::debug!("Lab Content Hash (ID): {}", content_hash);
    tracing::debug!("Lab Version: {}", lab_version);

    tracing::debug!(
        "Verifying integrity of {} manifest files...",
        probe.files_to_verify.len()
    );

    let mut file_hashes: HashMap<String, String> = HashMap::new();

    let verify_cursor = std::io::Cursor::new(&data);
    let mut verify_archive = tar::Archive::new(verify_cursor);
    let verify_entries = verify_archive.entries().map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to read tar entries: {}", e),
        ))
    })?;

    let hardlink_map: HashMap<String, String> = probe
        .hardlinks
        .iter()
        .map(|(link, target)| (link.clone(), target.clone()))
        .collect();

    let mut hardlink_targets: HashSet<String> = HashSet::new();
    for path in &probe.files_to_verify {
        if hardlink_map.contains_key(path) {
            let mut current = path.as_str();
            let mut depth = 0;
            while let Some(next) = hardlink_map.get(current) {
                current = next.as_str();
                depth += 1;
                if depth > 100 {
                    break;
                }
            }
            hardlink_targets.insert(current.to_string());
        }
    }

    for entry_result in verify_entries {
        let mut entry = entry_result.map_err(|e| {
            CoreError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to read tar entry: {}", e),
            ))
        })?;

        if !entry.header().entry_type().is_file() {
            continue;
        }

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

        let rel_path = match strip_tar_prefix(&raw_path_str, &probe.prefix) {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => continue,
        };

        if !probe.files_to_verify.contains(&rel_path) && !hardlink_targets.contains(&rel_path) {
            continue;
        }

        let mut hasher = Sha256::new();
        let mut buf = [0u8; HASH_BUFFER_SIZE];
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
        }
        file_hashes.insert(rel_path, format!("{:x}", hasher.finalize()));
    }

    for path in &probe.files_to_verify {
        if hardlink_map.contains_key(path) {
            let mut current = path.as_str();
            let mut depth = 0;
            while let Some(next) = hardlink_map.get(current) {
                current = next.as_str();
                depth += 1;
                if depth > 100 {
                    break;
                }
            }
            if let Some(hash) = file_hashes.get(current).cloned() {
                file_hashes.insert(path.clone(), hash);
            }
        }
    }

    tracing::debug!(
        "Hashed {} files (of {} in manifest)",
        file_hashes.len(),
        probe.files_to_verify.len(),
    );

    for entry in &probe.manifest.files {
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

    let root_meta_bytes = probe
        .file_contents
        .get(&probe.manifest.metadata)
        .ok_or_else(|| {
            CoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Root metadata '{}' not found in tar",
                    probe.manifest.metadata
                ),
            ))
        })?;
    let root_meta: RootMetadata =
        serde_json::from_slice(root_meta_bytes).map_err(CoreError::Json)?;

    if root_meta.repx_version != EXPECTED_REPX_VERSION {
        tracing::warn!(
            "Lab version mismatch: binary expects '{}', lab has '{}'. Proceeding anyway.",
            EXPECTED_REPX_VERSION,
            root_meta.repx_version
        );
    } else {
        tracing::debug!("repx_version check passed: {}", root_meta.repx_version);
    }

    let host_tools_dir_name = probe
        .dir_paths
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

    let manifest_key = probe
        .file_contents
        .keys()
        .find(|k| k.starts_with("lab/") && k.ends_with("lab-metadata.json"))
        .cloned()
        .unwrap_or_default();

    let mut referenced_files = Vec::new();
    referenced_files.push(PathBuf::from(&manifest_key));
    referenced_files.push(PathBuf::from(&probe.manifest.metadata));
    referenced_files.push(PathBuf::from("host-tools").join(&host_tools_dir_name));

    {
        let mut seen = HashSet::new();
        for entry in &probe.manifest.files {
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

    let tar_dir_name = {
        let trimmed = prefix.trim_end_matches('/');
        trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
    };

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
        tar_dir_name: Some(tar_dir_name),
    };

    for run_rel_path in root_meta.runs {
        lab.referenced_files.push(PathBuf::from(&run_rel_path));

        let run_meta_bytes = probe.file_contents.get(&run_rel_path).ok_or_else(|| {
            CoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Run metadata '{}' not found in tar", run_rel_path),
            ))
        })?;

        let mut run_meta: RunMetadataForLoading =
            serde_json::from_slice(run_meta_bytes).map_err(CoreError::Json)?;
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

    if !probe.dir_paths.contains("jobs") {
        return Err(CoreError::IntegrityError(
            "'jobs' directory not found in lab tar".to_string(),
        ));
    }

    for run in lab.runs.values() {
        if let Some(image_rel_path) = &run.image {
            let image_path_str = image_rel_path.to_string_lossy().to_string();
            if !probe.known_paths.contains(&image_path_str)
                && !probe.dir_paths.contains(&image_path_str)
            {
                return Err(CoreError::IntegrityError(format!(
                    "image file '{}' not found in tar.",
                    image_path_str
                )));
            }
        }
    }

    for (job_id, job) in &lab.jobs {
        let job_pkg_str = job.path_in_lab.to_string_lossy().to_string();
        if !probe.dir_paths.contains(&job_pkg_str) {
            return Err(CoreError::IntegrityError(format!(
                "Job package directory not found for job '{}' at '{}' in tar",
                job_id, job_pkg_str
            )));
        }
    }

    for (link_path, target) in &probe.symlinks {
        let link_parent = Path::new(link_path).parent().unwrap_or(Path::new(""));
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
    let tar_prefix = detect_tar_prefix(tar_path)?;

    let file = File::open(tar_path).map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to open tar '{}': {}", tar_path.display(), e),
        ))
    })?;
    let mut archive = tar::Archive::new(file);
    let mut result = Vec::new();

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

        if let Some(rel) = strip_tar_prefix(&raw_path_str, &tar_prefix) {
            if !rel.is_empty() && rel.starts_with(prefix) {
                result.push(rel.to_string());
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::useless_vec)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    const TEST_VERSION: &str = env!("CARGO_PKG_VERSION");

    fn sha256_hex(data: &[u8]) -> String {
        format!("{:x}", Sha256::digest(data))
    }

    fn build_test_lab_tar(path: &Path) {
        build_test_lab_tar_with_options(path, TestLabOptions::default());
    }

    #[derive(Default)]
    struct TestLabOptions {
        add_escaping_symlink: Option<String>,
        add_hardlink: Option<(String, String)>,
        extra_files: Vec<(String, Vec<u8>)>,
        flat: bool,
    }

    fn build_test_lab_tar_with_options(path: &Path, opts: TestLabOptions) {
        let prefix = if opts.flat { "" } else { "result/" };

        let job_id = "abc123-test-job-1.0";
        let run_name = "test-run";
        let run_exe_content = b"#!/bin/sh\necho hello\n";
        let run_exe_path = format!("jobs/{}/run.sh", job_id);

        let run_metadata = serde_json::json!({
            "name": run_name,
            "jobs": {
                job_id: {
                    "name": "test-job",
                    "params": {},
                    "executables": {
                        "main": {
                            "path": run_exe_path,
                            "inputs": [],
                            "outputs": {}
                        }
                    }
                }
            }
        });
        let run_meta_bytes = serde_json::to_vec_pretty(&run_metadata).unwrap();
        let run_meta_hash = sha256_hex(&run_meta_bytes);
        let run_meta_rel = format!(
            "revision/{}-metadata-{}.json",
            &run_meta_hash[..16],
            run_name
        );

        let root_metadata = serde_json::json!({
            "repx_version": TEST_VERSION,
            "gitHash": "deadbeef",
            "runs": [run_meta_rel],
            "groups": {}
        });
        let root_meta_bytes = serde_json::to_vec_pretty(&root_metadata).unwrap();
        let root_meta_hash = sha256_hex(&root_meta_bytes);
        let root_meta_rel = format!("{}-metadata.json", &root_meta_hash[..16]);

        let files = vec![
            (root_meta_rel.clone(), root_meta_bytes.clone()),
            (run_meta_rel.clone(), run_meta_bytes.clone()),
            (run_exe_path.clone(), run_exe_content.to_vec()),
        ];

        let file_entries: Vec<serde_json::Value> = files
            .iter()
            .map(|(p, content)| {
                serde_json::json!({
                    "path": p,
                    "sha256": sha256_hex(content),
                })
            })
            .collect();

        let manifest = serde_json::json!({
            "labId": "test-lab-content-hash",
            "lab_version": "1.0",
            "metadata": root_meta_rel,
            "files": file_entries,
        });
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        let manifest_hash = sha256_hex(&manifest_bytes);
        let manifest_rel = format!("lab/{}-lab-metadata.json", &manifest_hash[..16]);

        let file = File::create(path).unwrap();
        let mut builder = tar::Builder::new(file);

        let add_dir = |builder: &mut tar::Builder<File>, dir_path: &str| {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, dir_path, &[][..]).unwrap();
        };

        let add_file =
            |builder: &mut tar::Builder<File>, file_path: &str, content: &[u8], mode: u32| {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(content.len() as u64);
                header.set_mode(mode);
                header.set_cksum();
                builder
                    .append_data(&mut header, file_path, content)
                    .unwrap();
            };

        if !opts.flat {
            add_dir(&mut builder, &format!("{}.", prefix).replace("/.", ""));
        }
        add_dir(&mut builder, &format!("{}lab", prefix));
        add_dir(&mut builder, &format!("{}revision", prefix));
        add_dir(&mut builder, &format!("{}host-tools", prefix));
        add_dir(&mut builder, &format!("{}host-tools/default", prefix));
        add_dir(&mut builder, &format!("{}host-tools/default/bin", prefix));
        add_dir(&mut builder, &format!("{}jobs", prefix));
        add_dir(&mut builder, &format!("{}jobs/{}", prefix, job_id));

        add_file(
            &mut builder,
            &format!("{}{}", prefix, manifest_rel),
            &manifest_bytes,
            0o644,
        );
        add_file(
            &mut builder,
            &format!("{}{}", prefix, root_meta_rel),
            &root_meta_bytes,
            0o644,
        );
        add_file(
            &mut builder,
            &format!("{}{}", prefix, run_meta_rel),
            &run_meta_bytes,
            0o644,
        );
        add_file(
            &mut builder,
            &format!("{}{}", prefix, run_exe_path),
            run_exe_content,
            0o755,
        );

        if let Some((link_path, target_path)) = &opts.add_hardlink {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Link);
            header.set_size(0);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_link(
                    &mut header,
                    format!("{}{}", prefix, link_path),
                    format!("{}{}", prefix, target_path),
                )
                .unwrap();
        }

        if let Some(target) = &opts.add_escaping_symlink {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            header.set_cksum();
            builder
                .append_link(&mut header, format!("{}badlink", prefix), target)
                .unwrap();
        }

        for (rel_path, content) in &opts.extra_files {
            add_file(
                &mut builder,
                &format!("{}{}", prefix, rel_path),
                content,
                0o644,
            );
        }

        builder.finish().unwrap();
    }

    #[test]
    fn test_lab_source_from_path_directory() {
        let dir = tempfile::tempdir().unwrap();
        let source = LabSource::from_path(dir.path());
        assert!(matches!(source, LabSource::Directory(_)));
        assert!(!source.is_tar());
        assert_eq!(source.path(), dir.path());
    }

    #[test]
    fn test_lab_source_from_path_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.tar");
        File::create(&file_path).unwrap();
        let source = LabSource::from_path(&file_path);
        assert!(matches!(source, LabSource::Tar(_)));
        assert!(source.is_tar());
        assert_eq!(source.path(), file_path);
    }

    #[test]
    fn test_lab_source_display() {
        let source = LabSource::Directory(PathBuf::from("/some/path"));
        assert_eq!(format!("{}", source), "/some/path");
        let source = LabSource::Tar(PathBuf::from("/some/file.tar"));
        assert_eq!(format!("{}", source), "/some/file.tar");
    }

    #[test]
    fn test_load_from_tar_basic() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("lab.tar");
        build_test_lab_tar(&tar_path);

        let lab = load_from_tar(&tar_path).unwrap();

        assert_eq!(lab.content_hash, "test-lab-content-hash");
        assert_eq!(lab.lab_version, "1.0");
        assert_eq!(lab.repx_version, TEST_VERSION);
        assert_eq!(lab.git_hash, "deadbeef");
        assert_eq!(lab.runs.len(), 1);
        assert!(lab.runs.contains_key(&RunId::from("test-run".to_string())));
        assert_eq!(lab.jobs.len(), 1);
        assert!(lab.jobs.contains_key(&"abc123-test-job-1.0".into()));
        assert_eq!(lab.host_tools_dir_name, "default");
    }

    #[test]
    fn test_load_from_tar_dot_prefix() {
        let dir = tempfile::tempdir().unwrap();

        let tar_path = dir.path().join("lab.tar");
        build_test_lab_tar(&tar_path);

        let extract_dir = dir.path().join("extracted");
        fs::create_dir_all(&extract_dir).unwrap();
        assert!(std::process::Command::new("tar")
            .arg("xf")
            .arg(&tar_path)
            .arg("--strip-components=1")
            .arg("-C")
            .arg(&extract_dir)
            .status()
            .unwrap()
            .success());

        let dot_tar_path = dir.path().join("dot.tar");
        assert!(std::process::Command::new("tar")
            .arg("cf")
            .arg(&dot_tar_path)
            .arg("-C")
            .arg(&extract_dir)
            .arg(".")
            .status()
            .unwrap()
            .success());

        let lab = load_from_tar(&dot_tar_path).unwrap();
        assert_eq!(lab.content_hash, "test-lab-content-hash");
        assert_eq!(lab.runs.len(), 1);
        assert_eq!(lab.jobs.len(), 1);
    }

    #[test]
    fn test_load_from_tar_flat_layout() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("flat.tar");
        build_test_lab_tar_with_options(
            &tar_path,
            TestLabOptions {
                flat: true,
                ..Default::default()
            },
        );

        let lab = load_from_tar(&tar_path).unwrap();
        assert_eq!(lab.content_hash, "test-lab-content-hash");
        assert_eq!(lab.runs.len(), 1);
        assert_eq!(lab.jobs.len(), 1);
    }

    #[test]
    fn test_load_from_tar_lab_in_dirname() {
        let dir = tempfile::tempdir().unwrap();

        let tar_path = dir.path().join("lab.tar");
        build_test_lab_tar(&tar_path);

        let extract_dir = dir.path().join("extracted");
        fs::create_dir_all(&extract_dir).unwrap();
        assert!(std::process::Command::new("tar")
            .arg("xf")
            .arg(&tar_path)
            .arg("--strip-components=1")
            .arg("-C")
            .arg(&extract_dir)
            .status()
            .unwrap()
            .success());

        let lab_in_name_tar = dir.path().join("lab-in-name.tar");
        assert!(std::process::Command::new("tar")
            .arg("cf")
            .arg(&lab_in_name_tar)
            .arg("-C")
            .arg(dir.path())
            .arg("--transform")
            .arg("s,^extracted,hpc-experiment-lab,")
            .arg("extracted")
            .status()
            .unwrap()
            .success());

        let output = std::process::Command::new("tar")
            .arg("tf")
            .arg(&lab_in_name_tar)
            .output()
            .unwrap();
        let listing = String::from_utf8_lossy(&output.stdout);
        assert!(
            listing.contains("hpc-experiment-lab/lab/"),
            "tar should contain hpc-experiment-lab/lab/..."
        );

        let lab = load_from_tar(&lab_in_name_tar).unwrap();
        assert_eq!(lab.content_hash, "test-lab-content-hash");
        assert_eq!(lab.runs.len(), 1);
        assert_eq!(lab.jobs.len(), 1);
    }

    #[test]
    fn test_load_from_tar_integrity_check() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("bad.tar");

        let file = File::create(&tar_path).unwrap();
        let mut builder = tar::Builder::new(file);

        let job_id = "abc123-test-job-1.0";
        let run_exe_path = format!("jobs/{}/run.sh", job_id);

        let run_metadata = serde_json::json!({
            "name": "test-run",
            "jobs": {
                job_id: {
                    "name": "test-job",
                    "params": {},
                    "executables": {
                        "main": {
                            "path": run_exe_path,
                            "inputs": [],
                            "outputs": {}
                        }
                    }
                }
            }
        });
        let run_meta_bytes = serde_json::to_vec_pretty(&run_metadata).unwrap();
        let run_meta_hash = sha256_hex(&run_meta_bytes);
        let run_meta_rel = format!("revision/{}-metadata-test-run.json", &run_meta_hash[..16]);

        let root_metadata = serde_json::json!({
            "repx_version": TEST_VERSION,
            "gitHash": "deadbeef",
            "runs": [run_meta_rel],
            "groups": {}
        });
        let root_meta_bytes = serde_json::to_vec_pretty(&root_metadata).unwrap();
        let root_meta_hash = sha256_hex(&root_meta_bytes);
        let root_meta_rel = format!("{}-metadata.json", &root_meta_hash[..16]);

        let original_exe_content = b"#!/bin/sh\necho hello\n";
        let file_entries = serde_json::json!([
            {"path": root_meta_rel, "sha256": sha256_hex(&root_meta_bytes)},
            {"path": run_meta_rel, "sha256": sha256_hex(&run_meta_bytes)},
            {"path": run_exe_path, "sha256": sha256_hex(original_exe_content)},
        ]);

        let manifest = serde_json::json!({
            "labId": "test-lab-content-hash",
            "lab_version": "1.0",
            "metadata": root_meta_rel,
            "files": file_entries,
        });
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        let manifest_hash = sha256_hex(&manifest_bytes);

        let add_dir = |b: &mut tar::Builder<File>, p: &str| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Directory);
            h.set_size(0);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, p, &[][..]).unwrap();
        };
        let add_file = |b: &mut tar::Builder<File>, p: &str, c: &[u8]| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Regular);
            h.set_size(c.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, p, c).unwrap();
        };

        add_dir(&mut builder, "result");
        add_dir(&mut builder, "result/lab");
        add_dir(&mut builder, "result/revision");
        add_dir(&mut builder, "result/host-tools");
        add_dir(&mut builder, "result/host-tools/default");
        add_dir(&mut builder, "result/host-tools/default/bin");
        add_dir(&mut builder, "result/jobs");
        add_dir(&mut builder, &format!("result/jobs/{}", job_id));

        add_file(
            &mut builder,
            &format!("result/lab/{}-lab-metadata.json", &manifest_hash[..16]),
            &manifest_bytes,
        );
        add_file(
            &mut builder,
            &format!("result/{}", root_meta_rel),
            &root_meta_bytes,
        );
        add_file(
            &mut builder,
            &format!("result/{}", run_meta_rel),
            &run_meta_bytes,
        );
        add_file(
            &mut builder,
            &format!("result/{}", run_exe_path),
            b"CORRUPTED CONTENT",
        );

        builder.finish().unwrap();

        let result = load_from_tar(&tar_path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            CoreError::IntegrityHashMismatch { path, .. } => {
                assert!(path.contains("run.sh"));
            }
            other => panic!("Expected IntegrityHashMismatch, got: {:?}", other),
        }
    }

    #[test]
    fn test_load_from_tar_hardlinks() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("hardlink.tar");

        let params_content = b"{\"key\": \"value\"}";
        let extra_path = "jobs/abc123-test-job-1.0/params-original.json";

        build_test_lab_tar_with_options(
            &tar_path,
            TestLabOptions {
                extra_files: vec![(extra_path.to_string(), params_content.to_vec())],
                add_hardlink: Some((
                    "jobs/abc123-test-job-1.0/params-link.json".to_string(),
                    extra_path.to_string(),
                )),
                ..Default::default()
            },
        );

        let lab = load_from_tar(&tar_path).unwrap();
        assert_eq!(lab.content_hash, "test-lab-content-hash");
    }

    #[test]
    fn test_load_from_tar_hardlink_chain() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("chain.tar");

        let job_id = "abc123-test-job-1.0";
        let run_exe_content = b"#!/bin/sh\necho hello\n";
        let run_exe_path = format!("jobs/{}/run.sh", job_id);

        let run_metadata = serde_json::json!({
            "name": "test-run",
            "jobs": {
                job_id: {
                    "name": "test-job",
                    "params": {},
                    "executables": {
                        "main": {"path": run_exe_path, "inputs": [], "outputs": {}}
                    }
                }
            }
        });
        let run_meta_bytes = serde_json::to_vec_pretty(&run_metadata).unwrap();
        let run_meta_hash = sha256_hex(&run_meta_bytes);
        let run_meta_rel = format!("revision/{}-metadata-test-run.json", &run_meta_hash[..16]);

        let root_metadata = serde_json::json!({
            "repx_version": TEST_VERSION,
            "gitHash": "deadbeef",
            "runs": [run_meta_rel],
            "groups": {}
        });
        let root_meta_bytes = serde_json::to_vec_pretty(&root_metadata).unwrap();
        let root_meta_hash = sha256_hex(&root_meta_bytes);
        let root_meta_rel = format!("{}-metadata.json", &root_meta_hash[..16]);

        let store_path = "store/run.sh";
        let intermediate_path = "intermediate/run.sh";

        let files = vec![
            (root_meta_rel.clone(), root_meta_bytes.clone()),
            (run_meta_rel.clone(), run_meta_bytes.clone()),
            (run_exe_path.clone(), run_exe_content.to_vec()),
        ];

        let file_entries: Vec<serde_json::Value> = files
            .iter()
            .map(|(p, content)| {
                serde_json::json!({
                    "path": p,
                    "sha256": sha256_hex(content),
                })
            })
            .collect();

        let manifest = serde_json::json!({
            "labId": "chain-test-hash",
            "lab_version": "1.0",
            "metadata": root_meta_rel,
            "files": file_entries,
        });
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        let manifest_hash = sha256_hex(&manifest_bytes);
        let manifest_rel = format!("lab/{}-lab-metadata.json", &manifest_hash[..16]);

        let file = File::create(&tar_path).unwrap();
        let mut builder = tar::Builder::new(file);

        let add_dir = |b: &mut tar::Builder<File>, p: &str| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Directory);
            h.set_size(0);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, p, &[][..]).unwrap();
        };
        let add_file = |b: &mut tar::Builder<File>, p: &str, c: &[u8]| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Regular);
            h.set_size(c.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, p, c).unwrap();
        };

        add_dir(&mut builder, "result");
        add_dir(&mut builder, "result/lab");
        add_dir(&mut builder, "result/revision");
        add_dir(&mut builder, "result/host-tools");
        add_dir(&mut builder, "result/host-tools/default");
        add_dir(&mut builder, "result/host-tools/default/bin");
        add_dir(&mut builder, "result/jobs");
        add_dir(&mut builder, &format!("result/jobs/{}", job_id));
        add_dir(&mut builder, "result/store");
        add_dir(&mut builder, "result/intermediate");

        add_file(
            &mut builder,
            &format!("result/{}", manifest_rel),
            &manifest_bytes,
        );
        add_file(
            &mut builder,
            &format!("result/{}", root_meta_rel),
            &root_meta_bytes,
        );
        add_file(
            &mut builder,
            &format!("result/{}", run_meta_rel),
            &run_meta_bytes,
        );

        add_file(
            &mut builder,
            &format!("result/{}", store_path),
            run_exe_content,
        );

        {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Link);
            h.set_size(0);
            h.set_mode(0o755);
            h.set_cksum();
            builder
                .append_link(
                    &mut h,
                    format!("result/{}", intermediate_path),
                    format!("result/{}", store_path),
                )
                .unwrap();
        }

        {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Link);
            h.set_size(0);
            h.set_mode(0o755);
            h.set_cksum();
            builder
                .append_link(
                    &mut h,
                    format!("result/{}", run_exe_path),
                    format!("result/{}", intermediate_path),
                )
                .unwrap();
        }

        builder.finish().unwrap();

        let lab = load_from_tar(&tar_path).unwrap();
        assert_eq!(lab.content_hash, "chain-test-hash");
        assert_eq!(lab.runs.len(), 1);
        assert_eq!(lab.jobs.len(), 1);
    }

    #[test]
    fn test_load_from_tar_symlink_escape_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("escape.tar");
        build_test_lab_tar_with_options(
            &tar_path,
            TestLabOptions {
                add_escaping_symlink: Some("/etc/passwd".to_string()),
                ..Default::default()
            },
        );

        let result = load_from_tar(&tar_path);
        assert!(result.is_err());
        match result.unwrap_err() {
            CoreError::SymlinkEscape { link, target } => {
                assert!(link.to_string_lossy().contains("badlink"));
                assert_eq!(target, PathBuf::from("/etc/passwd"));
            }
            other => panic!("Expected SymlinkEscape, got: {:?}", other),
        }
    }

    #[test]
    fn test_load_from_tar_symlink_escape_relative() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("escape_rel.tar");
        build_test_lab_tar_with_options(
            &tar_path,
            TestLabOptions {
                add_escaping_symlink: Some("../../etc/passwd".to_string()),
                ..Default::default()
            },
        );

        let result = load_from_tar(&tar_path);
        assert!(result.is_err());
        match result.unwrap_err() {
            CoreError::SymlinkEscape { .. } => {}
            other => panic!("Expected SymlinkEscape, got: {:?}", other),
        }
    }

    #[test]
    fn test_load_from_tar_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("no_manifest.tar");

        let file = File::create(&tar_path).unwrap();
        let mut builder = tar::Builder::new(file);

        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, "result", &[][..]).unwrap();
        builder.finish().unwrap();

        let result = load_from_tar(&tar_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_tar_missing_jobs_dir() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("no_jobs.tar");

        let job_id = "abc123-test-job-1.0";
        let run_exe_path = format!("jobs/{}/run.sh", job_id);
        let run_metadata = serde_json::json!({
            "name": "test-run",
            "jobs": {
                job_id: {
                    "name": "test-job",
                    "params": {},
                    "executables": {"main": {"path": run_exe_path, "inputs": [], "outputs": {}}}
                }
            }
        });
        let run_meta_bytes = serde_json::to_vec_pretty(&run_metadata).unwrap();
        let run_meta_hash = sha256_hex(&run_meta_bytes);
        let run_meta_rel = format!("revision/{}-metadata-test-run.json", &run_meta_hash[..16]);

        let root_metadata = serde_json::json!({
            "repx_version": TEST_VERSION,
            "gitHash": "deadbeef",
            "runs": [run_meta_rel],
            "groups": {}
        });
        let root_meta_bytes = serde_json::to_vec_pretty(&root_metadata).unwrap();
        let root_meta_hash = sha256_hex(&root_meta_bytes);
        let root_meta_rel = format!("{}-metadata.json", &root_meta_hash[..16]);

        let run_exe_content = b"#!/bin/sh\necho hello\n";
        let file_entries = serde_json::json!([
            {"path": root_meta_rel, "sha256": sha256_hex(&root_meta_bytes)},
            {"path": run_meta_rel, "sha256": sha256_hex(&run_meta_bytes)},
            {"path": run_exe_path, "sha256": sha256_hex(run_exe_content)},
        ]);
        let manifest = serde_json::json!({
            "labId": "test-lab-content-hash",
            "lab_version": "1.0",
            "metadata": root_meta_rel,
            "files": file_entries,
        });
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        let manifest_hash = sha256_hex(&manifest_bytes);

        let file = File::create(&tar_path).unwrap();
        let mut builder = tar::Builder::new(file);
        let add_dir = |b: &mut tar::Builder<File>, p: &str| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Directory);
            h.set_size(0);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, p, &[][..]).unwrap();
        };
        let add_file = |b: &mut tar::Builder<File>, p: &str, c: &[u8]| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Regular);
            h.set_size(c.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, p, c).unwrap();
        };

        add_dir(&mut builder, "result");
        add_dir(&mut builder, "result/lab");
        add_dir(&mut builder, "result/revision");
        add_dir(&mut builder, "result/host-tools");
        add_dir(&mut builder, "result/host-tools/default");
        add_dir(&mut builder, "result/host-tools/default/bin");

        add_file(
            &mut builder,
            &format!("result/lab/{}-lab-metadata.json", &manifest_hash[..16]),
            &manifest_bytes,
        );
        add_file(
            &mut builder,
            &format!("result/{}", root_meta_rel),
            &root_meta_bytes,
        );
        add_file(
            &mut builder,
            &format!("result/{}", run_meta_rel),
            &run_meta_bytes,
        );
        builder.finish().unwrap();

        let result = load_from_tar(&tar_path);
        assert!(result.is_err(), "Expected error when jobs/ dir is missing");
    }

    #[test]
    fn test_load_dispatches_to_tar() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("lab.tar");
        build_test_lab_tar(&tar_path);

        let source = LabSource::Tar(tar_path);
        let lab = load(&source).unwrap();
        assert_eq!(lab.content_hash, "test-lab-content-hash");
    }

    #[test]
    fn test_list_tar_entries_revision() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("lab.tar");
        build_test_lab_tar(&tar_path);

        let entries = list_tar_entries(&tar_path, "revision/").unwrap();
        assert!(!entries.is_empty());
        assert!(entries.iter().all(|e| e.starts_with("revision/")));
        assert!(entries.iter().any(|e| e.contains("metadata-test-run.json")));
    }

    #[test]
    fn test_list_tar_entries_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("lab.tar");
        build_test_lab_tar(&tar_path);

        let entries = list_tar_entries(&tar_path, "jobs/").unwrap();
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|e| e.contains("abc123-test-job")));
    }

    #[test]
    fn test_list_tar_entries_nonexistent_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("lab.tar");
        build_test_lab_tar(&tar_path);

        let entries = list_tar_entries(&tar_path, "nonexistent/").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_tar_and_directory_produce_same_lab() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("lab.tar");
        build_test_lab_tar(&tar_path);

        let extract_dir = dir.path().join("extracted");
        fs::create_dir_all(&extract_dir).unwrap();
        let status = std::process::Command::new("tar")
            .arg("xf")
            .arg(&tar_path)
            .arg("--strip-components=1")
            .arg("-C")
            .arg(&extract_dir)
            .status()
            .unwrap();
        assert!(status.success());

        let lab_tar = load_from_tar(&tar_path).unwrap();
        let lab_dir = load_from_path(&extract_dir).unwrap();

        assert_eq!(lab_tar.content_hash, lab_dir.content_hash);
        assert_eq!(lab_tar.lab_version, lab_dir.lab_version);
        assert_eq!(lab_tar.repx_version, lab_dir.repx_version);
        assert_eq!(lab_tar.git_hash, lab_dir.git_hash);
        assert_eq!(lab_tar.host_tools_dir_name, lab_dir.host_tools_dir_name);
        assert_eq!(lab_tar.runs.len(), lab_dir.runs.len());
        assert_eq!(lab_tar.jobs.len(), lab_dir.jobs.len());

        for (run_id, run_tar) in &lab_tar.runs {
            let run_dir = lab_dir.runs.get(run_id).unwrap();
            assert_eq!(run_tar.image, run_dir.image);
            assert_eq!(run_tar.jobs.len(), run_dir.jobs.len());
        }

        for (job_id, job_tar) in &lab_tar.jobs {
            let job_dir = lab_dir.jobs.get(job_id).unwrap();
            assert_eq!(job_tar.name, job_dir.name);
            assert_eq!(job_tar.path_in_lab, job_dir.path_in_lab);
            assert_eq!(job_tar.params, job_dir.params);
        }
    }
}

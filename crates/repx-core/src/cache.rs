use crate::errors::CoreError;
use crate::fs_utils::path_to_string;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CacheKey {
    Rootfs {
        image_hash: String,
    },
    ImageStaging {
        image_hash: String,
    },
    OverlayCapability,

    HostTools {
        content_hash: String,
    },
    LabTar {
        content_hash: String,
    },
    RemoteLabTar {
        content_hash: String,
        target: String,
    },
    LabExtraction {
        content_hash: String,
    },

    ImageExtract {
        image_hash: String,
    },
    LayerExtract {
        layer_hash: String,
    },
    LayerFlatStore {
        layer_hash: String,
    },
    LayerDedup {
        layer_hash: String,
    },

    LocalBinary {
        binary_hash: String,
    },
    RemoteBinary {
        binary_hash: String,
        target: String,
    },
    RemoteRsync {
        binary_hash: String,
        target: String,
    },
    RemoteLayerDedup {
        layer_hash: String,
        target: String,
    },

    ImageFromTar {
        filename: String,
    },

    JobOutcome {
        job_id: String,
    },
    ScatterResult {
        orchestrator_id: String,
    },
    StepMarkers {
        branch_id: String,
        step_id: String,
    },
    SinkStepValidation {
        branch_id: String,
    },
}

impl CacheKey {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Rootfs { .. } => "rootfs",
            Self::ImageStaging { .. } => "image-staging",
            Self::OverlayCapability => "overlay-capability",
            Self::HostTools { .. } => "host-tools",
            Self::LabTar { .. } => "lab-tar",
            Self::RemoteLabTar { .. } => "remote-lab-tar",
            Self::LabExtraction { .. } => "lab-extraction",
            Self::ImageExtract { .. } => "image-extract",
            Self::LayerExtract { .. } => "layer-extract",
            Self::LayerFlatStore { .. } => "layer-flat-store",
            Self::LayerDedup { .. } => "layer-dedup",
            Self::LocalBinary { .. } => "local-binary",
            Self::RemoteBinary { .. } => "remote-binary",
            Self::RemoteRsync { .. } => "remote-rsync",
            Self::RemoteLayerDedup { .. } => "remote-layer-dedup",
            Self::ImageFromTar { .. } => "image-from-tar",
            Self::JobOutcome { .. } => "job-outcome",
            Self::ScatterResult { .. } => "scatter-result",
            Self::StepMarkers { .. } => "step-markers",
            Self::SinkStepValidation { .. } => "sink-step-validation",
        }
    }

    pub fn key_id(&self) -> String {
        match self {
            Self::Rootfs { image_hash } => image_hash.clone(),
            Self::ImageStaging { image_hash } => image_hash.clone(),
            Self::OverlayCapability => "(singleton)".to_string(),
            Self::HostTools { content_hash } => content_hash.clone(),
            Self::LabTar { content_hash } => content_hash.clone(),
            Self::RemoteLabTar {
                content_hash,
                target,
            } => format!("{content_hash}@{target}"),
            Self::LabExtraction { content_hash } => content_hash.clone(),
            Self::ImageExtract { image_hash } => image_hash.clone(),
            Self::LayerExtract { layer_hash } => layer_hash.clone(),
            Self::LayerFlatStore { layer_hash } => layer_hash.clone(),
            Self::LayerDedup { layer_hash } => layer_hash.clone(),
            Self::LocalBinary { binary_hash } => binary_hash.clone(),
            Self::RemoteBinary {
                binary_hash,
                target,
            } => format!("{binary_hash}@{target}"),
            Self::RemoteRsync {
                binary_hash,
                target,
            } => format!("{binary_hash}@{target}"),
            Self::RemoteLayerDedup { layer_hash, target } => format!("{layer_hash}@{target}"),
            Self::ImageFromTar { filename } => filename.clone(),
            Self::JobOutcome { job_id } => job_id.clone(),
            Self::ScatterResult { orchestrator_id } => orchestrator_id.clone(),
            Self::StepMarkers { branch_id, step_id } => format!("{branch_id}/{step_id}"),
            Self::SinkStepValidation { branch_id } => branch_id.clone(),
        }
    }
}

impl fmt::Display for CacheKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.type_name(), self.key_id())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetadata {
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    pub description: String,
    pub key_type: String,
    pub key_id: String,
}

impl CacheMetadata {
    pub fn new(key: &CacheKey, description: impl Into<String>) -> Self {
        Self {
            created_at: Utc::now(),
            content_hash: None,
            size_bytes: None,
            description: description.into(),
            key_type: key.type_name().to_string(),
            key_id: key.key_id(),
        }
    }

    pub fn with_content_hash(mut self, hash: impl Into<String>) -> Self {
        self.content_hash = Some(hash.into());
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size_bytes = Some(size);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheStatus {
    Hit(PathBuf),
    Miss,
    Stale(PathBuf),
}

impl CacheStatus {
    pub fn is_hit(&self) -> bool {
        matches!(self, Self::Hit(_))
    }

    pub fn hit_path(&self) -> Option<&Path> {
        match self {
            Self::Hit(p) => Some(p),
            _ => None,
        }
    }
}

pub trait CacheStore {
    fn status(&self, key: &CacheKey) -> Result<CacheStatus, CoreError>;

    fn path(&self, key: &CacheKey) -> PathBuf;

    fn mark_ready(&self, key: &CacheKey, metadata: CacheMetadata) -> Result<(), CoreError>;

    fn invalidate(&self, key: &CacheKey) -> Result<(), CoreError>;

    fn remove(&self, key: &CacheKey) -> Result<(), CoreError>;

    fn list(&self) -> Result<Vec<(CacheKey, CacheMetadata)>, CoreError>;

    fn clear(&self) -> Result<u64, CoreError>;

    fn disk_usage(&self) -> Result<u64, CoreError>;
}

pub struct FsCache {
    root: PathBuf,
}

impl FsCache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn artifact_path(&self, key: &CacheKey) -> PathBuf {
        match key {
            CacheKey::Rootfs { image_hash } => {
                self.root.join("images").join(image_hash).join("rootfs")
            }

            CacheKey::ImageStaging { image_hash } => {
                self.root.join("images").join(image_hash).join("image")
            }

            CacheKey::OverlayCapability => {
                self.root.join("capabilities").join("overlay_support.json")
            }

            CacheKey::HostTools { content_hash } => self
                .root
                .join("temp")
                .join(format!("host-tools-{content_hash}")),

            CacheKey::LabTar { content_hash } => {
                self.root.join("temp").join(format!("{content_hash}.tar"))
            }

            CacheKey::RemoteLabTar { content_hash, .. } => self
                .root
                .join("lab-tars")
                .join(format!("{content_hash}.tar")),

            CacheKey::LabExtraction { content_hash } => self.root.join("labs").join(content_hash),

            CacheKey::ImageExtract { image_hash } => self.root.join("images").join(image_hash),

            CacheKey::LayerExtract { layer_hash } => self.root.join("layers").join(layer_hash),

            CacheKey::LayerFlatStore { layer_hash } => self
                .root
                .join("store")
                .join(format!("{layer_hash}-layer.tar")),

            CacheKey::LayerDedup { layer_hash } => self.root.join("layers").join(layer_hash),

            CacheKey::LocalBinary { binary_hash } => {
                self.root.join("bin").join(binary_hash).join("repx")
            }

            CacheKey::RemoteBinary { binary_hash, .. } => {
                self.root.join("bin").join(binary_hash).join("repx")
            }

            CacheKey::RemoteRsync { binary_hash, .. } => {
                self.root.join("bin").join(binary_hash).join("rsync")
            }

            CacheKey::RemoteLayerDedup { layer_hash, .. } => self
                .root
                .join("store")
                .join(format!("{layer_hash}-layer.tar")),

            CacheKey::ImageFromTar { filename } => self.root.join("lab-images").join(filename),

            CacheKey::JobOutcome { job_id } => self.root.join("outputs").join(job_id).join("repx"),

            CacheKey::ScatterResult { orchestrator_id } => {
                self.root.join("scatter").join(orchestrator_id)
            }

            CacheKey::StepMarkers { branch_id, step_id } => self
                .root
                .join("branch")
                .join(branch_id)
                .join(format!("step-{step_id}"))
                .join("repx"),

            CacheKey::SinkStepValidation { branch_id } => self.root.join("branch").join(branch_id),
        }
    }

    fn metadata_path(&self, key: &CacheKey) -> PathBuf {
        let base = self.artifact_path(key);
        if base.is_dir() || self.is_directory_entry(key) {
            base.parent()
                .unwrap_or(&base)
                .join(format!(".{}.repx-cache.json", dir_name_or_fallback(&base)))
        } else {
            let name = base
                .file_name()
                .map(path_to_string)
                .unwrap_or_else(|| "unknown".to_string());
            base.with_file_name(format!(".{name}.repx-cache.json"))
        }
    }

    fn is_directory_entry(&self, key: &CacheKey) -> bool {
        matches!(
            key,
            CacheKey::Rootfs { .. }
                | CacheKey::ImageStaging { .. }
                | CacheKey::HostTools { .. }
                | CacheKey::LabExtraction { .. }
                | CacheKey::ImageExtract { .. }
                | CacheKey::LayerExtract { .. }
                | CacheKey::LayerDedup { .. }
                | CacheKey::JobOutcome { .. }
                | CacheKey::ScatterResult { .. }
                | CacheKey::StepMarkers { .. }
                | CacheKey::SinkStepValidation { .. }
        )
    }

    fn read_metadata(&self, key: &CacheKey) -> Option<CacheMetadata> {
        let meta_path = self.metadata_path(key);
        let content = fs::read_to_string(&meta_path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn scan_metadata_files(&self) -> Result<Vec<PathBuf>, CoreError> {
        let mut results = Vec::new();
        self.scan_dir_for_metadata(&self.root, &mut results)?;
        Ok(results)
    }

    fn scan_dir_for_metadata(
        &self,
        dir: &Path,
        results: &mut Vec<PathBuf>,
    ) -> Result<(), CoreError> {
        Self::scan_dir_recursive(dir, results)
    }

    fn scan_dir_recursive(dir: &Path, results: &mut Vec<PathBuf>) -> Result<(), CoreError> {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(CoreError::path_io(dir, e)),
        };
        for entry in entries {
            let entry = entry.map_err(CoreError::Io)?;
            let path = entry.path();
            if path.is_dir() {
                Self::scan_dir_recursive(&path, results)?;
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".repx-cache.json") && name.starts_with('.') {
                    results.push(path);
                }
            }
        }
        Ok(())
    }

    fn path_size(path: &Path) -> u64 {
        if path.is_file() {
            fs::metadata(path).map(|m| m.len()).unwrap_or(0)
        } else if path.is_dir() {
            walkdir::WalkDir::new(path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
                .sum()
        } else {
            0
        }
    }
}

fn dir_name_or_fallback(path: &Path) -> String {
    path.file_name()
        .map(path_to_string)
        .unwrap_or_else(|| "unknown".to_string())
}

impl CacheStore for FsCache {
    fn status(&self, key: &CacheKey) -> Result<CacheStatus, CoreError> {
        let artifact = self.artifact_path(key);
        if !artifact.exists() {
            return Ok(CacheStatus::Miss);
        }

        if let Some(metadata) = self.read_metadata(key) {
            if let Some(ref stored_hash) = metadata.content_hash {
                let expected = match key {
                    CacheKey::HostTools { content_hash }
                    | CacheKey::LabTar { content_hash }
                    | CacheKey::RemoteLabTar { content_hash, .. }
                    | CacheKey::LabExtraction { content_hash } => Some(content_hash.as_str()),
                    _ => None,
                };
                if let Some(expected) = expected {
                    if stored_hash != expected {
                        return Ok(CacheStatus::Stale(artifact));
                    }
                }
            }
            return Ok(CacheStatus::Hit(artifact));
        }

        Ok(CacheStatus::Miss)
    }

    fn path(&self, key: &CacheKey) -> PathBuf {
        self.artifact_path(key)
    }

    fn mark_ready(&self, key: &CacheKey, metadata: CacheMetadata) -> Result<(), CoreError> {
        let meta_path = self.metadata_path(key);
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent).map_err(|e| CoreError::path_io(parent, e))?;
        }
        let json = serde_json::to_string_pretty(&metadata)?;
        crate::fs_utils::write_atomic_nosync(&meta_path, json.as_bytes())?;
        Ok(())
    }

    fn invalidate(&self, key: &CacheKey) -> Result<(), CoreError> {
        let meta_path = self.metadata_path(key);
        match fs::remove_file(&meta_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CoreError::path_io(&meta_path, e)),
        }
    }

    fn remove(&self, key: &CacheKey) -> Result<(), CoreError> {
        self.invalidate(key)?;

        let artifact = self.artifact_path(key);
        if !artifact.exists() {
            return Ok(());
        }

        if artifact.is_dir() {
            crate::fs_utils::force_remove_dir(&artifact)
                .map_err(|e| CoreError::path_io(&artifact, e))?;
        } else {
            fs::remove_file(&artifact).map_err(|e| CoreError::path_io(&artifact, e))?;
        }
        Ok(())
    }

    fn list(&self) -> Result<Vec<(CacheKey, CacheMetadata)>, CoreError> {
        let meta_files = self.scan_metadata_files()?;
        let mut entries = Vec::new();

        for meta_path in meta_files {
            let content = match fs::read_to_string(&meta_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let metadata: CacheMetadata = match serde_json::from_str(&content) {
                Ok(m) => m,
                Err(_) => continue,
            };

            if let Some(key) = cache_key_from_metadata(&metadata) {
                entries.push((key, metadata));
            }
        }

        Ok(entries)
    }

    fn clear(&self) -> Result<u64, CoreError> {
        let entries = self.list()?;
        let count = entries.len() as u64;
        for (key, _) in &entries {
            let _ = self.remove(key);
        }
        Ok(count)
    }

    fn disk_usage(&self) -> Result<u64, CoreError> {
        let entries = self.list()?;
        let mut total = 0u64;
        for (key, _) in &entries {
            total += Self::path_size(&self.artifact_path(key));
        }
        Ok(total)
    }
}

fn cache_key_from_metadata(meta: &CacheMetadata) -> Option<CacheKey> {
    let id = &meta.key_id;
    match meta.key_type.as_str() {
        "rootfs" => Some(CacheKey::Rootfs {
            image_hash: id.clone(),
        }),
        "image-staging" => Some(CacheKey::ImageStaging {
            image_hash: id.clone(),
        }),
        "overlay-capability" => Some(CacheKey::OverlayCapability),
        "host-tools" => Some(CacheKey::HostTools {
            content_hash: id.clone(),
        }),
        "lab-tar" => Some(CacheKey::LabTar {
            content_hash: id.clone(),
        }),
        "remote-lab-tar" => {
            let (hash, target) = split_at_sign(id)?;
            Some(CacheKey::RemoteLabTar {
                content_hash: hash,
                target,
            })
        }
        "lab-extraction" => Some(CacheKey::LabExtraction {
            content_hash: id.clone(),
        }),
        "image-extract" => Some(CacheKey::ImageExtract {
            image_hash: id.clone(),
        }),
        "layer-extract" => Some(CacheKey::LayerExtract {
            layer_hash: id.clone(),
        }),
        "layer-flat-store" => Some(CacheKey::LayerFlatStore {
            layer_hash: id.clone(),
        }),
        "layer-dedup" => Some(CacheKey::LayerDedup {
            layer_hash: id.clone(),
        }),
        "local-binary" => Some(CacheKey::LocalBinary {
            binary_hash: id.clone(),
        }),
        "remote-binary" => {
            let (hash, target) = split_at_sign(id)?;
            Some(CacheKey::RemoteBinary {
                binary_hash: hash,
                target,
            })
        }
        "remote-rsync" => {
            let (hash, target) = split_at_sign(id)?;
            Some(CacheKey::RemoteRsync {
                binary_hash: hash,
                target,
            })
        }
        "remote-layer-dedup" => {
            let (hash, target) = split_at_sign(id)?;
            Some(CacheKey::RemoteLayerDedup {
                layer_hash: hash,
                target,
            })
        }
        "image-from-tar" => Some(CacheKey::ImageFromTar {
            filename: id.clone(),
        }),
        "job-outcome" => Some(CacheKey::JobOutcome { job_id: id.clone() }),
        "scatter-result" => Some(CacheKey::ScatterResult {
            orchestrator_id: id.clone(),
        }),
        "step-markers" => {
            let (branch, step) = id.split_once('/')?;
            Some(CacheKey::StepMarkers {
                branch_id: branch.to_string(),
                step_id: step.to_string(),
            })
        }
        "sink-step-validation" => Some(CacheKey::SinkStepValidation {
            branch_id: id.clone(),
        }),
        _ => None,
    }
}

fn split_at_sign(s: &str) -> Option<(String, String)> {
    let pos = s.rfind('@')?;
    Some((s[..pos].to_string(), s[pos + 1..].to_string()))
}

pub static KNOWN_CACHE_TYPES: &[&str] = &[
    "rootfs",
    "image-staging",
    "overlay-capability",
    "host-tools",
    "lab-tar",
    "remote-lab-tar",
    "lab-extraction",
    "image-extract",
    "layer-extract",
    "layer-flat-store",
    "layer-dedup",
    "local-binary",
    "remote-binary",
    "remote-rsync",
    "remote-layer-dedup",
    "image-from-tar",
    "job-outcome",
    "scatter-result",
    "step-markers",
    "sink-step-validation",
];

pub trait CacheStoreExt: CacheStore {
    fn get_hit(&self, key: &CacheKey) -> Result<Option<PathBuf>, CoreError> {
        match self.status(key)? {
            CacheStatus::Hit(p) => Ok(Some(p)),
            _ => Ok(None),
        }
    }

    fn is_cached(&self, key: &CacheKey) -> Result<bool, CoreError> {
        Ok(self.status(key)?.is_hit())
    }

    fn filter_missing(&self, keys: &[CacheKey]) -> Result<Vec<CacheKey>, CoreError> {
        let mut missing = Vec::new();
        for key in keys {
            if !self.is_cached(key)? {
                missing.push(key.clone());
            }
        }
        Ok(missing)
    }

    fn ensure_fresh(&self, key: &CacheKey) -> Result<bool, CoreError> {
        match self.status(key)? {
            CacheStatus::Hit(_) => Ok(true),
            CacheStatus::Stale(_) => {
                self.remove(key)?;
                Ok(false)
            }
            CacheStatus::Miss => Ok(false),
        }
    }
}

impl<T: CacheStore> CacheStoreExt for T {}

#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub total_entries: u64,
    pub total_size_bytes: u64,
    pub entries_by_type: BTreeMap<String, u64>,
    pub oldest: Option<DateTime<Utc>>,
    pub newest: Option<DateTime<Utc>>,
}

impl CacheStats {
    pub fn from_entries(entries: &[(CacheKey, CacheMetadata)]) -> Self {
        let mut stats = Self {
            total_entries: entries.len() as u64,
            ..Self::default()
        };

        for (key, meta) in entries {
            *stats
                .entries_by_type
                .entry(key.type_name().to_string())
                .or_insert(0) += 1;

            if let Some(size) = meta.size_bytes {
                stats.total_size_bytes += size;
            }

            match stats.oldest {
                None => stats.oldest = Some(meta.created_at),
                Some(old) if meta.created_at < old => stats.oldest = Some(meta.created_at),
                _ => {}
            }
            match stats.newest {
                None => stats.newest = Some(meta.created_at),
                Some(new) if meta.created_at > new => stats.newest = Some(meta.created_at),
                _ => {}
            }
        }

        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_cache() -> (tempfile::TempDir, FsCache) {
        let dir = tempdir().expect("failed to create tempdir");
        let cache = FsCache::new(dir.path().to_path_buf());
        (dir, cache)
    }

    #[test]
    fn test_key_display() {
        let key = CacheKey::Rootfs {
            image_hash: "abc123".to_string(),
        };
        assert_eq!(key.to_string(), "rootfs:abc123");

        let key = CacheKey::RemoteBinary {
            binary_hash: "def".to_string(),
            target: "safari".to_string(),
        };
        assert_eq!(key.to_string(), "remote-binary:def@safari");
    }

    #[test]
    fn test_status_miss_when_empty() {
        let (_dir, cache) = make_cache();
        let key = CacheKey::LabTar {
            content_hash: "abc".to_string(),
        };
        let status = cache.status(&key).expect("status failed");
        assert_eq!(status, CacheStatus::Miss);
    }

    #[test]
    fn test_status_miss_for_untracked_artifact() {
        let (_dir, cache) = make_cache();
        let key = CacheKey::LabTar {
            content_hash: "abc".to_string(),
        };
        let path = cache.path(&key);
        fs::create_dir_all(path.parent().expect("no parent")).expect("mkdir failed");
        fs::write(&path, b"fake tar").expect("write failed");

        let status = cache.status(&key).expect("status failed");
        assert_eq!(status, CacheStatus::Miss);
    }

    #[test]
    fn test_mark_ready_and_status() {
        let (_dir, cache) = make_cache();
        let key = CacheKey::LabTar {
            content_hash: "abc".to_string(),
        };
        let path = cache.path(&key);
        fs::create_dir_all(path.parent().expect("no parent")).expect("mkdir failed");
        fs::write(&path, b"fake tar").expect("write failed");

        let meta = CacheMetadata::new(&key, "test lab tar").with_content_hash("abc");
        cache
            .mark_ready(&key, meta.clone())
            .expect("mark_ready failed");

        let status = cache.status(&key).expect("status failed");
        assert_eq!(status, CacheStatus::Hit(path.clone()));

        let read_meta = cache.read_metadata(&key).expect("no metadata");
        assert_eq!(read_meta.content_hash.as_deref(), Some("abc"));
        assert_eq!(read_meta.description, "test lab tar");
    }

    #[test]
    fn test_stale_on_hash_mismatch() {
        let (_dir, cache) = make_cache();
        let key = CacheKey::LabExtraction {
            content_hash: "new_hash".to_string(),
        };
        let path = cache.path(&key);
        fs::create_dir_all(&path).expect("mkdir failed");

        let meta = CacheMetadata::new(&key, "lab extraction").with_content_hash("old_hash");
        let meta_path = cache.metadata_path(&key);
        fs::create_dir_all(meta_path.parent().expect("no parent")).expect("mkdir failed");
        let json = serde_json::to_string_pretty(&meta).expect("json failed");
        fs::write(&meta_path, json).expect("write failed");

        let status = cache.status(&key).expect("status failed");
        assert_eq!(status, CacheStatus::Stale(path));
    }

    #[test]
    fn test_host_tools_isolated_by_content_hash() {
        let (_dir, cache) = make_cache();
        let key_a = CacheKey::HostTools {
            content_hash: "hash_a".to_string(),
        };
        let key_b = CacheKey::HostTools {
            content_hash: "hash_b".to_string(),
        };

        assert_ne!(cache.path(&key_a), cache.path(&key_b));

        let path_a = cache.path(&key_a);
        fs::create_dir_all(&path_a).expect("mkdir failed");
        let meta_a = CacheMetadata::new(&key_a, "host tools A").with_content_hash("hash_a");
        cache.mark_ready(&key_a, meta_a).expect("mark_ready failed");

        assert!(cache.status(&key_a).expect("status failed").is_hit());
        assert_eq!(
            cache.status(&key_b).expect("status failed"),
            CacheStatus::Miss
        );
    }

    #[test]
    fn test_ensure_fresh_removes_stale() {
        let (_dir, cache) = make_cache();
        let key = CacheKey::LabExtraction {
            content_hash: "new_hash".to_string(),
        };
        let path = cache.path(&key);
        fs::create_dir_all(&path).expect("mkdir failed");
        fs::write(path.join("stale_file.txt"), b"old data").expect("write failed");

        let meta = CacheMetadata::new(&key, "stale").with_content_hash("old_hash");
        cache.mark_ready(&key, meta).expect("mark_ready failed");

        let fresh = cache.ensure_fresh(&key).expect("ensure_fresh failed");
        assert!(!fresh);
        assert!(!path.exists(), "stale artifact should be removed");
        assert!(
            !cache.metadata_path(&key).exists(),
            "stale sidecar should be removed"
        );

        assert_eq!(
            cache.status(&key).expect("status failed"),
            CacheStatus::Miss
        );
    }

    #[test]
    fn test_no_sidecar_means_miss_not_hit() {
        let (_dir, cache) = make_cache();
        let key = CacheKey::HostTools {
            content_hash: "abc".to_string(),
        };
        let path = cache.path(&key);
        fs::create_dir_all(&path).expect("mkdir failed");

        assert_eq!(
            cache.status(&key).expect("status failed"),
            CacheStatus::Miss
        );

        assert!(!cache.ensure_fresh(&key).expect("ensure_fresh failed"));
    }

    #[test]
    fn test_invalidate() {
        let (_dir, cache) = make_cache();
        let key = CacheKey::LabTar {
            content_hash: "x".to_string(),
        };
        let path = cache.path(&key);
        fs::create_dir_all(path.parent().expect("no parent")).expect("mkdir failed");
        fs::write(&path, b"data").expect("write failed");

        let meta = CacheMetadata::new(&key, "test");
        cache.mark_ready(&key, meta).expect("mark_ready failed");
        assert!(cache.metadata_path(&key).exists());

        cache.invalidate(&key).expect("invalidate failed");
        assert!(!cache.metadata_path(&key).exists());
        assert!(path.exists());
    }

    #[test]
    fn test_remove() {
        let (_dir, cache) = make_cache();
        let key = CacheKey::LabTar {
            content_hash: "x".to_string(),
        };
        let path = cache.path(&key);
        fs::create_dir_all(path.parent().expect("no parent")).expect("mkdir failed");
        fs::write(&path, b"data").expect("write failed");

        let meta = CacheMetadata::new(&key, "test");
        cache.mark_ready(&key, meta).expect("mark_ready failed");

        cache.remove(&key).expect("remove failed");
        assert!(!cache.metadata_path(&key).exists());
        assert!(!path.exists());
    }

    #[test]
    fn test_list_and_clear() {
        let (_dir, cache) = make_cache();

        for hash in &["a", "b"] {
            let key = CacheKey::LabTar {
                content_hash: hash.to_string(),
            };
            let path = cache.path(&key);
            fs::create_dir_all(path.parent().expect("no parent")).expect("mkdir failed");
            fs::write(&path, b"data").expect("write failed");
            let meta = CacheMetadata::new(&key, "test").with_content_hash(*hash);
            cache.mark_ready(&key, meta).expect("mark_ready failed");
        }

        let entries = cache.list().expect("list failed");
        assert_eq!(entries.len(), 2);

        let cleared = cache.clear().expect("clear failed");
        assert_eq!(cleared, 2);

        let entries = cache.list().expect("list after clear failed");
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_directory_entry_remove() {
        let (_dir, cache) = make_cache();
        let key = CacheKey::Rootfs {
            image_hash: "img123".to_string(),
        };
        let path = cache.path(&key);
        fs::create_dir_all(&path).expect("mkdir failed");
        fs::write(path.join("file.txt"), b"content").expect("write failed");

        let meta = CacheMetadata::new(&key, "rootfs");
        cache.mark_ready(&key, meta).expect("mark_ready failed");

        cache.remove(&key).expect("remove failed");
        assert!(!path.exists());
    }

    #[test]
    fn test_directory_entry_remove_readonly() {
        use std::os::unix::fs::PermissionsExt;

        let (_dir, cache) = make_cache();
        let key = CacheKey::LabExtraction {
            content_hash: "readonly_hash".to_string(),
        };
        let path = cache.path(&key);
        let nested = path.join("store").join("pkg-abc");
        fs::create_dir_all(&nested).expect("mkdir failed");

        fs::write(nested.join("binary"), b"ELF fake").expect("write failed");
        fs::write(nested.join("lib.so"), b"shared obj").expect("write failed");
        fs::set_permissions(nested.join("binary"), fs::Permissions::from_mode(0o444))
            .expect("chmod file");
        fs::set_permissions(nested.join("lib.so"), fs::Permissions::from_mode(0o444))
            .expect("chmod file");
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o555)).expect("chmod dir");
        fs::set_permissions(path.join("store"), fs::Permissions::from_mode(0o555))
            .expect("chmod dir");

        let meta = CacheMetadata::new(&key, "lab extraction with readonly nix files");
        cache.mark_ready(&key, meta).expect("mark_ready failed");

        if std::env::var("USER").as_deref() != Ok("root") {
            assert!(fs::remove_dir_all(&nested).is_err());
        }

        cache
            .remove(&key)
            .expect("remove of readonly dir should succeed");
        assert!(!path.exists());
    }

    #[test]
    fn test_ensure_fresh_removes_stale_readonly() {
        use std::os::unix::fs::PermissionsExt;

        let (_dir, cache) = make_cache();
        let key = CacheKey::LabExtraction {
            content_hash: "new_hash_ro".to_string(),
        };
        let path = cache.path(&key);
        let nested = path.join("store").join("nix-pkg");
        fs::create_dir_all(&nested).expect("mkdir failed");

        fs::write(nested.join("bin"), b"stale binary").expect("write failed");
        fs::set_permissions(nested.join("bin"), fs::Permissions::from_mode(0o444))
            .expect("chmod file");
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o555)).expect("chmod dir");
        fs::set_permissions(path.join("store"), fs::Permissions::from_mode(0o555))
            .expect("chmod dir");

        let meta = CacheMetadata::new(&key, "stale extraction").with_content_hash("old_hash_ro");
        cache.mark_ready(&key, meta).expect("mark_ready failed");

        let fresh = cache
            .ensure_fresh(&key)
            .expect("ensure_fresh should not fail");
        assert!(!fresh, "should return false for stale entry");
        assert!(
            !path.exists(),
            "stale read-only directory should be removed"
        );
        assert!(
            !cache.metadata_path(&key).exists(),
            "stale sidecar should be removed"
        );
    }

    #[test]
    fn test_cache_key_roundtrip_via_metadata() {
        let keys = vec![
            CacheKey::Rootfs {
                image_hash: "abc".to_string(),
            },
            CacheKey::OverlayCapability,
            CacheKey::RemoteBinary {
                binary_hash: "def".to_string(),
                target: "safari".to_string(),
            },
            CacheKey::StepMarkers {
                branch_id: "b1".to_string(),
                step_id: "s2".to_string(),
            },
        ];

        for key in &keys {
            let meta = CacheMetadata::new(key, "test");
            let reconstructed = cache_key_from_metadata(&meta).expect("failed to reconstruct key");
            assert_eq!(&reconstructed, key, "roundtrip failed for {key}");
        }
    }

    #[test]
    fn test_cache_stats() {
        let entries = vec![
            (
                CacheKey::Rootfs {
                    image_hash: "a".to_string(),
                },
                CacheMetadata::new(
                    &CacheKey::Rootfs {
                        image_hash: "a".to_string(),
                    },
                    "rootfs a",
                )
                .with_size(1000),
            ),
            (
                CacheKey::LabTar {
                    content_hash: "b".to_string(),
                },
                CacheMetadata::new(
                    &CacheKey::LabTar {
                        content_hash: "b".to_string(),
                    },
                    "lab tar b",
                )
                .with_size(500),
            ),
        ];

        let stats = CacheStats::from_entries(&entries);
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.total_size_bytes, 1500);
        assert_eq!(stats.entries_by_type.get("rootfs"), Some(&1));
        assert_eq!(stats.entries_by_type.get("lab-tar"), Some(&1));
    }

    #[test]
    fn test_cache_store_ext_filter_missing() {
        let (_dir, cache) = make_cache();

        let key_exists = CacheKey::LabTar {
            content_hash: "exists".to_string(),
        };
        let key_missing = CacheKey::LabTar {
            content_hash: "missing".to_string(),
        };

        let path = cache.path(&key_exists);
        fs::create_dir_all(path.parent().expect("no parent")).expect("mkdir failed");
        fs::write(&path, b"data").expect("write failed");
        let meta = CacheMetadata::new(&key_exists, "test").with_content_hash("exists");
        cache
            .mark_ready(&key_exists, meta)
            .expect("mark_ready failed");

        let missing = cache
            .filter_missing(&[key_exists.clone(), key_missing.clone()])
            .expect("filter_missing failed");
        assert_eq!(missing, vec![key_missing]);
    }
}

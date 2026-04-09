use crate::blueprint::{BinSpec, HostToolSpec};
use crate::expand::{ExpandedJob, ExpandedLab};
use crate::metadata;
use crate::util::{sha256_file, write_hashed};
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const JOB_BATCH_SIZE: usize = 4096;

#[derive(serde::Serialize)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
}

pub struct AssemblyStats {
    pub total_jobs: u64,
    pub unique_jobs: u64,
    pub script_copies: u64,
}

pub fn assemble_lab(
    lab: &ExpandedLab,
    output_dir: &Path,
    lab_version: &str,
) -> Result<AssemblyStats> {
    let stats_total = AtomicU64::new(0);
    let stats_unique = AtomicU64::new(0);
    let stats_scripts = AtomicU64::new(0);

    let file_entries: Mutex<Vec<FileEntry>> = Mutex::new(Vec::new());

    let jobs_dir = output_dir.join("jobs");
    let store_dir = output_dir.join("store");
    let revision_dir = output_dir.join("revision");
    let host_tools_dir = output_dir.join("host-tools");
    let lab_dir = output_dir.join("lab");

    for d in [
        &jobs_dir,
        &store_dir,
        &revision_dir,
        &host_tools_dir,
        &lab_dir,
    ] {
        fs::create_dir_all(d).with_context(|| format!("creating {}", d.display()))?;
    }

    let host_tool_entries = assemble_host_tools(
        &lab.blueprint.host_tools.binaries,
        &lab.blueprint.host_tools.hash,
        output_dir,
    )
    .context("assembling host tools")?;
    file_entries.lock().unwrap().extend(host_tool_entries);

    let dedup_set = dashmap_lite::DedupSet::new();
    let assembly_errors: Mutex<Vec<String>> = Mutex::new(Vec::new());

    for run in &lab.runs {
        let batch_entries: Vec<Vec<FileEntry>> = run
            .jobs
            .par_chunks(JOB_BATCH_SIZE)
            .map(|batch| {
                let mut local_entries: Vec<FileEntry> = Vec::new();

                for job in batch {
                    stats_total.fetch_add(1, Ordering::Relaxed);

                    if !dedup_set.insert(&job.job_dir_name) {
                        continue;
                    }
                    stats_unique.fetch_add(1, Ordering::Relaxed);

                    match assemble_job(job, &jobs_dir, output_dir) {
                        Ok(entries) => {
                            stats_scripts
                                .fetch_add(job.script_sources.len() as u64, Ordering::Relaxed);
                            local_entries.extend(entries);
                        }
                        Err(e) => {
                            let msg = format!("job {}: {e}", job.job_dir_name);
                            eprintln!("ERROR assembling {msg}");
                            assembly_errors.lock().unwrap().push(msg);
                        }
                    }
                }

                local_entries
            })
            .collect();

        let mut all = file_entries.lock().unwrap();
        for batch in batch_entries {
            all.extend(batch);
        }
    }

    let errors = assembly_errors.into_inner().unwrap();
    if !errors.is_empty() {
        let summary = if errors.len() <= 5 {
            errors.join("; ")
        } else {
            format!(
                "{}; ... and {} more",
                errors[..5].join("; "),
                errors.len() - 5
            )
        };
        anyhow::bail!("{} job(s) failed to assemble: {}", errors.len(), summary);
    }

    let meta_entries = metadata::write_all_metadata(lab, output_dir).context("writing metadata")?;
    file_entries.lock().unwrap().extend(meta_entries);

    let mut entries = file_entries.into_inner().unwrap();

    let pre_existing = collect_pre_existing_files(output_dir, &entries)?;
    entries.extend(pre_existing);

    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let lab_id = output_dir
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.split('-').next().unwrap_or(s))
        .unwrap_or("unknown")
        .to_string();

    let manifest = serde_json::json!({
        "labId": lab_id,
        "lab_version": lab_version,
        "metadata": "revision/metadata-top.json",
        "files": entries,
    });

    let manifest_json = serde_json::to_string(&manifest)?;
    let manifest_path = lab_dir.join("lab-metadata.json");
    write_and_hash(
        &manifest_path,
        manifest_json.as_bytes(),
        output_dir,
        &mut entries,
    )?;

    Ok(AssemblyStats {
        total_jobs: stats_total.load(Ordering::Relaxed),
        unique_jobs: stats_unique.load(Ordering::Relaxed),
        script_copies: stats_scripts.load(Ordering::Relaxed),
    })
}

fn assemble_job(job: &ExpandedJob, jobs_dir: &Path, output_dir: &Path) -> Result<Vec<FileEntry>> {
    let job_dir = jobs_dir.join(&job.job_dir_name);
    let bin_dir = job_dir.join("bin");
    fs::create_dir_all(&bin_dir)?;

    let mut entries: Vec<FileEntry> = Vec::with_capacity(job.script_sources.len() + 2);

    for src in &job.script_sources {
        let source_path = PathBuf::from(&src.drv_path).join("bin").join(&src.bin_name);
        let dest_path = bin_dir.join(&src.dest_name);

        if fs::hard_link(&source_path, &dest_path).is_err() {
            fs::copy(&source_path, &dest_path)?;
        }

        let mut perms = fs::metadata(&dest_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest_path, perms)?;

        let hash = sha256_file(&dest_path)?;
        let rel = rel_path(&dest_path, output_dir);
        entries.push(FileEntry {
            path: rel,
            sha256: hash,
        });
    }

    let params_path = job_dir.join(format!("{}-parameters.json", job.pname));
    let hash = write_hashed(&params_path, job.parameters_json.as_bytes())?;
    entries.push(FileEntry {
        path: rel_path(&params_path, output_dir),
        sha256: hash,
    });

    let deps_path = job_dir.join("nix-input-dependencies.json");
    let hash = write_hashed(&deps_path, job.dependency_manifest_json.as_bytes())?;
    entries.push(FileEntry {
        path: rel_path(&deps_path, output_dir),
        sha256: hash,
    });

    Ok(entries)
}

fn write_and_hash(
    path: &Path,
    data: &[u8],
    output_dir: &Path,
    entries: &mut Vec<FileEntry>,
) -> Result<()> {
    let hash = write_hashed(path, data)?;
    entries.push(FileEntry {
        path: rel_path(path, output_dir),
        sha256: hash,
    });
    Ok(())
}

fn rel_path(path: &Path, output_dir: &Path) -> String {
    path.strip_prefix(output_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn collect_pre_existing_files(output_dir: &Path, known: &[FileEntry]) -> Result<Vec<FileEntry>> {
    let known_set: HashSet<&str> = known.iter().map(|e| e.path.as_str()).collect();
    let mut entries = Vec::new();

    fn walk(
        dir: &Path,
        output_dir: &Path,
        known: &HashSet<&str>,
        entries: &mut Vec<FileEntry>,
    ) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(&path, output_dir, known, entries)?;
            } else if path.is_file() {
                let rel = rel_path(&path, output_dir);
                if !known.contains(rel.as_str()) {
                    let hash = sha256_file(&path)?;
                    entries.push(FileEntry {
                        path: rel,
                        sha256: hash,
                    });
                }
            }
        }
        Ok(())
    }

    walk(output_dir, output_dir, &known_set, &mut entries)?;
    Ok(entries)
}

fn assemble_host_tools(
    tools: &[HostToolSpec],
    tools_hash: &str,
    output_dir: &Path,
) -> Result<Vec<FileEntry>> {
    let store_dir = output_dir.join("store");
    let bin_dir = output_dir.join("host-tools").join(tools_hash).join("bin");
    fs::create_dir_all(&bin_dir)?;

    let mut entries = Vec::new();

    for tool in tools {
        match &tool.bins {
            None => {
                let pkg_bin = PathBuf::from(&tool.pkg_path).join("bin");
                if !pkg_bin.exists() {
                    anyhow::bail!(
                        "Host tool pkg_path not found: {} (looking for {}/bin)",
                        tool.pkg_path,
                        tool.pkg_path
                    );
                }
                if pkg_bin.is_dir() {
                    for entry in fs::read_dir(&pkg_bin)? {
                        let entry = entry?;
                        let bin_name = entry.file_name();
                        let bin_name_str = bin_name.to_string_lossy();
                        let store_name = format!("{}-{}", tool.pkg_hash, bin_name_str);
                        let store_path = store_dir.join(&store_name);

                        if !store_path.exists() {
                            fs::copy(entry.path(), &store_path)?;
                            let hash = sha256_file(&store_path)?;
                            entries.push(FileEntry {
                                path: rel_path(&store_path, output_dir),
                                sha256: hash,
                            });
                        }

                        let link_path = bin_dir.join(&*bin_name_str);
                        let rel_target = format!("../../../store/{store_name}");
                        let _ = fs::remove_file(&link_path);
                        std::os::unix::fs::symlink(&rel_target, &link_path)?;
                    }
                }
            }
            Some(bins) => {
                for bin_spec in bins {
                    let (src_name, dst_name) = match bin_spec {
                        BinSpec::Simple(name) => (name.as_str(), name.as_str()),
                        BinSpec::Renamed { src, dst } => (src.as_str(), dst.as_str()),
                    };

                    let store_name = format!("{}-{src_name}", tool.pkg_hash);
                    let store_path = store_dir.join(&store_name);

                    if !store_path.exists() {
                        let src_path = PathBuf::from(&tool.pkg_path).join("bin").join(src_name);
                        if !src_path.exists() {
                            anyhow::bail!("Host tool binary not found: {}", src_path.display());
                        }
                        fs::copy(&src_path, &store_path)?;
                        let hash = sha256_file(&store_path)?;
                        entries.push(FileEntry {
                            path: rel_path(&store_path, output_dir),
                            sha256: hash,
                        });
                    }

                    let link_path = bin_dir.join(dst_name);
                    let rel_target = format!("../../../store/{store_name}");
                    let _ = fs::remove_file(&link_path);
                    std::os::unix::fs::symlink(&rel_target, &link_path)?;
                }
            }
        }
    }

    Ok(entries)
}

mod dashmap_lite {
    use std::collections::HashSet;
    use std::hash::{DefaultHasher, Hash, Hasher};
    use std::sync::Mutex;

    const SHARDS: usize = 256;

    pub struct DedupSet {
        shards: Vec<Mutex<HashSet<u64>>>,
    }

    impl DedupSet {
        pub fn new() -> Self {
            Self {
                shards: (0..SHARDS).map(|_| Mutex::new(HashSet::new())).collect(),
            }
        }

        pub fn insert(&self, key: &str) -> bool {
            let hash = {
                let mut h = DefaultHasher::new();
                key.hash(&mut h);
                h.finish()
            };
            let shard_idx = (hash as usize) % SHARDS;
            self.shards[shard_idx].lock().unwrap().insert(hash)
        }
    }
}

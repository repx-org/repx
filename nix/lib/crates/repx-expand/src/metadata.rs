use crate::blueprint::{ContainerMode, StageType};
use crate::expand::ExpandedLab;
use crate::io::FileEntry;
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(Serialize)]
struct JobMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    stage_type: String,
    params: BTreeMap<String, Value>,
    executables: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource_hints: Option<BTreeMap<String, Value>>,
}

#[derive(Serialize)]
struct RunMetadata {
    #[serde(rename = "type")]
    meta_type: String,
    name: String,
    #[serde(rename = "gitHash")]
    git_hash: String,
    dependencies: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    jobs: BTreeMap<String, JobMetadata>,
}

#[derive(Serialize)]
struct RootMetadata {
    repx_version: String,
    #[serde(rename = "type")]
    meta_type: String,
    #[serde(rename = "gitHash")]
    git_hash: String,
    runs: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    groups: BTreeMap<String, Vec<String>>,
}

pub fn write_all_metadata(lab: &ExpandedLab, output_dir: &Path) -> Result<Vec<FileEntry>> {
    let revision_dir = output_dir.join("revision");
    fs::create_dir_all(&revision_dir)?;

    let bp = &lab.blueprint;
    let mut all_entries: Vec<FileEntry> = Vec::new();
    let mut run_metadata_paths: Vec<String> = Vec::new();
    let mut run_metadata_filenames: BTreeMap<String, String> = BTreeMap::new();

    for run in &lab.runs {
        let mut resolved_deps: BTreeMap<String, String> = BTreeMap::new();
        for (dep_name, dep_type) in &run.inter_run_dep_types {
            if let Some(dep_filename) = run_metadata_filenames.get(dep_name) {
                let rel_path = format!("revision/{dep_filename}");
                resolved_deps.insert(rel_path, dep_type.clone());
            }
        }

        let image_path = match bp.container_mode {
            ContainerMode::Unified => bp.unified_image_path.clone(),
            ContainerMode::PerRun => run.image_path.clone(),
            ContainerMode::None => None,
        };

        let mut seen_jobs: HashSet<String> = HashSet::new();
        let mut jobs_meta: BTreeMap<String, JobMetadata> = BTreeMap::new();

        for job in &run.jobs {
            if !seen_jobs.insert(job.job_dir_name.clone()) {
                continue;
            }
            let stage_type_str = match job.stage_type {
                StageType::Simple => "simple",
                StageType::ScatterGather => "scatter-gather",
            };
            let executables: BTreeMap<String, Value> = job
                .executables
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap()))
                .collect();

            jobs_meta.insert(
                job.job_dir_name.clone(),
                JobMetadata {
                    name: Some(job.job_name.clone()),
                    stage_type: stage_type_str.to_string(),
                    params: job.resolved_parameters.clone(),
                    executables,
                    resource_hints: job.resources.clone(),
                },
            );
        }

        let run_meta = RunMetadata {
            meta_type: "run".to_string(),
            name: run.name.clone(),
            git_hash: bp.git_hash.clone(),
            dependencies: resolved_deps,
            image: image_path,
            jobs: jobs_meta,
        };

        let run_json = serde_json::to_string(&run_meta)?;
        let filename = format!("metadata-{}.json", run.name);
        let filepath = revision_dir.join(&filename);
        let hash = write_hashed(&filepath, run_json.as_bytes())?;

        let rel = filepath
            .strip_prefix(output_dir)
            .unwrap()
            .to_string_lossy()
            .to_string();
        all_entries.push(FileEntry {
            path: rel,
            sha256: hash,
        });

        run_metadata_paths.push(format!("revision/{filename}"));
        run_metadata_filenames.insert(run.name.clone(), filename);
    }

    let root_meta = RootMetadata {
        repx_version: bp.repx_version.clone(),
        meta_type: "root".to_string(),
        git_hash: bp.git_hash.clone(),
        runs: run_metadata_paths,
        groups: bp.groups.clone(),
    };

    let root_json = serde_json::to_string(&root_meta)?;
    let root_path = revision_dir.join("metadata-top.json");
    let hash = write_hashed(&root_path, root_json.as_bytes())?;
    let rel = root_path
        .strip_prefix(output_dir)
        .unwrap()
        .to_string_lossy()
        .to_string();
    all_entries.push(FileEntry {
        path: rel,
        sha256: hash,
    });

    Ok(all_entries)
}

fn write_hashed(path: &Path, data: &[u8]) -> Result<String> {
    let mut f = std::io::BufWriter::with_capacity(data.len().max(8192), fs::File::create(path)?);
    f.write_all(data)?;
    f.flush()?;

    let mut hasher = Sha256::new();
    hasher.update(data);
    Ok(hex_encode(hasher.finalize().as_slice()))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

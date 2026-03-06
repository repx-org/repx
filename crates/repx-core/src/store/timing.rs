use crate::errors::CoreError;
use chrono::{DateTime, Utc};
use nix::fcntl::Flock;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

const TIMING_FILE: &str = "timing.json";
const LOCK_FILE: &str = ".timing.lock";

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JobTimestamps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatched: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished: Option<DateTime<Utc>>,
}

fn get_timing_path(output_dir: &Path) -> PathBuf {
    output_dir.join(TIMING_FILE)
}

pub fn read_timestamps(output_dir: &Path) -> Result<JobTimestamps, CoreError> {
    let path = get_timing_path(output_dir);
    if !path.exists() {
        return Ok(JobTimestamps::default());
    }
    let content = fs::read_to_string(&path)?;
    let timestamps: JobTimestamps = serde_json::from_str(&content)?;
    Ok(timestamps)
}

pub fn write_timestamps(output_dir: &Path, timestamps: &JobTimestamps) -> Result<(), CoreError> {
    let path = get_timing_path(output_dir);
    let content = serde_json::to_string_pretty(timestamps)?;
    crate::fs_utils::write_atomic(&path, content.as_bytes())?;
    Ok(())
}

fn update_timestamps<F>(output_dir: &Path, update_fn: F) -> Result<(), CoreError>
where
    F: FnOnce(&mut JobTimestamps),
{
    fs::create_dir_all(output_dir)?;

    let lock_path = output_dir.join(LOCK_FILE);
    let lock_file = File::create(&lock_path)?;
    let _lock = Flock::lock(lock_file, nix::fcntl::FlockArg::LockExclusive)
        .map_err(|(_file, errno)| std::io::Error::other(errno))?;

    let mut timestamps = match read_timestamps(output_dir) {
        Ok(ts) => ts,
        Err(e) => {
            tracing::warn!(
                "Failed to read timestamps from '{}': {}. Using defaults.",
                output_dir.display(),
                e
            );
            JobTimestamps::default()
        }
    };
    update_fn(&mut timestamps);
    write_timestamps(output_dir, &timestamps)
}

pub fn record_dispatched(output_dir: &Path) -> Result<(), CoreError> {
    update_timestamps(output_dir, |ts| {
        if ts.dispatched.is_none() {
            ts.dispatched = Some(Utc::now());
        }
    })
}

pub fn record_started(output_dir: &Path) -> Result<(), CoreError> {
    update_timestamps(output_dir, |ts| {
        if ts.started.is_none() {
            ts.started = Some(Utc::now());
        }
    })
}

pub fn record_finished(output_dir: &Path) -> Result<(), CoreError> {
    update_timestamps(output_dir, |ts| {
        if ts.finished.is_none() {
            ts.finished = Some(Utc::now());
        }
    })
}

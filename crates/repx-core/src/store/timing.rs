use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const TIMING_FILE: &str = "timing.json";

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

pub fn read_timestamps(output_dir: &Path) -> Result<JobTimestamps, AppError> {
    let path = get_timing_path(output_dir);
    if !path.exists() {
        return Ok(JobTimestamps::default());
    }
    let content = fs::read_to_string(&path)?;
    let timestamps: JobTimestamps = serde_json::from_str(&content)?;
    Ok(timestamps)
}

pub fn write_timestamps(output_dir: &Path, timestamps: &JobTimestamps) -> Result<(), AppError> {
    let path = get_timing_path(output_dir);
    let content = serde_json::to_string_pretty(timestamps)?;
    fs::write(path, content)?;
    Ok(())
}

fn update_timestamps<F>(output_dir: &Path, update_fn: F) -> Result<(), AppError>
where
    F: FnOnce(&mut JobTimestamps),
{
    let mut timestamps = read_timestamps(output_dir).unwrap_or_default();
    update_fn(&mut timestamps);
    write_timestamps(output_dir, &timestamps)
}

pub fn record_dispatched(output_dir: &Path) -> Result<(), AppError> {
    update_timestamps(output_dir, |ts| {
        if ts.dispatched.is_none() {
            ts.dispatched = Some(Utc::now());
        }
    })
}

pub fn record_started(output_dir: &Path) -> Result<(), AppError> {
    update_timestamps(output_dir, |ts| {
        if ts.started.is_none() {
            ts.started = Some(Utc::now());
        }
    })
}

pub fn record_finished(output_dir: &Path) -> Result<(), AppError> {
    update_timestamps(output_dir, |ts| {
        if ts.finished.is_none() {
            ts.finished = Some(Utc::now());
        }
    })
}

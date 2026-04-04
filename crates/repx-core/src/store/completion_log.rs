use crate::{engine::JobStatus, errors::CoreError, model::JobId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const COMPLETIONS_FILE: &str = "completions.jsonl";

#[derive(Debug, Serialize, Deserialize)]
struct CompletionRecord {
    id: String,
    s: CompletionStatus,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
enum CompletionStatus {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "fail")]
    Fail,
}

pub fn completions_path(base_path: &Path) -> PathBuf {
    base_path.join("outputs").join(COMPLETIONS_FILE)
}

pub fn append_completion(
    base_path: &Path,
    job_id: &JobId,
    succeeded: bool,
) -> Result<(), CoreError> {
    let path = completions_path(base_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let record = CompletionRecord {
        id: job_id.to_string(),
        s: if succeeded {
            CompletionStatus::Ok
        } else {
            CompletionStatus::Fail
        },
    };

    let mut line = serde_json::to_string(&record)?;
    line.push('\n');

    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

pub fn read_completions(
    base_path: &Path,
    location: &str,
) -> Result<Option<HashMap<JobId, JobStatus>>, CoreError> {
    let path = completions_path(base_path);
    if !path.exists() {
        return Ok(None);
    }

    let file = fs::File::open(&path)?;
    let reader = BufReader::with_capacity(1024 * 1024, file);
    let mut outcomes = HashMap::new();

    for (line_num, line_result) in reader.lines().enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    "Completion log line {} unreadable ({}), skipping",
                    line_num + 1,
                    e
                );
                continue;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let record: CompletionRecord = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    "Completion log line {} parse error ({}), skipping: {}",
                    line_num + 1,
                    e,
                    truncate_for_log(trimmed, 120)
                );
                continue;
            }
        };

        let job_id = JobId::from(record.id);
        let status = match record.s {
            CompletionStatus::Ok => JobStatus::Succeeded {
                location: location.to_string(),
            },
            CompletionStatus::Fail => JobStatus::Failed {
                location: location.to_string(),
            },
        };
        outcomes.insert(job_id, status);
    }

    Ok(Some(outcomes))
}

fn truncate_for_log(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_append_and_read() {
        let dir = tempdir().expect("tempdir");
        let base = dir.path();

        let job1 = JobId::from("abc123-stage-a-1.0");
        let job2 = JobId::from("def456-stage-b-2.0");
        let job3 = JobId::from("ghi789-stage-c-3.0");

        append_completion(base, &job1, true).expect("append job1");
        append_completion(base, &job2, false).expect("append job2");
        append_completion(base, &job3, true).expect("append job3");

        let outcomes = read_completions(base, "test-target")
            .expect("read")
            .expect("some outcomes");
        assert_eq!(outcomes.len(), 3);

        assert!(matches!(
            outcomes.get(&job1),
            Some(JobStatus::Succeeded { .. })
        ));
        assert!(matches!(
            outcomes.get(&job2),
            Some(JobStatus::Failed { .. })
        ));
        assert!(matches!(
            outcomes.get(&job3),
            Some(JobStatus::Succeeded { .. })
        ));
    }

    #[test]
    fn test_last_write_wins() {
        let dir = tempdir().expect("tempdir");
        let base = dir.path();

        let job = JobId::from("abc123-retry-job-1.0");

        append_completion(base, &job, false).expect("append fail");
        append_completion(base, &job, true).expect("append success");

        let outcomes = read_completions(base, "test")
            .expect("read")
            .expect("some outcomes");
        assert!(matches!(
            outcomes.get(&job),
            Some(JobStatus::Succeeded { .. })
        ));
    }

    #[test]
    fn test_no_log_returns_none() {
        let dir = tempdir().expect("tempdir");
        let result = read_completions(dir.path(), "test").expect("read");
        assert!(result.is_none());
    }

    #[test]
    fn test_corrupt_lines_skipped() {
        let dir = tempdir().expect("tempdir");
        let base = dir.path();
        let path = completions_path(base);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");

        let content = r#"{"id":"good-job-1","s":"ok"}
this is garbage
{"id":"good-job-2","s":"fail"}
{"truncated json
{"id":"good-job-3","s":"ok"}
"#;
        fs::write(&path, content).expect("write test data");

        let outcomes = read_completions(base, "test")
            .expect("read")
            .expect("some outcomes");
        assert_eq!(outcomes.len(), 3);
        assert!(matches!(
            outcomes.get(&JobId::from("good-job-1")),
            Some(JobStatus::Succeeded { .. })
        ));
        assert!(matches!(
            outcomes.get(&JobId::from("good-job-2")),
            Some(JobStatus::Failed { .. })
        ));
        assert!(matches!(
            outcomes.get(&JobId::from("good-job-3")),
            Some(JobStatus::Succeeded { .. })
        ));
    }

    #[test]
    fn test_empty_log() {
        let dir = tempdir().expect("tempdir");
        let base = dir.path();
        let path = completions_path(base);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, "").expect("write empty");

        let outcomes = read_completions(base, "test")
            .expect("read")
            .expect("some outcomes");
        assert!(outcomes.is_empty());
    }
}

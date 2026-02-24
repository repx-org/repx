use super::{Client, ClientEvent, SubmitOptions};
use crate::error::{ClientError, Result};
use crate::resources;
use crate::targets::Target;
use num_cpus;
use repx_core::{
    engine,
    errors::ConfigError,
    model::{Job, JobId},
};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use sysinfo::System;

const DEFAULT_JOB_MEM_BYTES: u64 = 1024 * 1024 * 1024;
const DEFAULT_JOB_CPUS: u32 = 1;

fn parse_mem_to_bytes(mem_str: &str) -> Option<u64> {
    let mem_str = mem_str.trim().to_uppercase();
    let (num_str, multiplier) = if let Some(n) = mem_str.strip_suffix('T') {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else if let Some(n) = mem_str.strip_suffix('G') {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = mem_str.strip_suffix('M') {
        (n, 1024u64 * 1024)
    } else if let Some(n) = mem_str.strip_suffix('K') {
        (n, 1024u64)
    } else {
        (mem_str.as_str(), 1u64)
    };
    num_str.parse::<u64>().ok().map(|n| n * multiplier)
}

fn get_job_mem_bytes(job: &Job, resources_config: &Option<repx_core::config::Resources>) -> u64 {
    let hints = job.resource_hints.as_ref();

    let dummy_id = JobId("".into());
    let directives = resources::resolve_for_job(&dummy_id, "", resources_config, hints);

    directives
        .mem
        .as_ref()
        .and_then(|m| parse_mem_to_bytes(m))
        .unwrap_or(DEFAULT_JOB_MEM_BYTES)
}

fn get_job_cpus(job: &Job, resources_config: &Option<repx_core::config::Resources>) -> u32 {
    let hints = job.resource_hints.as_ref();
    let dummy_id = JobId("".into());
    let directives = resources::resolve_for_job(&dummy_id, "", resources_config, hints);
    directives.cpus_per_task.unwrap_or(DEFAULT_JOB_CPUS)
}

pub(crate) fn build_steps_json(
    job: &Job,
    artifacts_base: &std::path::Path,
) -> std::result::Result<(String, String), String> {
    use serde_json::json;
    use std::collections::HashSet;

    let step_entries: Vec<(&String, &repx_core::model::Executable)> = job
        .executables
        .iter()
        .filter(|(k, _)| k.starts_with("step-"))
        .collect();

    if step_entries.is_empty() {
        return Err(
            "Scatter-gather job has no step executables (expected step-<name> keys)".into(),
        );
    }

    let mut steps = serde_json::Map::new();
    let mut all_step_names: Vec<String> = Vec::new();

    for (key, exe) in &step_entries {
        let step_name = key.strip_prefix("step-").unwrap().to_string();
        all_step_names.push(step_name.clone());

        let exe_path = artifacts_base.join(&exe.path);

        let outputs: serde_json::Map<String, serde_json::Value> = exe
            .outputs
            .iter()
            .filter_map(|(name, val)| {
                val.as_str()
                    .map(|s| (name.clone(), serde_json::Value::String(s.to_string())))
            })
            .collect();

        let inputs: Vec<serde_json::Value> = exe
            .inputs
            .iter()
            .map(|m| {
                let mut obj = serde_json::Map::new();
                if let Some(ref source) = m.source {
                    obj.insert("source".into(), json!(source));
                }
                if let Some(ref source_output) = m.source_output {
                    obj.insert("source_output".into(), json!(source_output));
                }
                obj.insert("target_input".into(), json!(m.target_input));
                if let Some(ref job_id) = m.job_id {
                    obj.insert("job_id".into(), json!(job_id.0));
                }
                if let Some(ref mapping_type) = m.mapping_type {
                    obj.insert("type".into(), json!(mapping_type));
                }
                serde_json::Value::Object(obj)
            })
            .collect();

        let mut step_obj = serde_json::Map::new();
        step_obj.insert(
            "exe_path".into(),
            json!(exe_path.to_string_lossy().to_string()),
        );
        step_obj.insert("deps".into(), json!(exe.deps));
        step_obj.insert("outputs".into(), serde_json::Value::Object(outputs));
        step_obj.insert("inputs".into(), serde_json::Value::Array(inputs));

        if let Some(ref hints) = exe.resource_hints {
            step_obj.insert(
                "resource_hints".into(),
                serde_json::to_value(hints).unwrap(),
            );
        }

        steps.insert(step_name, serde_json::Value::Object(step_obj));
    }

    let all_deps: HashSet<String> = step_entries
        .iter()
        .flat_map(|(_, exe)| exe.deps.iter().cloned())
        .collect();
    let sink_candidates: Vec<&String> = all_step_names
        .iter()
        .filter(|name| !all_deps.contains(*name))
        .collect();

    if sink_candidates.len() != 1 {
        return Err(format!(
            "Expected exactly one sink step but found {}: {:?}",
            sink_candidates.len(),
            sink_candidates
        ));
    }
    let sink_step = sink_candidates[0].clone();

    let sink_key = format!("step-{}", sink_step);
    let sink_exe = job.executables.get(&sink_key).unwrap();
    let last_step_outputs_json = serde_json::to_string(&sink_exe.outputs)
        .map_err(|e| format!("Failed to serialize sink step outputs: {}", e))?;

    let steps_metadata = json!({
        "steps": steps,
        "sink_step": sink_step
    });
    let steps_json = serde_json::to_string(&steps_metadata)
        .map_err(|e| format!("Failed to serialize steps metadata: {}", e))?;

    Ok((steps_json, last_step_outputs_json))
}

struct ResourceTracker {
    total_mem_bytes: u64,
    total_cpus: usize,
    used_mem_bytes: u64,
    used_cpus: usize,
    in_flight: HashMap<JobId, (u64, u32)>,
}

impl ResourceTracker {
    fn new() -> Self {
        let sys = System::new_all();
        let total_mem_bytes = sys.total_memory();
        let total_cpus = num_cpus::get();

        tracing::debug!(
            "Local scheduler resource limits: {} RAM, {} CPUs",
            format_bytes(total_mem_bytes),
            total_cpus
        );

        Self {
            total_mem_bytes,
            total_cpus,
            used_mem_bytes: 0,
            used_cpus: 0,
            in_flight: HashMap::new(),
        }
    }

    fn can_fit(&self, job_id: &JobId, mem_bytes: u64, cpus: u32) -> bool {
        if self.in_flight.is_empty() {
            if mem_bytes > self.total_mem_bytes || cpus as usize > self.total_cpus {
                tracing::warn!(
                    "Job '{}' requests {} RAM and {} CPUs, which exceeds system limits ({} RAM, {} CPUs). Running anyway.",
                    job_id.short_id(),
                    format_bytes(mem_bytes),
                    cpus,
                    format_bytes(self.total_mem_bytes),
                    self.total_cpus
                );
            }
            return true;
        }

        let mem_fits = self.used_mem_bytes + mem_bytes <= self.total_mem_bytes;
        let cpus_fit = self.used_cpus + cpus as usize <= self.total_cpus;
        mem_fits && cpus_fit
    }

    fn reserve(&mut self, job_id: JobId, mem_bytes: u64, cpus: u32) {
        self.used_mem_bytes += mem_bytes;
        self.used_cpus += cpus as usize;
        self.in_flight.insert(job_id, (mem_bytes, cpus));
    }

    fn release(&mut self, job_id: &JobId) {
        if let Some((mem, cpus)) = self.in_flight.remove(job_id) {
            self.used_mem_bytes = self.used_mem_bytes.saturating_sub(mem);
            self.used_cpus = self.used_cpus.saturating_sub(cpus as usize);
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 * 1024 {
        format!("{}T", bytes / (1024 * 1024 * 1024 * 1024))
    } else if bytes >= 1024 * 1024 * 1024 {
        format!("{}G", bytes / (1024 * 1024 * 1024))
    } else if bytes >= 1024 * 1024 {
        format!("{}M", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{}K", bytes / 1024)
    } else {
        format!("{}B", bytes)
    }
}

pub fn submit_local_batch_run(
    client: &Client,
    jobs_in_batch: HashMap<JobId, &Job>,
    target: Arc<dyn Target>,
    _target_name: &str,
    repx_binary_path: &Path,
    options: &SubmitOptions,
    send: impl Fn(ClientEvent),
) -> Result<String> {
    send(ClientEvent::SubmittingJobs {
        total: jobs_in_batch.len(),
    });

    let all_deps: HashSet<JobId> = jobs_in_batch
        .values()
        .flat_map(|job| {
            job.executables
                .values()
                .flat_map(|exe| exe.inputs.iter().filter_map(|m| m.job_id.as_ref()))
        })
        .cloned()
        .collect();
    let raw_statuses = client.get_statuses_for_active_target(
        target.name(),
        Some(repx_core::model::SchedulerType::Local),
    )?;
    let all_job_statuses = engine::determine_job_statuses(&client.lab, &raw_statuses);
    let mut completed_jobs: HashSet<JobId> = all_job_statuses
        .into_iter()
        .filter(|(id, status)| {
            matches!(status, repx_core::engine::JobStatus::Succeeded { .. })
                && (all_deps.contains(id) || jobs_in_batch.contains_key(id))
        })
        .map(|(id, _)| id)
        .collect();

    let mut jobs_left: HashSet<JobId> = jobs_in_batch.keys().cloned().collect();
    let total_to_submit = jobs_in_batch.len();
    let mut submitted_count = 0;
    let mut active_handles: Vec<(
        JobId,
        std::thread::JoinHandle<std::io::Result<std::process::Output>>,
    )> = vec![];
    let concurrency = options.num_jobs.unwrap_or_else(num_cpus::get);

    let mut resource_tracker = ResourceTracker::new();

    let mut failed_jobs: Vec<(JobId, String)> = vec![];
    let mut blocked_jobs: HashSet<JobId> = HashSet::new();

    loop {
        let mut finished_indices = Vec::new();
        for (i, (_id, handle)) in active_handles.iter().enumerate() {
            if handle.is_finished() {
                finished_indices.push(i);
            }
        }

        for i in finished_indices.into_iter().rev() {
            let (job_id, handle) = active_handles.remove(i);

            resource_tracker.release(&job_id);

            let join_res = handle.join();
            match join_res {
                Ok(output_res) => {
                    let output = output_res.map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                        if options.continue_on_failure {
                            failed_jobs.push((job_id.clone(), stderr));
                            for (candidate_id, candidate_job) in &jobs_in_batch {
                                if jobs_left.contains(candidate_id) {
                                    let depends_on_failed =
                                        candidate_job.executables.values().any(|exe| {
                                            exe.inputs
                                                .iter()
                                                .filter_map(|m| m.job_id.as_ref())
                                                .any(|dep_id| dep_id == &job_id)
                                        });
                                    if depends_on_failed {
                                        blocked_jobs.insert(candidate_id.clone());
                                    }
                                }
                            }
                        } else {
                            return Err(ClientError::Config(ConfigError::General(format!(
                                "Local run script failed: {}",
                                stderr
                            ))));
                        }
                    } else {
                        completed_jobs.insert(job_id.clone());
                    }
                }
                Err(e) => {
                    if options.continue_on_failure {
                        failed_jobs
                            .push((job_id, format!("Failed to launch local process: {:?}", e)));
                    } else {
                        return Err(ClientError::Config(ConfigError::General(format!(
                            "Failed to launch local process: {:?}",
                            e
                        ))));
                    }
                }
            }
        }

        if jobs_left.is_empty() && active_handles.is_empty() {
            break;
        }

        let slots_available = concurrency.saturating_sub(active_handles.len());

        for blocked_id in &blocked_jobs {
            jobs_left.remove(blocked_id);
        }
        blocked_jobs.clear();

        if slots_available > 0 && !jobs_left.is_empty() {
            let failed_job_ids: HashSet<&JobId> = failed_jobs.iter().map(|(id, _)| id).collect();

            let mut ready_candidates: Vec<JobId> = jobs_left
                .iter()
                .filter(|job_id| {
                    let job = jobs_in_batch.get(job_id).unwrap();
                    let is_schedulable_type = job.stage_type != repx_core::model::StageType::Worker
                        && job.stage_type != repx_core::model::StageType::Gather;

                    let entrypoint_exe = job
                        .executables
                        .get("main")
                        .or_else(|| job.executables.get("scatter"))
                        .unwrap_or_else(|| {
                            panic!(
                                "Job '{}' missing required executable 'main' or 'scatter'",
                                job_id
                            )
                        });

                    let deps_are_met = entrypoint_exe
                        .inputs
                        .iter()
                        .filter_map(|m| m.job_id.as_ref())
                        .all(|dep_id| completed_jobs.contains(dep_id));

                    let no_failed_deps = entrypoint_exe
                        .inputs
                        .iter()
                        .filter_map(|m| m.job_id.as_ref())
                        .all(|dep_id| !failed_job_ids.contains(dep_id));

                    is_schedulable_type && deps_are_met && no_failed_deps
                })
                .cloned()
                .collect();

            ready_candidates.sort();

            if ready_candidates.is_empty() && active_handles.is_empty() {
                if !failed_jobs.is_empty() {
                    break;
                }
                return Err(ClientError::Config(ConfigError::General(
                    "Cycle detected in job dependency graph or missing dependency.".to_string(),
                )));
            }

            let mut spawned_this_iteration = 0;
            for job_id in ready_candidates.into_iter() {
                if spawned_this_iteration >= slots_available {
                    break;
                }

                let job = jobs_in_batch.get(&job_id).unwrap();

                let job_mem = get_job_mem_bytes(job, &options.resources);
                let job_cpus = get_job_cpus(job, &options.resources);

                if !resource_tracker.can_fit(&job_id, job_mem, job_cpus) {
                    tracing::debug!(
                        "Job '{}' waiting for resources ({} RAM, {} CPUs needed)",
                        job_id.short_id(),
                        format_bytes(job_mem),
                        job_cpus
                    );
                    continue;
                }

                jobs_left.remove(&job_id);
                resource_tracker.reserve(job_id.clone(), job_mem, job_cpus);

                let image_path_opt = client
                    .lab
                    .runs
                    .values()
                    .find(|r| r.jobs.contains(&job_id))
                    .and_then(|r| r.image.as_deref());
                let image_tag = image_path_opt
                    .and_then(|p| p.file_stem())
                    .and_then(|s| s.to_str());

                let execution_type = if options.execution_type.is_none() && image_tag.is_none() {
                    "native"
                } else {
                    options.execution_type.as_deref().unwrap_or_else(|| {
                        let scheduler_config = target.config().local.as_ref().unwrap();
                        target
                            .config()
                            .default_execution_type
                            .as_deref()
                            .filter(|&et| {
                                scheduler_config.execution_types.contains(&et.to_string())
                            })
                            .or_else(|| {
                                scheduler_config.execution_types.first().map(|s| s.as_str())
                            })
                            .unwrap_or("native")
                    })
                };
                let mut args = Vec::new();

                if job.stage_type == repx_core::model::StageType::ScatterGather {
                    args.push("internal-scatter-gather".to_string());
                } else {
                    args.push("internal-execute".to_string());
                };

                args.push("--job-id".to_string());
                args.push(job_id.0.clone());

                args.push("--runtime".to_string());
                args.push(execution_type.to_string());

                if let Some(tag) = image_tag {
                    args.push("--image-tag".to_string());
                    args.push(tag.to_string());
                }

                args.push("--base-path".to_string());
                args.push(target.base_path().to_string_lossy().to_string());

                if let Some(local_path) = &target.config().node_local_path {
                    args.push("--node-local-path".to_string());
                    args.push(local_path.to_string_lossy().to_string());
                }

                args.push("--host-tools-dir".to_string());
                args.push(client.lab.host_tools_dir_name.clone());

                if target.config().mount_host_paths {
                    if !target.config().mount_paths.is_empty() {
                        return Err(ClientError::Config(ConfigError::General(format!(
                            "Cannot specify both 'mount_host_paths = true' and 'mount_paths' for job '{}'.",
                            job_id
                        ))));
                    }
                    args.push("--mount-host-paths".to_string());
                } else {
                    for path in &target.config().mount_paths {
                        args.push("--mount-paths".to_string());
                        args.push(path.clone());
                    }
                }

                if job.stage_type == repx_core::model::StageType::ScatterGather {
                    let scatter_exe = job.executables.get("scatter").unwrap();
                    let gather_exe = job.executables.get("gather").unwrap();

                    let artifacts_base = target.artifacts_base_path();
                    let job_package_path_on_target =
                        artifacts_base.join(format!("jobs/{}", job_id));
                    let scatter_exe_path = artifacts_base.join(&scatter_exe.path);
                    let gather_exe_path = artifacts_base.join(&gather_exe.path);

                    let (steps_json, last_step_outputs_json) =
                        build_steps_json(job, &artifacts_base)
                            .map_err(|e| ClientError::Config(ConfigError::General(e)))?;

                    args.push("--job-package-path".to_string());
                    args.push(job_package_path_on_target.to_string_lossy().to_string());

                    args.push("--scatter-exe-path".to_string());
                    args.push(scatter_exe_path.to_string_lossy().to_string());

                    args.push("--gather-exe-path".to_string());
                    args.push(gather_exe_path.to_string_lossy().to_string());

                    args.push("--steps-json".to_string());
                    args.push(steps_json);

                    args.push("--last-step-outputs-json".to_string());
                    args.push(last_step_outputs_json);

                    args.push("--scheduler".to_string());
                    args.push("local".to_string());

                    args.push("--step-sbatch-opts".to_string());
                    args.push("".to_string());
                } else {
                    let main_exe = job.executables.get("main").unwrap();
                    let executable_path_on_target =
                        target.artifacts_base_path().join(&main_exe.path);
                    args.push("--executable-path".to_string());
                    args.push(executable_path_on_target.to_string_lossy().to_string());
                }

                let child = target.spawn_repx_job(repx_binary_path, &args)?;
                submitted_count += 1;
                spawned_this_iteration += 1;
                let pid = child.id();

                send(ClientEvent::JobStarted {
                    job_id: job_id.clone(),
                    pid,
                    total: total_to_submit,
                    current: submitted_count,
                });

                let handle = thread::spawn(move || child.wait_with_output());
                active_handles.push((job_id, handle));
            }
        }

        if !active_handles.is_empty() {
            thread::sleep(Duration::from_millis(50));
        }
    }

    if !failed_jobs.is_empty() {
        let num_failed = failed_jobs.len();
        let num_succeeded = submitted_count - num_failed;
        let num_skipped = total_to_submit - submitted_count;

        let mut error_msg = format!(
            "{} job(s) failed, {} succeeded, {} skipped due to failed dependencies:\n",
            num_failed, num_succeeded, num_skipped
        );
        for (job_id, stderr) in &failed_jobs {
            error_msg.push_str(&format!("\n=== {} ===\n{}\n", job_id, stderr));
        }
        return Err(ClientError::Config(ConfigError::General(error_msg)));
    }

    Ok(format!(
        "Successfully executed {} jobs locally.",
        submitted_count
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mem_to_bytes() {
        assert_eq!(parse_mem_to_bytes("1G"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_mem_to_bytes("512M"), Some(512 * 1024 * 1024));
        assert_eq!(
            parse_mem_to_bytes("2T"),
            Some(2 * 1024 * 1024 * 1024 * 1024)
        );
        assert_eq!(parse_mem_to_bytes("1024K"), Some(1024 * 1024));
        assert_eq!(parse_mem_to_bytes("4096"), Some(4096));

        assert_eq!(parse_mem_to_bytes("8g"), Some(8 * 1024 * 1024 * 1024));
        assert_eq!(parse_mem_to_bytes("256m"), Some(256 * 1024 * 1024));

        assert_eq!(parse_mem_to_bytes("  4G  "), Some(4 * 1024 * 1024 * 1024));

        assert_eq!(parse_mem_to_bytes("invalid"), None);
        assert_eq!(parse_mem_to_bytes("G"), None);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(1024), "1K");
        assert_eq!(format_bytes(1024 * 1024), "1M");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1G");
        assert_eq!(format_bytes(1024 * 1024 * 1024 * 1024), "1T");
        assert_eq!(format_bytes(4 * 1024 * 1024 * 1024), "4G");
    }

    #[test]
    fn test_resource_tracker_can_fit() {
        let mut tracker = ResourceTracker {
            total_mem_bytes: 16 * 1024 * 1024 * 1024,
            total_cpus: 8,
            used_mem_bytes: 0,
            used_cpus: 0,
            in_flight: HashMap::new(),
        };

        let job1 = JobId("job1".into());
        let job2 = JobId("job2".into());
        let job3 = JobId("job3".into());

        assert!(tracker.can_fit(&job1, 8 * 1024 * 1024 * 1024, 4));

        tracker.reserve(job1.clone(), 8 * 1024 * 1024 * 1024, 4);

        assert!(tracker.can_fit(&job2, 4 * 1024 * 1024 * 1024, 2));
        tracker.reserve(job2.clone(), 4 * 1024 * 1024 * 1024, 2);

        assert!(!tracker.can_fit(&job3, 8 * 1024 * 1024 * 1024, 4));

        assert!(tracker.can_fit(&job3, 2 * 1024 * 1024 * 1024, 1));

        assert!(!tracker.can_fit(&job3, 6 * 1024 * 1024 * 1024, 1));

        assert!(!tracker.can_fit(&job3, 2 * 1024 * 1024 * 1024, 4));
    }

    #[test]
    fn test_resource_tracker_reserve_and_release() {
        let mut tracker = ResourceTracker {
            total_mem_bytes: 16 * 1024 * 1024 * 1024,
            total_cpus: 8,
            used_mem_bytes: 0,
            used_cpus: 0,
            in_flight: HashMap::new(),
        };

        let job1 = JobId("job1".into());
        let job2 = JobId("job2".into());

        tracker.reserve(job1.clone(), 4 * 1024 * 1024 * 1024, 2);
        assert_eq!(tracker.used_mem_bytes, 4 * 1024 * 1024 * 1024);
        assert_eq!(tracker.used_cpus, 2);
        assert_eq!(tracker.in_flight.len(), 1);

        tracker.reserve(job2.clone(), 8 * 1024 * 1024 * 1024, 4);
        assert_eq!(tracker.used_mem_bytes, 12 * 1024 * 1024 * 1024);
        assert_eq!(tracker.used_cpus, 6);
        assert_eq!(tracker.in_flight.len(), 2);

        tracker.release(&job1);
        assert_eq!(tracker.used_mem_bytes, 8 * 1024 * 1024 * 1024);
        assert_eq!(tracker.used_cpus, 4);
        assert_eq!(tracker.in_flight.len(), 1);

        tracker.release(&job2);
        assert_eq!(tracker.used_mem_bytes, 0);
        assert_eq!(tracker.used_cpus, 0);
        assert_eq!(tracker.in_flight.len(), 0);
    }

    #[test]
    fn test_resource_tracker_oversized_job_allowed_when_empty() {
        let tracker = ResourceTracker {
            total_mem_bytes: 8 * 1024 * 1024 * 1024,
            total_cpus: 4,
            used_mem_bytes: 0,
            used_cpus: 0,
            in_flight: HashMap::new(),
        };

        let big_job = JobId("big_job".into());

        assert!(tracker.can_fit(&big_job, 32 * 1024 * 1024 * 1024, 16));
    }

    #[test]
    fn test_resource_tracker_oversized_job_blocked_when_busy() {
        let mut tracker = ResourceTracker {
            total_mem_bytes: 8 * 1024 * 1024 * 1024,
            total_cpus: 4,
            used_mem_bytes: 0,
            used_cpus: 0,
            in_flight: HashMap::new(),
        };

        let small_job = JobId("small_job".into());
        let big_job = JobId("big_job".into());

        tracker.reserve(small_job.clone(), 1024 * 1024 * 1024, 1);

        assert!(!tracker.can_fit(&big_job, 32 * 1024 * 1024 * 1024, 16));
    }

    #[test]
    fn test_resource_tracker_release_unknown_job_is_safe() {
        let mut tracker = ResourceTracker {
            total_mem_bytes: 8 * 1024 * 1024 * 1024,
            total_cpus: 4,
            used_mem_bytes: 4 * 1024 * 1024 * 1024,
            used_cpus: 2,
            in_flight: HashMap::new(),
        };

        let unknown_job = JobId("unknown".into());

        tracker.release(&unknown_job);

        assert_eq!(tracker.used_mem_bytes, 4 * 1024 * 1024 * 1024);
        assert_eq!(tracker.used_cpus, 2);
    }
}

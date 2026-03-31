use super::{Client, ClientEvent, SubmitOptions, WorkUnitPhase};
use crate::error::{ClientError, Result};
use crate::resources;
use crate::targets::Target;
use num_cpus;
use repx_core::{
    constants::{dirs, targets},
    engine,
    errors::CoreError,
    model::{Job, JobId},
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Child;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::System;

const DEFAULT_JOB_MEM_BYTES: u64 = 1024 * 1024 * 1024;
const DEFAULT_JOB_CPUS: u32 = 1;
const POLL_INTERVAL_MS: u64 = 50;

type ActiveHandle = (
    WorkUnitId,
    Arc<Mutex<Option<Child>>>,
    std::thread::JoinHandle<std::io::Result<std::process::Output>>,
    Instant,
);

fn get_job_mem_bytes(job: &Job, resources_config: &Option<repx_core::config::Resources>) -> u64 {
    let hints = job.resource_hints.as_ref();

    let dummy_id = JobId::from("");
    let directives = resources::resolve_for_job(&dummy_id, "", resources_config, hints);

    directives
        .mem
        .as_ref()
        .and_then(|m| m.to_bytes())
        .unwrap_or(DEFAULT_JOB_MEM_BYTES)
}

fn get_job_cpus(job: &Job, resources_config: &Option<repx_core::config::Resources>) -> u32 {
    let hints = job.resource_hints.as_ref();
    let dummy_id = JobId::from("");
    let directives = resources::resolve_for_job(&dummy_id, "", resources_config, hints);
    directives.cpus_per_task.unwrap_or(DEFAULT_JOB_CPUS)
}

#[allow(clippy::expect_used)]
pub(crate) fn build_steps_json(
    job: &Job,
    artifacts_base: &std::path::Path,
) -> Result<(String, String)> {
    use serde_json::json;
    use std::collections::HashSet;

    let step_entries: Vec<(&String, &repx_core::model::Executable)> = job
        .executables
        .iter()
        .filter(|(k, _)| k.starts_with("step-"))
        .collect();

    if step_entries.is_empty() {
        return Err(ClientError::Config(CoreError::InconsistentMetadata {
            detail: "Scatter-gather job has no step executables (expected step-<name> keys)"
                .to_string(),
        }));
    }

    let mut steps = serde_json::Map::new();
    let mut all_step_names: Vec<String> = Vec::new();

    for (key, exe) in &step_entries {
        let step_name = key
            .strip_prefix("step-")
            .expect("prefix guaranteed by starts_with filter")
            .to_string();
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
                    obj.insert("job_id".into(), json!(job_id.as_str()));
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
            if let Ok(hints_val) = serde_json::to_value(hints) {
                step_obj.insert("resource_hints".into(), hints_val);
            }
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
        return Err(ClientError::Config(CoreError::InconsistentMetadata {
            detail: format!(
                "Expected exactly one sink step but found {}: {:?}",
                sink_candidates.len(),
                sink_candidates
            ),
        }));
    }
    let sink_step = sink_candidates[0].clone();

    let sink_key = format!("step-{}", sink_step);
    let sink_exe = job.executables.get(&sink_key).ok_or_else(|| {
        ClientError::Config(CoreError::MissingExecutable {
            job_id: sink_key.clone(),
            executable: "step".to_string(),
        })
    })?;
    let last_step_outputs_json = serde_json::to_string(&sink_exe.outputs).map_err(|e| {
        ClientError::Config(CoreError::SerializationError(format!(
            "Failed to serialize sink step outputs: {}",
            e
        )))
    })?;

    let steps_metadata = json!({
        "steps": steps,
        "sink_step": sink_step
    });
    let steps_json = serde_json::to_string(&steps_metadata).map_err(|e| {
        ClientError::Config(CoreError::SerializationError(format!(
            "Failed to serialize steps metadata: {}",
            e
        )))
    })?;

    Ok((steps_json, last_step_outputs_json))
}

struct ResourceTracker {
    total_mem_bytes: u64,
    total_cpus: usize,
    used_mem_bytes: u64,
    used_cpus: usize,
    in_flight: HashMap<WorkUnitId, (u64, u32)>,
}

impl ResourceTracker {
    fn new(mem_override: Option<u64>) -> Self {
        let total_cpus = num_cpus::get();
        let total_mem_bytes = match mem_override {
            Some(m) => {
                let mut sys = System::new();
                sys.refresh_memory();
                let system_mem = sys.total_memory();
                tracing::info!(
                    "Memory override: {} (system has {})",
                    format_bytes(m),
                    format_bytes(system_mem),
                );
                m
            }
            None => {
                let mut sys = System::new();
                sys.refresh_memory();
                sys.total_memory()
            }
        };

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

    fn can_fit(&self, id: &WorkUnitId, mem_bytes: u64, cpus: u32) -> bool {
        if self.in_flight.is_empty() {
            if mem_bytes > self.total_mem_bytes || cpus as usize > self.total_cpus {
                tracing::warn!(
                    "Unit '{}' requests {} RAM and {} CPUs, which exceeds system limits ({} RAM, {} CPUs). Running anyway.",
                    id.short_id(),
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

    fn reserve(&mut self, id: WorkUnitId, mem_bytes: u64, cpus: u32) {
        self.used_mem_bytes += mem_bytes;
        self.used_cpus += cpus as usize;
        self.in_flight.insert(id, (mem_bytes, cpus));
    }

    fn release(&mut self, id: &WorkUnitId) {
        if let Some((mem, cpus)) = self.in_flight.remove(id) {
            self.used_mem_bytes = self.used_mem_bytes.saturating_sub(mem);
            self.used_cpus = self.used_cpus.saturating_sub(cpus as usize);
        }
    }
}

#[derive(Debug, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
struct WorkUnitId(String);

impl WorkUnitId {
    fn from_job(id: &JobId) -> Self {
        Self(id.to_string())
    }
    fn scatter(job_id: &JobId) -> Self {
        Self(format!("{}::scatter", job_id.as_str()))
    }
    fn step(job_id: &JobId, branch: usize, step: &str) -> Self {
        Self(format!("{}::b{}::s-{}", job_id.as_str(), branch, step))
    }
    fn gather(job_id: &JobId) -> Self {
        Self(format!("{}::gather", job_id.as_str()))
    }
    fn short_id(&self) -> &str {
        if self.0.len() > 40 {
            &self.0[..40]
        } else {
            &self.0
        }
    }

    fn phase(&self) -> Option<WorkUnitPhase> {
        if self.0.ends_with("::scatter") {
            Some(WorkUnitPhase::Scatter)
        } else if self.0.ends_with("::gather") {
            Some(WorkUnitPhase::Gather)
        } else if let Some(suffix) = self.0.split_once("::b") {
            let rest = suffix.1;
            if let Some((branch_str, step_part)) = rest.split_once("::s-") {
                if let Ok(branch) = branch_str.parse::<usize>() {
                    return Some(WorkUnitPhase::Step {
                        branch,
                        step: step_part.to_string(),
                    });
                }
            }
            None
        } else {
            None
        }
    }
}

impl std::fmt::Display for WorkUnitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

struct WorkUnit<'a> {
    deps: Vec<WorkUnitId>,
    mem_bytes: u64,
    cpus: u32,
    job: &'a Job,
    job_id: JobId,
    extra_args: Vec<String>,
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

fn build_sg_common_args(
    job_id: &JobId,
    job: &Job,
    target: &dyn Target,
    client: &Client,
    execution_type: &str,
    image_tag: Option<&str>,
    verbose: repx_core::logging::Verbosity,
) -> std::result::Result<Vec<String>, ClientError> {
    let artifacts_base = target.artifacts_base_path();
    let scatter_exe = job.executables.get("scatter").ok_or_else(|| {
        ClientError::Config(CoreError::MissingExecutable {
            job_id: job_id.to_string(),
            executable: "scatter".to_string(),
        })
    })?;
    let gather_exe = job.executables.get("gather").ok_or_else(|| {
        ClientError::Config(CoreError::MissingExecutable {
            job_id: job_id.to_string(),
            executable: "gather".to_string(),
        })
    })?;
    let job_package_path = artifacts_base.join(format!("jobs/{}", job_id));
    let scatter_exe_path = artifacts_base.join(&scatter_exe.path);
    let gather_exe_path = artifacts_base.join(&gather_exe.path);

    let (steps_json, last_step_outputs_json) = build_steps_json(job, &artifacts_base)?;

    let mut args = verbose.as_args();
    args.extend_from_slice(&[
        "internal-scatter-gather".to_string(),
        "--job-id".to_string(),
        job_id.to_string(),
        "--runtime".to_string(),
        execution_type.to_string(),
    ]);
    if let Some(tag) = image_tag {
        args.push("--image-tag".to_string());
        args.push(tag.to_string());
    }
    args.extend_from_slice(&[
        "--base-path".to_string(),
        target.base_path().to_string_lossy().to_string(),
    ]);
    if let Some(local_path) = &target.config().node_local_path {
        args.push("--node-local-path".to_string());
        args.push(local_path.to_string_lossy().to_string());
    }
    args.extend_from_slice(&[
        "--host-tools-dir".to_string(),
        client.lab.host_tools_dir_name.clone(),
    ]);
    match target.config().mount_policy() {
        repx_core::model::MountPolicy::AllHostPaths => {
            args.push("--mount-host-paths".to_string());
        }
        repx_core::model::MountPolicy::SpecificPaths(paths) => {
            for path in &paths {
                args.push("--mount-paths".to_string());
                args.push(path.clone());
            }
        }
        repx_core::model::MountPolicy::Isolated => {}
    }
    args.extend_from_slice(&[
        "--job-package-path".to_string(),
        job_package_path.to_string_lossy().to_string(),
        "--scatter-exe-path".to_string(),
        scatter_exe_path.to_string_lossy().to_string(),
        "--gather-exe-path".to_string(),
        gather_exe_path.to_string_lossy().to_string(),
        "--steps-json".to_string(),
        steps_json,
        "--last-step-outputs-json".to_string(),
        last_step_outputs_json,
        "--scheduler".to_string(),
        targets::LOCAL.to_string(),
        "--step-sbatch-opts".to_string(),
        String::new(),
    ]);
    Ok(args)
}

fn build_simple_job_args(
    job_id: &JobId,
    job: &Job,
    target: &dyn Target,
    client: &Client,
    execution_type: &str,
    image_tag: Option<&str>,
    verbose: repx_core::logging::Verbosity,
) -> std::result::Result<Vec<String>, ClientError> {
    let main_exe = job.executables.get("main").ok_or_else(|| {
        ClientError::Config(CoreError::MissingExecutable {
            job_id: job_id.to_string(),
            executable: "main".to_string(),
        })
    })?;
    let executable_path = target.artifacts_base_path().join(&main_exe.path);

    let mut args = verbose.as_args();
    args.extend_from_slice(&[
        "internal-execute".to_string(),
        "--job-id".to_string(),
        job_id.to_string(),
        "--runtime".to_string(),
        execution_type.to_string(),
    ]);
    if let Some(tag) = image_tag {
        args.push("--image-tag".to_string());
        args.push(tag.to_string());
    }
    args.extend_from_slice(&[
        "--base-path".to_string(),
        target.base_path().to_string_lossy().to_string(),
    ]);
    if let Some(local_path) = &target.config().node_local_path {
        args.push("--node-local-path".to_string());
        args.push(local_path.to_string_lossy().to_string());
    }
    args.extend_from_slice(&[
        "--host-tools-dir".to_string(),
        client.lab.host_tools_dir_name.clone(),
    ]);
    match target.config().mount_policy() {
        repx_core::model::MountPolicy::AllHostPaths => {
            args.push("--mount-host-paths".to_string());
        }
        repx_core::model::MountPolicy::SpecificPaths(paths) => {
            for path in &paths {
                args.push("--mount-paths".to_string());
                args.push(path.clone());
            }
        }
        repx_core::model::MountPolicy::Isolated => {}
    }
    args.extend_from_slice(&[
        "--executable-path".to_string(),
        executable_path.to_string_lossy().to_string(),
    ]);
    Ok(args)
}

fn resolve_local_execution_type(
    image_tag: Option<&str>,
    options: &SubmitOptions,
    target: &dyn Target,
) -> String {
    super::resolve_execution_type(
        image_tag,
        options.execution_type.as_deref(),
        target.config(),
        target.config().local.as_ref(),
    )
}

fn resolve_image_tag<'a>(job_id: &JobId, client: &'a Client) -> Option<&'a str> {
    client
        .lab
        .runs
        .values()
        .find(|r| r.jobs.contains(job_id))
        .and_then(|r| r.image.as_deref())
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
}

fn get_step_resources(
    job: &Job,
    step_exe_key: &str,
    resources_config: &Option<repx_core::config::Resources>,
    job_id: &JobId,
) -> (u64, u32) {
    let step_exe = job.executables.get(step_exe_key);
    let worker_hints = step_exe.and_then(|e| e.resource_hints.as_ref());
    let orchestrator_hints = job.resource_hints.as_ref();

    let directives = resources::resolve_worker_resources(
        job_id,
        "",
        resources_config,
        orchestrator_hints,
        worker_hints,
    );

    let mem = directives
        .mem
        .as_ref()
        .and_then(|m| m.to_bytes())
        .unwrap_or(DEFAULT_JOB_MEM_BYTES);
    let cpus = directives.cpus_per_task.unwrap_or(DEFAULT_JOB_CPUS);
    (mem, cpus)
}

#[allow(clippy::expect_used)]
fn expand_scatter_gather<'a>(
    job_id: &JobId,
    job: &'a Job,
    target: &dyn Target,
    client: &Client,
    execution_type: &str,
    image_tag: Option<&str>,
    options: &SubmitOptions,
) -> std::result::Result<Vec<(WorkUnitId, WorkUnit<'a>)>, ClientError> {
    let base_path = target.base_path();
    let job_root = base_path.join(dirs::OUTPUTS).join(job_id.as_str());
    let work_items_path = job_root.join("scatter").join("out").join("work_items.json");
    let work_items_str = target.read_remote_file(&work_items_path).map_err(|e| {
        ClientError::Config(CoreError::CommandFailed(format!(
            "Failed to read work_items.json after scatter for '{}': {}",
            job_id, e
        )))
    })?;
    let work_items: Vec<Value> = serde_json::from_str(&work_items_str).map_err(|e| {
        ClientError::Config(CoreError::SerializationError(format!(
            "Failed to parse work_items.json for '{}': {}",
            job_id, e
        )))
    })?;

    let step_exes: Vec<(String, &repx_core::model::Executable)> = job
        .executables
        .iter()
        .filter(|(k, _)| k.starts_with("step-"))
        .map(|(k, v)| {
            (
                k.strip_prefix("step-")
                    .expect("prefix guaranteed by starts_with filter")
                    .to_string(),
                v,
            )
        })
        .collect();

    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    let step_names_owned: Vec<String> = step_exes.iter().map(|(n, _)| n.clone()).collect();
    for (name, exe) in &step_exes {
        in_degree.entry(name.as_str()).or_insert(0);
        for dep in &exe.deps {
            *in_degree.entry(name.as_str()).or_insert(0) += 1;
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(name.as_str());
        }
    }
    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&n, _)| n)
        .collect();
    queue.sort();
    let mut topo_order = Vec::new();
    while let Some(name) = queue.pop() {
        topo_order.push(name.to_string());
        if let Some(deps) = dependents.get(name) {
            let mut newly_ready: Vec<&str> = Vec::new();
            for &dep_name in deps {
                let deg = in_degree
                    .get_mut(dep_name)
                    .expect("in_degree must contain all step names from initialization");
                *deg -= 1;
                if *deg == 0 {
                    newly_ready.push(dep_name);
                }
            }
            newly_ready.sort();
            newly_ready.reverse();
            queue.extend(newly_ready);
        }
    }

    let all_deps_set: HashSet<&str> = step_exes
        .iter()
        .flat_map(|(_, exe)| exe.deps.iter().map(|d| d.as_str()))
        .collect();
    let sinks: Vec<&str> = step_names_owned
        .iter()
        .map(|s| s.as_str())
        .filter(|n| !all_deps_set.contains(n))
        .collect();
    let sink_step = sinks
        .first()
        .ok_or_else(|| {
            ClientError::Config(CoreError::InconsistentMetadata {
                detail: "No sink step found in scatter-gather step DAG".to_string(),
            })
        })?
        .to_string();

    let scatter_id = WorkUnitId::scatter(job_id);

    let sg_common = build_sg_common_args(
        job_id,
        job,
        target,
        client,
        execution_type,
        image_tag,
        options.verbose,
    )?;

    let mut units = Vec::new();

    for branch_idx in 0..work_items.len() {
        for step_name in &topo_order {
            let step_id = WorkUnitId::step(job_id, branch_idx, step_name);
            let exe_key = format!("step-{}", step_name);

            let step_exe = job.executables.get(&exe_key);
            let intra_deps: Vec<String> = step_exe.map(|e| e.deps.clone()).unwrap_or_default();
            let mut deps: Vec<WorkUnitId> = intra_deps
                .iter()
                .map(|dep| WorkUnitId::step(job_id, branch_idx, dep))
                .collect();
            if deps.is_empty() {
                deps.push(scatter_id.clone());
            }

            let (mem, cpus) = get_step_resources(job, &exe_key, &options.resources, job_id);

            let mut extra_args = sg_common.clone();
            extra_args.extend_from_slice(&[
                "--phase".to_string(),
                "step".to_string(),
                "--branch-idx".to_string(),
                branch_idx.to_string(),
                "--step-name".to_string(),
                step_name.clone(),
            ]);

            units.push((
                step_id,
                WorkUnit {
                    deps,
                    mem_bytes: mem,
                    cpus,
                    job,
                    job_id: job_id.clone(),
                    extra_args,
                },
            ));
        }
    }

    let gather_deps: Vec<WorkUnitId> = (0..work_items.len())
        .map(|b| WorkUnitId::step(job_id, b, &sink_step))
        .collect();
    let gather_mem = get_job_mem_bytes(job, &options.resources);
    let gather_cpus = get_job_cpus(job, &options.resources);

    let mut gather_extra = sg_common;
    gather_extra.extend_from_slice(&["--phase".to_string(), "gather".to_string()]);

    units.push((
        WorkUnitId::gather(job_id),
        WorkUnit {
            deps: gather_deps,
            mem_bytes: gather_mem,
            cpus: gather_cpus,
            job,
            job_id: job_id.clone(),
            extra_args: gather_extra,
        },
    ));

    Ok(units)
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
    let total_jobs = jobs_in_batch.len();
    let concurrency = options.num_jobs.unwrap_or_else(num_cpus::get);
    send(ClientEvent::SubmittingJobs {
        total: total_jobs,
        concurrency: Some(concurrency),
    });
    let mut succeeded_work_units: usize = 0;

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
    let completed_job_ids: HashSet<JobId> = all_job_statuses
        .into_iter()
        .filter(|(id, status)| {
            matches!(status, repx_core::engine::JobStatus::Succeeded { .. })
                && (all_deps.contains(id) || jobs_in_batch.contains_key(id))
        })
        .map(|(id, _)| id)
        .collect();

    let mut completion_map: HashMap<JobId, WorkUnitId> = HashMap::new();

    let mut work_units: HashMap<WorkUnitId, WorkUnit> = HashMap::new();
    let mut units_left: HashSet<WorkUnitId> = HashSet::new();
    let mut completed: HashSet<WorkUnitId> = HashSet::new();

    for (job_id_ref, &job) in &jobs_in_batch {
        let job_id = job_id_ref.clone();

        if job.stage_type == repx_core::model::StageType::Worker
            || job.stage_type == repx_core::model::StageType::Gather
        {
            continue;
        }

        if completed_job_ids.contains(&job_id) {
            let unit_id = if job.stage_type == repx_core::model::StageType::ScatterGather {
                WorkUnitId::gather(&job_id)
            } else {
                WorkUnitId::from_job(&job_id)
            };
            completed.insert(unit_id.clone());
            completion_map.insert(job_id, unit_id);
            continue;
        }

        let image_tag = resolve_image_tag(&job_id, client);
        let execution_type = resolve_local_execution_type(image_tag, options, target.as_ref());

        let entrypoint_exe = job
            .executables
            .get("main")
            .or_else(|| job.executables.get("scatter"))
            .ok_or_else(|| {
                ClientError::Config(CoreError::MissingExecutable {
                    job_id: job_id.to_string(),
                    executable: "main or scatter".to_string(),
                })
            })?;
        let job_deps: Vec<WorkUnitId> = entrypoint_exe
            .inputs
            .iter()
            .filter_map(|m| m.job_id.as_ref())
            .map(|dep_job_id| {
                if let Some(dep_job) = jobs_in_batch.get(dep_job_id) {
                    if dep_job.stage_type == repx_core::model::StageType::ScatterGather {
                        WorkUnitId::gather(dep_job_id)
                    } else {
                        WorkUnitId::from_job(dep_job_id)
                    }
                } else {
                    WorkUnitId::from_job(dep_job_id)
                }
            })
            .collect();

        if job.stage_type == repx_core::model::StageType::ScatterGather {
            let scatter_id = WorkUnitId::scatter(&job_id);
            let mem = get_job_mem_bytes(job, &options.resources);
            let cpus = get_job_cpus(job, &options.resources);

            let mut extra_args = build_sg_common_args(
                &job_id,
                job,
                target.as_ref(),
                client,
                &execution_type,
                image_tag,
                options.verbose,
            )?;
            extra_args.extend_from_slice(&["--phase".to_string(), "scatter-only".to_string()]);

            work_units.insert(
                scatter_id.clone(),
                WorkUnit {
                    deps: job_deps,
                    mem_bytes: mem,
                    cpus,
                    job,
                    job_id: job_id.clone(),
                    extra_args,
                },
            );
            units_left.insert(scatter_id);

            completion_map.insert(job_id, WorkUnitId::gather(job_id_ref));
        } else {
            let unit_id = WorkUnitId::from_job(&job_id);
            let mem = get_job_mem_bytes(job, &options.resources);
            let cpus = get_job_cpus(job, &options.resources);

            let extra_args = build_simple_job_args(
                &job_id,
                job,
                target.as_ref(),
                client,
                &execution_type,
                image_tag,
                options.verbose,
            )?;

            work_units.insert(
                unit_id.clone(),
                WorkUnit {
                    deps: job_deps,
                    mem_bytes: mem,
                    cpus,
                    job,
                    job_id: job_id.clone(),
                    extra_args,
                },
            );
            units_left.insert(unit_id.clone());

            completion_map.insert(job_id, unit_id);
        }
    }

    for dep_id in &completed_job_ids {
        if !completion_map.contains_key(dep_id) {
            let unit_id = WorkUnitId::from_job(dep_id);
            completed.insert(unit_id.clone());
            completion_map.insert(dep_id.clone(), unit_id);
        }
    }

    let mut dependents: HashMap<WorkUnitId, Vec<WorkUnitId>> = HashMap::new();
    for (unit_id, unit) in &work_units {
        for dep in &unit.deps {
            dependents
                .entry(dep.clone())
                .or_default()
                .push(unit_id.clone());
        }
    }

    let mut total_work_units = units_left.len();

    let mut resource_tracker = ResourceTracker::new(options.mem_override);
    let mut active_handles: Vec<ActiveHandle> = vec![];
    let mut failed_units: Vec<(WorkUnitId, String)> = vec![];
    let mut blocked_units: HashSet<WorkUnitId> = HashSet::new();
    let mut submitted_count: usize = 0;

    loop {
        if let Some(ref flag) = options.cancel_flag {
            if flag.load(Ordering::SeqCst) {
                tracing::warn!(
                    "Cancellation requested, killing {} running processes...",
                    active_handles.len()
                );
                for (uid, child_handle, _, _) in &active_handles {
                    if let Ok(mut guard) = child_handle.lock() {
                        if let Some(ref mut child) = *guard {
                            tracing::debug!("Killing process for unit {}", uid);
                            let _ = child.kill();
                        }
                    }
                }
                thread::sleep(Duration::from_millis(100));
                return Err(ClientError::Config(CoreError::CommandFailed(
                    "Batch run cancelled by user".to_string(),
                )));
            }
        }

        let mut finished_indices = Vec::new();
        for (i, (_id, _, handle, _)) in active_handles.iter().enumerate() {
            if handle.is_finished() {
                finished_indices.push(i);
            }
        }
        let any_finished = !finished_indices.is_empty();

        for i in finished_indices.into_iter().rev() {
            let (unit_id, _, handle, started_at) = active_handles.remove(i);
            resource_tracker.release(&unit_id);

            match handle.join() {
                Ok(output_res) => {
                    let output = output_res.map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                    let phase = unit_id.phase();
                    let wall_time = Some(started_at.elapsed());

                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                        if options.continue_on_failure {
                            failed_units.push((unit_id.clone(), stderr));

                            let reported_via_completion_map =
                                completion_map.iter().any(|(jid, comp_uid)| {
                                    comp_uid == &unit_id && jobs_in_batch.contains_key(jid)
                                });
                            if reported_via_completion_map {
                                for (jid, comp_uid) in &completion_map {
                                    if comp_uid == &unit_id && jobs_in_batch.contains_key(jid) {
                                        send(ClientEvent::JobFailed {
                                            job_id: jid.clone(),
                                            phase: phase.clone(),
                                            wall_time,
                                        });
                                    }
                                }
                            } else if let Some(u) = work_units.get(&unit_id) {
                                send(ClientEvent::JobFailed {
                                    job_id: u.job_id.clone(),
                                    phase: phase.clone(),
                                    wall_time,
                                });
                            }

                            let failed_job_id = work_units.get(&unit_id).map(|u| u.job_id.clone());
                            if let Some(dependent_ids) = dependents.get(&unit_id) {
                                for candidate_id in dependent_ids {
                                    if !units_left.contains(candidate_id) {
                                        continue;
                                    }
                                    let Some(candidate) = work_units.get(candidate_id) else {
                                        continue;
                                    };
                                    blocked_units.insert(candidate_id.clone());
                                    if let Some(ref blocked_by_jid) = failed_job_id {
                                        let blocked_phase = candidate_id.phase();
                                        let reported_via_completion_map =
                                            completion_map.iter().any(|(jid, comp_uid)| {
                                                comp_uid == candidate_id
                                                    && jobs_in_batch.contains_key(jid)
                                            });
                                        if reported_via_completion_map {
                                            for (jid, comp_uid) in &completion_map {
                                                if comp_uid == candidate_id
                                                    && jobs_in_batch.contains_key(jid)
                                                {
                                                    send(ClientEvent::JobBlocked {
                                                        job_id: jid.clone(),
                                                        blocked_by: blocked_by_jid.clone(),
                                                        phase: blocked_phase.clone(),
                                                    });
                                                }
                                            }
                                        } else {
                                            send(ClientEvent::JobBlocked {
                                                job_id: candidate.job_id.clone(),
                                                blocked_by: blocked_by_jid.clone(),
                                                phase: blocked_phase.clone(),
                                            });
                                        }
                                    }
                                }
                            }
                        } else {
                            return Err(ClientError::Config(CoreError::CommandFailed(format!(
                                "Local run failed: {}",
                                stderr
                            ))));
                        }
                    } else {
                        completed.insert(unit_id.clone());
                        succeeded_work_units += 1;

                        let unit = work_units.get(&unit_id);
                        if let Some(u) = unit {
                            let is_scatter = u.job.stage_type
                                == repx_core::model::StageType::ScatterGather
                                && unit_id == WorkUnitId::scatter(&u.job_id);
                            if is_scatter {
                                let image_tag = resolve_image_tag(&u.job_id, client);
                                let execution_type = resolve_local_execution_type(
                                    image_tag,
                                    options,
                                    target.as_ref(),
                                );
                                let expanded = expand_scatter_gather(
                                    &u.job_id,
                                    u.job,
                                    target.as_ref(),
                                    client,
                                    &execution_type,
                                    image_tag,
                                    options,
                                )?;
                                let num_expanded = expanded.len();
                                for (new_id, new_unit) in expanded {
                                    units_left.insert(new_id.clone());
                                    work_units.insert(new_id, new_unit);
                                }
                                total_work_units += num_expanded;
                            }
                        }

                        let reported_via_completion_map =
                            completion_map.iter().any(|(jid, comp_uid)| {
                                comp_uid == &unit_id && jobs_in_batch.contains_key(jid)
                            });
                        if reported_via_completion_map {
                            for (jid, comp_uid) in &completion_map {
                                if comp_uid == &unit_id && jobs_in_batch.contains_key(jid) {
                                    send(ClientEvent::JobSucceeded {
                                        job_id: jid.clone(),
                                        phase: phase.clone(),
                                        wall_time,
                                    });
                                }
                            }
                        } else if let Some(u) = work_units.get(&unit_id) {
                            send(ClientEvent::JobSucceeded {
                                job_id: u.job_id.clone(),
                                phase: phase.clone(),
                                wall_time,
                            });
                        }
                    }
                }
                Err(e) => {
                    if options.continue_on_failure {
                        let phase = unit_id.phase();
                        let wall_time = Some(started_at.elapsed());
                        failed_units.push((unit_id.clone(), format!("Process panicked: {:?}", e)));

                        let reported_via_completion_map =
                            completion_map.iter().any(|(jid, comp_uid)| {
                                comp_uid == &unit_id && jobs_in_batch.contains_key(jid)
                            });
                        if reported_via_completion_map {
                            for (jid, comp_uid) in &completion_map {
                                if comp_uid == &unit_id && jobs_in_batch.contains_key(jid) {
                                    send(ClientEvent::JobFailed {
                                        job_id: jid.clone(),
                                        phase: phase.clone(),
                                        wall_time,
                                    });
                                }
                            }
                        } else if let Some(u) = work_units.get(&unit_id) {
                            send(ClientEvent::JobFailed {
                                job_id: u.job_id.clone(),
                                phase: phase.clone(),
                                wall_time,
                            });
                        }
                    } else {
                        return Err(ClientError::Config(CoreError::CommandFailed(format!(
                            "Process panicked: {:?}",
                            e
                        ))));
                    }
                }
            }
        }

        if units_left.is_empty() && active_handles.is_empty() {
            break;
        }

        for blocked_id in &blocked_units {
            units_left.remove(blocked_id);
        }
        blocked_units.clear();

        if any_finished {
            let failed_count = failed_units.len();
            let running_count = active_handles.len();
            let blocked_count = total_work_units.saturating_sub(
                units_left.len() + running_count + succeeded_work_units + failed_count,
            );
            let pending_count = units_left.len();

            send(ClientEvent::LocalProgress {
                running: running_count,
                succeeded: succeeded_work_units,
                failed: failed_count,
                blocked: blocked_count,
                pending: pending_count,
                total: total_work_units,
            });
        }

        let slots_available = concurrency.saturating_sub(active_handles.len());
        if slots_available > 0 && !units_left.is_empty() {
            let failed_ids: HashSet<&WorkUnitId> = failed_units.iter().map(|(id, _)| id).collect();

            let mut ready: Vec<WorkUnitId> = units_left
                .iter()
                .filter(|uid| match work_units.get(uid) {
                    Some(unit) => {
                        let deps_met = unit.deps.iter().all(|d| completed.contains(d));
                        let no_failed = unit.deps.iter().all(|d| !failed_ids.contains(d));
                        deps_met && no_failed
                    }
                    None => false,
                })
                .cloned()
                .collect();

            ready.sort();

            if ready.is_empty() && active_handles.is_empty() {
                if !failed_units.is_empty() {
                    break;
                }
                return Err(ClientError::Config(CoreError::CycleDetected {
                    context: "dependency graph or missing dependency".to_string(),
                }));
            }

            let mut spawned = 0;
            for uid in ready {
                if spawned >= slots_available {
                    break;
                }
                let unit = match work_units.get(&uid) {
                    Some(u) => u,
                    None => continue,
                };

                if !resource_tracker.can_fit(&uid, unit.mem_bytes, unit.cpus) {
                    tracing::debug!(
                        "Unit '{}' waiting for resources ({} RAM, {} CPUs needed)",
                        uid.short_id(),
                        format_bytes(unit.mem_bytes),
                        unit.cpus
                    );
                    continue;
                }

                units_left.remove(&uid);
                resource_tracker.reserve(uid.clone(), unit.mem_bytes, unit.cpus);

                let child = target.spawn_repx_job(repx_binary_path, &unit.extra_args)?;
                submitted_count += 1;
                spawned += 1;

                let pid = child.id();
                send(ClientEvent::JobStarted {
                    job_id: unit.job_id.clone(),
                    pid,
                    total: total_work_units,
                    current: submitted_count,
                    phase: uid.phase(),
                });

                let child_handle = Arc::new(Mutex::new(Some(child)));
                let child_handle_clone = child_handle.clone();
                let handle = thread::spawn(move || {
                    let guard_result = child_handle_clone.lock();
                    match guard_result {
                        Ok(mut guard) => {
                            if let Some(child) = guard.take() {
                                child.wait_with_output()
                            } else {
                                Err(std::io::Error::new(
                                    std::io::ErrorKind::Interrupted,
                                    "Process was killed",
                                ))
                            }
                        }
                        Err(_) => Err(std::io::Error::other("Mutex was poisoned")),
                    }
                });
                active_handles.push((uid, child_handle, handle, Instant::now()));
            }
        }

        if !active_handles.is_empty() {
            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }
    }

    if !failed_units.is_empty() {
        let num_failed = failed_units.len();
        let mut error_msg = format!("{} unit(s) failed:\n", num_failed);
        for (uid, stderr) in &failed_units {
            error_msg.push_str(&format!("\n=== {} ===\n{}\n", uid, stderr));
        }
        return Err(ClientError::Config(CoreError::CommandFailed(error_msg)));
    }

    Ok(format!(
        "Successfully executed {} work unit(s) locally.",
        succeeded_work_units
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mem_to_bytes() {
        use repx_core::model::Memory;
        assert_eq!(Memory::from("1G").to_bytes(), Some(1024 * 1024 * 1024));
        assert_eq!(Memory::from("512M").to_bytes(), Some(512 * 1024 * 1024));
        assert_eq!(
            Memory::from("2T").to_bytes(),
            Some(2 * 1024 * 1024 * 1024 * 1024)
        );
        assert_eq!(Memory::from("1024K").to_bytes(), Some(1024 * 1024));
        assert_eq!(Memory::from("4096").to_bytes(), Some(4096));

        assert_eq!(Memory::from("8g").to_bytes(), Some(8 * 1024 * 1024 * 1024));
        assert_eq!(Memory::from("256m").to_bytes(), Some(256 * 1024 * 1024));

        assert_eq!(
            Memory::from("  4G  ").to_bytes(),
            Some(4 * 1024 * 1024 * 1024)
        );

        assert_eq!(Memory::from("invalid").to_bytes(), None);
        assert_eq!(Memory::from("G").to_bytes(), None);
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

        let u1 = WorkUnitId::from_job(&JobId::from("job1"));
        let u2 = WorkUnitId::from_job(&JobId::from("job2"));
        let u3 = WorkUnitId::from_job(&JobId::from("job3"));

        assert!(tracker.can_fit(&u1, 8 * 1024 * 1024 * 1024, 4));

        tracker.reserve(u1.clone(), 8 * 1024 * 1024 * 1024, 4);

        assert!(tracker.can_fit(&u2, 4 * 1024 * 1024 * 1024, 2));
        tracker.reserve(u2.clone(), 4 * 1024 * 1024 * 1024, 2);

        assert!(!tracker.can_fit(&u3, 8 * 1024 * 1024 * 1024, 4));

        assert!(tracker.can_fit(&u3, 2 * 1024 * 1024 * 1024, 1));

        assert!(!tracker.can_fit(&u3, 6 * 1024 * 1024 * 1024, 1));

        assert!(!tracker.can_fit(&u3, 2 * 1024 * 1024 * 1024, 4));
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

        let u1 = WorkUnitId::from_job(&JobId::from("job1"));
        let u2 = WorkUnitId::from_job(&JobId::from("job2"));

        tracker.reserve(u1.clone(), 4 * 1024 * 1024 * 1024, 2);
        assert_eq!(tracker.used_mem_bytes, 4 * 1024 * 1024 * 1024);
        assert_eq!(tracker.used_cpus, 2);
        assert_eq!(tracker.in_flight.len(), 1);

        tracker.reserve(u2.clone(), 8 * 1024 * 1024 * 1024, 4);
        assert_eq!(tracker.used_mem_bytes, 12 * 1024 * 1024 * 1024);
        assert_eq!(tracker.used_cpus, 6);
        assert_eq!(tracker.in_flight.len(), 2);

        tracker.release(&u1);
        assert_eq!(tracker.used_mem_bytes, 8 * 1024 * 1024 * 1024);
        assert_eq!(tracker.used_cpus, 4);
        assert_eq!(tracker.in_flight.len(), 1);

        tracker.release(&u2);
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

        let big = WorkUnitId::from_job(&JobId::from("big_job"));

        assert!(tracker.can_fit(&big, 32 * 1024 * 1024 * 1024, 16));
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

        let small = WorkUnitId::from_job(&JobId::from("small_job"));
        let big = WorkUnitId::from_job(&JobId::from("big_job"));

        tracker.reserve(small.clone(), 1024 * 1024 * 1024, 1);

        assert!(!tracker.can_fit(&big, 32 * 1024 * 1024 * 1024, 16));
    }

    #[test]
    fn test_resource_tracker_release_unknown_is_safe() {
        let mut tracker = ResourceTracker {
            total_mem_bytes: 8 * 1024 * 1024 * 1024,
            total_cpus: 4,
            used_mem_bytes: 4 * 1024 * 1024 * 1024,
            used_cpus: 2,
            in_flight: HashMap::new(),
        };

        let unknown = WorkUnitId::from_job(&JobId::from("unknown"));

        tracker.release(&unknown);

        assert_eq!(tracker.used_mem_bytes, 4 * 1024 * 1024 * 1024);
        assert_eq!(tracker.used_cpus, 2);
    }
}

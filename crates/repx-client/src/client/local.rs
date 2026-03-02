use super::{Client, ClientEvent, SubmitOptions};
use crate::error::{ClientError, Result};
use crate::resources;
use crate::targets::Target;
use num_cpus;
use repx_core::{
    constants::dirs,
    engine,
    errors::ConfigError,
    model::{Job, JobId},
};
use serde_json::Value;
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
        return Err(format!(
            "Expected exactly one sink step but found {}: {:?}",
            sink_candidates.len(),
            sink_candidates
        ));
    }
    let sink_step = sink_candidates[0].clone();

    let sink_key = format!("step-{}", sink_step);
    let sink_exe = job
        .executables
        .get(&sink_key)
        .ok_or_else(|| format!("Sink step executable '{}' not found in job", sink_key))?;
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
    in_flight: HashMap<WorkUnitId, (u64, u32)>,
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
        Self(id.0.clone())
    }
    fn scatter(job_id: &JobId) -> Self {
        Self(format!("{}::scatter", job_id.0))
    }
    fn step(job_id: &JobId, branch: usize, step: &str) -> Self {
        Self(format!("{}::b{}::s-{}", job_id.0, branch, step))
    }
    fn gather(job_id: &JobId) -> Self {
        Self(format!("{}::gather", job_id.0))
    }
    fn short_id(&self) -> &str {
        if self.0.len() > 40 {
            &self.0[..40]
        } else {
            &self.0
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
) -> std::result::Result<Vec<String>, ClientError> {
    let artifacts_base = target.artifacts_base_path();
    let scatter_exe = job.executables.get("scatter").ok_or_else(|| {
        ClientError::Config(ConfigError::General(format!(
            "Scatter-gather job '{}' missing required 'scatter' executable",
            job_id
        )))
    })?;
    let gather_exe = job.executables.get("gather").ok_or_else(|| {
        ClientError::Config(ConfigError::General(format!(
            "Scatter-gather job '{}' missing required 'gather' executable",
            job_id
        )))
    })?;
    let job_package_path = artifacts_base.join(format!("jobs/{}", job_id));
    let scatter_exe_path = artifacts_base.join(&scatter_exe.path);
    let gather_exe_path = artifacts_base.join(&gather_exe.path);

    let (steps_json, last_step_outputs_json) = build_steps_json(job, &artifacts_base)
        .map_err(|e| ClientError::Config(ConfigError::General(e)))?;

    let mut args = vec![
        "internal-scatter-gather".to_string(),
        "--job-id".to_string(),
        job_id.0.clone(),
        "--runtime".to_string(),
        execution_type.to_string(),
    ];
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
    if target.config().mount_host_paths {
        args.push("--mount-host-paths".to_string());
    } else {
        for path in &target.config().mount_paths {
            args.push("--mount-paths".to_string());
            args.push(path.clone());
        }
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
        "local".to_string(),
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
) -> std::result::Result<Vec<String>, ClientError> {
    let main_exe = job.executables.get("main").ok_or_else(|| {
        ClientError::Config(ConfigError::General(format!(
            "Job '{}' missing required 'main' executable",
            job_id
        )))
    })?;
    let executable_path = target.artifacts_base_path().join(&main_exe.path);

    let mut args = vec![
        "internal-execute".to_string(),
        "--job-id".to_string(),
        job_id.0.clone(),
        "--runtime".to_string(),
        execution_type.to_string(),
    ];
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
    if target.config().mount_host_paths {
        args.push("--mount-host-paths".to_string());
    } else {
        for path in &target.config().mount_paths {
            args.push("--mount-paths".to_string());
            args.push(path.clone());
        }
    }
    args.extend_from_slice(&[
        "--executable-path".to_string(),
        executable_path.to_string_lossy().to_string(),
    ]);
    Ok(args)
}

fn resolve_execution_type<'a>(
    image_tag: Option<&str>,
    options: &'a SubmitOptions,
    target: &'a dyn Target,
) -> &'a str {
    if options.execution_type.is_none() && image_tag.is_none() {
        return "native";
    }
    options.execution_type.as_deref().unwrap_or_else(|| {
        let scheduler_config = match target.config().local.as_ref() {
            Some(cfg) => cfg,
            None => return "native",
        };
        target
            .config()
            .default_execution_type
            .as_deref()
            .filter(|&et| scheduler_config.execution_types.contains(&et.to_string()))
            .or_else(|| scheduler_config.execution_types.first().map(|s| s.as_str()))
            .unwrap_or("native")
    })
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
        .and_then(|m| parse_mem_to_bytes(m))
        .unwrap_or(DEFAULT_JOB_MEM_BYTES);
    let cpus = directives.cpus_per_task.unwrap_or(DEFAULT_JOB_CPUS);
    (mem, cpus)
}

fn expand_scatter_gather<'a>(
    job_id: &JobId,
    job: &'a Job,
    target: &dyn Target,
    client: &Client,
    execution_type: &str,
    image_tag: Option<&str>,
    resources_config: &Option<repx_core::config::Resources>,
) -> std::result::Result<Vec<(WorkUnitId, WorkUnit<'a>)>, ClientError> {
    let base_path = target.base_path();
    let job_root = base_path.join(dirs::OUTPUTS).join(&job_id.0);
    let work_items_path = job_root.join("scatter").join("out").join("work_items.json");
    let work_items_str = target.read_remote_file(&work_items_path).map_err(|e| {
        ClientError::Config(ConfigError::General(format!(
            "Failed to read work_items.json after scatter for '{}': {}",
            job_id, e
        )))
    })?;
    let work_items: Vec<Value> = serde_json::from_str(&work_items_str).map_err(|e| {
        ClientError::Config(ConfigError::General(format!(
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
            ClientError::Config(ConfigError::General(
                "No sink step found in scatter-gather step DAG".into(),
            ))
        })?
        .to_string();

    let scatter_id = WorkUnitId::scatter(job_id);

    let sg_common = build_sg_common_args(job_id, job, target, client, execution_type, image_tag)?;

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

            let (mem, cpus) = get_step_resources(job, &exe_key, resources_config, job_id);

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
    let gather_mem = get_job_mem_bytes(job, resources_config);
    let gather_cpus = get_job_cpus(job, resources_config);

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
    send(ClientEvent::SubmittingJobs { total: total_jobs });

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
    let concurrency = options.num_jobs.unwrap_or_else(num_cpus::get);

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
        let execution_type = resolve_execution_type(image_tag, options, target.as_ref());

        let entrypoint_exe = job
            .executables
            .get("main")
            .or_else(|| job.executables.get("scatter"))
            .ok_or_else(|| {
                ClientError::Config(ConfigError::General(format!(
                    "Job '{}' missing required executable 'main' or 'scatter'",
                    job_id
                )))
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
                execution_type,
                image_tag,
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
                execution_type,
                image_tag,
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

    let mut resource_tracker = ResourceTracker::new();
    let mut active_handles: Vec<(
        WorkUnitId,
        std::thread::JoinHandle<std::io::Result<std::process::Output>>,
    )> = vec![];
    let mut failed_units: Vec<(WorkUnitId, String)> = vec![];
    let mut blocked_units: HashSet<WorkUnitId> = HashSet::new();
    let mut submitted_count: usize = 0;

    loop {
        let mut finished_indices = Vec::new();
        for (i, (_id, handle)) in active_handles.iter().enumerate() {
            if handle.is_finished() {
                finished_indices.push(i);
            }
        }
        let any_finished = !finished_indices.is_empty();

        for i in finished_indices.into_iter().rev() {
            let (unit_id, handle) = active_handles.remove(i);
            resource_tracker.release(&unit_id);

            match handle.join() {
                Ok(output_res) => {
                    let output = output_res.map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                        if options.continue_on_failure {
                            failed_units.push((unit_id.clone(), stderr));
                            for (candidate_id, candidate) in &work_units {
                                if units_left.contains(candidate_id)
                                    && candidate.deps.contains(&unit_id)
                                {
                                    blocked_units.insert(candidate_id.clone());
                                }
                            }
                        } else {
                            return Err(ClientError::Config(ConfigError::General(format!(
                                "Local run failed: {}",
                                stderr
                            ))));
                        }
                    } else {
                        completed.insert(unit_id.clone());

                        let unit = work_units.get(&unit_id);
                        if let Some(u) = unit {
                            let is_scatter = u.job.stage_type
                                == repx_core::model::StageType::ScatterGather
                                && unit_id == WorkUnitId::scatter(&u.job_id);
                            if is_scatter {
                                let image_tag = resolve_image_tag(&u.job_id, client);
                                let execution_type =
                                    resolve_execution_type(image_tag, options, target.as_ref());
                                let expanded = expand_scatter_gather(
                                    &u.job_id,
                                    u.job,
                                    target.as_ref(),
                                    client,
                                    execution_type,
                                    image_tag,
                                    &options.resources,
                                )?;
                                for (new_id, new_unit) in expanded {
                                    units_left.insert(new_id.clone());
                                    work_units.insert(new_id, new_unit);
                                }
                            }
                        }

                        for (jid, comp_uid) in &completion_map {
                            if comp_uid == &unit_id && jobs_in_batch.contains_key(jid) {
                                send(ClientEvent::JobSucceeded {
                                    job_id: jid.clone(),
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    if options.continue_on_failure {
                        failed_units.push((unit_id, format!("Process panicked: {:?}", e)));
                    } else {
                        return Err(ClientError::Config(ConfigError::General(format!(
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
            let succeeded_count = completed_job_ids
                .iter()
                .chain(
                    completion_map
                        .iter()
                        .filter(|(_, uid)| completed.contains(uid))
                        .map(|(jid, _)| jid),
                )
                .filter(|id| jobs_in_batch.contains_key(id))
                .collect::<HashSet<_>>()
                .len();
            let failed_count = failed_units.len();
            let running_count = active_handles.len();
            let pending_count =
                total_jobs.saturating_sub(succeeded_count + failed_count + running_count);
            let blocked_count = total_jobs
                .saturating_sub(succeeded_count + failed_count + running_count + pending_count);

            send(ClientEvent::LocalProgress {
                running: running_count,
                succeeded: succeeded_count,
                failed: failed_count,
                blocked: blocked_count,
                pending: pending_count,
                total: total_jobs,
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
                return Err(ClientError::Config(ConfigError::General(
                    "Cycle detected in dependency graph or missing dependency.".to_string(),
                )));
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
                    total: total_jobs,
                    current: submitted_count,
                });

                let handle = thread::spawn(move || child.wait_with_output());
                active_handles.push((uid, handle));
            }
        }

        if !active_handles.is_empty() {
            thread::sleep(Duration::from_millis(50));
        }
    }

    if !failed_units.is_empty() {
        let num_failed = failed_units.len();
        let mut error_msg = format!("{} unit(s) failed:\n", num_failed);
        for (uid, stderr) in &failed_units {
            error_msg.push_str(&format!("\n=== {} ===\n{}\n", uid, stderr));
        }
        return Err(ClientError::Config(ConfigError::General(error_msg)));
    }

    Ok(format!(
        "Successfully executed {} jobs locally.",
        total_jobs
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

        let u1 = WorkUnitId::from_job(&JobId("job1".into()));
        let u2 = WorkUnitId::from_job(&JobId("job2".into()));
        let u3 = WorkUnitId::from_job(&JobId("job3".into()));

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

        let u1 = WorkUnitId::from_job(&JobId("job1".into()));
        let u2 = WorkUnitId::from_job(&JobId("job2".into()));

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

        let big = WorkUnitId::from_job(&JobId("big_job".into()));

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

        let small = WorkUnitId::from_job(&JobId("small_job".into()));
        let big = WorkUnitId::from_job(&JobId("big_job".into()));

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

        let unknown = WorkUnitId::from_job(&JobId("unknown".into()));

        tracker.release(&unknown);

        assert_eq!(tracker.used_mem_bytes, 4 * 1024 * 1024 * 1024);
        assert_eq!(tracker.used_cpus, 2);
    }
}

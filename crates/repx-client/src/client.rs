use crate::resources::SbatchDirectives;
use crate::targets::SlurmState;
use crate::{
    error::{ClientError, Result},
    orchestration::OrchestrationPlan,
    resources,
    targets::{local::LocalTarget, ssh::SshTarget, Target},
};
use fs_err;
use num_cpus;
use repx_core::{
    config::{Config, Resources},
    engine,
    error::AppError,
    lab, log_debug, log_info,
    model::{Job, JobId, Lab, RunId},
};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command};
use std::str::FromStr;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{mpsc::Sender, Arc, Mutex},
};
use xdg;

#[derive(Debug)]
pub enum ClientEvent {
    DeployingBinary,
    GeneratingSlurmScripts {
        num_jobs: usize,
    },
    ExecutingOrchestrator,
    SyncingArtifacts {
        total: u64,
    },
    SyncingArtifactProgress {
        path: PathBuf,
    },
    SyncingFinished,
    SubmittingJobs {
        total: usize,
    },
    JobSubmitted {
        job_id: JobId,
        slurm_id: u32,
        total: usize,
        current: usize,
    },

    JobStarted {
        job_id: JobId,
        pid: u32,
        total: usize,
        current: usize,
    },
    WaveCompleted {
        wave: usize,
        num_jobs: usize,
    },
}

type SlurmIdMap = Arc<Mutex<HashMap<JobId, (String, u32)>>>;

#[derive(Clone)]
pub struct Client {
    config: Arc<Config>,
    lab_path: Arc<PathBuf>,
    lab: Arc<Lab>,
    targets: Arc<HashMap<String, Arc<dyn Target>>>,
    slurm_map: SlurmIdMap,
}

fn generate_repx_invoker_script(
    job_id: &JobId,
    job_root_on_target: &PathBuf,
    directives: &SbatchDirectives,
    repx_command_to_wrap: String,
) -> Result<String> {
    let mut s = String::from("#!/usr/bin/env bash\n");
    s.push_str(&format!("#SBATCH --job-name={}\n", job_id.0));
    s.push_str(&format!(
        "#SBATCH --chdir={}\n",
        job_root_on_target.display()
    ));
    let log_path = job_root_on_target.join("repx").join("slurm-%j.out");
    s.push_str(&format!("#SBATCH --output={}\n", log_path.display()));
    s.push_str(&format!("#SBATCH --error={}\n", log_path.display()));

    if let Some(p) = &directives.partition {
        s.push_str(&format!("#SBATCH --partition={}\n", p));
    }
    if let Some(c) = directives.cpus_per_task {
        s.push_str(&format!("#SBATCH --cpus-per-task={}\n", c));
    }
    if let Some(m) = &directives.mem {
        s.push_str(&format!("#SBATCH --mem={}\n", m));
    }
    if let Some(t) = &directives.time {
        s.push_str(&format!("#SBATCH --time={}\n", t));
    }
    for opt in &directives.sbatch_opts {
        s.push_str(&format!("#SBATCH {}\n", opt));
    }

    s.push_str("\nset -e\n\n");
    s.push_str("# This script invokes the repx binary to handle execution.\n");
    s.push_str(&repx_command_to_wrap);
    s.push('\n');

    Ok(s)
}

impl Client {
    pub fn new(config: Config, lab_path: PathBuf) -> Result<Self> {
        let mut targets: HashMap<String, Arc<dyn Target>> = HashMap::new();
        for (name, target_config) in &config.targets {
            let target: Arc<dyn Target> = if name == "local" {
                Arc::new(LocalTarget {
                    name: name.clone(),
                    config: target_config.clone(),
                })
            } else if let Some(address) = &target_config.address {
                Arc::new(SshTarget {
                    name: name.clone(),
                    address: address.clone(),
                    config: target_config.clone(),
                })
            } else {
                return Err(ClientError::Core(AppError::ConfigurationError(format!(
                    "Target '{}' is not 'local' and has no 'address' specified.",
                    name
                ))));
            };
            targets.insert(name.clone(), target);
        }

        let lab = lab::load_from_path(&lab_path).map_err(ClientError::Core)?;

        let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
        let state_home = xdg_dirs.get_state_home().ok_or_else(|| {
            AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not find state home directory",
            ))
        })?;
        let map_path = state_home.join("slurm_map.json");
        let slurm_map_data = fs_err::read_to_string(map_path)
            .ok()
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        Ok(Self {
            config: Arc::new(config),
            lab_path: Arc::new(lab_path),
            lab: Arc::new(lab),
            targets: Arc::new(targets),
            slurm_map: Arc::new(Mutex::new(slurm_map_data)),
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn lab(&self) -> Result<&Lab> {
        Ok(&self.lab)
    }

    pub fn lab_path(&self) -> &Path {
        &self.lab_path
    }

    fn save_slurm_map(&self) -> Result<()> {
        let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
        let map_path = xdg_dirs
            .place_state_file("slurm_map.json")
            .map_err(AppError::from)?;
        let data = self.slurm_map.lock().unwrap();
        let json_string = serde_json::to_string_pretty(&*data).map_err(AppError::from)?;
        fs_err::write(map_path, json_string).map_err(AppError::from)?;
        Ok(())
    }

    pub fn get_statuses(
        &self,
    ) -> Result<(
        BTreeMap<RunId, engine::JobStatus>,
        HashMap<JobId, engine::JobStatus>,
    )> {
        let mut all_outcomes = HashMap::new();
        for target in self.targets.values() {
            let outcomes = target.check_outcome_markers()?;
            all_outcomes.extend(outcomes);
        }

        let mut slurm_map_guard = self.slurm_map.lock().unwrap();
        let mut map_was_changed = false;
        slurm_map_guard.retain(|job_id, _| {
            let is_done = matches!(
                all_outcomes.get(job_id),
                Some(engine::JobStatus::Succeeded { .. }) | Some(engine::JobStatus::Failed { .. })
            );
            if is_done {
                map_was_changed = true;
            }
            !is_done
        });

        drop(slurm_map_guard);

        if map_was_changed {
            self.save_slurm_map()?;
        }

        let mut job_statuses = all_outcomes;

        for target in self.targets.values() {
            if target.config().slurm.is_some() {
                let queued_jobs = target.squeue()?;
                for (job_id, squeue_info) in queued_jobs {
                    job_statuses.entry(job_id).or_insert(
                        if squeue_info.state == SlurmState::Running {
                            engine::JobStatus::Running
                        } else {
                            engine::JobStatus::Queued
                        },
                    );
                }
            }
        }

        let final_statuses = engine::determine_job_statuses(&self.lab, &job_statuses);
        let run_statuses = engine::determine_run_aggregate_statuses(&self.lab, &final_statuses);

        Ok((run_statuses, final_statuses))
    }

    pub fn get_statuses_for_active_target(
        &self,
        active_target_name: &str,
    ) -> Result<HashMap<JobId, engine::JobStatus>> {
        let mut job_statuses = HashMap::new();
        let target = self
            .targets
            .get(active_target_name)
            .ok_or_else(|| ClientError::TargetNotFound(active_target_name.to_string()))?;

        let outcomes = target.check_outcome_markers()?;
        job_statuses.extend(outcomes.clone());

        let mut slurm_map_guard = self.slurm_map.lock().unwrap();
        let mut map_was_changed = false;
        slurm_map_guard.retain(|job_id, (target_name, _slurm_id)| {
            if target_name != active_target_name {
                return true;
            }
            let is_done = matches!(
                outcomes.get(job_id),
                Some(engine::JobStatus::Succeeded { .. }) | Some(engine::JobStatus::Failed { .. })
            );
            if is_done {
                map_was_changed = true;
            }
            !is_done
        });

        drop(slurm_map_guard);

        if map_was_changed {
            self.save_slurm_map()?;
        }

        if target.config().slurm.is_some() {
            let queued_jobs = target.squeue()?;
            for (job_id, squeue_info) in queued_jobs {
                job_statuses
                    .entry(job_id)
                    .or_insert(if squeue_info.state == SlurmState::Running {
                        engine::JobStatus::Running
                    } else {
                        engine::JobStatus::Queued
                    });
            }
        }

        Ok(job_statuses)
    }

    pub fn submit_run(
        &self,
        run_spec: String,
        target_name: &str,
        scheduler: &str,
        execution_type: Option<&str>,
        resources: Option<Resources>,
        num_jobs: Option<usize>,
        event_sender: Option<Sender<ClientEvent>>,
    ) -> Result<String> {
        self.submit_batch_run(
            vec![run_spec],
            target_name,
            scheduler,
            execution_type,
            resources,
            num_jobs,
            event_sender,
        )
    }

    pub fn submit_batch_run(
        &self,
        run_specs: Vec<String>,
        target_name: &str,
        scheduler: &str,
        execution_type: Option<&str>,
        resources: Option<Resources>,
        num_jobs: Option<usize>,
        event_sender: Option<Sender<ClientEvent>>,
    ) -> Result<String> {
        let send = |event: ClientEvent| {
            if let Some(sender) = &event_sender {
                let _ = sender.send(event);
            }
        };

        let target = self
            .targets
            .get(target_name)
            .ok_or_else(|| ClientError::TargetNotFound(target_name.to_string()))?;

        let mut full_dependency_set = HashSet::new();
        for spec in &run_specs {
            let run_id = RunId(spec.clone());
            let final_job_ids = repx_core::resolver::resolve_all_final_job_ids(&self.lab, &run_id)?;
            for final_job_id in final_job_ids {
                let graph = engine::build_dependency_graph(&self.lab, final_job_id);
                full_dependency_set.extend(graph);
            }
        }

        if full_dependency_set.is_empty() {
            return Ok(
                "All selected jobs are already complete or no jobs were specified.".to_string(),
            );
        }

        let raw_statuses = self.get_statuses_for_active_target(target_name)?;
        let job_statuses = engine::determine_job_statuses(&self.lab, &raw_statuses);
        let jobs_to_run: HashMap<JobId, &Job> = full_dependency_set
            .into_iter()
            .filter(|job_id| {
                !matches!(
                    job_statuses.get(job_id),
                    Some(engine::JobStatus::Succeeded { .. })
                )
            })
            .map(|job_id| (job_id.clone(), self.lab.jobs.get(&job_id).unwrap()))
            .collect();

        if jobs_to_run.is_empty() {
            return Ok("All required jobs for this submission are already complete.".to_string());
        }
        let jobs_to_run_ids: std::collections::HashSet<JobId> =
            jobs_to_run.keys().cloned().collect();

        let jobs_to_submit: HashMap<JobId, &Job> = jobs_to_run
            .iter()
            .filter(|(_job_id, job)| {
                let entrypoint_exe = job
                    .executables
                    .get("main")
                    .or_else(|| job.executables.get("scatter"))
                    .ok_or_else(|| {
                        AppError::ConfigurationError(format!(
                            "Job '{}' has no 'main' or 'scatter' executable defined.",
                            _job_id
                        ))
                    })
                    .unwrap();

                let has_deps_in_batch = entrypoint_exe
                    .inputs
                    .iter()
                    .filter_map(|m| m.job_id.as_ref())
                    .any(|job_id| jobs_to_run_ids.contains(job_id));
                !has_deps_in_batch
            })
            .map(|(id, job)| (id.clone(), *job))
            .collect();

        if jobs_to_submit.is_empty() && scheduler == "slurm" {
            return Ok(
                "All schedulable jobs for this submission are already complete.".to_string(),
            );
        }

        for (job_id, job) in &jobs_to_run {
            if job.stage_type == "scatter-gather" {
                Self::generate_and_write_inputs_json(&self.lab, job, job_id, &target, "scatter")?;
            } else {
                Self::generate_and_write_inputs_json(&self.lab, job, job_id, &target, "main")?;
            }
        }

        send(ClientEvent::DeployingBinary);
        let remote_repx_binary_path = target.deploy_repx_binary()?;
        log_info!(
            "repx binary deployed to: {}",
            remote_repx_binary_path.display()
        );

        send(ClientEvent::SyncingArtifacts { total: 1 });
        target.sync_lab_root(&self.lab_path)?;
        send(ClientEvent::SyncingFinished);

        match scheduler {
            "slurm" => self.submit_slurm_batch_run(
                jobs_to_submit,
                target.clone(),
                target_name,
                &remote_repx_binary_path,
                execution_type,
                resources,
                send,
            ),
            "local" => {
                let num_jobs = num_jobs.unwrap_or_else(num_cpus::get);
                self.submit_local_batch_run(
                    jobs_to_run,
                    target.clone(),
                    target_name,
                    &remote_repx_binary_path,
                    execution_type,
                    resources,
                    num_jobs,
                    send,
                )
            }
            _ => Err(ClientError::Core(AppError::ConfigurationError(format!(
                "Unsupported scheduler: '{}'. Must be 'slurm' or 'local'.",
                scheduler
            )))),
        }
    }

    fn submit_slurm_batch_run(
        &self,
        jobs_to_submit: HashMap<JobId, &Job>,
        target: Arc<dyn Target>,
        target_name: &str,
        remote_repx_binary_path: &Path,
        execution_type_override: Option<&str>,
        resources: Option<Resources>,
        send: impl Fn(ClientEvent),
    ) -> Result<String> {
        let remote_repx_command = remote_repx_binary_path.to_string_lossy();
        send(ClientEvent::GeneratingSlurmScripts {
            num_jobs: jobs_to_submit.len(),
        });
        let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
        let cache_home = xdg_dirs.get_cache_home().ok_or_else(|| {
            ClientError::Core(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not find cache home directory",
            )))
        })?;
        let local_batch_dir = cache_home.join("submissions").join(&self.lab.content_hash);
        fs_err::create_dir_all(&local_batch_dir).map_err(AppError::from)?;

        let mut plan = OrchestrationPlan::new(target.base_path(), &self.lab.content_hash);

        for (job_id, job) in &jobs_to_submit {
            let job_root_on_target = target.base_path().join("outputs").join(&job_id.0);

            let execution_type = execution_type_override.unwrap_or_else(|| {
                let scheduler_config = target.config().slurm.as_ref().unwrap();
                target
                    .config()
                    .default_execution_type
                    .as_deref()
                    .filter(|&et| scheduler_config.execution_types.contains(&et.to_string()))
                    .or_else(|| scheduler_config.execution_types.first().map(|s| s.as_str()))
                    .unwrap_or("native")
            });
            let image_path_opt = self
                .lab
                .runs
                .values()
                .find(|r| r.jobs.contains(job_id))
                .and_then(|r| r.image.as_deref());
            let image_tag = image_path_opt
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str());

            let repx_args = format!(
                "--job-id {} --runtime {} {} --base-path {}",
                job_id,
                execution_type,
                image_tag
                    .map(|t| format!("--image-tag {}", t))
                    .unwrap_or_default(),
                target.base_path().display()
            );

            let (repx_command_to_wrap, directives) = if job.stage_type == "scatter-gather" {
                let scatter_exe = job.executables.get("scatter").ok_or_else(|| {
                    AppError::ConfigurationError(
                        "Scatter-gather job missing 'scatter' executable".into(),
                    )
                })?;
                let worker_exe = job.executables.get("worker").ok_or_else(|| {
                    AppError::ConfigurationError(
                        "Scatter-gather job missing 'worker' executable".into(),
                    )
                })?;
                let gather_exe = job.executables.get("gather").ok_or_else(|| {
                    AppError::ConfigurationError(
                        "Scatter-gather job missing 'gather' executable".into(),
                    )
                })?;

                let artifacts_base = target.artifacts_base_path();
                let scatter_exe_path = artifacts_base.join(&scatter_exe.path);
                let worker_exe_path = artifacts_base.join(&worker_exe.path);
                let gather_exe_path = artifacts_base.join(&gather_exe.path);

                let worker_outputs_json =
                    serde_json::to_string(&worker_exe.outputs).map_err(AppError::from)?;

                let scatter_gather_args = format!(
                    "--job-package-path {} --scatter-exe-path {} --worker-exe-path {} --gather-exe-path {} --worker-outputs-json '{}'",
                    target.artifacts_base_path().join(format!("jobs/{}", job_id)).display(),
                    scatter_exe_path.display(),
                    worker_exe_path.display(),
                    gather_exe_path.display(),
                    worker_outputs_json
                );

                let main_directives = resources::resolve_for_job(job_id, target_name, &resources);
                let worker_directives =
                    resources::resolve_worker_resources(job_id, target_name, &resources);
                let worker_opts_str = worker_directives.to_shell_string();

                let command = format!(
                    "{} internal-scatter-gather {} {} --worker-sbatch-opts '{}' --scheduler slurm",
                    remote_repx_command, repx_args, scatter_gather_args, worker_opts_str
                );
                (command, main_directives)
            } else {
                let main_exe = job.executables.get("main").ok_or_else(|| {
                    AppError::ConfigurationError(format!(
                        "Simple job '{}' missing 'main' executable",
                        job_id
                    ))
                })?;
                let executable_path_on_target = target.artifacts_base_path().join(&main_exe.path);

                let repx_args = format!(
                    "{} --executable-path {}",
                    repx_args,
                    executable_path_on_target.display()
                );
                let directives = resources::resolve_for_job(job_id, target_name, &resources);
                let command = format!("{} internal-execute {}", remote_repx_command, repx_args);
                (command, directives)
            };

            let script_content = generate_repx_invoker_script(
                job_id,
                &job_root_on_target,
                &directives,
                repx_command_to_wrap,
            )?;

            let mut hasher = Sha256::new();
            hasher.update(&script_content);
            let hash_bytes = hasher.finalize();
            let script_hash = format!("{:x}", hash_bytes);

            let script_path = local_batch_dir.join(format!("{}.sbatch", script_hash));
            let mut file = fs_err::File::create(script_path).map_err(AppError::from)?;
            file.write_all(script_content.as_bytes())
                .map_err(AppError::from)?;

            plan.add_job(job_id.clone(), job, script_hash);
        }

        let plan_filename = "plan.json";
        let plan_content = serde_json::to_string_pretty(&plan).map_err(AppError::from)?;
        fs_err::write(local_batch_dir.join(plan_filename), plan_content).map_err(AppError::from)?;

        send(ClientEvent::ExecutingOrchestrator);
        send(ClientEvent::SubmittingJobs {
            total: jobs_to_submit.len(),
        });

        let submission_dir_on_target = target
            .base_path()
            .join("submissions")
            .join(&self.lab.content_hash);
        target.sync_directory(&local_batch_dir, &submission_dir_on_target)?;

        let orchestrator_command = format!(
            "{} internal-orchestrate {}",
            remote_repx_command,
            submission_dir_on_target.join(plan_filename).display()
        );

        let orchestrator_output = target.run_command("sh", &["-c", &orchestrator_command])?;

        log_debug!(
            "Orchestrator raw output on target '{}':\n---\n{}\n---",
            target.name(),
            orchestrator_output
        );

        let mut submitted_count = 0;
        let total_to_submit = jobs_to_submit.len();
        for line in orchestrator_output.lines() {
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.len() == 2 {
                if let (Ok(repx_id), Ok(slurm_id)) =
                    (JobId::from_str(parts[0]), parts[1].parse::<u32>())
                {
                    self.slurm_map
                        .lock()
                        .unwrap()
                        .insert(repx_id.clone(), (target_name.to_string(), slurm_id));
                    submitted_count += 1;
                    send(ClientEvent::JobSubmitted {
                        job_id: repx_id,
                        slurm_id,
                        total: total_to_submit,
                        current: submitted_count,
                    });
                }
            }
        }

        self.save_slurm_map()?;
        Ok(format!(
            "Successfully submitted {} jobs via SLURM orchestrator.",
            submitted_count
        ))
    }

    fn submit_local_batch_run(
        &self,
        jobs_in_batch: HashMap<JobId, &Job>,
        target: Arc<dyn Target>,
        _target_name: &str,
        repx_binary_path: &Path,
        execution_type_override: Option<&str>,
        _resources: Option<Resources>,
        num_jobs: usize,
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
        let raw_statuses = self.get_statuses_for_active_target(target.name())?;
        let all_job_statuses = engine::determine_job_statuses(&self.lab, &raw_statuses);
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
        let mut wave_num = 0;

        while !jobs_left.is_empty() {
            wave_num += 1;
            let mut current_wave: Vec<JobId> = jobs_left
                .iter()
                .filter(|job_id| {
                    let job = jobs_in_batch.get(job_id).unwrap();
                    let is_schedulable_type =
                        job.stage_type != "worker" && job.stage_type != "gather";
                    let entrypoint_exe = job
                        .executables
                        .get("main")
                        .or_else(|| job.executables.get("scatter"))
                        .unwrap();
                    let deps_are_met = entrypoint_exe
                        .inputs
                        .iter()
                        .filter_map(|m| m.job_id.as_ref())
                        .all(|dep_id| completed_jobs.contains(dep_id));
                    is_schedulable_type && deps_are_met
                })
                .cloned()
                .collect();
            current_wave.sort();

            if current_wave.is_empty() && !jobs_left.is_empty() {
                return Err(ClientError::Core(AppError::ConfigurationError(
                    "Cycle detected in job dependency graph or missing dependency.".to_string(),
                )));
            }

            let wave_job_count = current_wave.len();
            let mut jobs_to_spawn = current_wave.into_iter();
            let mut active_handles: Vec<(JobId, Child)> = vec![];
            let mut finished_jobs_in_wave = 0;

            while finished_jobs_in_wave < wave_job_count {
                while active_handles.len() < num_jobs {
                    if let Some(job_id) = jobs_to_spawn.next() {
                        jobs_left.remove(&job_id);
                        let job = jobs_in_batch.get(&job_id).unwrap();

                        let stage_type = &job.stage_type;
                        let execution_type = execution_type_override.unwrap_or_else(|| {
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
                        });
                        let image_path_opt = self
                            .lab
                            .runs
                            .values()
                            .find(|r| r.jobs.contains(&job_id))
                            .and_then(|r| r.image.as_deref());
                        let image_tag = image_path_opt
                            .and_then(|p| p.file_stem())
                            .and_then(|s| s.to_str());

                        let mut cmd = Command::new(repx_binary_path);

                        if stage_type == "scatter-gather" {
                            cmd.arg("internal-scatter-gather");
                        } else {
                            cmd.arg("internal-execute");
                        };

                        cmd.arg("--job-id").arg(job_id.0.as_str());
                        cmd.arg("--runtime").arg(execution_type);
                        if let Some(tag) = image_tag {
                            cmd.arg("--image-tag").arg(tag);
                        }
                        cmd.arg("--base-path").arg(target.base_path());

                        if stage_type == "scatter-gather" {
                            let scatter_exe = job.executables.get("scatter").unwrap();
                            let worker_exe = job.executables.get("worker").unwrap();
                            let gather_exe = job.executables.get("gather").unwrap();

                            let artifacts_base = target.artifacts_base_path();
                            let job_package_path_on_target =
                                artifacts_base.join(format!("jobs/{}", job_id));
                            let scatter_exe_path = artifacts_base.join(&scatter_exe.path);
                            let worker_exe_path = artifacts_base.join(&worker_exe.path);
                            let gather_exe_path = artifacts_base.join(&gather_exe.path);

                            let worker_outputs_json = serde_json::to_string(&worker_exe.outputs)
                                .map_err(AppError::from)?;

                            cmd.arg("--job-package-path")
                                .arg(job_package_path_on_target);
                            cmd.arg("--scatter-exe-path").arg(scatter_exe_path);
                            cmd.arg("--worker-exe-path").arg(worker_exe_path);
                            cmd.arg("--gather-exe-path").arg(gather_exe_path);
                            cmd.arg("--worker-outputs-json").arg(worker_outputs_json);

                            cmd.arg("--scheduler").arg("local");
                            cmd.arg("--worker-sbatch-opts").arg("");
                        } else {
                            let main_exe = job.executables.get("main").unwrap();
                            let executable_path_on_target =
                                target.artifacts_base_path().join(&main_exe.path);
                            cmd.arg("--executable-path").arg(executable_path_on_target);
                        }

                        cmd.stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped());

                        let child = cmd.spawn().map_err(|e| {
                            ClientError::Core(AppError::ProcessLaunchFailed {
                                command_name: repx_binary_path.to_string_lossy().to_string(),
                                source: e,
                            })
                        })?;

                        submitted_count += 1;
                        send(ClientEvent::JobStarted {
                            job_id: job_id.clone(),
                            pid: child.id(),
                            total: total_to_submit,
                            current: submitted_count,
                        });

                        active_handles.push((job_id, child));
                    } else {
                        break;
                    }
                }

                if active_handles.is_empty() {
                    break;
                }

                let (job_id, handle) = active_handles.remove(0);
                let output = handle.wait_with_output().map_err(AppError::from)?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(ClientError::Core(AppError::ExecutionFailed {
                        message: format!("Local execution of job '{}' failed.", job_id),
                        log_path: Some(
                            target
                                .base_path()
                                .join("outputs")
                                .join(job_id.0)
                                .join("repx"),
                        ),
                        log_summary: format!(
                            "Process exited with status {}.\n--- STDERR ---\n{}",
                            output.status, stderr
                        ),
                    }));
                }
                completed_jobs.insert(job_id.clone());
                finished_jobs_in_wave += 1;
            }

            send(ClientEvent::WaveCompleted {
                wave: wave_num,
                num_jobs: wave_job_count,
            });
        }

        Ok(format!(
            "Successfully executed {} jobs locally.",
            submitted_count
        ))
    }

    fn generate_and_write_inputs_json(
        lab: &Lab,
        job: &Job,
        job_id: &JobId,
        target: &Arc<dyn Target>,
        executable_name: &str,
    ) -> Result<()> {
        let mut inputs_map = serde_json::Map::new();

        let exe = job.executables.get(executable_name).ok_or_else(|| {
            AppError::ConfigurationError(format!(
                "Job '{}' missing required executable '{}'",
                job_id, executable_name
            ))
        })?;

        for mapping in &exe.inputs {
            if let (Some(dep_job_id), Some(source_output)) =
                (&mapping.job_id, &mapping.source_output)
            {
                let dep_job = lab
                    .jobs
                    .get(dep_job_id)
                    .ok_or_else(|| AppError::JobNotFound(dep_job_id.clone()))?;

                let dep_exe = if dep_job.stage_type == "scatter-gather" {
                    dep_job.executables.get("gather")
                } else {
                    dep_job.executables.get("main")
                }
                .ok_or_else(|| {
                    AppError::ConfigurationError(format!(
                        "Could not find output executable for dependency job '{}'",
                        dep_job_id
                    ))
                })?;

                let value_template_val =
                    dep_exe
                        .outputs
                        .get(source_output)
                        .ok_or_else(|| {
                            ClientError::Core(AppError::ConfigurationError(format!(
                            "Inconsistent metadata: job '{}' requires output '{}' from dependency '{}', but this output is not defined in the dependency's metadata.",
                            job_id, source_output, dep_job_id
                        )))
                        })?;

                let value_template = value_template_val.as_str().ok_or_else(|| {
                    ClientError::Core(AppError::ConfigurationError(format!(
                        "Inconsistent metadata: job '{}' requires output '{}' from dependency '{}', but this output is not a string path template.",
                        job_id, source_output, dep_job_id
                    )))
                })?;

                let dep_output_dir = target
                    .base_path()
                    .join("outputs")
                    .join(&dep_job_id.0)
                    .join("out");
                let final_path = value_template.replace("$out", &dep_output_dir.to_string_lossy());

                inputs_map.insert(
                    mapping.target_input.clone(),
                    serde_json::Value::String(final_path),
                );
            }
        }

        let json_content = serde_json::to_string_pretty(&serde_json::Value::Object(inputs_map))
            .map_err(AppError::from)?;

        let inputs_json_path_on_target = target
            .base_path()
            .join("outputs")
            .join(&job_id.0)
            .join("repx")
            .join("inputs.json");

        log_info!(
            "Generating inputs.json for job '{}' on target '{}'",
            job_id,
            target.name()
        );
        log_debug!(
            "Writing inputs.json to '{}' with content:\n{}",
            inputs_json_path_on_target.display(),
            json_content
        );

        target.write_remote_file(&inputs_json_path_on_target, &json_content)
    }

    pub fn get_log_tail(
        &self,
        job_id: JobId,
        target_name: &str,
        line_count: u32,
    ) -> Result<Vec<String>> {
        let target = self
            .targets
            .get(target_name)
            .ok_or_else(|| ClientError::TargetNotFound(target_name.to_string()))?;

        let slurm_info = {
            let slurm_map_guard = self.slurm_map.lock().unwrap();
            slurm_map_guard.get(&job_id).cloned()
        };

        if let Some((slurm_target_name, slurm_id)) = slurm_info {
            if slurm_target_name == target_name {
                let log_path = target
                    .base_path()
                    .join("outputs")
                    .join(&job_id.0)
                    .join("repx")
                    .join(format!("slurm-{}.out", slurm_id));

                return target.read_remote_file_tail(&log_path, line_count);
            }
        }

        let log_path = target
            .base_path()
            .join("outputs")
            .join(&job_id.0)
            .join("repx")
            .join("stdout.log");

        target.read_remote_file_tail(&log_path, line_count)
    }

    pub fn cancel_job(&self, job_id: JobId) -> Result<()> {
        let slurm_info = {
            let slurm_map_guard = self.slurm_map.lock().unwrap();
            slurm_map_guard.get(&job_id).cloned()
        };

        if let Some((target_name, slurm_id)) = slurm_info {
            let target = self.targets.get(&target_name).ok_or_else(|| {
                ClientError::Core(AppError::ConfigurationError(format!(
                    "Inconsistent state: target '{}' from slurm_map not found.",
                    target_name
                )))
            })?;
            return target.scancel(slurm_id);
        }
        Ok(())
    }
}

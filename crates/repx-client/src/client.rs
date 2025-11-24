use crate::{
    error::{ClientError, Result},
    inputs,
    targets::{local::LocalTarget, ssh::SshTarget, Target},
};
use fs_err;
use repx_core::{
    config::{Config, Resources},
    engine,
    error::AppError,
    lab, log_info,
    model::{Job, JobId, Lab, RunId},
};
use std::path::Path;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{mpsc::Sender, Arc, Mutex},
};
use xdg;

pub mod local;
pub mod slurm;
pub mod status;

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

#[derive(Default)]
pub struct SubmitOptions {
    pub execution_type: Option<String>,
    pub resources: Option<Resources>,
    pub num_jobs: Option<usize>,
    pub event_sender: Option<Sender<ClientEvent>>,
}
#[derive(Clone)]
pub struct Client {
    pub(crate) config: Arc<Config>,
    pub(crate) lab_path: Arc<PathBuf>,
    pub(crate) lab: Arc<Lab>,
    pub(crate) targets: Arc<HashMap<String, Arc<dyn Target>>>,
    pub(crate) slurm_map: SlurmIdMap,
}

impl Client {
    pub fn new(config: Config, lab_path: PathBuf) -> Result<Self> {
        let lab = lab::load_from_path(&lab_path).map_err(ClientError::Core)?;
        let lab_arc = Arc::new(lab);

        let mut targets: HashMap<String, Arc<dyn Target>> = HashMap::new();
        for (name, target_config) in &config.targets {
            let target: Arc<dyn Target> = if name == "local" {
                Arc::new(LocalTarget {
                    name: name.clone(),
                    config: target_config.clone(),
                    local_tools_path: lab_arc.host_tools_path.clone(),
                })
            } else if let Some(address) = &target_config.address {
                Arc::new(SshTarget {
                    name: name.clone(),
                    address: address.clone(),
                    config: target_config.clone(),
                    local_tools_path: lab_arc.host_tools_path.clone(),
                    host_tools_dir_name: lab_arc.host_tools_dir_name.clone(),
                })
            } else {
                return Err(ClientError::Core(AppError::ConfigurationError(format!(
                    "Target '{}' is not 'local' and has no 'address' specified.",
                    name
                ))));
            };
            targets.insert(name.clone(), target);
        }

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
            lab: lab_arc,
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

    pub(crate) fn save_slurm_map(&self) -> Result<()> {
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
        status::get_statuses(self)
    }

    pub fn get_statuses_for_active_target(
        &self,
        active_target_name: &str,
    ) -> Result<HashMap<JobId, engine::JobStatus>> {
        status::get_statuses_for_active_target(self, active_target_name)
    }

    pub fn submit_run(
        &self,
        run_spec: String,
        target_name: &str,
        scheduler: &str,
        options: SubmitOptions,
    ) -> Result<String> {
        self.submit_batch_run(vec![run_spec], target_name, scheduler, options)
    }
    pub fn submit_batch_run(
        &self,
        run_specs: Vec<String>,
        target_name: &str,
        scheduler: &str,
        options: SubmitOptions,
    ) -> Result<String> {
        let send = |event: ClientEvent| {
            if let Some(sender) = &options.event_sender {
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

        send(ClientEvent::DeployingBinary);
        let remote_repx_binary_path = target.deploy_repx_binary()?;
        log_info!(
            "repx binary deployed to: {}",
            remote_repx_binary_path.display()
        );

        send(ClientEvent::SyncingArtifacts { total: 1 });
        target.sync_lab_root(&self.lab_path)?;
        send(ClientEvent::SyncingFinished);

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
                inputs::generate_and_write_inputs_json(
                    &self.lab,
                    &self.lab_path,
                    job,
                    job_id,
                    target.clone(),
                    "scatter",
                )?;
            } else {
                inputs::generate_and_write_inputs_json(
                    &self.lab,
                    &self.lab_path,
                    job,
                    job_id,
                    target.clone(),
                    "main",
                )?;
            }
        }

        match scheduler {
            "slurm" => slurm::submit_slurm_batch_run(
                self,
                jobs_to_submit,
                target.clone(),
                target_name,
                &remote_repx_binary_path,
                &options,
                send,
            ),
            "local" => local::submit_local_batch_run(
                self,
                jobs_to_run,
                target.clone(),
                target_name,
                &remote_repx_binary_path,
                &options,
                send,
            ),
            _ => Err(ClientError::Core(AppError::ConfigurationError(format!(
                "Unsupported scheduler: '{}'. Must be 'slurm' or 'local'.",
                scheduler
            )))),
        }
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

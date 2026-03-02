use crate::{
    error::{ClientError, Result},
    targets::{local::LocalTarget, ssh::SshTarget, Target},
};
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets, Attribute, Cell, Color, Table};
use fs_err;
use repx_core::{
    config::{Config, Resources},
    constants::{dirs, logs},
    engine,
    errors::ConfigError,
    lab,
    model::{Job, JobId, Lab, RunId, SchedulerType},
};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{mpsc::Sender, Arc, Mutex},
};

pub mod local;
pub mod slurm;
pub mod status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogType {
    Auto,
    Stdout,
    Stderr,
}

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
    JobSucceeded {
        job_id: JobId,
    },
    JobFailed {
        job_id: JobId,
    },
    JobBlocked {
        job_id: JobId,
        blocked_by: JobId,
    },
    LocalProgress {
        running: usize,
        succeeded: usize,
        failed: usize,
        blocked: usize,
        pending: usize,
        total: usize,
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
    pub continue_on_failure: bool,
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
        let lab = lab::load_from_path(&lab_path)?;
        let lab_arc = Arc::new(lab);

        let local_base_path = if let Some(local_target) = config.targets.get("local") {
            local_target.base_path.clone()
        } else {
            return Err(ClientError::Config(ConfigError::General(
                 "A 'local' target must be defined in config.toml to store client state and temporary files.\n\
                  Tip: You can define a 'data-only' local target by setting a base_path without any execution types:\n\
                  \n\
                  [targets.local]\n\
                  base_path = \"~/.local/share/repx\"\n\
                  # No executables needed if only submitting to remote targets\n".to_string()
             )));
        };

        let client_state_dir = local_base_path.join("repx").join("state");
        let client_temp_dir = local_base_path.join("repx").join("temp");
        fs_err::create_dir_all(&client_state_dir)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
        fs_err::create_dir_all(&client_temp_dir)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

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
                    local_temp_path: client_temp_dir.clone(),
                    host_tools_dir_name: lab_arc.host_tools_dir_name.clone(),
                })
            } else {
                return Err(ClientError::Config(ConfigError::General(format!(
                    "Target '{}' is not 'local' and has no 'address' specified.",
                    name
                ))));
            };
            targets.insert(name.clone(), target);
        }

        let lab_path_abs = fs_err::canonicalize(&lab_path).unwrap_or(lab_path.clone());
        let lab_hash = {
            let mut hasher = Sha256::new();
            hasher.update(lab_path_abs.to_string_lossy().as_bytes());
            format!("{:x}", hasher.finalize())
        };
        let map_filename = format!("slurm_map_{}.json", lab_hash);
        let map_path = client_state_dir.join(map_filename);

        let slurm_map_data = match fs_err::read_to_string(&map_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(data) => data,
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse SLURM map file at {}: {}. Starting with empty map.",
                        map_path.display(),
                        e
                    );
                    HashMap::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => {
                tracing::warn!(
                    "Failed to read SLURM map file at {}: {}. Starting with empty map.",
                    map_path.display(),
                    e
                );
                HashMap::new()
            }
        };

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

    pub fn get_target(&self, name: &str) -> Option<Arc<dyn Target>> {
        self.targets.get(name).cloned()
    }

    pub(crate) fn save_slurm_map(&self) -> Result<()> {
        let local_base_path = if let Some(local_target) = self.config.targets.get("local") {
            local_target.base_path.clone()
        } else {
            return Err(ClientError::Config(ConfigError::General(
                "A 'local' target must be defined in config.toml to save client state.\n\
                 Tip: Add a [targets.local] section with a base_path."
                    .to_string(),
            )));
        };

        let client_state_dir = local_base_path.join("repx").join("state");
        fs_err::create_dir_all(&client_state_dir)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;

        let lab_path_abs =
            fs_err::canonicalize(&*self.lab_path).unwrap_or(self.lab_path.to_path_buf());
        let lab_hash = {
            let mut hasher = Sha256::new();
            hasher.update(lab_path_abs.to_string_lossy().as_bytes());
            format!("{:x}", hasher.finalize())
        };
        let map_filename = format!("slurm_map_{}.json", lab_hash);

        let map_path = client_state_dir.join(map_filename);
        let data = self
            .slurm_map
            .lock()
            .expect("slurm_map mutex must not be poisoned");
        let json_string = serde_json::to_string_pretty(&*data)
            .map_err(|e| ClientError::Config(ConfigError::Json(e)))?;
        fs_err::write(map_path, json_string)
            .map_err(|e| ClientError::Config(ConfigError::Io(e)))?;
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
        active_scheduler: Option<SchedulerType>,
    ) -> Result<HashMap<JobId, engine::JobStatus>> {
        status::get_statuses_for_active_target(self, active_target_name, active_scheduler)
    }

    pub fn submit_run(
        &self,
        run_spec: String,
        target_name: &str,
        scheduler: SchedulerType,
        options: SubmitOptions,
    ) -> Result<String> {
        self.submit_batch_run(vec![run_spec], target_name, scheduler, options)
    }
    pub fn submit_batch_run(
        &self,
        run_specs: Vec<String>,
        target_name: &str,
        scheduler: SchedulerType,
        options: SubmitOptions,
    ) -> Result<String> {
        use crate::submission;

        let send = |event: ClientEvent| {
            if let Some(sender) = &options.event_sender {
                let _ = sender.send(event);
            }
        };

        let target = self
            .targets
            .get(target_name)
            .ok_or_else(|| ClientError::TargetNotFound(target_name.to_string()))?;

        let project_id = submission::generate_project_id(&self.lab_path);

        let full_dependency_set = submission::resolve_dependency_graph(&self.lab, &run_specs)?;
        if full_dependency_set.is_empty() {
            return Ok(
                "All selected jobs are already complete or no jobs were specified.".to_string(),
            );
        }

        send(ClientEvent::DeployingBinary);
        let remote_repx_binary_path = target.deploy_repx_binary()?;
        tracing::info!(
            "repx binary deployed to: {}",
            remote_repx_binary_path.display()
        );

        send(ClientEvent::SyncingArtifacts { total: 1 });
        target.sync_lab_root(&self.lab_path)?;
        send(ClientEvent::SyncingArtifactProgress {
            path: PathBuf::from("lab"),
        });

        if let Err(e) = target.register_gc_root(&project_id, &self.lab.content_hash) {
            tracing::info!("Warning: Failed to register GC root: {}", e);
        }
        send(ClientEvent::SyncingFinished);

        let raw_statuses = self.get_statuses_for_active_target(target_name, Some(scheduler))?;
        let job_statuses = engine::determine_job_statuses(&self.lab, &raw_statuses);
        let jobs_to_run =
            submission::filter_jobs_to_run(&self.lab, &full_dependency_set, &job_statuses);

        if jobs_to_run.is_empty() {
            let details =
                self.format_completed_jobs_msg(full_dependency_set.iter().cloned(), target.clone());
            return Ok(format!(
                "All required jobs for this submission are already complete.\n{}",
                details
            ));
        }
        let jobs_to_run_ids: HashSet<JobId> = jobs_to_run.keys().cloned().collect();

        let images_to_sync = submission::collect_images_to_sync(&self.lab, &jobs_to_run_ids);
        if !images_to_sync.is_empty() {
            send(ClientEvent::SyncingArtifacts {
                total: images_to_sync.len() as u64,
            });
            let local_target = self.targets.get("local").ok_or_else(|| {
                ClientError::Config(ConfigError::General(
                    "Local target ('local') must be defined in the configuration.".to_string(),
                ))
            })?;
            submission::sync_images(
                &self.lab_path,
                target,
                local_target,
                &images_to_sync,
                options.event_sender.as_ref(),
            )?;
            send(ClientEvent::SyncingFinished);
        }

        let jobs_to_submit: HashMap<JobId, &Job> = if scheduler == SchedulerType::Slurm {
            jobs_to_run
                .iter()
                .map(|(id, job)| (id.clone(), *job))
                .collect()
        } else {
            submission::filter_jobs_for_local_submission(&jobs_to_run, &jobs_to_run_ids)?
        };

        if jobs_to_submit.is_empty() && scheduler == SchedulerType::Slurm {
            return Ok(
                "All schedulable jobs for this submission are already complete.".to_string(),
            );
        }

        submission::generate_inputs_for_jobs(
            &self.lab,
            &self.lab_path,
            &jobs_to_run,
            target.clone(),
        )?;

        let result = match scheduler {
            SchedulerType::Slurm => slurm::submit_slurm_batch_run(
                self,
                jobs_to_submit,
                target.clone(),
                target_name,
                &remote_repx_binary_path,
                &options,
                send,
            ),
            SchedulerType::Local => local::submit_local_batch_run(
                self,
                jobs_to_run,
                target.clone(),
                target_name,
                &remote_repx_binary_path,
                &options,
                send,
            ),
        };

        match result {
            Ok(res) => Ok(format!(
                "{}\n{}",
                res,
                self.format_completed_jobs_msg(jobs_to_run_ids.into_iter(), target.clone())
            )),
            Err(e) => Err(e),
        }
    }
    pub fn get_log_tail(
        &self,
        job_id: JobId,
        target_name: &str,
        line_count: u32,
        log_type: LogType,
    ) -> Result<Vec<String>> {
        let target = self
            .targets
            .get(target_name)
            .ok_or_else(|| ClientError::TargetNotFound(target_name.to_string()))?;

        let log_path = match log_type {
            LogType::Stderr => target
                .base_path()
                .join(dirs::OUTPUTS)
                .join(&job_id.0)
                .join(dirs::REPX)
                .join(logs::STDERR),
            LogType::Stdout => target
                .base_path()
                .join(dirs::OUTPUTS)
                .join(&job_id.0)
                .join(dirs::REPX)
                .join(logs::STDOUT),
            LogType::Auto => {
                let slurm_info = {
                    let slurm_map_guard = self
                        .slurm_map
                        .lock()
                        .expect("slurm_map mutex must not be poisoned");
                    slurm_map_guard.get(&job_id).cloned()
                };

                if let Some((slurm_target_name, slurm_id)) = slurm_info {
                    if slurm_target_name == target_name {
                        target
                            .base_path()
                            .join(dirs::OUTPUTS)
                            .join(&job_id.0)
                            .join(dirs::REPX)
                            .join(format!("slurm-{}.out", slurm_id))
                    } else {
                        target
                            .base_path()
                            .join(dirs::OUTPUTS)
                            .join(&job_id.0)
                            .join(dirs::REPX)
                            .join(logs::STDOUT)
                    }
                } else {
                    target
                        .base_path()
                        .join(dirs::OUTPUTS)
                        .join(&job_id.0)
                        .join(dirs::REPX)
                        .join(logs::STDOUT)
                }
            }
        };

        target.read_remote_file_tail(&log_path, line_count)
    }

    pub fn cancel_job(&self, job_id: JobId) -> Result<()> {
        let slurm_info = {
            let slurm_map_guard = self
                .slurm_map
                .lock()
                .expect("slurm_map mutex must not be poisoned");
            slurm_map_guard.get(&job_id).cloned()
        };

        if let Some((target_name, slurm_id)) = slurm_info {
            let target = self.targets.get(&target_name).ok_or_else(|| {
                ClientError::Config(ConfigError::General(format!(
                    "Inconsistent state: target '{}' from slurm_map not found.",
                    target_name
                )))
            })?;

            target.scancel(slurm_id)?;

            let manifest_path = target
                .base_path()
                .join(dirs::OUTPUTS)
                .join(&job_id.0)
                .join(dirs::REPX)
                .join(repx_core::constants::manifests::WORKER_SLURM_IDS);

            if let Ok(content) = target.read_remote_file_tail(&manifest_path, 10000) {
                let full_content = content.join("\n");
                if let Ok(worker_ids) = serde_json::from_str::<Vec<u32>>(&full_content) {
                    if !worker_ids.is_empty() {
                        tracing::info!(
                            "Cancelling {} scatter-gather worker jobs for {}",
                            worker_ids.len(),
                            job_id
                        );
                        if let Err(e) = target.scancel_batch(&worker_ids) {
                            tracing::warn!("Failed to cancel worker jobs for {}: {}", job_id, e);
                        }
                    }
                }
            }

            return Ok(());
        }
        Ok(())
    }

    fn format_completed_jobs_msg(
        &self,
        job_ids: impl Iterator<Item = JobId>,
        target: Arc<dyn Target>,
    ) -> String {
        let mut sorted_ids: Vec<_> = job_ids.collect();
        sorted_ids.sort();

        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS)
            .set_header(vec![
                Cell::new("Job ID")
                    .add_attribute(Attribute::Bold)
                    .fg(Color::Cyan),
                Cell::new("Output Directory")
                    .add_attribute(Attribute::Bold)
                    .fg(Color::Cyan),
            ]);

        for job_id in sorted_ids {
            let out_dir = target
                .base_path()
                .join(dirs::OUTPUTS)
                .join(&job_id.0)
                .join(dirs::REPX);
            table.add_row(vec![
                Cell::new(job_id.0.as_str()).fg(Color::Yellow),
                Cell::new(out_dir.to_string_lossy().as_ref()),
            ]);
        }
        table.to_string()
    }
}

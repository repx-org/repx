use crate::{
    error::{ClientError, Result},
    targets::{local::LocalTarget, ssh::SshTarget, Target},
};
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets, Attribute, Cell, Color, Table};
use fs_err;
use repx_core::{
    cache::{CacheKey, CacheMetadata, CacheStore, CacheStoreExt, FsCache},
    config::{Config, Resources},
    constants::{dirs, logs, targets},
    engine,
    errors::CoreError,
    lab,
    lab::LabSource,
    model::{Job, JobId, Lab, RunId, SchedulerType},
};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{atomic::AtomicBool, mpsc::Sender, Arc, Mutex},
};

pub mod local;
pub mod scheduler;
pub mod slurm;
pub mod status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogType {
    Auto,
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkUnitPhase {
    Scatter,
    Step { branch: usize, step: String },
    Gather,
}

impl std::fmt::Display for WorkUnitPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkUnitPhase::Scatter => write!(f, "scatter"),
            WorkUnitPhase::Step { branch, step } => write!(f, "branch {}, step: {}", branch, step),
            WorkUnitPhase::Gather => write!(f, "gather"),
        }
    }
}

#[derive(Debug)]
pub enum ClientEvent {
    DeployingBinary,
    CreatingLabTar,
    SyncingLabTar,
    CheckingJobStatuses,
    PreparingInputs {
        num_jobs: usize,
    },
    PreparingInputProgress {
        job_id: JobId,
        current: usize,
        total: usize,
    },
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
        concurrency: Option<usize>,
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
        phase: Option<WorkUnitPhase>,
    },
    JobSucceeded {
        job_id: JobId,
        phase: Option<WorkUnitPhase>,
        wall_time: Option<Duration>,
    },
    JobFailed {
        job_id: JobId,
        phase: Option<WorkUnitPhase>,
        wall_time: Option<Duration>,
    },
    JobBlocked {
        job_id: JobId,
        blocked_by: JobId,
        phase: Option<WorkUnitPhase>,
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SlurmJobEntry {
    pub target_name: String,
    pub slurm_id: u32,
}

type SlurmIdMap = Arc<Mutex<HashMap<JobId, SlurmJobEntry>>>;

pub(crate) fn lock_slurm_map(
    map: &Mutex<HashMap<JobId, SlurmJobEntry>>,
) -> std::sync::MutexGuard<'_, HashMap<JobId, SlurmJobEntry>> {
    map.lock().unwrap_or_else(|e| {
        tracing::warn!(
            "SLURM ID map mutex was poisoned (a thread panicked while holding it). \
             Recovering with potentially stale data. This may cause incorrect job \
             status reporting or cancellation of wrong SLURM jobs."
        );
        e.into_inner()
    })
}

pub(crate) fn resolve_execution_type(
    image_tag: Option<&str>,
    explicit_execution_type: Option<&str>,
    target_config: &repx_core::config::Target,
    scheduler_config: Option<&repx_core::config::SchedulerConfig>,
) -> String {
    use repx_core::model::ExecutionType;

    if explicit_execution_type.is_none() && image_tag.is_none() {
        return ExecutionType::Native.to_string();
    }
    explicit_execution_type
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let sched_config = match scheduler_config {
                Some(cfg) => cfg,
                None => return ExecutionType::Native.to_string(),
            };
            target_config
                .default_execution_type
                .filter(|et| sched_config.execution_types.contains(et))
                .map(|et| et.to_string())
                .or_else(|| sched_config.execution_types.first().map(|s| s.to_string()))
                .unwrap_or_else(|| ExecutionType::Native.to_string())
        })
}

#[derive(Default, Clone, Debug)]
pub struct LabTarInfo {
    pub remote_tar_path: PathBuf,
    pub node_local_base: PathBuf,
    pub content_hash: String,
    pub lab_dir_name: String,
}

#[derive(Default)]
pub struct SubmitOptions {
    pub execution_type: Option<String>,
    pub resources: Option<Resources>,
    pub num_jobs: Option<usize>,
    pub mem_override: Option<u64>,
    pub event_sender: Option<Sender<ClientEvent>>,
    pub continue_on_failure: bool,
    pub verbose: repx_core::logging::Verbosity,
    pub cancel_flag: Option<Arc<AtomicBool>>,
    pub artifact_store: repx_core::model::ArtifactStore,
}

pub struct SubmissionTarget {
    pub target: Arc<dyn Target>,
    pub target_name: String,
    pub repx_binary_path: PathBuf,
}

#[derive(Clone)]
pub struct Client {
    pub(crate) config: Config,
    pub(crate) lab_source: LabSource,
    pub(crate) lab: Lab,
    pub(crate) targets: HashMap<String, Arc<dyn Target>>,
    pub(crate) slurm_map: SlurmIdMap,
    pub(crate) slurm_map_path: PathBuf,
    pub(crate) cache: Arc<FsCache>,
}

impl Client {
    pub fn new(config: Config, source: LabSource) -> Result<Self> {
        let lab = lab::load(&source)?;

        let local_base_path = if let Some(local_target) = config.targets.get(targets::LOCAL) {
            local_target.base_path.clone()
        } else {
            return Err(ClientError::Config(CoreError::MissingLocalTarget));
        };

        let client_state_dir = local_base_path.join("repx").join("state");
        let client_temp_dir = local_base_path.join("repx").join("temp");
        fs_err::create_dir_all(&client_state_dir).map_err(ClientError::Io)?;
        fs_err::create_dir_all(&client_temp_dir).map_err(ClientError::Io)?;

        let cache = Arc::new(FsCache::new(local_base_path.join("repx")));

        let local_tools_path = if lab.host_tools_path.is_relative() {
            if let LabSource::Tar(tar_path) = &source {
                let ht_key = CacheKey::HostTools {
                    content_hash: lab.content_hash.clone(),
                };
                let tools_root = cache.path(&ht_key);
                if !cache.ensure_fresh(&ht_key)? {
                    crate::tar_extract::extract_host_tools_from_tar(tar_path, &tools_root)?;
                    let meta = CacheMetadata::new(&ht_key, "host tools extracted from lab tar")
                        .with_content_hash(&lab.content_hash);
                    cache.mark_ready(&ht_key, meta)?;
                }
                tools_root.join(&lab.host_tools_path)
            } else {
                lab.host_tools_path.clone()
            }
        } else {
            lab.host_tools_path.clone()
        };

        let mut targets: HashMap<String, Arc<dyn Target>> = HashMap::new();
        for (name, target_config) in &config.targets {
            let target: Arc<dyn Target> = if name == targets::LOCAL {
                Arc::new(LocalTarget {
                    name: name.clone(),
                    config: target_config.clone(),
                    local_tools_path: local_tools_path.clone(),
                })
            } else if let Some(address) = &target_config.address {
                Arc::new(SshTarget {
                    name: name.clone(),
                    address: address.clone(),
                    config: target_config.clone(),
                    local_tools_path: local_tools_path.clone(),
                    local_temp_path: client_temp_dir.clone(),
                    host_tools_dir_name: lab.host_tools_dir_name.clone(),
                })
            } else {
                return Err(ClientError::Config(CoreError::InvalidConfig {
                    detail: format!(
                        "Target '{}' is not 'local' and has no 'address' specified.",
                        name
                    ),
                }));
            };
            targets.insert(name.clone(), target);
        }

        let source_path = source.path();
        let source_path_abs =
            fs_err::canonicalize(source_path).unwrap_or(source_path.to_path_buf());
        let lab_hash = {
            let mut hasher = Sha256::new();
            hasher.update(source_path_abs.to_string_lossy().as_bytes());
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
            config,
            lab_source: source,
            lab,
            targets,
            slurm_map: Arc::new(Mutex::new(slurm_map_data)),
            slurm_map_path: map_path,
            cache,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn lab(&self) -> &Lab {
        &self.lab
    }

    pub fn lab_source(&self) -> &LabSource {
        &self.lab_source
    }

    pub fn get_target(&self, name: &str) -> Option<Arc<dyn Target>> {
        self.targets.get(name).cloned()
    }

    pub(crate) fn save_slurm_map(&self) -> Result<()> {
        if let Some(parent) = self.slurm_map_path.parent() {
            fs_err::create_dir_all(parent).map_err(ClientError::Io)?;
        }

        let data_clone = {
            let guard = lock_slurm_map(&self.slurm_map);
            guard.clone()
        };

        let json_string = serde_json::to_string_pretty(&data_clone).map_err(ClientError::Json)?;

        let temp_path = self.slurm_map_path.with_extension("json.tmp");
        fs_err::write(&temp_path, &json_string).map_err(ClientError::Io)?;
        fs_err::rename(&temp_path, &self.slurm_map_path).map_err(ClientError::Io)?;

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
                if let Err(e) = sender.send(event) {
                    tracing::debug!("Failed to send client event: {}", e);
                }
            }
        };

        let target = self
            .targets
            .get(target_name)
            .ok_or_else(|| ClientError::TargetNotFound(target_name.to_string()))?;

        let project_id = submission::generate_project_id(&self.lab_source);

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

        let use_node_local = options.artifact_store == repx_core::model::ArtifactStore::NodeLocal;
        let lab_tar_remote_path = if use_node_local {
            let node_local = target.config().node_local_path.as_ref().ok_or_else(|| {
                ClientError::Config(CoreError::InvalidConfig {
                    detail: "--lab-tar requires node_local_path to be set on the target".into(),
                })
            })?;

            send(ClientEvent::CreatingLabTar);

            let tar_filename = format!("{}.tar", self.lab.content_hash);

            let local_tar_path = match &self.lab_source {
                LabSource::Tar(tar_path) => tar_path.clone(),
                LabSource::Directory(dir_path) => {
                    let cache_key = CacheKey::LabTar {
                        content_hash: self.lab.content_hash.clone(),
                    };
                    let tar_path = self.cache.path(&cache_key);
                    std::fs::create_dir_all(tar_path.parent().unwrap_or(Path::new("/")))
                        .map_err(ClientError::Io)?;

                    if self.cache.ensure_fresh(&cache_key)? {
                        tracing::info!("Lab tar already cached: {:?}", tar_path);
                    } else {
                        let resolved_lab = dir_path.canonicalize().map_err(ClientError::Io)?;
                        let status = std::process::Command::new("tar")
                            .arg("cf")
                            .arg(&tar_path)
                            .arg("-C")
                            .arg(resolved_lab.parent().unwrap_or(Path::new("/")))
                            .arg(
                                resolved_lab
                                    .file_name()
                                    .unwrap_or(std::ffi::OsStr::new("result")),
                            )
                            .status()
                            .map_err(ClientError::Io)?;
                        if !status.success() {
                            return Err(ClientError::Config(CoreError::InvalidConfig {
                                detail: format!("Failed to create lab tar at {:?}", tar_path),
                            }));
                        }
                        let meta = CacheMetadata::new(&cache_key, "lab tar archive")
                            .with_content_hash(&self.lab.content_hash);
                        self.cache.mark_ready(&cache_key, meta)?;
                        tracing::info!("Created lab tar: {:?}", tar_path);
                    }
                    tar_path
                }
            };

            send(ClientEvent::SyncingLabTar);
            let remote_tar_dir = target.base_path().join("lab-tars");
            let remote_tar_path = remote_tar_dir.join(&tar_filename);
            if !remote_tar_path.exists() {
                target.sync_file(&local_tar_path, &remote_tar_path)?;
            } else {
                tracing::info!(
                    "Lab tar already exists at {:?}, skipping copy",
                    remote_tar_path
                );
            }

            let lab_tar_info = LabTarInfo {
                remote_tar_path: remote_tar_path.clone(),
                node_local_base: node_local
                    .join("repx")
                    .join("labs")
                    .join(&self.lab.content_hash),
                content_hash: self.lab.content_hash.clone(),
                lab_dir_name: match &self.lab.tar_dir_name {
                    Some(name) => name.clone(),
                    None => {
                        let p = self.lab_source.path();
                        let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                        canonical
                            .file_name()
                            .unwrap_or(std::ffi::OsStr::new("result"))
                            .to_string_lossy()
                            .into_owned()
                    }
                },
            };
            Some(lab_tar_info)
        } else {
            None
        };

        send(ClientEvent::SyncingArtifacts { total: 1 });
        match &self.lab_source {
            LabSource::Tar(tar_path) => {
                if use_node_local {
                    let local_target = self
                        .get_target(targets::LOCAL)
                        .ok_or(ClientError::Config(CoreError::MissingLocalTarget))?;
                    let local_ht_cache = local_target
                        .base_path()
                        .join("repx")
                        .join("temp")
                        .join("host-tools-cache");
                    if !local_ht_cache.join("host-tools").exists() {
                        std::fs::create_dir_all(&local_ht_cache).map_err(ClientError::Io)?;
                        crate::tar_extract::extract_host_tools_from_tar(tar_path, &local_ht_cache)?;
                    }
                    let remote_ht_dest = target.artifacts_base_path().join("host-tools");
                    target.sync_directory(&local_ht_cache.join("host-tools"), &remote_ht_dest)?;
                } else {
                    target.sync_lab_from_tar_via_rsync(tar_path)?;
                }
            }
            LabSource::Directory(dir_path) => {
                if use_node_local {
                    target.sync_lab_root_metadata_only(dir_path)?;
                } else {
                    target.sync_lab_root(dir_path)?;
                }
            }
        }
        send(ClientEvent::SyncingArtifactProgress {
            path: PathBuf::from("lab"),
        });
        send(ClientEvent::SyncingFinished);

        if let Err(e) = target.register_gc_root(&project_id, &self.lab.content_hash) {
            tracing::warn!("Failed to register GC root: {}. The next `repx gc` may delete this experiment's results.", e);
        }

        send(ClientEvent::CheckingJobStatuses);
        let raw_statuses = self.get_statuses_for_active_target(target_name, Some(scheduler))?;
        let job_statuses = engine::determine_job_statuses(&self.lab, raw_statuses);
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

        if !use_node_local {
            let images_to_sync = submission::collect_images_to_sync(&self.lab, &jobs_to_run_ids);
            if !images_to_sync.is_empty() {
                send(ClientEvent::SyncingArtifacts {
                    total: images_to_sync.len() as u64,
                });
                let local_target = self
                    .targets
                    .get(targets::LOCAL)
                    .ok_or(ClientError::Config(CoreError::MissingLocalTarget))?;
                submission::sync_images(
                    &self.lab_source,
                    target,
                    local_target,
                    &images_to_sync,
                    options.event_sender.as_ref(),
                )?;
                send(ClientEvent::SyncingFinished);
            }
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

        if scheduler != SchedulerType::Slurm {
            send(ClientEvent::PreparingInputs {
                num_jobs: jobs_to_run.len(),
            });
            submission::generate_inputs_for_jobs(
                &self.lab,
                &self.lab_source,
                &jobs_to_run,
                target.clone(),
                options.event_sender.as_ref(),
            )?;
        }

        let sub_target = SubmissionTarget {
            target: target.clone(),
            target_name: target_name.to_string(),
            repx_binary_path: remote_repx_binary_path.clone(),
        };

        let result = match scheduler {
            SchedulerType::Slurm => slurm::submit_slurm_batch_run(
                self,
                jobs_to_submit,
                &sub_target,
                &options,
                lab_tar_remote_path.as_ref(),
                send,
            ),
            SchedulerType::Local => {
                let local_artifacts = if let Some(ref info) = lab_tar_remote_path {
                    let local_base = &info.node_local_base;
                    let extraction_cache_root = local_base
                        .parent()
                        .and_then(|p| p.parent())
                        .unwrap_or(local_base);
                    let extraction_cache = FsCache::new(extraction_cache_root.to_path_buf());
                    let ext_key = CacheKey::LabExtraction {
                        content_hash: info.content_hash.clone(),
                    };
                    if !extraction_cache.ensure_fresh(&ext_key)? {
                        repx_core::fs_utils::force_remove_dir(local_base)
                            .map_err(ClientError::Io)?;
                        std::fs::create_dir_all(local_base).map_err(ClientError::Io)?;
                        let status = std::process::Command::new("tar")
                            .arg("xf")
                            .arg(&info.remote_tar_path)
                            .arg("-C")
                            .arg(local_base)
                            .status()
                            .map_err(ClientError::Io)?;
                        if !status.success() {
                            return Err(ClientError::Config(CoreError::InvalidConfig {
                                detail: format!("Failed to extract lab tar to {:?}", local_base),
                            }));
                        }
                        let meta =
                            CacheMetadata::new(&ext_key, "lab tar extracted to node-local storage")
                                .with_content_hash(&info.content_hash);
                        extraction_cache.mark_ready(&ext_key, meta)?;
                    }
                    Some(local_base.join(&info.lab_dir_name))
                } else {
                    None
                };
                local::submit_local_batch_run(
                    self,
                    jobs_to_run,
                    &sub_target,
                    &options,
                    local_artifacts.as_ref(),
                    send,
                )
            }
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
                .join(job_id.as_str())
                .join(dirs::REPX)
                .join(logs::STDERR),
            LogType::Stdout => target
                .base_path()
                .join(dirs::OUTPUTS)
                .join(job_id.as_str())
                .join(dirs::REPX)
                .join(logs::STDOUT),
            LogType::Auto => {
                let slurm_info = {
                    let slurm_map_guard = lock_slurm_map(&self.slurm_map);
                    slurm_map_guard.get(&job_id).cloned()
                };

                if let Some(entry) = slurm_info {
                    if entry.target_name == target_name {
                        target
                            .base_path()
                            .join(dirs::OUTPUTS)
                            .join(job_id.as_str())
                            .join(dirs::REPX)
                            .join(format!("slurm-{}.out", entry.slurm_id))
                    } else {
                        target
                            .base_path()
                            .join(dirs::OUTPUTS)
                            .join(job_id.as_str())
                            .join(dirs::REPX)
                            .join(logs::STDOUT)
                    }
                } else {
                    target
                        .base_path()
                        .join(dirs::OUTPUTS)
                        .join(job_id.as_str())
                        .join(dirs::REPX)
                        .join(logs::STDOUT)
                }
            }
        };

        target.read_remote_file_tail(&log_path, line_count)
    }

    pub fn cancel_job(&self, job_id: JobId) -> Result<()> {
        let slurm_info = {
            let slurm_map_guard = lock_slurm_map(&self.slurm_map);
            slurm_map_guard.get(&job_id).cloned()
        };

        if let Some(entry) = slurm_info {
            let target = self.targets.get(&entry.target_name).ok_or_else(|| {
                ClientError::Config(CoreError::TargetNotConfigured {
                    name: entry.target_name.clone(),
                })
            })?;

            target.scancel(entry.slurm_id)?;

            let manifest_path = target
                .base_path()
                .join(dirs::OUTPUTS)
                .join(job_id.as_str())
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
                .join(job_id.as_str())
                .join(dirs::REPX);
            table.add_row(vec![
                Cell::new(job_id.as_str()).fg(Color::Yellow),
                Cell::new(out_dir.to_string_lossy().as_ref()),
            ]);
        }
        table.to_string()
    }
}

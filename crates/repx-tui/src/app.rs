pub mod jobs;
pub mod targets;

use crate::app::{jobs::JobsState, targets::TargetsState};
use crate::model::{StatusCounts, TuiExecutor, TuiRowItem, TuiScheduler, TuiTarget};
use repx_client::{error::ClientError, Client, SubmitOptions};
use repx_core::{
    config::Resources,
    engine, log_info, log_warn,
    model::{JobId, Lab},
    theme::Theme,
};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum PanelFocus {
    Jobs,
    Targets,
}

#[derive(Debug, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
    SpaceMenu,
    GMenu,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum StatusFilter {
    All,
    Failed,
    Running,
    Pending,
    Completed,
}

impl StatusFilter {
    pub fn as_str(&self) -> &'static str {
        match self {
            StatusFilter::All => "all",
            StatusFilter::Failed => "Failed",
            StatusFilter::Running => "Running",
            StatusFilter::Pending => "Pending",
            StatusFilter::Completed => "Succeeded",
        }
    }
}

const STATUS_FILTERS: [StatusFilter; 5] = [
    StatusFilter::All,
    StatusFilter::Failed,
    StatusFilter::Running,
    StatusFilter::Pending,
    StatusFilter::Completed,
];
type TargetPollUpdate = Result<
    (
        String,
        std::collections::HashMap<repx_core::model::JobId, repx_core::engine::JobStatus>,
    ),
    ClientError,
>;

#[derive(Debug)]
pub enum LogPollerCommand {
    Watch(JobId),
    Stop,
}
type LogUpdate = (JobId, Result<Vec<String>, ClientError>);
pub enum SubmissionResult {
    Success {
        submitted_job_ids: HashSet<JobId>,
    },
    Failure {
        failed_run_or_job_id: String,
        affected_job_ids: HashSet<JobId>,
        error: String,
    },
}

pub enum ExternalAction {
    ExploreLocal(PathBuf),
    ExploreRemote {
        address: String,
        path: PathBuf,
    },
    EditLocal(Vec<PathBuf>),
    EditRemote {
        address: String,
        paths: Vec<PathBuf>,
    },
}
pub struct App {
    pub client: Arc<Client>,
    pub theme: Theme,
    pub lab: Lab,
    pub jobs_state: JobsState,
    pub targets_state: TargetsState,
    pub status_history: VecDeque<StatusCounts>,
    pub completion_rate_history: VecDeque<f64>,
    last_completed_count: usize,
    pub tick_rate: Duration,
    pub should_quit: bool,
    pub input_mode: InputMode,
    status_rx: Receiver<TargetPollUpdate>,
    log_cmd_tx: Sender<LogPollerCommand>,
    log_result_rx: Receiver<LogUpdate>,
    submission_tx: Sender<SubmissionResult>,
    submission_rx: Receiver<SubmissionResult>,
    pub is_loading: bool,
    resources: Option<Resources>,
    pub focused_panel: PanelFocus,
    pub pending_action: Option<ExternalAction>,
    pub system_logs: Vec<String>,
    system_log_rx: Receiver<String>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: Client,
        theme: Theme,
        status_rx: Receiver<TargetPollUpdate>,
        log_cmd_tx: Sender<LogPollerCommand>,
        log_result_rx: Receiver<LogUpdate>,
        submission_tx: Sender<SubmissionResult>,
        submission_rx: Receiver<SubmissionResult>,
        system_log_rx: Receiver<String>,
        resources: Option<Resources>,
        initial_active_target: String,
        active_target_ref: Arc<Mutex<String>>,
        active_scheduler_ref: Arc<Mutex<String>>,
    ) -> Result<Self, ClientError> {
        log_info!("Initializing new App instance.");
        let lab = client.lab()?.clone();
        let is_native_lab = lab.is_native();

        let targets = client
            .config()
            .targets
            .iter()
            .map(|(name, target_config)| {
                let mut available_schedulers = Vec::new();
                let mut available_executors = std::collections::HashMap::new();

                if let Some(conf) = &target_config.local {
                    available_schedulers.push(TuiScheduler::Local);
                    let executors: Vec<TuiExecutor> = conf
                        .execution_types
                        .iter()
                        .filter_map(|s| s.parse().ok())
                        .filter(|e| !is_native_lab || *e == TuiExecutor::Native)
                        .collect();
                    available_executors.insert(TuiScheduler::Local, executors);
                }
                if let Some(conf) = &target_config.slurm {
                    available_schedulers.push(TuiScheduler::Slurm);
                    let executors: Vec<TuiExecutor> = conf
                        .execution_types
                        .iter()
                        .filter_map(|s| s.parse().ok())
                        .filter(|e| !is_native_lab || *e == TuiExecutor::Native)
                        .collect();
                    available_executors.insert(TuiScheduler::Slurm, executors);
                }

                available_schedulers.retain(|s| {
                    available_executors
                        .get(s)
                        .map(|v| !v.is_empty())
                        .unwrap_or(false)
                });

                let default_scheduler: TuiScheduler = target_config
                    .default_scheduler
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(TuiScheduler::Local);

                let actual_scheduler = if available_schedulers.contains(&default_scheduler) {
                    default_scheduler
                } else {
                    available_schedulers
                        .first()
                        .copied()
                        .unwrap_or(TuiScheduler::Local)
                };

                let selected_scheduler_idx = available_schedulers
                    .iter()
                    .position(|&s| s == actual_scheduler)
                    .unwrap_or(0);

                let default_executor: TuiExecutor = target_config
                    .default_execution_type
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(TuiExecutor::Native);

                let selected_executor_idx = available_executors
                    .get(&actual_scheduler)
                    .and_then(|execs| {
                        execs
                            .iter()
                            .position(|&e| e == default_executor)
                            .or(Some(0))
                    })
                    .unwrap_or(0);

                TuiTarget {
                    name: name.clone(),
                    state: if name == &initial_active_target {
                        crate::model::TargetState::Active
                    } else {
                        crate::model::TargetState::Inactive
                    },
                    available_schedulers,
                    available_executors,
                    selected_scheduler_idx,
                    selected_executor_idx,
                }
            })
            .collect();

        let tick_rate = client.config().tui_tick_rate();
        let mut app = Self {
            client: Arc::new(client),
            theme,
            lab,
            jobs_state: JobsState::new(),
            targets_state: TargetsState::new(targets, active_target_ref, active_scheduler_ref),
            status_history: VecDeque::new(),
            completion_rate_history: VecDeque::new(),
            last_completed_count: 0,
            tick_rate,
            should_quit: false,
            input_mode: InputMode::Normal,
            status_rx,
            log_cmd_tx,
            log_result_rx,
            submission_tx,
            submission_rx,
            system_log_rx,
            system_logs: Vec::new(),
            is_loading: true,
            resources,
            focused_panel: PanelFocus::Jobs,
            pending_action: None,
        };

        app.jobs_state.init_from_lab(&app.lab);
        app.jobs_state.rebuild_display_list(&app.lab);

        log_info!("Performing initial data update.");

        if !app.jobs_state.display_rows.is_empty() {
            app.jobs_state.table_state.select(Some(0));
            app.on_selection_change();
        }

        log_info!("App initialized successfully.");
        Ok(app)
    }

    pub fn lab(&self) -> &Lab {
        &self.lab
    }

    pub fn check_for_updates(&mut self) {
        while let Ok(update_result) = self.status_rx.try_recv() {
            match update_result {
                Ok((target_name, job_statuses)) => {
                    let active_target = self.targets_state.get_active_target_name();
                    if target_name != active_target {
                        log_info!(
                            "Ignoring status update from '{}' (active: '{}')",
                            target_name,
                            active_target
                        );
                        return;
                    }

                    let was_loading = self.is_loading;
                    self.is_loading = false;

                    log_info!("Received status update. Applying new statuses.");
                    self.jobs_state.apply_statuses(&self.lab, job_statuses);
                    if was_loading {
                        let (_, current_completed_count) = self.calculate_current_counts();
                        self.last_completed_count = current_completed_count;
                    }
                    self.jobs_state.rebuild_display_list(&self.lab);
                }
                Err(e) => {
                    self.is_loading = false;
                    log_warn!("Background status update failed: {}", e);
                }
            }
        }
    }
    pub fn check_for_log_updates(&mut self) {
        while let Ok((job_id, log_result)) = self.log_result_rx.try_recv() {
            if let Some(job) = self
                .jobs_state
                .jobs
                .iter_mut()
                .find(|j| j.full_id == job_id)
            {
                log_info!("Received log update for job '{}'", job_id);
                match log_result {
                    Ok(lines) => job.logs = lines,
                    Err(e) => job.logs = vec![format!("[Error fetching log: {}]", e)],
                }
            }
        }
    }

    pub fn check_for_system_log_updates(&mut self) {
        while let Ok(line) = self.system_log_rx.try_recv() {
            self.system_logs.push(line);
            if self.system_logs.len() > 200 {
                self.system_logs.remove(0);
            }
        }
    }

    pub fn check_for_submission_updates(&mut self) {
        while let Ok(result) = self.submission_rx.try_recv() {
            match result {
                SubmissionResult::Success { submitted_job_ids } => {
                    log_info!(
                        "Received submission success for {} jobs.",
                        submitted_job_ids.len()
                    );
                    for job in self.jobs_state.jobs.iter_mut() {
                        if submitted_job_ids.contains(&job.full_id) && job.status == "Submitting..."
                        {
                            job.status = "Queued".to_string();
                        }
                    }
                }
                SubmissionResult::Failure {
                    failed_run_or_job_id,
                    affected_job_ids,
                    error,
                } => {
                    log_info!(
                        "Received submission failure for '{}': {} (affected {} jobs)",
                        failed_run_or_job_id,
                        error,
                        affected_job_ids.len()
                    );
                    for job in self.jobs_state.jobs.iter_mut() {
                        if affected_job_ids.contains(&job.full_id) && job.status == "Submitting..."
                        {
                            job.status = "Submit Failed".to_string();
                        }
                    }
                }
            }
            self.jobs_state.rebuild_display_list(&self.lab);
        }
    }

    pub fn on_tick(&mut self) {
        if !self.is_loading {
            self.update_history_data();
        }
    }

    fn calculate_current_counts(&self) -> (StatusCounts, usize) {
        let mut counts = StatusCounts::default();
        let mut current_completed_count: usize = 0;

        if self.jobs_state.jobs.is_empty() {
            return (counts, current_completed_count);
        }

        for job in &self.jobs_state.jobs {
            counts.total += 1;
            match job.status.as_str() {
                "Succeeded" => {
                    counts.succeeded += 1;
                    current_completed_count += 1;
                }
                "Failed" | "Submit Failed" => {
                    counts.failed += 1;
                    current_completed_count += 1;
                }
                "Running" => counts.running += 1,
                "Pending" => counts.pending += 1,
                "Queued" => counts.queued += 1,
                "Blocked" => counts.blocked += 1,
                "Submitting..." => counts.submitting += 1,
                _ => counts.unknown += 1,
            }
        }
        (counts, current_completed_count)
    }

    fn update_history_data(&mut self) {
        let (counts, current_completed_count) = self.calculate_current_counts();
        if counts.total == 0 {
            return;
        }

        let newly_completed = current_completed_count.saturating_sub(self.last_completed_count);
        self.last_completed_count = current_completed_count;

        self.status_history.push_back(counts);

        self.completion_rate_history
            .push_back(newly_completed as f64);
    }

    pub fn on_selection_change(&mut self) {
        let selected_row_id = self.jobs_state.table_state.selected().and_then(|i| {
            self.jobs_state
                .display_rows
                .get(i)
                .map(|row| row.id.clone())
        });

        let mut job_id_to_watch: Option<JobId> = None;
        if let Some(row_id) = selected_row_id {
            if let Some(last_segment) = row_id.split('/').next_back() {
                if let Some(job_id_str) = last_segment.strip_prefix("job:") {
                    let job_id = JobId(job_id_str.to_string());

                    let master_index = self
                        .jobs_state
                        .jobs
                        .iter()
                        .position(|j| j.full_id == job_id);
                    self.update_context_for_job(master_index);
                    job_id_to_watch = Some(job_id);
                }
            }
        }

        if let Some(job_id) = job_id_to_watch {
            self.log_cmd_tx.send(LogPollerCommand::Watch(job_id)).ok();
        } else {
            self.log_cmd_tx.send(LogPollerCommand::Stop).ok();
        }
    }

    pub fn next_job(&mut self) {
        self.jobs_state.next();
        self.on_selection_change();
    }

    pub fn previous_job(&mut self) {
        self.jobs_state.previous();
        self.on_selection_change();
    }

    pub fn scroll_down_half_page(&mut self) {
        self.jobs_state.scroll_down_half();
        self.on_selection_change();
    }

    pub fn scroll_up_half_page(&mut self) {
        self.jobs_state.scroll_up_half();
        self.on_selection_change();
    }

    pub fn next_target_cell(&mut self) {
        self.targets_state.next_cell();
    }

    pub fn previous_target_cell(&mut self) {
        self.targets_state.previous_cell();
    }

    pub fn toggle_target_cell_edit(&mut self) {
        self.targets_state.toggle_edit();
    }

    pub fn next_target_cell_value(&mut self) {
        self.targets_state.cycle_value_next();
    }

    pub fn previous_target_cell_value(&mut self) {
        self.targets_state.cycle_value_prev();
    }

    pub fn next_target(&mut self) {
        self.targets_state.next();
    }

    pub fn previous_target(&mut self) {
        self.targets_state.previous();
    }

    pub fn set_active_target(&mut self) {
        self.targets_state.set_active();
        log_info!(
            "Active target changed to: {}",
            self.targets_state.get_active_target_name()
        );
        self.is_loading = true;
        self.jobs_state.reset_statuses();
        self.jobs_state.rebuild_display_list(&self.lab);
    }
    pub fn set_focused_panel(&mut self, panel: PanelFocus) {
        self.focused_panel = panel;
    }

    pub fn increase_tick_rate(&mut self) {
        let new_millis = (self.tick_rate.as_millis() + 250).min(10000);
        self.tick_rate = Duration::from_millis(new_millis as u64);
    }

    pub fn decrease_tick_rate(&mut self) {
        let new_millis = self.tick_rate.as_millis().saturating_sub(250).max(250);
        self.tick_rate = Duration::from_millis(new_millis as u64);
    }

    pub fn quit(&mut self) {
        log_info!("Quit action triggered.");
        self.should_quit = true;
    }

    pub fn toggle_reverse(&mut self) {
        self.jobs_state.is_reversed = !self.jobs_state.is_reversed;
        self.jobs_state.rebuild_display_list(&self.lab);
    }

    pub fn toggle_selection_and_move_down(&mut self) {
        let current_selection = self.jobs_state.table_state.selected();

        if let Some(selected_idx_in_display) = current_selection {
            if let Some(row) = self.jobs_state.display_rows.get(selected_idx_in_display) {
                if !self.jobs_state.selected_jobs.remove(&row.id) {
                    self.jobs_state.selected_jobs.insert(row.id.clone());
                }
            }
            self.next_job();
        } else if !self.jobs_state.display_rows.is_empty() {
            self.jobs_state.table_state.select(Some(0));
            if let Some(row) = self.jobs_state.display_rows.first() {
                self.jobs_state.selected_jobs.insert(row.id.clone());
            }
            self.next_job();
        }
    }

    pub fn toggle_collapse_selected(&mut self) {
        if !self.jobs_state.is_tree_view {
            return;
        }
        if let Some(selected_idx) = self.jobs_state.table_state.selected() {
            if let Some(row) = self.jobs_state.display_rows.get(selected_idx) {
                if !self.jobs_state.collapsed_nodes.remove(&row.id) {
                    self.jobs_state.collapsed_nodes.insert(row.id.clone());
                }
                self.jobs_state.rebuild_display_list(&self.lab);
            }
        }
    }

    pub fn clear_selection(&mut self) {
        self.jobs_state.selected_jobs.clear();
    }

    pub fn select_all(&mut self) {
        self.jobs_state.selected_jobs = self
            .jobs_state
            .display_rows
            .iter()
            .map(|row| row.id.clone())
            .collect();
    }
    pub fn next_status_filter(&mut self) {
        let current_index = STATUS_FILTERS
            .iter()
            .position(|&s| s == self.jobs_state.status_filter)
            .unwrap_or(0);
        let next_index = (current_index + 1) % STATUS_FILTERS.len();
        log_info!(
            "Status filter changed to: {}",
            STATUS_FILTERS[next_index].as_str()
        );
        self.jobs_state.status_filter = STATUS_FILTERS[next_index];
        self.jobs_state.rebuild_display_list(&self.lab);
    }

    pub fn previous_status_filter(&mut self) {
        let current_index = STATUS_FILTERS
            .iter()
            .position(|&s| s == self.jobs_state.status_filter)
            .unwrap_or(0);
        let prev_index = if current_index == 0 {
            STATUS_FILTERS.len() - 1
        } else {
            current_index - 1
        };
        log_info!(
            "Status filter changed to: {}",
            STATUS_FILTERS[prev_index].as_str()
        );
        self.jobs_state.status_filter = STATUS_FILTERS[prev_index];
        self.jobs_state.rebuild_display_list(&self.lab);
    }
    pub fn rebuild_display_list(&mut self) {
        self.jobs_state.rebuild_display_list(&self.lab);
        self.on_selection_change();
    }
    pub fn open_job_definition_selected(&mut self) {
        let selected_idx = if let Some(i) = self.jobs_state.table_state.selected() {
            i
        } else {
            return;
        };

        let row = if let Some(r) = self.jobs_state.display_rows.get(selected_idx) {
            r
        } else {
            return;
        };

        let job_id = match &row.item {
            TuiRowItem::Job { job } => &job.full_id,
            TuiRowItem::Run { .. } => return,
        };

        let job_def = if let Some(j) = self.lab.jobs.get(job_id) {
            j
        } else {
            return;
        };

        let exe = job_def
            .executables
            .get("main")
            .or_else(|| job_def.executables.get("scatter"));

        let exe_path_rel = if let Some(e) = exe {
            &e.path
        } else {
            return;
        };

        let target_name = self.targets_state.get_active_target_name();
        let config = self.client.config();
        let target_config = if let Some(t) = config.targets.get(&target_name) {
            t
        } else {
            return;
        };

        let artifacts_base = target_config.base_path.join("artifacts");
        let full_path = artifacts_base.join(exe_path_rel);

        if let Some(addr) = &target_config.address {
            self.pending_action = Some(ExternalAction::EditRemote {
                address: addr.clone(),
                paths: vec![full_path],
            });
        } else {
            self.pending_action = Some(ExternalAction::EditLocal(vec![full_path]));
        }
    }

    pub fn open_job_logs_selected(&mut self) {
        let selected_idx = if let Some(i) = self.jobs_state.table_state.selected() {
            i
        } else {
            return;
        };

        let row = if let Some(r) = self.jobs_state.display_rows.get(selected_idx) {
            r
        } else {
            return;
        };

        let job_id = match &row.item {
            TuiRowItem::Job { job } => &job.full_id,
            TuiRowItem::Run { .. } => return,
        };

        let target_name = self.targets_state.get_active_target_name();
        let config = self.client.config();
        let target_config = if let Some(t) = config.targets.get(&target_name) {
            t
        } else {
            return;
        };

        let job_repx_dir = target_config
            .base_path
            .join("outputs")
            .join(job_id.0.as_str())
            .join("repx");

        let stderr_log = job_repx_dir.join("stderr.log");
        let stdout_log = job_repx_dir.join("stdout.log");
        let paths = vec![stderr_log, stdout_log];

        if let Some(addr) = &target_config.address {
            self.pending_action = Some(ExternalAction::EditRemote {
                address: addr.clone(),
                paths,
            });
        } else {
            self.pending_action = Some(ExternalAction::EditLocal(paths));
        }
    }

    pub fn open_global_logs(&mut self) {
        let xdg_dirs = xdg::BaseDirectories::with_prefix("repx");
        if let Some(cache_home) = xdg_dirs.get_cache_home() {
            let repx_log = cache_home.join("repx.log");
            let tui_log = cache_home.join("repx-tui.log");
            self.pending_action = Some(ExternalAction::EditLocal(vec![repx_log, tui_log]));
        } else {
            repx_core::log_warn!("Could not determine XDG base directories for repx logs.");
        }
    }

    fn get_target_ids_for_action(&self) -> Vec<String> {
        let get_id = |path_id: &str| -> Option<String> {
            path_id
                .split('/')
                .next_back()
                .and_then(|segment| segment.split_once(':'))
                .map(|(_, id)| id.to_string())
        };
        if !self.jobs_state.selected_jobs.is_empty() {
            self.jobs_state
                .selected_jobs
                .iter()
                .filter_map(|s| get_id(s))
                .collect()
        } else if let Some(selected_idx) = self.jobs_state.table_state.selected() {
            if let Some(row) = self.jobs_state.display_rows.get(selected_idx) {
                let id_str = match &row.item {
                    TuiRowItem::Run { id } => id.to_string(),
                    TuiRowItem::Job { job } => job.full_id.to_string(),
                };
                vec![id_str]
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    }

    pub fn run_selected(&mut self) {
        let raw_selected_ids = self.get_target_ids_for_action();
        if raw_selected_ids.is_empty() {
            log_info!("'Run' action triggered but no items were selected/targeted.");
            return;
        }

        let mut selected_jobs_set = HashSet::new();
        for id_str in &raw_selected_ids {
            let run_id = repx_core::model::RunId(id_str.to_string());
            if let Some(run) = self.lab.runs.get(&run_id) {
                selected_jobs_set.extend(run.jobs.iter().cloned());
            } else if let Ok(resolved_ids) =
                repx_core::resolver::resolve_all_final_job_ids(&self.lab, &run_id)
            {
                for job_id in resolved_ids {
                    let dep_graph = engine::build_dependency_graph(&self.lab, job_id);
                    selected_jobs_set.extend(dep_graph);
                }
            }
        }

        if selected_jobs_set.is_empty() {
            log_info!("Selection resolved to no runnable jobs.");
            self.clear_selection();
            return;
        }

        let all_dependencies_in_selection: HashSet<JobId> = selected_jobs_set
            .iter()
            .flat_map(|job_id| {
                self.lab.jobs.get(job_id).map_or_else(Vec::new, |j| {
                    j.executables
                        .values()
                        .flat_map(|e| e.inputs.clone())
                        .collect()
                })
            })
            .filter_map(|mapping| mapping.job_id)
            .collect();

        let final_job_ids_to_submit: Vec<JobId> = selected_jobs_set
            .iter()
            .filter(|job_id| !all_dependencies_in_selection.contains(*job_id))
            .cloned()
            .collect();

        let ids_to_potentially_run: Vec<String> =
            final_job_ids_to_submit.into_iter().map(|id| id.0).collect();

        let ids_to_run: Vec<String> = ids_to_potentially_run
            .into_iter()
            .filter(|id_str| {
                if let Some(job) = self.jobs_state.jobs.iter().find(|j| j.full_id.0 == *id_str) {
                    let is_submittable = !matches!(
                        job.status.as_str(),
                        "Succeeded" | "Running" | "Queued" | "Submitting..."
                    );
                    if !is_submittable {
                        log_info!(
                            "Skipping submission for final job '{}' with status '{}'",
                            id_str,
                            job.status
                        );
                    }
                    is_submittable
                } else {
                    true
                }
            })
            .collect();

        if ids_to_run.is_empty() {
            log_info!("All selected items are already completed or in progress. No action taken.");
            self.clear_selection();
            self.rebuild_display_list();
            return;
        }

        let all_jobs_to_submit: HashSet<JobId> = ids_to_run
            .iter()
            .flat_map(|id_str| {
                let job_id = repx_core::model::JobId(id_str.to_string());
                engine::build_dependency_graph(&self.lab, &job_id)
            })
            .collect();

        log_info!(
            "Planning to submit {} jobs across {} final job submissions.",
            all_jobs_to_submit.len(),
            ids_to_run.len()
        );
        for job in self.jobs_state.jobs.iter_mut() {
            if all_jobs_to_submit.contains(&job.full_id)
                && !matches!(job.status.as_str(), "Succeeded" | "Running" | "Queued")
            {
                job.status = "Submitting...".to_string();
            }
        }
        let target_name = self.targets_state.get_active_target_name();
        let active_tui_target = self
            .targets_state
            .items
            .iter()
            .find(|t| t.name == target_name)
            .unwrap();
        let scheduler = active_tui_target
            .get_selected_scheduler()
            .as_str()
            .to_string();
        let execution_type = active_tui_target
            .get_selected_executor()
            .as_str()
            .to_string();

        let config = self.client.config();
        let target_config = config.targets.get(&target_name).unwrap();

        let num_jobs = if scheduler != "local" {
            None
        } else {
            target_config
                .local
                .as_ref()
                .and_then(|c| c.local_concurrency)
                .or_else(|| Some(num_cpus::get()))
        };

        let client_clone = self.client.clone();
        let submission_tx_clone = self.submission_tx.clone();
        let resources_clone = self.resources.clone();
        let run_specs_to_submit = ids_to_run;
        thread::spawn(move || {
            log_info!(
                "Submitting batch run for final jobs {:?} to target '{}'",
                &run_specs_to_submit,
                &target_name
            );

            let options = SubmitOptions {
                execution_type: Some(execution_type),
                resources: resources_clone,
                num_jobs,
                event_sender: None,
            };

            match client_clone.submit_batch_run(
                run_specs_to_submit.clone(),
                &target_name,
                &scheduler,
                options,
            ) {
                Ok(msg) => {
                    log_info!("Batch submission successful: {}", msg);
                    let _ = submission_tx_clone.send(SubmissionResult::Success {
                        submitted_job_ids: all_jobs_to_submit,
                    });
                }
                Err(e) => {
                    let err_string = e.to_string();
                    log_warn!("Batch submission failed: {}", err_string);
                    let _ = submission_tx_clone.send(SubmissionResult::Failure {
                        failed_run_or_job_id: run_specs_to_submit.join(", "),
                        affected_job_ids: all_jobs_to_submit,
                        error: err_string,
                    });
                }
            }
        });

        self.clear_selection();
        self.rebuild_display_list();
    }

    pub fn toggle_tree_view(&mut self) {
        self.jobs_state.is_tree_view = !self.jobs_state.is_tree_view;
        self.jobs_state.rebuild_display_list(&self.lab);
    }

    pub fn cancel_selected(&mut self) {
        let ids_to_cancel = self.get_target_ids_for_action();
        log_info!("'Cancel' action triggered for: {:?}", ids_to_cancel);

        for job_id_str in ids_to_cancel {
            let job_id = JobId(job_id_str);
            log_info!("Sending cancel request for job '{}'", job_id);
            let _ = self.client.cancel_job(job_id);
        }
        self.clear_selection();
    }

    pub fn debug_selected(&mut self) {}

    pub fn show_path_selected(&mut self) {}
    pub fn yank_selected_path(&mut self) {
        if let Some(path) = self.get_selected_job_path() {
            let target_name = self.targets_state.get_active_target_name();
            let config = self.client.config();
            let target_config = config.targets.get(&target_name);

            let is_remote = target_config.and_then(|t| t.address.as_ref()).is_some();
            let path_str = if is_remote {
                path.to_string_lossy().replace('\\', "/")
            } else {
                path.to_string_lossy().to_string()
            };

            thread::spawn(move || {
                let mut copied = false;
                match arboard::Clipboard::new() {
                    Ok(mut clipboard) => {
                        if let Err(e) = clipboard.set_text(path_str.clone()) {
                            log_warn!("Failed to yank path to clipboard (arboard): {}", e);
                        } else {
                            copied = true;
                        }
                    }
                    Err(e) => {
                        log_warn!("Failed to initialize clipboard: {}", e);
                    }
                }

                if copied {
                    log_info!("Yanked path: {}", path_str);
                }
            });
        } else {
            log_info!("No job selected to yank path from.");
        }
    }

    pub fn explore_selected_path(&mut self) {
        if let Some(path) = self.get_selected_job_path() {
            let target_name = self.targets_state.get_active_target_name();
            let config = self.client.config();
            let target_config = config.targets.get(&target_name);

            if let Some(addr) = target_config.and_then(|t| t.address.as_ref()) {
                self.pending_action = Some(ExternalAction::ExploreRemote {
                    address: addr.clone(),
                    path,
                });
            } else if path.exists() {
                self.pending_action = Some(ExternalAction::ExploreLocal(path));
            } else {
                log_warn!("Path does not exist: {}", path.display());
            }
        } else {
            log_info!("No job selected to explore.");
        }
    }
    pub fn consume_pending_action(&mut self) -> Option<ExternalAction> {
        self.pending_action.take()
    }

    fn get_selected_job_path(&self) -> Option<PathBuf> {
        let selected_idx = self.jobs_state.table_state.selected()?;
        let row = self.jobs_state.display_rows.get(selected_idx)?;

        match &row.item {
            TuiRowItem::Job { job } => {
                let target_name = self.targets_state.get_active_target_name();
                let config = self.client.config();
                let target_config = config.targets.get(&target_name)?;
                Some(
                    target_config
                        .base_path
                        .join("outputs")
                        .join(job.full_id.to_string())
                        .join("out"),
                )
            }
            TuiRowItem::Run { .. } => None,
        }
    }
    fn update_context_for_job(&mut self, master_index: Option<usize>) {
        if let Some(master_index) = master_index {
            if let Some(job) = self.jobs_state.jobs.get_mut(master_index) {
                if let Some(lab_job) = self.lab.jobs.get(&job.full_id) {
                    let dependencies: HashSet<_> = lab_job
                        .executables
                        .values()
                        .flat_map(|e| e.inputs.iter())
                        .filter_map(|m| m.job_id.as_ref())
                        .collect();
                    job.context_depends_on = dependencies
                        .iter()
                        .map(|d| d.short_id())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let dependents: Vec<_> = self
                        .lab
                        .jobs
                        .iter()
                        .filter(|(_, j)| {
                            j.executables
                                .values()
                                .flat_map(|e| &e.inputs)
                                .any(|m| m.job_id.as_ref() == Some(&job.full_id))
                        })
                        .map(|(id, _)| id.short_id())
                        .collect();
                    job.context_dependents = dependents.join(", ");
                }
            }
        }
    }

    pub fn go_to_top(&mut self) {
        if !self.jobs_state.display_rows.is_empty() {
            self.jobs_state.table_state.select(Some(0));
            self.on_selection_change();
        }
    }

    pub fn go_to_end(&mut self) {
        if !self.jobs_state.display_rows.is_empty() {
            let last_index = self.jobs_state.display_rows.len() - 1;
            self.jobs_state.table_state.select(Some(last_index));
            self.on_selection_change();
        }
    }
}

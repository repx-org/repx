use crate::model::{TargetState, TuiDisplayRow, TuiJob, TuiRowItem, TuiTarget};
use ratatui::widgets::TableState;
use repx_client::{error::ClientError, Client};
use repx_core::{
    config::Resources,
    engine, log_info, log_warn,
    model::{JobId, Lab},
};
use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const MAX_GRAPH_HISTORY: usize = 1000;

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
    pub fn to_str(&self) -> &'static str {
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

type StatusUpdate = Result<
    (
        std::collections::BTreeMap<repx_core::model::RunId, repx_core::engine::JobStatus>,
        std::collections::HashMap<repx_core::model::JobId, repx_core::engine::JobStatus>,
    ),
    ClientError,
>;

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

pub struct App {
    pub client: Arc<Client>,
    pub lab: Lab,
    pub table_state: TableState,
    pub targets_table_state: TableState,
    jobs: Vec<TuiJob>,
    pub display_rows: Vec<TuiDisplayRow>,
    pub targets: Vec<TuiTarget>,
    pub progress_data: VecDeque<f64>,
    pub throughput_data: VecDeque<f64>,
    pub tick_rate: Duration,
    pub should_quit: bool,
    pub input_mode: InputMode,
    pub filter_text: String,
    pub status_filter: StatusFilter,
    pub selected_jobs: HashSet<String>,
    pub collapsed_nodes: HashSet<String>,
    pub is_reversed: bool,
    status_rx: Receiver<TargetPollUpdate>,
    log_cmd_tx: Sender<LogPollerCommand>,
    log_result_rx: Receiver<LogUpdate>,
    submission_tx: Sender<SubmissionResult>,
    submission_rx: Receiver<SubmissionResult>,
    pub is_loading: bool,
    pub is_tree_view: bool,
    resources: Option<Resources>,
    pub active_target: Arc<Mutex<String>>,
    pub focused_panel: PanelFocus,
}

impl App {
    pub fn new(
        client: Client,
        status_rx: Receiver<TargetPollUpdate>,
        log_cmd_tx: Sender<LogPollerCommand>,
        log_result_rx: Receiver<LogUpdate>,
        submission_tx: Sender<SubmissionResult>,
        submission_rx: Receiver<SubmissionResult>,
        resources: Option<Resources>,
        initial_active_target: String,
        active_target: Arc<Mutex<String>>,
    ) -> Result<Self, ClientError> {
        log_info!("Initializing new App instance.");
        let targets = client
            .config()
            .targets
            .iter()
            .map(|(name, _target)| TuiTarget {
                name: name.clone(),
                state: if name == &initial_active_target {
                    TargetState::Active
                } else {
                    TargetState::Inactive
                },
                activity: vec![0.0; 100],
            })
            .collect();

        let lab = client.lab()?.clone();
        let tick_rate = client.config().tui_tick_rate();

        let mut app = Self {
            client: Arc::new(client),
            lab,
            table_state: TableState::default(),
            targets_table_state: TableState::default(),
            jobs: Vec::new(),
            display_rows: Vec::new(),
            targets,
            progress_data: VecDeque::with_capacity(MAX_GRAPH_HISTORY),
            throughput_data: VecDeque::with_capacity(MAX_GRAPH_HISTORY),
            tick_rate,
            should_quit: false,
            input_mode: InputMode::Normal,
            filter_text: String::new(),
            status_filter: StatusFilter::All,
            selected_jobs: HashSet::new(),
            collapsed_nodes: HashSet::new(),
            is_reversed: false,
            status_rx,
            log_cmd_tx,
            log_result_rx,
            submission_tx,
            submission_rx,
            is_loading: true,
            is_tree_view: true,
            resources,
            active_target,
            focused_panel: PanelFocus::Jobs,
        };

        app.build_initial_job_list();
        app.rebuild_display_list();
        log_info!("Performing initial data update.");

        if !app.display_rows.is_empty() {
            app.table_state.select(Some(0));
            app.on_selection_change();
        }
        if !app.targets.is_empty() {
            app.targets_table_state.select(Some(0));
        }

        log_info!("App initialized successfully.");
        Ok(app)
    }

    pub fn lab(&self) -> &Lab {
        &self.lab
    }

    pub fn check_for_updates(&mut self) {
        if let Ok(update_result) = self.status_rx.try_recv() {
            self.is_loading = false;
            match update_result {
                Ok((_target_name, job_statuses)) => {
                    log_info!("Received status update. Applying new statuses.");
                    self.apply_statuses(job_statuses);
                    self.rebuild_display_list();
                }
                Err(e) => {
                    log_warn!("Background status update failed: {}", e);
                }
            }
        }
    }

    pub fn check_for_log_updates(&mut self) {
        if let Ok((job_id, log_result)) = self.log_result_rx.try_recv() {
            if let Some(job) = self.jobs.iter_mut().find(|j| j.full_id == job_id) {
                log_info!("Received log update for job '{}'", job_id);
                match log_result {
                    Ok(lines) => job.logs = lines,
                    Err(e) => job.logs = vec![format!("[Error fetching log: {}]", e)],
                }
            }
        }
    }

    pub fn check_for_submission_updates(&mut self) {
        if let Ok(result) = self.submission_rx.try_recv() {
            match result {
                SubmissionResult::Success { submitted_job_ids } => {
                    log_info!(
                        "Received submission success for {} jobs.",
                        submitted_job_ids.len()
                    );
                    for job in self.jobs.iter_mut() {
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
                    for job in self.jobs.iter_mut() {
                        if affected_job_ids.contains(&job.full_id) && job.status == "Submitting..."
                        {
                            job.status = "Submit Failed".to_string();
                        }
                    }
                }
            }
            self.rebuild_display_list();
        }
    }

    fn build_initial_job_list(&mut self) {
        log_info!("Job list is empty, building initial list from lab definition.");
        let mut all_jobs = Vec::new();
        let mut sorted_runs: Vec<_> = self.lab.runs.iter().collect();
        sorted_runs.sort_by_key(|(k, _)| (*k).clone());
        for (run_id, run) in sorted_runs {
            let mut sorted_jobs: Vec<_> = run.jobs.clone();
            sorted_jobs.sort();
            for job_id in sorted_jobs {
                let short_id = job_id.short_id();
                let (id_part, name_part) = short_id
                    .split_once('-')
                    .map_or((short_id.as_str(), ""), |(id, name)| (id, name));

                let tui_job = TuiJob {
                    full_id: job_id.clone(),
                    id: id_part.to_string(),
                    name: name_part.to_string(),
                    run: run_id.to_string(),
                    worker: "-".to_string(),
                    elapsed: "-".to_string(),
                    status: "Unknown".to_string(),
                    context_depends_on: "-".to_string(),
                    context_dependents: "-".to_string(),
                    logs: vec!["Awaiting update...".to_string()],
                };
                all_jobs.push(tui_job);
            }
        }
        self.jobs = all_jobs;
    }

    fn apply_statuses(
        &mut self,
        job_statuses_from_target: std::collections::HashMap<JobId, engine::JobStatus>,
    ) {
        let full_job_statuses =
            repx_core::engine::determine_job_statuses(&self.lab, &job_statuses_from_target);

        for job in self.jobs.iter_mut() {
            if job.status == "Submitting..." {
                if let Some(status) = full_job_statuses.get(&job.full_id) {
                    if matches!(
                        status,
                        engine::JobStatus::Pending | engine::JobStatus::Blocked { .. }
                    ) {
                        continue;
                    }
                }
            }

            let (status_str, worker) = match full_job_statuses.get(&job.full_id) {
                Some(engine::JobStatus::Succeeded { location }) => ("Succeeded", location.clone()),
                Some(engine::JobStatus::Failed { location }) => ("Failed", location.clone()),
                Some(engine::JobStatus::Pending) => ("Pending", "-".to_string()),
                Some(engine::JobStatus::Queued) => ("Queued", "-".to_string()),
                Some(engine::JobStatus::Running) => ("Running", "-".to_string()),
                Some(engine::JobStatus::Blocked { .. }) => ("Blocked", "-".to_string()),
                None => ("Unknown", "-".to_string()),
            };
            job.status = status_str.to_string();
            job.worker = worker.split(':').last().unwrap_or(&worker).to_string();
        }
    }

    pub fn update_progress_graph(&mut self) {
        self.progress_data.clear();
        let job_rows: Vec<_> = self
            .display_rows
            .iter()
            .filter(|row| matches!(row.item, TuiRowItem::Job { .. }))
            .collect();
        if job_rows.is_empty() {
            self.progress_data.resize(MAX_GRAPH_HISTORY, 0.0);
            return;
        }
        let completed_count = job_rows
            .iter()
            .filter(|&&row| {
                if let TuiRowItem::Job { job } = &row.item {
                    job.status == "Succeeded"
                } else {
                    false
                }
            })
            .count();
        let progress = (completed_count as f64 / job_rows.len() as f64) * 100.0;

        self.progress_data.resize(MAX_GRAPH_HISTORY, progress);
    }

    pub fn on_selection_change(&mut self) {
        let selected_row_id = self
            .table_state
            .selected()
            .and_then(|i| self.display_rows.get(i).map(|row| row.id.clone()));

        let mut job_id_to_watch: Option<JobId> = None;

        if let Some(row_id) = selected_row_id {
            if let Some(last_segment) = row_id.split('/').last() {
                if let Some(job_id_str) = last_segment.strip_prefix("job:") {
                    let job_id = JobId(job_id_str.to_string());

                    let master_index = self.jobs.iter().position(|j| j.full_id == job_id);
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
        let max_len = self.display_rows.len();
        if max_len == 0 {
            self.table_state.select(None);
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => (i + 1).min(max_len - 1),
            None => 0,
        };
        self.table_state.select(Some(i));
        self.on_selection_change();
    }

    pub fn previous_job(&mut self) {
        let max_len = self.display_rows.len();
        if max_len == 0 {
            self.table_state.select(None);
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.table_state.select(Some(i));
        self.on_selection_change();
    }

    pub fn next_target(&mut self) {
        let max_len = self.targets.len();
        if max_len == 0 {
            self.targets_table_state.select(None);
            return;
        }
        let i = match self.targets_table_state.selected() {
            Some(i) => (i + 1).min(max_len - 1),
            None => 0,
        };
        self.targets_table_state.select(Some(i));
    }

    pub fn previous_target(&mut self) {
        let max_len = self.targets.len();
        if max_len == 0 {
            self.targets_table_state.select(None);
            return;
        }
        let i = match self.targets_table_state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.targets_table_state.select(Some(i));
    }

    pub fn set_active_target(&mut self) {
        if let Some(selected_idx) = self.targets_table_state.selected() {
            if let Some(target) = self.targets.get(selected_idx) {
                let new_active_target = target.name.clone();
                log_info!("Setting new active target: {}", new_active_target);

                *self.active_target.lock().unwrap() = new_active_target.clone();

                for t in self.targets.iter_mut() {
                    if t.name == new_active_target {
                        t.state = TargetState::Active;
                    } else if t.state == TargetState::Active {
                        t.state = TargetState::Inactive;
                    }
                }
            }
        }
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
        self.is_reversed = !self.is_reversed;
        self.rebuild_display_list();
    }

    pub fn toggle_selection_and_move_down(&mut self) {
        let current_selection = self.table_state.selected();

        if let Some(selected_idx_in_display) = current_selection {
            if let Some(row) = self.display_rows.get(selected_idx_in_display) {
                if !self.selected_jobs.remove(&row.id) {
                    self.selected_jobs.insert(row.id.clone());
                }
            }
            self.next_job();
        } else if !self.display_rows.is_empty() {
            self.table_state.select(Some(0));
            if let Some(row) = self.display_rows.get(0) {
                self.selected_jobs.insert(row.id.clone());
            }
            self.next_job();
        }
    }

    pub fn toggle_collapse_selected(&mut self) {
        if !self.is_tree_view {
            return;
        }
        if let Some(selected_idx) = self.table_state.selected() {
            if let Some(row) = self.display_rows.get(selected_idx) {
                if !self.collapsed_nodes.remove(&row.id) {
                    self.collapsed_nodes.insert(row.id.clone());
                }
                self.rebuild_display_list();
            }
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected_jobs.clear();
    }

    pub fn select_all(&mut self) {
        self.selected_jobs = self.display_rows.iter().map(|row| row.id.clone()).collect();
    }

    pub fn next_status_filter(&mut self) {
        let current_index = STATUS_FILTERS
            .iter()
            .position(|&s| s == self.status_filter)
            .unwrap_or(0);
        let next_index = (current_index + 1) % STATUS_FILTERS.len();
        log_info!(
            "Status filter changed to: {}",
            STATUS_FILTERS[next_index].to_str()
        );
        self.status_filter = STATUS_FILTERS[next_index];
        self.rebuild_display_list();
    }

    pub fn previous_status_filter(&mut self) {
        let current_index = STATUS_FILTERS
            .iter()
            .position(|&s| s == self.status_filter)
            .unwrap_or(0);
        let prev_index = if current_index == 0 {
            STATUS_FILTERS.len() - 1
        } else {
            current_index - 1
        };
        log_info!(
            "Status filter changed to: {}",
            STATUS_FILTERS[prev_index].to_str()
        );
        self.status_filter = STATUS_FILTERS[prev_index];
        self.rebuild_display_list();
    }

    pub fn rebuild_display_list(&mut self) {
        log_info!(
            "Applying filter. Text: '{}', Status: '{}', Reversed: {}",
            self.filter_text,
            self.status_filter.to_str(),
            self.is_reversed
        );
        let previously_selected_id = self
            .table_state
            .selected()
            .and_then(|i| self.display_rows.get(i))
            .map(|row| row.id.clone());

        self.display_rows.clear();
        let filter = self.filter_text.to_lowercase();

        if self.is_tree_view {
            self.build_tree_view(&filter);
        } else {
            let filtered_jobs = self
                .jobs
                .iter()
                .enumerate()
                .filter(|(_i, job)| {
                    let status_match = match self.status_filter {
                        StatusFilter::All => true,
                        _ => job.status == self.status_filter.to_str(),
                    };

                    let text_match = job.id.to_lowercase().contains(&filter)
                        || job.name.to_lowercase().contains(&filter)
                        || job.run.to_lowercase().contains(&filter);

                    status_match && text_match
                })
                .map(|(i, _job)| i)
                .collect::<Vec<_>>();

            for job_idx in filtered_jobs {
                let job = &self.jobs[job_idx];
                self.display_rows.push(TuiDisplayRow {
                    item: TuiRowItem::Job { job: job.clone() },
                    id: format!("job:{}", job.full_id),
                    depth: 0,
                    parent_prefix: "".to_string(),
                    is_last_child: false,
                });
            }
        }

        if !self.is_tree_view && self.is_reversed {
            self.display_rows.reverse();
        }

        let new_len = self.display_rows.len();
        let new_selected_index = if let Some(id) = previously_selected_id {
            self.display_rows
                .iter()
                .position(|r| r.id == id)
                .or(Some(0))
        } else {
            Some(0)
        };

        if new_len == 0 || new_selected_index.is_none() {
            self.table_state.select(None);
        } else if let Some(idx) = new_selected_index {
            self.table_state
                .select(Some(idx.min(new_len.saturating_sub(1))));
        } else {
            self.table_state.select(Some(0));
        }

        self.on_selection_change();
        self.update_progress_graph();
    }

    fn build_tree_view(&mut self, filter: &str) {
        let mut new_display_rows = Vec::new();

        let visible_job_ids: HashSet<JobId>;
        if filter.is_empty() && self.status_filter == StatusFilter::All {
            visible_job_ids = self.lab.jobs.keys().cloned().collect();
        } else {
            let directly_matching_job_ids: HashSet<JobId> = self
                .jobs
                .iter()
                .filter(|job| {
                    let status_match = match self.status_filter {
                        StatusFilter::All => true,
                        _ => job.status == self.status_filter.to_str(),
                    };
                    let text_match = filter.is_empty()
                        || job.id.to_lowercase().contains(filter)
                        || job.name.to_lowercase().contains(filter);
                    status_match && text_match
                })
                .map(|job| job.full_id.clone())
                .collect();

            let mut dependents_map: std::collections::HashMap<JobId, Vec<JobId>> =
                std::collections::HashMap::new();
            for (job_id, job) in &self.lab.jobs {
                for dep_id in job
                    .executables
                    .values()
                    .flat_map(|exe| exe.inputs.iter())
                    .filter_map(|mapping| mapping.job_id.as_ref())
                {
                    dependents_map
                        .entry(dep_id.clone())
                        .or_default()
                        .push(job_id.clone());
                }
            }
            let mut calculated_visible_ids = directly_matching_job_ids.clone();
            let mut queue: std::collections::VecDeque<_> =
                directly_matching_job_ids.iter().cloned().collect();
            while let Some(job_id) = queue.pop_front() {
                if let Some(dependents) = dependents_map.get(&job_id) {
                    for dependent_id in dependents {
                        if calculated_visible_ids.insert(dependent_id.clone()) {
                            queue.push_back(dependent_id.clone());
                        }
                    }
                }
            }
            visible_job_ids = calculated_visible_ids;
        }

        let mut run_ids: Vec<_> = self.lab.runs.keys().cloned().collect();
        run_ids.sort();
        let visible_runs: Vec<_> = run_ids
            .iter()
            .filter(|run_id| {
                let run = self.lab.runs.get(run_id).unwrap();
                let run_name_matches =
                    !filter.is_empty() && run_id.0.to_lowercase().contains(filter);
                let has_visible_jobs = run
                    .jobs
                    .iter()
                    .any(|job_id| visible_job_ids.contains(job_id));
                run_name_matches || has_visible_jobs
            })
            .cloned()
            .collect();

        let num_runs = visible_runs.len();
        for (i, run_id) in visible_runs.iter().enumerate() {
            let run = self.lab.runs.get(&run_id).unwrap();
            let run_unique_id = format!("run:{}", run_id);
            new_display_rows.push(TuiDisplayRow {
                item: TuiRowItem::Run { id: run_id.clone() },
                id: run_unique_id.clone(),
                depth: 0,
                is_last_child: i == num_runs - 1,
                parent_prefix: "".to_string(),
            });
            if !self.collapsed_nodes.contains(&run_unique_id) {
                let run_jobs_set: HashSet<_> = run.jobs.iter().collect();
                let mut dep_ids_in_run: HashSet<&JobId> = HashSet::new();
                for job_id in &run.jobs {
                    if let Some(job_def) = self.lab.jobs.get(job_id) {
                        for dep_id in job_def
                            .executables
                            .values()
                            .flat_map(|exe| exe.inputs.iter())
                            .filter_map(|mapping| mapping.job_id.as_ref())
                        {
                            if run_jobs_set.contains(dep_id) {
                                dep_ids_in_run.insert(dep_id);
                            }
                        }
                    }
                }

                let mut visible_top_level_jobs: Vec<_> = run_jobs_set
                    .iter()
                    .filter(|j| !dep_ids_in_run.contains(*j))
                    .map(|j| *j)
                    .filter(|job_id| visible_job_ids.contains(job_id))
                    .cloned()
                    .collect();
                visible_top_level_jobs.sort();
                if self.is_reversed {
                    visible_top_level_jobs.reverse();
                }
                let num_top_jobs = visible_top_level_jobs.len();
                let prefix = if i == num_runs - 1 { "    " } else { "│   " };
                for (j, job_id) in visible_top_level_jobs.iter().enumerate() {
                    Self::add_job_and_deps_to_tree_recursive(
                        &mut new_display_rows,
                        &self.lab,
                        &self.jobs,
                        &self.collapsed_nodes,
                        job_id,
                        1,
                        j == num_top_jobs - 1,
                        prefix.to_string(),
                        &visible_job_ids,
                        &run_unique_id,
                    );
                }
            }
        }
        self.display_rows = new_display_rows;
    }

    fn add_job_and_deps_to_tree_recursive(
        display_rows: &mut Vec<TuiDisplayRow>,
        lab: &Lab,
        all_tui_jobs: &[TuiJob],
        collapsed_nodes: &HashSet<String>,
        job_id: &JobId,
        depth: usize,
        is_last: bool,
        prefix: String,
        visible_job_ids: &HashSet<JobId>,
        parent_path: &str,
    ) {
        let job_instance_id = format!("{}/job:{}", parent_path, job_id);
        let tui_job = all_tui_jobs.iter().find(|j| &j.full_id == job_id).unwrap();

        display_rows.push(TuiDisplayRow {
            item: TuiRowItem::Job {
                job: tui_job.clone(),
            },
            id: job_instance_id.clone(),
            depth,
            is_last_child: is_last,
            parent_prefix: prefix.clone(),
        });

        if !collapsed_nodes.contains(&job_instance_id) {
            let sorted_deps = {
                let lab_job = lab.jobs.get(job_id).unwrap();
                let mut deps: Vec<_> = lab_job
                    .executables
                    .values()
                    .flat_map(|exe| exe.inputs.iter())
                    .filter_map(|m| m.job_id.clone())
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
                deps.sort();
                deps
            };

            let visible_deps: Vec<_> = sorted_deps
                .into_iter()
                .filter(|dep_id| visible_job_ids.contains(dep_id))
                .collect();

            let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
            let num_deps = visible_deps.len();
            for (i, dep_id) in visible_deps.iter().enumerate() {
                Self::add_job_and_deps_to_tree_recursive(
                    display_rows,
                    lab,
                    all_tui_jobs,
                    collapsed_nodes,
                    dep_id,
                    depth + 1,
                    i == num_deps - 1,
                    new_prefix.clone(),
                    visible_job_ids,
                    &job_instance_id,
                );
            }
        }
    }

    fn get_target_ids_for_action(&self) -> Vec<String> {
        let get_id = |path_id: &str| -> Option<String> {
            path_id
                .split('/')
                .last()
                .and_then(|segment| segment.split_once(':'))
                .map(|(_, id)| id.to_string())
        };
        if !self.selected_jobs.is_empty() {
            self.selected_jobs
                .iter()
                .filter_map(|s| get_id(s))
                .collect()
        } else if let Some(selected_idx) = self.table_state.selected() {
            if let Some(row) = self.display_rows.get(selected_idx) {
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
            } else {
                if let Ok(resolved_ids) =
                    repx_core::resolver::resolve_all_final_job_ids(&self.lab, &run_id)
                {
                    for job_id in resolved_ids {
                        let dep_graph = engine::build_dependency_graph(&self.lab, job_id);
                        selected_jobs_set.extend(dep_graph);
                    }
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
                self.lab.jobs.get(job_id).map_or_else(
                    || Vec::new(),
                    |j| {
                        j.executables
                            .values()
                            .flat_map(|e| e.inputs.clone())
                            .collect()
                    },
                )
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
                if let Some(job) = self.jobs.iter().find(|j| j.full_id.0 == *id_str) {
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

        for job in self.jobs.iter_mut() {
            if all_jobs_to_submit.contains(&job.full_id) {
                if !matches!(job.status.as_str(), "Succeeded" | "Running" | "Queued") {
                    job.status = "Submitting...".to_string();
                }
            }
        }

        let target_name = self.active_target.lock().unwrap().clone();

        let config = self.client.config();
        let target = config.targets.get(&target_name);

        let scheduler = target
            .and_then(|t| t.scheduler.as_deref())
            .or(config.default_scheduler.as_deref())
            .unwrap_or("slurm")
            .to_string();

        let num_jobs = if scheduler != "local" {
            None
        } else {
            Some(
                target
                    .and_then(|t| t.local_concurrency)
                    .unwrap_or_else(num_cpus::get),
            )
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

            match client_clone.submit_batch_run(
                run_specs_to_submit.clone(),
                &target_name,
                &scheduler,
                resources_clone,
                num_jobs,
                None,
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
        self.is_tree_view = !self.is_tree_view;
        self.rebuild_display_list();
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

    pub fn follow_logs_selected(&mut self) {}

    fn update_context_for_job(&mut self, master_index: Option<usize>) {
        if let Some(master_index) = master_index {
            if let Some(job) = self.jobs.get_mut(master_index) {
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
        if !self.display_rows.is_empty() {
            self.table_state.select(Some(0));
            self.on_selection_change();
        }
    }

    pub fn go_to_end(&mut self) {
        if !self.display_rows.is_empty() {
            let last_index = self.display_rows.len() - 1;
            self.table_state.select(Some(last_index));
            self.on_selection_change();
        }
    }
}

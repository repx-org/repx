use crate::app::StatusFilter;
use crate::model::{TuiDisplayRow, TuiJob, TuiRowItem};
use ratatui::widgets::TableState;
use repx_core::engine::{self, JobStatus};
use repx_core::model::{JobId, Lab};
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone)]
enum FilterType {
    Global,
    Id,
    Name,
    Run,
    Params,
    Status,
}

struct ParsedFilter {
    filter_type: FilterType,
    term: String,
}

pub struct JobsState {
    pub jobs: Vec<TuiJob>,
    pub display_rows: Vec<TuiDisplayRow>,
    pub table_state: TableState,
    pub selected_jobs: HashSet<String>,
    pub collapsed_nodes: HashSet<String>,
    pub filter_text: String,
    pub status_filter: StatusFilter,
    pub is_reversed: bool,
    pub is_tree_view: bool,
    pub viewport_height: usize,
}

impl JobsState {
    pub fn new() -> Self {
        Self {
            jobs: Vec::new(),
            display_rows: Vec::new(),
            table_state: TableState::default(),
            selected_jobs: HashSet::new(),
            collapsed_nodes: HashSet::new(),
            filter_text: String::new(),
            status_filter: StatusFilter::All,
            is_reversed: false,
            is_tree_view: true,
            viewport_height: 0,
        }
    }

    pub fn init_from_lab(&mut self, lab: &Lab) {
        let mut all_jobs = Vec::new();
        let mut sorted_runs: Vec<_> = lab.runs.iter().collect();
        sorted_runs.sort_by_key(|(k, _)| (*k).clone());
        for (run_id, run) in sorted_runs {
            let mut sorted_jobs: Vec<_> = run.jobs.clone();
            sorted_jobs.sort();
            for job_id in sorted_jobs {
                let short_id = job_id.short_id();
                let (id_part, name_part) = short_id
                    .split_once('-')
                    .map_or((short_id.as_str(), ""), |(id, name)| (id, name));

                let job_def = lab.jobs.get(&job_id).unwrap();
                let tui_job = TuiJob {
                    full_id: job_id.clone(),
                    id: id_part.to_string(),
                    name: name_part.to_string(),
                    run: run_id.to_string(),
                    params: job_def.params.clone(),
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

    pub fn reset_statuses(&mut self) {
        for job in self.jobs.iter_mut() {
            job.status = "Unknown".to_string();
        }
    }

    pub fn apply_statuses(
        &mut self,
        lab: &Lab,
        statuses: std::collections::HashMap<JobId, engine::JobStatus>,
    ) {
        let full_job_statuses = engine::determine_job_statuses(lab, &statuses);

        for job in self.jobs.iter_mut() {
            if job.status == "Submitting..." {
                if let Some(status) = full_job_statuses.get(&job.full_id) {
                    if matches!(status, JobStatus::Pending | JobStatus::Blocked { .. }) {
                        continue;
                    }
                }
            }

            let status_str = match full_job_statuses.get(&job.full_id) {
                Some(JobStatus::Succeeded { .. }) => "Succeeded",
                Some(JobStatus::Failed { .. }) => "Failed",
                Some(JobStatus::Pending) => "Pending",
                Some(JobStatus::Queued) => "Queued",
                Some(JobStatus::Running) => "Running",
                Some(JobStatus::Blocked { .. }) => "Blocked",
                None => "Unknown",
            };
            job.status = status_str.to_string();
        }
    }
    pub fn next(&mut self) {
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
    }

    pub fn previous(&mut self) {
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
    }

    pub fn rebuild_display_list(&mut self, lab: &Lab) {
        let previously_selected_id = self
            .table_state
            .selected()
            .and_then(|i| self.display_rows.get(i))
            .map(|row| row.id.clone());

        self.display_rows.clear();
        let filters = self.parse_filter_text(&self.filter_text);

        if self.is_tree_view {
            self.build_tree_view(lab, &filters);
        } else {
            self.build_flat_list(&filters);
        }

        if !self.is_tree_view && self.is_reversed {
            self.display_rows.reverse();
        }

        self.restore_selection(previously_selected_id);
    }

    fn build_flat_list(&mut self, filters: &[ParsedFilter]) {
        let filtered_indices: Vec<usize> = self
            .jobs
            .iter()
            .enumerate()
            .filter(|(_i, job)| self.job_matches(job, filters))
            .map(|(i, _)| i)
            .collect();

        for idx in filtered_indices {
            let job = &self.jobs[idx];
            self.display_rows.push(TuiDisplayRow {
                item: TuiRowItem::Job {
                    job: Box::new(job.clone()),
                },
                id: format!("job:{}", job.full_id),
                depth: 0,
                parent_prefix: "".to_string(),
                is_last_child: false,
            });
        }
    }

    fn build_tree_view(&mut self, lab: &Lab, filters: &[ParsedFilter]) {
        let visible_job_ids = self.calculate_visible_job_ids(lab, filters);
        let mut run_ids: Vec<_> = lab.runs.keys().cloned().collect();
        run_ids.sort();

        let visible_runs: Vec<_> = run_ids
            .iter()
            .filter(|run_id| {
                let run = lab.runs.get(run_id).unwrap();
                let name_match = self.run_matches(&run_id.0, filters);
                let has_jobs = run.jobs.iter().any(|id| visible_job_ids.contains(id));
                name_match || has_jobs
            })
            .cloned()
            .collect();

        let num_runs = visible_runs.len();
        for (i, run_id) in visible_runs.iter().enumerate() {
            let run_unique_id = format!("run:{}", run_id);
            self.display_rows.push(TuiDisplayRow {
                item: TuiRowItem::Run { id: run_id.clone() },
                id: run_unique_id.clone(),
                depth: 0,
                is_last_child: i == num_runs - 1,
                parent_prefix: "".to_string(),
            });
            if !self.collapsed_nodes.contains(&run_unique_id) {
                self.add_run_children(
                    lab,
                    run_id,
                    &visible_job_ids,
                    &run_unique_id,
                    i == num_runs - 1,
                );
            }
        }
    }

    fn calculate_visible_job_ids(&self, lab: &Lab, filters: &[ParsedFilter]) -> HashSet<JobId> {
        if filters.is_empty() && self.status_filter == StatusFilter::All {
            return lab.jobs.keys().cloned().collect();
        }

        let directly_matching: HashSet<JobId> = self
            .jobs
            .iter()
            .filter(|job| self.job_matches(job, filters))
            .map(|job| job.full_id.clone())
            .collect();

        let mut dependents_map: std::collections::HashMap<JobId, Vec<JobId>> =
            std::collections::HashMap::new();
        for (job_id, job) in &lab.jobs {
            for dep_id in job
                .executables
                .values()
                .flat_map(|exe| exe.inputs.iter())
                .filter_map(|m| m.job_id.as_ref())
            {
                dependents_map
                    .entry(dep_id.clone())
                    .or_default()
                    .push(job_id.clone());
            }
        }

        let mut result = directly_matching.clone();
        let mut queue: VecDeque<_> = directly_matching.iter().cloned().collect();

        while let Some(job_id) = queue.pop_front() {
            if let Some(deps) = dependents_map.get(&job_id) {
                for dep in deps {
                    if result.insert(dep.clone()) {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }
        result
    }

    fn add_run_children(
        &mut self,
        lab: &Lab,
        run_id: &repx_core::model::RunId,
        visible_job_ids: &HashSet<JobId>,
        parent_path: &str,
        parent_is_last: bool,
    ) {
        let run = lab.runs.get(run_id).unwrap();
        let run_jobs_set: HashSet<_> = run.jobs.iter().collect();
        let mut dep_ids_in_run: HashSet<&JobId> = HashSet::new();

        for job_id in &run.jobs {
            if let Some(job) = lab.jobs.get(job_id) {
                for dep_id in job
                    .executables
                    .values()
                    .flat_map(|e| e.inputs.iter())
                    .filter_map(|m| m.job_id.as_ref())
                {
                    if run_jobs_set.contains(dep_id) {
                        dep_ids_in_run.insert(dep_id);
                    }
                }
            }
        }

        let mut top_jobs: Vec<_> = run_jobs_set
            .iter()
            .filter(|j| !dep_ids_in_run.contains(*j) && visible_job_ids.contains(*j))
            .cloned()
            .collect();
        top_jobs.sort();

        if self.is_reversed {
            top_jobs.reverse();
        }

        let prefix = if parent_is_last { "    " } else { "│   " };
        let count = top_jobs.len();

        for (j, job_id) in top_jobs.iter().enumerate() {
            self.add_job_recursive(
                lab,
                job_id,
                1,
                j == count - 1,
                prefix.to_string(),
                visible_job_ids,
                parent_path,
            );
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn add_job_recursive(
        &mut self,
        lab: &Lab,
        job_id: &JobId,
        depth: usize,
        is_last: bool,
        prefix: String,
        visible_job_ids: &HashSet<JobId>,
        parent_path: &str,
    ) {
        let job_instance_id = format!("{}/job:{}", parent_path, job_id);
        let tui_job = self.jobs.iter().find(|j| &j.full_id == job_id).unwrap();

        self.display_rows.push(TuiDisplayRow {
            item: TuiRowItem::Job {
                job: Box::new(tui_job.clone()),
            },
            id: job_instance_id.clone(),
            depth,
            is_last_child: is_last,
            parent_prefix: prefix.clone(),
        });

        if !self.collapsed_nodes.contains(&job_instance_id) {
            let mut deps: Vec<_> = lab
                .jobs
                .get(job_id)
                .unwrap()
                .executables
                .values()
                .flat_map(|e| e.inputs.iter())
                .filter_map(|m| m.job_id.clone())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            deps.sort();

            let visible_deps: Vec<_> = deps
                .into_iter()
                .filter(|d| visible_job_ids.contains(d))
                .collect();
            let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
            let count = visible_deps.len();

            for (i, dep_id) in visible_deps.iter().enumerate() {
                self.add_job_recursive(
                    lab,
                    dep_id,
                    depth + 1,
                    i == count - 1,
                    new_prefix.clone(),
                    visible_job_ids,
                    &job_instance_id,
                );
            }
        }
    }

    fn restore_selection(&mut self, previous_id: Option<String>) {
        let new_len = self.display_rows.len();
        let new_index = if let Some(id) = previous_id {
            self.display_rows
                .iter()
                .position(|r| r.id == id)
                .or(Some(0))
        } else {
            Some(0)
        };

        if new_len == 0 || new_index.is_none() {
            self.table_state.select(None);
        } else if let Some(idx) = new_index {
            self.table_state
                .select(Some(idx.min(new_len.saturating_sub(1))));
        }
    }

    pub fn scroll_down_half(&mut self) {
        if self.viewport_height == 0 || self.display_rows.is_empty() {
            return;
        }
        let current = self.table_state.selected().unwrap_or(0);
        let next = (current + self.viewport_height / 2).min(self.display_rows.len() - 1);
        self.table_state.select(Some(next));
    }

    pub fn scroll_up_half(&mut self) {
        if self.viewport_height == 0 || self.display_rows.is_empty() {
            return;
        }
        let current = self.table_state.selected().unwrap_or(0);
        let next = current.saturating_sub(self.viewport_height / 2);
        self.table_state.select(Some(next));
    }

    fn parse_filter_text(&self, text: &str) -> Vec<ParsedFilter> {
        if text.trim().is_empty() {
            return Vec::new();
        }

        let mut filters = Vec::new();
        let lower_text = text.to_lowercase();

        if !lower_text.contains('%') {
            filters.push(ParsedFilter {
                filter_type: FilterType::Global,
                term: lower_text,
            });
            return filters;
        }

        let parts: Vec<&str> = lower_text.split('%').collect();
        if let Some(first) = parts.first() {
            if !first.trim().is_empty() {
                filters.push(ParsedFilter {
                    filter_type: FilterType::Global,
                    term: first.trim().to_string(),
                });
            }
        }

        for part in parts.iter().skip(1) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (col_prefix, term) = match part.split_once(char::is_whitespace) {
                Some((c, t)) => (c, t.trim()),
                None => (part, ""),
            };

            let filter_type = if self.matches_column(col_prefix, &["id", "jobid"]) {
                Some(FilterType::Id)
            } else if self.matches_column(col_prefix, &["name", "item"]) {
                Some(FilterType::Name)
            } else if self.matches_column(col_prefix, &["run"]) {
                Some(FilterType::Run)
            } else if self.matches_column(col_prefix, &["params", "parameters", "param"]) {
                Some(FilterType::Params)
            } else if self.matches_column(col_prefix, &["status"]) {
                Some(FilterType::Status)
            } else {
                None
            };

            if let Some(ft) = filter_type {
                filters.push(ParsedFilter {
                    filter_type: ft,
                    term: term.to_string(),
                });
            }
        }
        filters
    }

    fn matches_column(&self, prefix: &str, candidates: &[&str]) -> bool {
        candidates.iter().any(|c| c.starts_with(prefix))
    }

    fn job_matches(&self, job: &TuiJob, filters: &[ParsedFilter]) -> bool {
        let status_match = match self.status_filter {
            StatusFilter::All => true,
            _ => job.status == self.status_filter.as_str(),
        };
        if !status_match {
            return false;
        }
        if filters.is_empty() {
            return true;
        }
        for filter in filters {
            let matches = match filter.filter_type {
                FilterType::Global => {
                    job.id.to_lowercase().contains(&filter.term)
                        || job.name.to_lowercase().contains(&filter.term)
                        || job.run.to_lowercase().contains(&filter.term)
                }
                FilterType::Id => job.id.to_lowercase().contains(&filter.term),
                FilterType::Name => job.name.to_lowercase().contains(&filter.term),
                FilterType::Run => job.run.to_lowercase().contains(&filter.term),
                FilterType::Params => self.params_match(&job.params, &filter.term),
                FilterType::Status => job.status.to_lowercase().contains(&filter.term),
            };
            if !matches {
                return false;
            }
        }
        true
    }

    fn run_matches(&self, run_id: &str, filters: &[ParsedFilter]) -> bool {
        if filters.is_empty() {
            return false;
        }
        for filter in filters {
            let matches = match filter.filter_type {
                FilterType::Global => run_id.to_lowercase().contains(&filter.term),
                FilterType::Run => run_id.to_lowercase().contains(&filter.term),
                _ => false,
            };
            if !matches {
                return false;
            }
        }
        true
    }

    fn params_match(&self, params: &serde_json::Value, term: &str) -> bool {
        if let Some(obj) = params.as_object() {
            for (k, v) in obj {
                if k.to_lowercase().contains(term) {
                    return true;
                }
                let val_str = if let Some(s) = v.as_str() {
                    s.to_string()
                } else {
                    v.to_string()
                };
                if val_str.to_lowercase().contains(term) {
                    return true;
                }
                // Check "key=val" format
                let combined = format!("{}={}", k, val_str);
                if combined.to_lowercase().contains(term) {
                    return true;
                }
            }
            false
        } else {
            params.to_string().to_lowercase().contains(term)
        }
    }
}

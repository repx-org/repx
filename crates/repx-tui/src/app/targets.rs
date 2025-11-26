use crate::model::{TargetState as StateEnum, TuiTarget};
use ratatui::widgets::TableState;
use std::sync::{Arc, Mutex};

pub struct TargetsState {
    pub items: Vec<TuiTarget>,
    pub table_state: TableState,
    pub focused_column: usize,
    pub is_editing_cell: bool,
    pub active_target_ref: Arc<Mutex<String>>,
}

impl TargetsState {
    pub fn new(items: Vec<TuiTarget>, active_target_ref: Arc<Mutex<String>>) -> Self {
        let mut state = Self {
            items,
            table_state: TableState::default(),
            focused_column: 3,
            is_editing_cell: false,
            active_target_ref,
        };
        if !state.items.is_empty() {
            state.table_state.select(Some(0));
        }
        state
    }

    pub fn next(&mut self) {
        let max = self.items.len();
        if max == 0 {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => (i + 1).min(max - 1),
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let max = self.items.len();
        if max == 0 {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn next_cell(&mut self) {
        self.focused_column = (self.focused_column + 1).min(3);
    }

    pub fn previous_cell(&mut self) {
        self.focused_column = self.focused_column.saturating_sub(1).max(1);
    }

    pub fn toggle_edit(&mut self) {
        if self.focused_column == 1 || self.focused_column == 2 {
            self.is_editing_cell = !self.is_editing_cell;
        }
    }

    pub fn cycle_value_next(&mut self) {
        if let Some(idx) = self.table_state.selected() {
            if let Some(target) = self.items.get_mut(idx) {
                match self.focused_column {
                    1 => {
                        let scheduler = target.get_selected_scheduler();
                        if let Some(executors) = target.available_executors.get(&scheduler) {
                            if !executors.is_empty() {
                                target.selected_executor_idx =
                                    (target.selected_executor_idx + 1) % executors.len();
                            }
                        }
                    }
                    2 => {
                        if !target.available_schedulers.is_empty() {
                            target.selected_scheduler_idx = (target.selected_scheduler_idx + 1)
                                % target.available_schedulers.len();
                            target.selected_executor_idx = 0;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn cycle_value_prev(&mut self) {
        if let Some(idx) = self.table_state.selected() {
            if let Some(target) = self.items.get_mut(idx) {
                match self.focused_column {
                    1 => {
                        let scheduler = target.get_selected_scheduler();
                        if let Some(executors) = target.available_executors.get(&scheduler) {
                            if !executors.is_empty() {
                                target.selected_executor_idx = if target.selected_executor_idx == 0
                                {
                                    executors.len() - 1
                                } else {
                                    target.selected_executor_idx - 1
                                };
                            }
                        }
                    }
                    2 => {
                        if !target.available_schedulers.is_empty() {
                            target.selected_scheduler_idx = if target.selected_scheduler_idx == 0 {
                                target.available_schedulers.len() - 1
                            } else {
                                target.selected_scheduler_idx - 1
                            };
                            target.selected_executor_idx = 0;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn set_active(&mut self) {
        if let Some(idx) = self.table_state.selected() {
            if let Some(target) = self.items.get(idx) {
                let new_name = target.name.clone();
                *self.active_target_ref.lock().unwrap() = new_name.clone();

                for t in self.items.iter_mut() {
                    if t.name == new_name {
                        t.state = StateEnum::Active;
                    } else if t.state == StateEnum::Active {
                        t.state = StateEnum::Inactive;
                    }
                }
            }
        }
    }

    pub fn get_active_target_name(&self) -> String {
        self.active_target_ref.lock().unwrap().clone()
    }
}

use crate::app::{App, InputMode, PanelFocus};
use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};
use repx_core::log_debug;
use std::io;
use std::time::Duration;

pub fn handle_key_event(key: KeyEvent, app: &mut App) {
    log_debug!(
        "Key event received: {:?}, Mode: {:?}, Focus: {:?}",
        key.code,
        app.input_mode,
        app.focused_panel
    );

    if app.input_mode == InputMode::Editing {
        handle_editing_mode_key_event(key, app);
        return;
    }

    if app.input_mode == InputMode::SpaceMenu {
        handle_space_menu_key_event(key, app);
        return;
    }

    if app.input_mode == InputMode::GMenu {
        handle_g_menu_key_event(key, app);
        return;
    }

    match key.code {
        KeyCode::Char('q') => app.quit(),
        KeyCode::Char(' ') => {
            app.input_mode = InputMode::SpaceMenu;
            return;
        }
        KeyCode::Char('g') => {
            app.input_mode = InputMode::GMenu;
            return;
        }
        KeyCode::Char('2') => {
            app.set_focused_panel(PanelFocus::Jobs);
            return;
        }
        KeyCode::Char('4') => {
            app.set_focused_panel(PanelFocus::Targets);
            return;
        }
        _ => {}
    }

    match app.focused_panel {
        PanelFocus::Jobs => handle_jobs_panel_key_event(key, app),
        PanelFocus::Targets => handle_targets_panel_key_event(key, app),
    }
}

fn handle_jobs_panel_key_event(key: KeyEvent, app: &mut App) {
    if key.modifiers == KeyModifiers::CONTROL {
        match key.code {
            KeyCode::Char('d') => {
                app.scroll_down_half_page();
                return;
            }
            KeyCode::Char('u') => {
                app.scroll_up_half_page();
                return;
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Down | KeyCode::Char('j') => app.next_job(),
        KeyCode::Up | KeyCode::Char('k') => app.previous_job(),
        KeyCode::Char('+') | KeyCode::Char('=') => app.increase_tick_rate(),
        KeyCode::Char('-') => app.decrease_tick_rate(),
        KeyCode::Char('t') => app.toggle_tree_view(),
        KeyCode::Char('.') => app.toggle_collapse_selected(),
        KeyCode::Char('x') => app.toggle_selection_and_move_down(),
        KeyCode::Esc => {
            app.clear_selection();
        }
        KeyCode::Char('/') | KeyCode::Char('f') => {
            app.input_mode = InputMode::Editing;
            app.jobs_state.filter_cursor_position = app.jobs_state.filter_text.len();
        }
        KeyCode::Char('l') => app.next_status_filter(),
        KeyCode::Char('h') => app.previous_status_filter(),
        KeyCode::Char('r') => app.toggle_reverse(),
        KeyCode::Char('%') => app.select_all(),
        _ => {}
    }
}
fn handle_targets_panel_key_event(key: KeyEvent, app: &mut App) {
    if app.targets_state.is_editing_cell {
        match key.code {
            KeyCode::Right | KeyCode::Char('l') => app.next_target_cell_value(),
            KeyCode::Left | KeyCode::Char('h') => app.previous_target_cell_value(),
            KeyCode::Enter | KeyCode::Esc => app.toggle_target_cell_edit(),
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => app.next_target(),
        KeyCode::Up | KeyCode::Char('k') => app.previous_target(),
        KeyCode::Right | KeyCode::Char('l') => app.next_target_cell(),
        KeyCode::Left | KeyCode::Char('h') => app.previous_target_cell(),
        KeyCode::Enter => match app.targets_state.focused_column {
            1 | 2 => app.toggle_target_cell_edit(),
            3 => app.set_active_target(),
            _ => {}
        },
        _ => {}
    }
}
fn handle_space_menu_key_event(key: KeyEvent, app: &mut App) {
    match key.code {
        KeyCode::Char('r') => {
            app.run_selected();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('c') => {
            app.cancel_selected();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('d') => {
            app.debug_selected();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('p') => {
            app.show_path_selected();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('l') => {
            app.open_global_logs();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('y') => {
            app.yank_selected_path();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('e') => {
            app.explore_selected_path();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char(' ') => {
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}

fn handle_g_menu_key_event(key: KeyEvent, app: &mut App) {
    match key.code {
        KeyCode::Char('g') => {
            app.go_to_top();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('e') => {
            app.go_to_end();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('d') => {
            app.open_job_definition_selected();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('l') => {
            app.open_job_logs_selected();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char(' ') => {
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}
fn handle_editing_mode_key_event(key: KeyEvent, app: &mut App) {
    match key.code {
        KeyCode::Char(c) => {
            let pos = app.jobs_state.filter_cursor_position;
            app.jobs_state.filter_text.insert(pos, c);
            app.jobs_state.filter_cursor_position = pos + c.len_utf8();
            app.rebuild_display_list();
        }
        KeyCode::Backspace => {
            let pos = app.jobs_state.filter_cursor_position;
            if pos > 0 {
                let prev_char_pos = app.jobs_state.filter_text[..pos]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                app.jobs_state.filter_text.remove(prev_char_pos);
                app.jobs_state.filter_cursor_position = prev_char_pos;
                app.rebuild_display_list();
            }
        }
        KeyCode::Left => {
            let pos = app.jobs_state.filter_cursor_position;
            if pos > 0 {
                let prev_char_pos = app.jobs_state.filter_text[..pos]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                app.jobs_state.filter_cursor_position = prev_char_pos;
            }
        }
        KeyCode::Right => {
            let pos = app.jobs_state.filter_cursor_position;
            let len = app.jobs_state.filter_text.len();
            if pos < len {
                let next_char_pos = app.jobs_state.filter_text[pos..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| pos + i)
                    .unwrap_or(len);
                app.jobs_state.filter_cursor_position = next_char_pos;
            }
        }
        KeyCode::Home => {
            app.jobs_state.filter_cursor_position = 0;
        }
        KeyCode::End => {
            app.jobs_state.filter_cursor_position = app.jobs_state.filter_text.len();
        }
        KeyCode::Enter | KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}
pub fn poll_event(timeout: Duration) -> io::Result<Option<CrosstermEvent>> {
    if event::poll(timeout)? {
        Ok(Some(crossterm::event::read()?))
    } else {
        Ok(None)
    }
}

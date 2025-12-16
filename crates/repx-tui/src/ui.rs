use crate::{
    app::{App, InputMode, PanelFocus},
    model::{TargetState, TuiDisplayRow, TuiRowItem},
    widgets::{color, BrailleGraph, GraphDirection, StackedBarChart},
};
use chrono::Local;
use ratatui::{
    prelude::*,
    widgets::{
        Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table,
    },
};
use repx_core::model::Lab;
use repx_core::theme::ElementStyle;
use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;

fn shorten_nix_store_path(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("/nix/store/") {
        if rest.len() > 32 {
            let (hash, suffix) = rest.split_at(32);
            if suffix.starts_with('-') && hash.chars().all(|c| c.is_ascii_alphanumeric()) {
                return format!("{}..{}", &hash[..7], suffix);
            }
        }
    }
    s.to_string()
}

fn get_color(app: &App, name: &str) -> Color {
    app.theme
        .palette
        .get(name)
        .and_then(|hex| Color::from_str(hex).ok())
        .unwrap_or(Color::Reset)
}

fn get_style(app: &App, element: &ElementStyle) -> Style {
    let color = get_color(app, &element.color);
    let mut style = Style::default().fg(color);
    for s in &element.styles {
        style = match s.as_str() {
            "bold" => style.add_modifier(Modifier::BOLD),
            "dimmed" => style.add_modifier(Modifier::DIM),
            "italic" => style.add_modifier(Modifier::ITALIC),
            "underlined" => style.add_modifier(Modifier::UNDERLINED),
            _ => style,
        }
    }
    style
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(0)])
        .split(f.area());

    draw_overview_panel(f, main_chunks[0], app);

    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(main_chunks[1]);

    draw_left_column(f, bottom_chunks[0], app);
    draw_right_column(f, bottom_chunks[1], app);

    if app.input_mode == InputMode::SpaceMenu {
        draw_space_menu_popup(f, f.area(), app);
    } else if app.input_mode == InputMode::GMenu {
        draw_g_menu_popup(f, f.area(), app);
    }
}

fn draw_overview_panel(f: &mut Frame, area: Rect, app: &mut App) {
    let overview_border_style = get_style(app, &app.theme.elements.panels.overview);
    let targets_border_style = get_style(app, &app.theme.elements.panels.targets);
    let loading_indicator = if app.is_loading { " [Updating...]" } else { "" };
    let store_path_str = {
        let active_target_name = app.targets_state.get_active_target_name();
        app.client
            .config()
            .targets
            .get(&active_target_name)
            .map(|t| t.base_path.display().to_string())
            .unwrap_or_else(|| "[unknown]".to_string())
    };
    let githash_short = if let Some(hash) = app.lab.git_hash.strip_suffix("-dirty") {
        format!("{}-dirty", hash.chars().take(7).collect::<String>())
    } else {
        app.lab.git_hash.chars().take(13).collect::<String>()
    };
    let rate_text = format!("{}ms", app.tick_rate.as_millis());
    let current_time = Local::now().format("%H:%M:%S").to_string();
    let overview_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(overview_border_style)
        .title_top(
            Line::from(vec![
                Span::styled("─┐", overview_border_style),
                Span::styled("¹", Style::default().add_modifier(Modifier::DIM)),
                Span::styled(
                    "OVERVIEW",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("┌┐", overview_border_style),
                Span::styled("store: ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("{} ", store_path_str),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled("┌─┐", overview_border_style),
                Span::styled("githash: ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("{}{}", githash_short, loading_indicator),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled("┌", overview_border_style),
            ])
            .alignment(Alignment::Left),
        )
        .title_top(
            Line::from(vec![
                Span::styled("┐", overview_border_style),
                Span::styled(
                    current_time,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("┌", overview_border_style),
            ])
            .alignment(Alignment::Center),
        )
        .title_top(
            Line::from(vec![
                Span::styled("┐", overview_border_style),
                Span::styled(
                    "-",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    format!(" {} ", rate_text),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    "+",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled("┌─", overview_border_style),
            ])
            .alignment(Alignment::Right),
        );

    let overview_inner_area = overview_block.inner(area);
    f.render_widget(overview_block, area);

    let top_inner_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(60)])
        .split(overview_inner_area);

    draw_graphs(f, top_inner_chunks[0], app);
    draw_targets(f, top_inner_chunks[1], app, targets_border_style);
}

fn draw_graphs(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Job Status History
            Constraint::Length(1),
            Constraint::Min(0), // Job Completion Rate
        ])
        .split(area);

    let left_area = chunks[0];
    let separator_area = chunks[1];
    let right_area = chunks[2];

    let status_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(left_area);
    let rate_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(right_area);

    // Left plot: Job Status History
    let bg = get_color(app, &app.theme.elements.graphs.background.color);
    let status_styles = &app.theme.elements.job_status;
    let status_colors: BTreeMap<&'static str, Color> = [
        ("Succeeded", get_color(app, &status_styles.succeeded.color)),
        ("Failed", get_color(app, &status_styles.failed.color)),
        ("Running", get_color(app, &status_styles.running.color)),
        ("Pending", get_color(app, &status_styles.pending.color)),
        ("Queued", get_color(app, &status_styles.queued.color)),
        ("Blocked", get_color(app, &status_styles.blocked.color)),
        (
            "Submitting...",
            get_color(app, &status_styles.submitting.color),
        ),
        ("Unknown", get_color(app, &status_styles.unknown.color)),
    ]
    .iter()
    .map(|(k, v)| (*k, color::muted(*v, bg)))
    .collect();

    let data: Vec<_> = app.status_history.iter().cloned().collect();
    let status_chart = StackedBarChart {
        data: &data,
        status_colors: &status_colors,
    };
    f.render_widget(status_chart, status_chunks[0]);
    f.render_widget(
        Paragraph::new("Job Status History")
            .style(Style::default().add_modifier(Modifier::DIM))
            .alignment(Alignment::Center),
        status_chunks[1],
    );

    // Separator
    let separator = Paragraph::new("│").style(Style::default().add_modifier(Modifier::DIM));
    f.render_widget(separator, separator_area);

    // Right plot: Job Completion Rate
    let rate_data: Vec<f64> = app.completion_rate_history.iter().copied().collect();
    let max_rate = rate_data
        .iter()
        .fold(0.0, |max, &val| val.max(max))
        .max(1.0);

    let rate_graph = BrailleGraph {
        data: &rate_data,
        max_value: max_rate,
        low_color: get_color(app, &app.theme.elements.graphs.rate_low.color),
        high_color: get_color(app, &app.theme.elements.graphs.rate_high.color),
        direction: GraphDirection::Upwards,
    };
    f.render_widget(rate_graph, rate_chunks[0]);

    f.render_widget(
        Paragraph::new("Job Completion Rate")
            .style(Style::default().add_modifier(Modifier::DIM))
            .alignment(Alignment::Center),
        rate_chunks[1],
    );
}
fn draw_targets(f: &mut Frame, area: Rect, app: &mut App, border_style: Style) {
    let targets_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .title_top(
            Line::from(vec![
                Span::styled("─┐", border_style),
                Span::styled("⁴", Style::default().add_modifier(Modifier::DIM)),
                Span::styled(
                    "TARGETS",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("┌─", border_style),
            ])
            .alignment(Alignment::Left),
        );
    let targets_inner_area = targets_block.inner(area);
    f.render_widget(targets_block, area);

    if !app.targets_state.items.is_empty() {
        let selected_row_idx = app.targets_state.table_state.selected();

        let row_highlight_style = if app.focused_panel == PanelFocus::Targets {
            get_style(app, &app.theme.elements.tables.row_highlight_fg).bg(get_color(
                app,
                &app.theme.elements.tables.row_highlight_bg.color,
            ))
        } else {
            Style::default()
        };

        let cell_highlight_style = if app.focused_panel == PanelFocus::Targets {
            get_style(app, &app.theme.elements.tables.cell_highlight_fg).bg(get_color(
                app,
                &app.theme.elements.tables.cell_highlight_bg.color,
            ))
        } else {
            Style::default()
        };

        let target_rows: Vec<Row> = app
            .targets_state
            .items
            .iter()
            .enumerate()
            .map(|(i, target)| {
                let is_selected_row = selected_row_idx == Some(i);

                let (state_text, state_style) = match target.state {
                    TargetState::Active => (
                        "[ACTIVE]",
                        get_style(app, &app.theme.elements.target_states.active),
                    ),
                    TargetState::Inactive => (
                        "[INACTIVE]",
                        get_style(app, &app.theme.elements.target_states.inactive),
                    ),
                    TargetState::Down => ("[DOWN]", Style::default().add_modifier(Modifier::DIM)),
                };
                let mut executor_text = target.get_selected_executor().as_str().to_string();
                if is_selected_row
                    && app.targets_state.focused_column == 1
                    && app.targets_state.is_editing_cell
                {
                    executor_text = format!("← {} →", executor_text);
                }

                let mut scheduler_text = target.get_selected_scheduler().as_str().to_string();
                if is_selected_row
                    && app.targets_state.focused_column == 2
                    && app.targets_state.is_editing_cell
                {
                    scheduler_text = format!("← {} →", scheduler_text);
                }
                let mut cells = vec![
                    Cell::from(target.name.clone()),
                    Cell::from(executor_text),
                    Cell::from(scheduler_text),
                    Cell::from(Span::styled(state_text, state_style)),
                ];

                if is_selected_row {
                    for (col_idx, cell) in cells.iter_mut().enumerate() {
                        let style = if col_idx == app.targets_state.focused_column {
                            cell_highlight_style
                        } else {
                            row_highlight_style
                        };
                        *cell = cell.clone().style(style);
                    }
                }

                Row::new(cells)
            })
            .collect();

        let header = Row::new(vec!["Target", "Executor", "Scheduler", "Status"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let table = Table::new(
            target_rows,
            [
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ],
        )
        .header(header)
        .highlight_symbol("");

        f.render_stateful_widget(
            table,
            targets_inner_area,
            &mut app.targets_state.table_state,
        );
    }
}
fn draw_left_column(f: &mut Frame, area: Rect, app: &App) {
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(12),
            Constraint::Min(0),
            Constraint::Length(8),
        ])
        .split(area);

    draw_context_panel(f, left_chunks[0], app);
    draw_logs_panel(f, left_chunks[1], app);
    draw_system_logs_panel(f, left_chunks[2], app);
}

fn draw_system_logs_panel(f: &mut Frame, area: Rect, app: &App) {
    let style = get_style(app, &app.theme.elements.panels.logs);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style)
        .title(" System Logs ");
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let height = inner_area.height as usize;
    let lines: Vec<Line> = app
        .system_logs
        .iter()
        .rev()
        .take(height)
        .rev()
        .map(|s| Line::from(Span::raw(s)))
        .collect();

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner_area);
}

fn draw_context_panel(f: &mut Frame, area: Rect, app: &App) {
    let context_border_style = get_style(app, &app.theme.elements.panels.context);
    let selected_job = app
        .jobs_state
        .table_state
        .selected()
        .and_then(|i| app.jobs_state.display_rows.get(i))
        .and_then(|row| {
            if let TuiRowItem::Job { job } = &row.item {
                Some(job)
            } else {
                None
            }
        });
    let context_title = if let Some(job) = selected_job {
        let job_display_id = if job.name.is_empty() {
            job.id.clone()
        } else {
            format!("{}-{}", job.id, job.name)
        };
        format!("[Job: {}]", job_display_id)
    } else {
        "[Job: (none)]".to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(context_border_style)
        .title_top(
            Line::from(vec![
                Span::styled("─┐", context_border_style),
                Span::styled("³", Style::default().add_modifier(Modifier::DIM)),
                Span::styled(
                    "CONTEXT",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("┌─┐", context_border_style),
                Span::styled("[Job: ", Style::default().fg(Color::White)),
                Span::styled(
                    context_title
                        .strip_prefix("[Job: ")
                        .unwrap()
                        .strip_suffix(']')
                        .unwrap(),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled("]", Style::default().add_modifier(Modifier::DIM)),
                Span::styled("┌", context_border_style),
            ])
            .alignment(Alignment::Left),
        );
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let content = if let Some(job) = selected_job {
        let mut lines = vec![
            Line::from(vec![Span::raw("Run: "), Span::raw(job.run.clone())]),
            Line::from(vec![
                Span::raw("Depends on: "),
                Span::raw(job.context_depends_on.clone()),
            ]),
            Line::from(vec![
                Span::raw("Dependents: "),
                Span::raw(job.context_dependents.clone()),
            ]),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "Parameters:",
                Style::default().add_modifier(Modifier::BOLD),
            )),
        ];

        if let Some(obj) = job.params.as_object() {
            for (k, v) in obj {
                let val_str = if let Some(s) = v.as_str() {
                    shorten_nix_store_path(s)
                } else {
                    v.to_string()
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {}: ", k), Style::default().fg(Color::Cyan)),
                    Span::raw(val_str),
                ]));
            }
        }

        Paragraph::new(lines)
    } else {
        Paragraph::new("Select a job to see its context.")
    };
    f.render_widget(content, inner_area);
}
fn draw_logs_panel(f: &mut Frame, area: Rect, app: &App) {
    let logs_border_style = get_style(app, &app.theme.elements.panels.logs);
    let selected_job = app
        .jobs_state
        .table_state
        .selected()
        .and_then(|i| app.jobs_state.display_rows.get(i))
        .and_then(|row| {
            if let TuiRowItem::Job { job } = &row.item {
                Some(job)
            } else {
                None
            }
        });
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(logs_border_style)
        .title_top(Line::from(vec![
            Span::styled("─┐", logs_border_style),
            Span::styled("⁵", Style::default().add_modifier(Modifier::DIM)),
            Span::styled(
                "LOG PREVIEW",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("┌─", logs_border_style),
        ]));
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let content = if let Some(job) = selected_job {
        Paragraph::new(
            job.logs
                .iter()
                .map(|log| Line::from(log.as_str()))
                .collect::<Vec<Line>>(),
        )
    } else {
        Paragraph::new("Select a job to see its logs.")
    };
    f.render_widget(content, inner_area);
}
fn draw_right_column(f: &mut Frame, area: Rect, app: &mut App) {
    let runs_jobs_border_style = get_style(app, &app.theme.elements.panels.runs_jobs);
    let filtered_count = app.jobs_state.display_rows.len();
    let counter_text = if filtered_count > 0 {
        let selected_index = app.jobs_state.table_state.selected().unwrap_or(0);
        format!("{}/{}", selected_index + 1, filtered_count)
    } else {
        "0/0".to_string()
    };
    let status_filter_text = app.jobs_state.status_filter.as_str();
    let right_title_content = format!("┐reverse┌┐tree┌┐{}┌─", status_filter_text);
    let right_title_width = right_title_content.chars().count() as u16 + 1;
    let left_title_prefix = "─┐";
    let left_title_key = "²";
    let left_title_text = "RUNS & JOBS";
    let left_title_border2 = "┌─┐";
    let left_title_suffix = "┌";
    let left_title_fixed_width = (left_title_prefix.chars().count()
        + left_title_key.chars().count()
        + left_title_text.chars().count()
        + left_title_border2.chars().count()
        + left_title_suffix.chars().count()) as u16;
    let max_filter_width = area
        .width
        .saturating_sub(left_title_fixed_width)
        .saturating_sub(right_title_width)
        .saturating_sub(2);

    let mut left_title_spans = vec![
        Span::styled(left_title_prefix, runs_jobs_border_style),
        Span::styled(left_title_key, Style::default().add_modifier(Modifier::DIM)),
        Span::styled(
            left_title_text,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(left_title_border2, runs_jobs_border_style),
    ];
    match app.input_mode {
        InputMode::Editing => {
            let text = &app.jobs_state.filter_text;
            let pos = app.jobs_state.filter_cursor_position;

            let text_before_cursor = &text[..pos];

            let mut text_with_cursor = format!("{}{}{}", text_before_cursor, "_", &text[pos..]);

            if text_with_cursor.chars().count() < "filter".len() {
                text_with_cursor
                    .push_str(&" ".repeat("filter".len() - text_with_cursor.chars().count()));
            }
            let cursor_char_idx = text_before_cursor.chars().count();

            let total_chars = text_with_cursor.chars().count();
            let available_width = max_filter_width as usize;

            let final_text = if total_chars > available_width {
                let start_char_idx = (cursor_char_idx + 1).saturating_sub(available_width);
                text_with_cursor
                    .chars()
                    .skip(start_char_idx)
                    .take(available_width)
                    .collect::<String>()
            } else {
                text_with_cursor
            };

            left_title_spans.push(Span::styled(final_text, Style::default()));
        }
        InputMode::Normal | InputMode::SpaceMenu | InputMode::GMenu => {
            if !app.jobs_state.filter_text.is_empty() {
                let text_to_truncate = &app.jobs_state.filter_text;
                let truncated_filter_text = if text_to_truncate.len() > max_filter_width as usize {
                    let start_index = text_to_truncate.len() - max_filter_width as usize;
                    &text_to_truncate[start_index..]
                } else {
                    text_to_truncate
                };
                left_title_spans.push(Span::styled(truncated_filter_text, Style::default()));
            } else if "filter".len() <= max_filter_width as usize {
                left_title_spans.extend(vec![
                    Span::styled(
                        "f",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::DIM),
                    ),
                    Span::styled("ilter", Style::default().fg(Color::White)),
                ]);
            }
        }
    };
    left_title_spans.push(Span::styled(left_title_suffix, runs_jobs_border_style));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(runs_jobs_border_style)
        .title_top(Line::from(left_title_spans).alignment(Alignment::Left))
        .title_top(
            Line::from(vec![
                Span::styled("┐", runs_jobs_border_style),
                Span::styled(
                    "r",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled("everse", Style::default().fg(Color::White)),
                Span::styled("┌", runs_jobs_border_style),
                Span::styled("┐", runs_jobs_border_style),
                Span::styled(
                    "t",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled("ree", Style::default().fg(Color::White)),
                Span::styled("┌", runs_jobs_border_style),
                Span::styled("┐", runs_jobs_border_style),
                Span::styled(
                    "←",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    format!(" {} ", status_filter_text),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    "→",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled("┌─", runs_jobs_border_style),
            ])
            .alignment(Alignment::Right),
        )
        .title_bottom(
            Line::from(vec![
                Span::styled("┘", runs_jobs_border_style),
                Span::styled(
                    "↑",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(" select ", Style::default().fg(Color::White)),
                Span::styled(
                    "↓",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled("└─┘", runs_jobs_border_style),
                Span::styled(
                    "c",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled("ancel ", Style::default().fg(Color::White)),
                Span::styled("└┘", runs_jobs_border_style),
                Span::styled(
                    "d",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled("ebug", Style::default().fg(Color::White)),
                Span::styled("└", runs_jobs_border_style),
            ])
            .alignment(Alignment::Left),
        )
        .title_bottom(
            Line::from(vec![
                Span::styled("┘", runs_jobs_border_style),
                Span::styled(counter_text, Style::default().fg(Color::White)),
                Span::styled("└─", runs_jobs_border_style),
            ])
            .alignment(Alignment::Right),
        );

    f.render_widget(&block, area);
    let inner_area = block.inner(area);

    let right_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner_area);
    let table_area = right_chunks[0];
    let scrollbar_area = right_chunks[1];
    let jobs_table = if app.jobs_state.is_tree_view {
        let header = Row::new(vec!["", "jobid:", "Item:", "Parameters:", "Status:"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let constraints = [
            Constraint::Length(1),
            Constraint::Length(8),
            Constraint::Length(35),
            Constraint::Min(20),
            Constraint::Length(10),
        ];
        let rows = build_tree_rows(
            app,
            &app.jobs_state.display_rows,
            &app.jobs_state.selected_jobs,
            &app.jobs_state.collapsed_nodes,
            app.lab(),
        );
        Table::new(rows, constraints)
            .header(header.height(1))
            .row_highlight_style(if app.focused_panel == PanelFocus::Jobs {
                get_style(app, &app.theme.elements.tables.row_highlight_fg).bg(get_color(
                    app,
                    &app.theme.elements.tables.row_highlight_bg.color,
                ))
            } else {
                Style::default()
            })
            .highlight_symbol("")
    } else {
        let header = Row::new(vec![
            "",
            "jobid:",
            "Item:",
            "Run:",
            "Parameters:",
            "Status:",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));
        let constraints = [
            Constraint::Length(1),
            Constraint::Length(8),
            Constraint::Length(25),
            Constraint::Length(15),
            Constraint::Min(20),
            Constraint::Length(10),
        ];
        let rows = build_flat_rows(
            app,
            &app.jobs_state.display_rows,
            &app.jobs_state.selected_jobs,
        );
        Table::new(rows, constraints)
            .header(header.height(1))
            .row_highlight_style(if app.focused_panel == PanelFocus::Jobs {
                get_style(app, &app.theme.elements.tables.row_highlight_fg).bg(get_color(
                    app,
                    &app.theme.elements.tables.row_highlight_bg.color,
                ))
            } else {
                Style::default()
            })
            .highlight_symbol("")
    };
    f.render_stateful_widget(jobs_table, table_area, &mut app.jobs_state.table_state);

    let viewport_height = table_area.height.saturating_sub(1) as usize;
    app.jobs_state.viewport_height = viewport_height;

    let mut scrollbar_state = ScrollbarState::default()
        .content_length(filtered_count)
        .position(app.jobs_state.table_state.selected().unwrap_or(0))
        .viewport_content_length(viewport_height);
    f.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .thumb_symbol("█")
            .track_style(Style::default().fg(Color::DarkGray)),
        scrollbar_area,
        &mut scrollbar_state,
    );
}

fn format_params_single_line(v: &serde_json::Value) -> String {
    if let Some(obj) = v.as_object() {
        obj.iter()
            .map(|(k, v)| {
                let val_str = if let Some(s) = v.as_str() {
                    let shortened = shorten_nix_store_path(s);
                    if shortened != s {
                        shortened
                    } else if s.contains('/') {
                        std::path::Path::new(s)
                            .file_name()
                            .and_then(|os| os.to_str())
                            .unwrap_or(s)
                            .to_string()
                    } else {
                        s.to_string()
                    }
                } else {
                    v.to_string()
                };
                format!("{}={}", k, val_str)
            })
            .collect::<Vec<_>>()
            .join(",")
    } else {
        String::new()
    }
}

fn build_flat_rows<'a>(
    app: &App,
    display_rows: &'a [TuiDisplayRow],
    selected_jobs: &HashSet<String>,
) -> Vec<Row<'a>> {
    display_rows
        .iter()
        .map(|row_data| {
            let (job, is_selected) = if let TuiRowItem::Job { job } = &row_data.item {
                (job, selected_jobs.contains(&row_data.id))
            } else {
                unreachable!();
            };

            let selector = if is_selected {
                Cell::from("█").style(get_style(app, &app.theme.elements.tables.selector))
            } else {
                Cell::from(" ")
            };
            let status_style = match job.status.as_str() {
                "Succeeded" => get_style(app, &app.theme.elements.job_status.succeeded),
                "Failed" => get_style(app, &app.theme.elements.job_status.failed),
                "Submit Failed" => get_style(app, &app.theme.elements.job_status.submit_failed),
                "Pending" => get_style(app, &app.theme.elements.job_status.pending),
                "Running" => get_style(app, &app.theme.elements.job_status.running),
                "Queued" => get_style(app, &app.theme.elements.job_status.queued),
                "Blocked" => get_style(app, &app.theme.elements.job_status.blocked),
                "Submitting..." => get_style(app, &app.theme.elements.job_status.submitting),
                _ => get_style(app, &app.theme.elements.job_status.unknown),
            };
            let status_cell = Cell::from(Span::styled(job.status.clone(), status_style));
            let params_str = format_params_single_line(&job.params);

            Row::new(vec![
                selector,
                Cell::from(job.id.clone()),
                Cell::from(job.name.clone()),
                Cell::from(job.run.clone()),
                Cell::from(params_str),
                status_cell,
            ])
        })
        .collect()
}

fn build_tree_rows<'a>(
    app: &App,
    display_rows: &'a [TuiDisplayRow],
    selected_jobs: &HashSet<String>,
    collapsed_nodes: &HashSet<String>,
    lab: &Lab,
) -> Vec<Row<'a>> {
    let mut rows = Vec::new();
    let mut ancestor_is_last_stack: Vec<bool> = Vec::new();

    for row_data in display_rows.iter() {
        let is_selected_for_action = selected_jobs.contains(&row_data.id);
        let selector = if is_selected_for_action {
            Cell::from("█").style(get_style(app, &app.theme.elements.tables.selector))
        } else {
            Cell::from(" ")
        };

        while ancestor_is_last_stack.len() > row_data.depth {
            ancestor_is_last_stack.pop();
        }

        match &row_data.item {
            TuiRowItem::Run { id } => {
                let run = lab.runs.get(id).unwrap();
                let is_expanded = !collapsed_nodes.contains(&row_data.id);

                let item_prefix = if !run.jobs.is_empty() {
                    if is_expanded {
                        "[-] "
                    } else {
                        "[+] "
                    }
                } else {
                    "    "
                };

                let display_text = id.to_string();
                let item_style = Style::default().add_modifier(Modifier::BOLD);

                rows.push(Row::new(vec![
                    selector,
                    Cell::from(""), // jobid
                    Cell::from(Line::from(vec![
                        Span::raw(item_prefix),
                        Span::styled(display_text, item_style),
                    ])),
                    Cell::from(""),
                    Cell::from(""),
                ]));

                ancestor_is_last_stack.push(row_data.is_last_child);
            }
            TuiRowItem::Job { job } => {
                let lab_job = lab.jobs.get(&job.full_id).unwrap();
                let has_children = lab_job.executables.values().any(|e| !e.inputs.is_empty());
                let is_expanded = !collapsed_nodes.contains(&row_data.id);

                let branch = if row_data.is_last_child { "└" } else { "├" };

                let item_marker = if has_children {
                    if is_expanded {
                        "[-]"
                    } else {
                        "[+]"
                    }
                } else {
                    "───"
                };

                let prefix: String = (0..row_data.depth)
                    .map(|i| {
                        if *ancestor_is_last_stack.get(i).unwrap_or(&false) {
                            "  "
                        } else {
                            "│ "
                        }
                    })
                    .collect();
                let corrected_prefix = if !ancestor_is_last_stack.first().cloned().unwrap_or(true) {
                    prefix
                } else {
                    prefix.get(2..).unwrap_or("").to_string()
                };
                ancestor_is_last_stack.push(row_data.is_last_child);

                let display_text = job.name.clone();
                let item_style = Style::default();
                let status_style = match job.status.as_str() {
                    "Succeeded" => get_style(app, &app.theme.elements.job_status.succeeded),
                    "Failed" => get_style(app, &app.theme.elements.job_status.failed),
                    "Submit Failed" => get_style(app, &app.theme.elements.job_status.submit_failed),
                    "Pending" => get_style(app, &app.theme.elements.job_status.pending),
                    "Running" => get_style(app, &app.theme.elements.job_status.running),
                    "Queued" => get_style(app, &app.theme.elements.job_status.queued),
                    "Blocked" => get_style(app, &app.theme.elements.job_status.blocked),
                    "Submitting..." => get_style(app, &app.theme.elements.job_status.submitting),
                    _ => get_style(app, &app.theme.elements.job_status.unknown),
                };
                let params_str = format_params_single_line(&job.params);
                let params = Cell::from(params_str);
                let status = Cell::from(Span::styled(job.status.clone(), status_style));

                rows.push(Row::new(vec![
                    selector,
                    Cell::from(job.id.clone()),
                    Cell::from(Line::from(vec![
                        Span::raw(format!(" {}{}{} ", corrected_prefix, branch, item_marker)),
                        Span::styled(display_text, item_style),
                    ])),
                    params,
                    status,
                ]));
            }
        }
    }
    rows
}
fn draw_space_menu_popup(f: &mut Frame, area: Rect, app: &App) {
    let popup_height = 10;
    let horizontal_padding = 2;
    let bottom_padding = 1;

    let popup_area = Rect {
        x: area.x + horizontal_padding,
        y: area
            .height
            .saturating_sub(popup_height)
            .saturating_sub(bottom_padding),
        width: area.width.saturating_sub(horizontal_padding * 2),
        height: popup_height,
    };

    let block = Block::default()
        .title(" Quick Actions ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(get_style(app, &app.theme.elements.popups.border));

    let inner_area = block.inner(popup_area);
    let shortcuts = [
        ("r", "Run Selected"),
        ("c", "Cancel Selected"),
        ("y", "Yank Path"),
        ("e", "Explore (Yazi)"),
        ("l", "Global Logs"),
        ("ESC", "Close Menu"),
    ];
    let mut rows = vec![];
    for chunk in shortcuts.chunks(3) {
        let mut cells = chunk
            .iter()
            .map(|(key, desc)| {
                Cell::from(Line::from(vec![
                    Span::styled(
                        format!(" {} ", key),
                        get_style(app, &app.theme.elements.popups.key_fg)
                            .bg(get_color(app, &app.theme.elements.popups.key_bg.color)),
                    ),
                    Span::raw(format!(" {}", desc)),
                ]))
            })
            .collect::<Vec<_>>();

        while cells.len() < 3 {
            cells.push(Cell::from(""));
        }
        rows.push(Row::new(cells).height(2));
    }

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ],
    )
    .column_spacing(2);

    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);
    f.render_widget(table, inner_area);
}

fn draw_g_menu_popup(f: &mut Frame, area: Rect, app: &App) {
    let popup_height = 6;
    let horizontal_padding = 2;
    let bottom_padding = 1;

    let popup_area = Rect {
        x: area.x + horizontal_padding,
        y: area
            .height
            .saturating_sub(popup_height)
            .saturating_sub(bottom_padding),
        width: area.width.saturating_sub(horizontal_padding * 2),
        height: popup_height,
    };

    let block = Block::default()
        .title(" Go To ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(get_style(app, &app.theme.elements.popups.border));

    let inner_area = block.inner(popup_area);

    let shortcuts = [
        ("g", "Go to Top"),
        ("e", "Go to End"),
        ("d", "Definition"),
        ("l", "Logs"),
        ("ESC", "Close Menu"),
    ];

    let mut rows = vec![];
    for chunk in shortcuts.chunks(3) {
        let mut cells = chunk
            .iter()
            .map(|(key, desc)| {
                Cell::from(Line::from(vec![
                    Span::styled(
                        format!(" {} ", key),
                        get_style(app, &app.theme.elements.popups.key_fg)
                            .bg(get_color(app, &app.theme.elements.popups.key_bg.color)),
                    ),
                    Span::raw(format!(" {}", desc)),
                ]))
            })
            .collect::<Vec<_>>();

        while cells.len() < 3 {
            cells.push(Cell::from(""));
        }
        rows.push(Row::new(cells).height(2));
    }

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ],
    )
    .column_spacing(2);

    f.render_widget(Clear, popup_area);
    f.render_widget(block, popup_area);
    f.render_widget(table, inner_area);
}

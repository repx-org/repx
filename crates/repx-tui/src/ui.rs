use crate::{
    app::{App, InputMode, PanelFocus},
    model::{TargetState, TuiDisplayRow, TuiRowItem},
    widgets::{BrailleGraph, GraphDirection},
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
use std::collections::{HashSet, VecDeque};

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
        draw_space_menu_popup(f, f.area());
    } else if app.input_mode == InputMode::GMenu {
        draw_g_menu_popup(f, f.area());
    }
}

fn draw_overview_panel(f: &mut Frame, area: Rect, app: &mut App) {
    let overview_border_style = Style::default().fg(Color::Magenta);
    let targets_border_style = Style::default().fg(Color::DarkGray);
    let loading_indicator = if app.is_loading { " [Updating...]" } else { "" };
    let store_path_str = {
        let active_target_name = app.active_target.lock().unwrap();
        app.client
            .config()
            .targets
            .get(&*active_target_name)
            .map(|t| t.base_path.display().to_string())
            .unwrap_or_else(|| "[unknown]".to_string())
    };
    let githash_short = app.lab.git_hash.chars().take(7).collect::<String>();

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
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled("┌─┐", overview_border_style),
                Span::styled("githash: ", Style::default().fg(Color::White)),
                Span::styled(
                    format!("{}{}", githash_short, loading_indicator),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled("┌", overview_border_style),
            ])
            .alignment(Alignment::Left),
        )
        .title_top(
            Line::from(vec![
                Span::styled("┐", overview_border_style),
                Span::styled(current_time, Style::default().add_modifier(Modifier::DIM)),
                Span::styled("┌", overview_border_style),
            ])
            .alignment(Alignment::Center),
        )
        .title_top(
            Line::from(vec![
                Span::styled("┐", overview_border_style),
                Span::styled("-", Style::default().add_modifier(Modifier::DIM)),
                Span::styled(
                    format!(" {} ", rate_text),
                    Style::default().fg(Color::White),
                ),
                Span::styled("+", Style::default().add_modifier(Modifier::DIM)),
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
    let graph_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(1),
            Constraint::Percentage(50),
        ])
        .split(area);

    let progress_area = graph_chunks[0];
    let divider_area = graph_chunks[1];
    let throughput_area = graph_chunks[2];

    let graph_width_in_cells = area.width as usize;
    let data_points_to_render = graph_width_in_cells * 2;

    let create_padded_slice = |data: &VecDeque<f64>| -> Vec<f64> {
        let current_data_len = data.len();
        let padding = data_points_to_render.saturating_sub(current_data_len);
        let data_slice = data
            .iter()
            .skip(current_data_len.saturating_sub(data_points_to_render));
        std::iter::repeat(0.0)
            .take(padding)
            .chain(data_slice.copied())
            .collect()
    };

    let progress_data_slice = create_padded_slice(&app.progress_data);
    let throughput_data_slice = create_padded_slice(&app.throughput_data);

    let progress_graph = BrailleGraph {
        data: &progress_data_slice,
        max_value: 100.0,
        low_color: Color::Rgb(0, 150, 0),
        high_color: Color::Rgb(100, 255, 100),
        direction: GraphDirection::Upwards,
    };
    f.render_widget(progress_graph, progress_area);

    let throughput_graph = BrailleGraph {
        data: &throughput_data_slice,
        max_value: 100.0,
        low_color: Color::Rgb(255, 160, 0),
        high_color: Color::Rgb(255, 255, 100),
        direction: GraphDirection::Downwards,
    };
    f.render_widget(throughput_graph, throughput_area);

    let label_style = Style::default().add_modifier(Modifier::DIM);
    let text = "througput─▼▲─progress";
    let text_width = text.len();
    let line_width = divider_area.width.saturating_sub(text_width as u16);
    let left_line_width = line_width / 2;
    let right_line_width = line_width - left_line_width;

    let divider = Paragraph::new(Line::from(vec![
        Span::styled("─".repeat(left_line_width as usize), label_style),
        Span::styled(text, label_style),
        Span::styled("─".repeat(right_line_width as usize), label_style),
    ]));
    f.render_widget(divider, divider_area);
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

    if !app.targets.is_empty() {
        let selected_row_idx = app.targets_table_state.selected();

        let row_highlight_style = if app.focused_panel == PanelFocus::Targets {
            Style::default()
                .bg(border_style.fg.unwrap_or(Color::Cyan))
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let cell_highlight_style = if app.focused_panel == PanelFocus::Targets {
            Style::default().bg(Color::LightYellow).fg(Color::Black)
        } else {
            Style::default()
        };

        let target_rows: Vec<Row> = app
            .targets
            .iter()
            .enumerate()
            .map(|(i, target)| {
                let is_selected_row = selected_row_idx == Some(i);

                let (state_text, state_style) = match target.state {
                    TargetState::Active => (
                        "[ACTIVE]",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    TargetState::Inactive => ("[INACTIVE]", Style::default().fg(Color::Yellow)),
                    TargetState::Down => ("[DOWN]", Style::default().add_modifier(Modifier::DIM)),
                };

                let mut executor_text = target.executor.to_str().to_string();
                if is_selected_row && app.targets_focused_column == 1 && app.is_editing_target_cell
                {
                    executor_text = format!("← {} →", executor_text);
                }

                let mut scheduler_text = target.scheduler.to_str().to_string();
                if is_selected_row && app.targets_focused_column == 2 && app.is_editing_target_cell
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
                        let style = if col_idx == app.targets_focused_column {
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

        f.render_stateful_widget(table, targets_inner_area, &mut app.targets_table_state);
    }
}

fn draw_left_column(f: &mut Frame, area: Rect, app: &App) {
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(12), Constraint::Min(0)])
        .split(area);

    draw_context_panel(f, left_chunks[0], app);
    draw_logs_panel(f, left_chunks[1], app);
}

fn draw_context_panel(f: &mut Frame, area: Rect, app: &App) {
    let context_border_style = Style::default().fg(Color::Green);
    let selected_job = app
        .table_state
        .selected()
        .and_then(|i| app.display_rows.get(i))
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
        Paragraph::new(vec![
            Line::from(vec![Span::raw("Run: "), Span::raw(job.run.clone())]),
            Line::from(vec![Span::raw("Elapsed: "), Span::raw(job.elapsed.clone())]),
            Line::from(vec![
                Span::raw("Depends on: "),
                Span::raw(job.context_depends_on.clone()),
            ]),
            Line::from(vec![
                Span::raw("Dependents: "),
                Span::raw(job.context_dependents.clone()),
            ]),
        ])
    } else {
        Paragraph::new("Select a job to see its context.")
    };
    f.render_widget(content, inner_area);
}

fn draw_logs_panel(f: &mut Frame, area: Rect, app: &App) {
    let logs_border_style = Style::default().fg(Color::Red);
    let selected_job = app
        .table_state
        .selected()
        .and_then(|i| app.display_rows.get(i))
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
    let runs_jobs_border_style = Style::default().fg(Color::Cyan);
    let filtered_count = app.display_rows.len();
    let counter_text = if filtered_count > 0 {
        let selected_index = app.table_state.selected().unwrap_or(0);
        format!("{}/{}", selected_index + 1, filtered_count)
    } else {
        "0/0".to_string()
    };

    let status_filter_text = app.status_filter.to_str();
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
            let mut text_to_truncate = format!("{}_", app.filter_text);
            if text_to_truncate.len() < "filter".len() {
                text_to_truncate = format!("{:<width$}", text_to_truncate, width = "filter".len());
            }

            let truncated_filter_text = if text_to_truncate.len() > max_filter_width as usize {
                let start_index = text_to_truncate.len() - max_filter_width as usize;
                text_to_truncate[start_index..].to_string()
            } else {
                text_to_truncate
            };
            left_title_spans.push(Span::styled(truncated_filter_text, Style::default()));
        }
        InputMode::Normal | InputMode::SpaceMenu | InputMode::GMenu => {
            if !app.filter_text.is_empty() {
                let text_to_truncate = &app.filter_text;
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

    let jobs_table = if app.is_tree_view {
        let header = Row::new(vec![
            "", "jobid:", "Item:", "Worker:", "Elapsed:", "Status:",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));
        let constraints = [
            Constraint::Length(1),
            Constraint::Length(8),
            Constraint::Min(35),
            Constraint::Min(10),
            Constraint::Length(10),
            Constraint::Length(10),
        ];
        let rows = build_tree_rows(
            &app.display_rows,
            &app.selected_jobs,
            &app.collapsed_nodes,
            app.lab(),
        );
        Table::new(rows, constraints)
            .header(header.height(1))
            .row_highlight_style(if app.focused_panel == PanelFocus::Jobs {
                Style::default()
                    .bg(runs_jobs_border_style.fg.unwrap_or(Color::Cyan))
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
            .highlight_symbol("")
    } else {
        let header = Row::new(vec![
            "", "jobid:", "Item:", "Run:", "Worker:", "Elapsed:", "Status:",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));
        let constraints = [
            Constraint::Length(1),
            Constraint::Length(8),
            Constraint::Min(25),
            Constraint::Min(15),
            Constraint::Min(10),
            Constraint::Length(10),
            Constraint::Length(10),
        ];
        let rows = build_flat_rows(&app.display_rows, &app.selected_jobs);
        Table::new(rows, constraints)
            .header(header.height(1))
            .row_highlight_style(if app.focused_panel == PanelFocus::Jobs {
                Style::default()
                    .bg(runs_jobs_border_style.fg.unwrap_or(Color::Cyan))
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
            .highlight_symbol("")
    };

    f.render_stateful_widget(jobs_table, table_area, &mut app.table_state);

    let viewport_height = table_area.height.saturating_sub(1) as usize;
    app.jobs_list_viewport_height = viewport_height;

    let mut scrollbar_state = ScrollbarState::default()
        .content_length(filtered_count)
        .position(app.table_state.selected().unwrap_or(0))
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

fn build_flat_rows<'a>(
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
                Cell::from("█").style(Style::default().fg(Color::Yellow))
            } else {
                Cell::from(" ")
            };
            let status_style = match job.status.as_str() {
                "Succeeded" => Style::default().fg(Color::Green),
                "Failed" => Style::default().fg(Color::Red),
                "Submit Failed" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                "Pending" => Style::default().fg(Color::Yellow),
                "Running" => Style::default().fg(Color::Cyan),
                "Queued" => Style::default().fg(Color::Rgb(189, 147, 249)),
                "Blocked" => Style::default().fg(Color::Magenta),
                "Submitting..." => Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
                _ => Style::default(),
            };
            let status_cell = Cell::from(Span::styled(job.status.clone(), status_style));

            Row::new(vec![
                selector,
                Cell::from(job.id.clone()),
                Cell::from(job.name.clone()),
                Cell::from(job.run.clone()),
                Cell::from(job.worker.clone()),
                Cell::from(job.elapsed.clone()),
                status_cell,
            ])
        })
        .collect()
}

fn build_tree_rows<'a>(
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
            Cell::from("█").style(Style::default().fg(Color::Yellow))
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

                let corrected_prefix = if !ancestor_is_last_stack.get(0).cloned().unwrap_or(true) {
                    prefix
                } else {
                    prefix.get(2..).unwrap_or("").to_string()
                };

                ancestor_is_last_stack.push(row_data.is_last_child);

                let display_text = job.name.clone();
                let item_style = Style::default();
                let status_style = match job.status.as_str() {
                    "Succeeded" => Style::default().fg(Color::Green),
                    "Failed" => Style::default().fg(Color::Red),
                    "Submit Failed" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    "Pending" => Style::default().fg(Color::Yellow),
                    "Running" => Style::default().fg(Color::Cyan),
                    "Queued" => Style::default().fg(Color::Rgb(189, 147, 249)),
                    "Blocked" => Style::default().fg(Color::Magenta),
                    "Submitting..." => Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                    _ => Style::default(),
                };
                let worker = Cell::from(job.worker.clone());
                let elapsed = Cell::from(job.elapsed.clone());
                let status = Cell::from(Span::styled(job.status.clone(), status_style));

                rows.push(Row::new(vec![
                    selector,
                    Cell::from(job.id.clone()),
                    Cell::from(Line::from(vec![
                        Span::raw(format!(" {}{}{} ", corrected_prefix, branch, item_marker)),
                        Span::styled(display_text, item_style),
                    ])),
                    worker,
                    elapsed,
                    status,
                ]));
            }
        }
    }
    rows
}

fn draw_space_menu_popup(f: &mut Frame, area: Rect) {
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
        .border_style(Style::default().fg(Color::Yellow));

    let inner_area = block.inner(popup_area);

    let shortcuts = [
        ("r", "Run Selected"),
        ("c", "Cancel Selected"),
        ("d", "Debug Shell"),
        ("p", "Show Path"),
        ("l", "Follow Logs"),
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
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
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

fn draw_g_menu_popup(f: &mut Frame, area: Rect) {
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
        .border_style(Style::default().fg(Color::Yellow));

    let inner_area = block.inner(popup_area);

    let shortcuts = [
        ("g", "Go to Top"),
        ("e", "Go to End"),
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
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
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

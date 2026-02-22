use crate::{
    app::App,
    model::{TuiDisplayRow, TuiRowItem},
    style::{get_style, status_style},
    widgets::tree_prefix::tree_prefix,
};
use ratatui::{
    prelude::*,
    widgets::{Cell, Row},
};
use repx_core::model::Lab;
use std::collections::HashSet;

pub fn shorten_nix_store_path(s: &str) -> String {
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

pub fn format_params_single_line(v: &serde_json::Value) -> String {
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

fn selector_cell<'a>(app: &App, is_selected: bool) -> Cell<'a> {
    if is_selected {
        Cell::from("█").style(get_style(app, &app.theme.elements.tables.selector))
    } else {
        Cell::from(" ")
    }
}

fn expand_marker(is_expanded: bool, has_children: bool) -> &'static str {
    if has_children {
        if is_expanded {
            "[-]"
        } else {
            "[+]"
        }
    } else {
        "───"
    }
}

pub fn build_flat_rows<'a>(
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

            let status = Cell::from(Span::styled(
                job.status.clone(),
                status_style(app, &job.status),
            ));

            Row::new(vec![
                selector_cell(app, is_selected),
                Cell::from(job.id.clone()),
                Cell::from(job.name.clone()),
                Cell::from(job.run.clone()),
                Cell::from(format_params_single_line(&job.params)),
                status,
            ])
        })
        .collect()
}

pub fn build_tree_rows<'a>(
    app: &App,
    display_rows: &'a [TuiDisplayRow],
    selected_jobs: &HashSet<String>,
    collapsed_nodes: &HashSet<String>,
    lab: &Lab,
) -> Vec<Row<'a>> {
    let mut rows = Vec::new();
    let mut ancestor_is_last: Vec<bool> = Vec::new();

    for row_data in display_rows {
        let is_selected = selected_jobs.contains(&row_data.id);

        while ancestor_is_last.len() > row_data.depth {
            ancestor_is_last.pop();
        }

        match &row_data.item {
            TuiRowItem::Group { name } => {
                let is_expanded = !collapsed_nodes.contains(&row_data.id);
                let marker = expand_marker(is_expanded, true);
                let prefix = tree_prefix(
                    &ancestor_is_last,
                    row_data.depth,
                    row_data.is_last_child,
                    marker,
                );

                let group_style = Style::default()
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED);

                rows.push(Row::new(vec![
                    selector_cell(app, is_selected),
                    Cell::from(""),
                    Cell::from(Line::from(vec![
                        Span::raw(prefix),
                        Span::styled(format!("@{}", name), group_style),
                    ])),
                    Cell::from(""),
                    Cell::from(""),
                ]));

                ancestor_is_last.push(row_data.is_last_child);
            }

            TuiRowItem::Run { id } => {
                let run = lab.runs.get(id).unwrap();
                let has_children = !run.jobs.is_empty();
                let is_expanded = !collapsed_nodes.contains(&row_data.id);
                let marker = expand_marker(is_expanded, has_children);
                let prefix = tree_prefix(
                    &ancestor_is_last,
                    row_data.depth,
                    row_data.is_last_child,
                    marker,
                );

                let run_style = Style::default().add_modifier(Modifier::BOLD);

                rows.push(Row::new(vec![
                    selector_cell(app, is_selected),
                    Cell::from(""),
                    Cell::from(Line::from(vec![
                        Span::raw(prefix),
                        Span::styled(id.to_string(), run_style),
                    ])),
                    Cell::from(""),
                    Cell::from(""),
                ]));

                ancestor_is_last.push(row_data.is_last_child);
            }

            TuiRowItem::Job { job } => {
                let lab_job = lab.jobs.get(&job.full_id).unwrap();
                let has_children = lab_job.executables.values().any(|e| !e.inputs.is_empty());
                let is_expanded = !collapsed_nodes.contains(&row_data.id);
                let marker = expand_marker(is_expanded, has_children);
                let prefix = tree_prefix(
                    &ancestor_is_last,
                    row_data.depth,
                    row_data.is_last_child,
                    marker,
                );

                let status = Cell::from(Span::styled(
                    job.status.clone(),
                    status_style(app, &job.status),
                ));

                rows.push(Row::new(vec![
                    selector_cell(app, is_selected),
                    Cell::from(job.id.clone()),
                    Cell::from(Line::from(vec![
                        Span::raw(prefix),
                        Span::raw(job.name.clone()),
                    ])),
                    Cell::from(format_params_single_line(&job.params)),
                    status,
                ]));

                ancestor_is_last.push(row_data.is_last_child);
            }
        }
    }

    rows
}

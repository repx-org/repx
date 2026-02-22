use crate::{
    app::App,
    model::{TuiDisplayRow, TuiRowItem},
    style::{get_style, status_style},
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

pub fn build_flat_rows<'a>(
    app: &App,
    display_rows: &'a [TuiDisplayRow],
    selected_jobs: &HashSet<String>,
    visible_range: Option<std::ops::Range<usize>>,
) -> Vec<Row<'a>> {
    let range = visible_range.unwrap_or(0..display_rows.len());
    let start = range.start.min(display_rows.len());
    let end = range.end.min(display_rows.len());

    display_rows[start..end]
        .iter()
        .map(|row_data| {
            let (job, is_selected) = if let TuiRowItem::Job { job } = &row_data.item {
                (job, selected_jobs.contains(&row_data.id))
            } else {
                unreachable!();
            };

            let status = Cell::from(Span::styled(
                job.status.as_str(),
                status_style(app, &job.status),
            ));

            Row::new(vec![
                selector_cell(app, is_selected),
                Cell::from(job.id.as_str()),
                Cell::from(job.name.as_str()),
                Cell::from(job.run.as_str()),
                Cell::from(job.params_str.as_str()),
                status,
            ])
        })
        .collect()
}

pub fn build_tree_rows<'a>(
    app: &App,
    display_rows: &'a [TuiDisplayRow],
    selected_jobs: &HashSet<String>,
    _collapsed_nodes: &HashSet<String>,
    _lab: &Lab,
    _visible_range: Option<std::ops::Range<usize>>,
) -> Vec<Row<'a>> {
    let mut rows = Vec::with_capacity(display_rows.len());

    for row_data in display_rows {
        let is_selected = selected_jobs.contains(&row_data.id);
        let prefix = row_data.cached_tree_prefix.as_deref().unwrap_or("");

        match &row_data.item {
            TuiRowItem::Group { name } => {
                let group_style = Style::default()
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED);

                rows.push(Row::new(vec![
                    selector_cell(app, is_selected),
                    Cell::from(""),
                    Cell::from(Line::from(vec![
                        Span::raw(prefix.to_string()),
                        Span::styled(format!("@{}", name), group_style),
                    ])),
                    Cell::from(""),
                    Cell::from(""),
                ]));
            }

            TuiRowItem::Run { id } => {
                let run_style = Style::default().add_modifier(Modifier::BOLD);

                rows.push(Row::new(vec![
                    selector_cell(app, is_selected),
                    Cell::from(""),
                    Cell::from(Line::from(vec![
                        Span::raw(prefix.to_string()),
                        Span::styled(id.to_string(), run_style),
                    ])),
                    Cell::from(""),
                    Cell::from(""),
                ]));
            }

            TuiRowItem::Job { job } => {
                let status = Cell::from(Span::styled(
                    job.status.as_str(),
                    status_style(app, &job.status),
                ));

                rows.push(Row::new(vec![
                    selector_cell(app, is_selected),
                    Cell::from(job.id.as_str()),
                    Cell::from(Line::from(vec![
                        Span::raw(prefix.to_string()),
                        Span::raw(job.name.as_str()),
                    ])),
                    Cell::from(job.params_str.as_str()),
                    status,
                ]));
            }
        }
    }

    rows
}

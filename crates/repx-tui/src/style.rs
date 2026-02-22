use crate::app::App;
use ratatui::prelude::*;
use repx_core::theme::ElementStyle;
use std::str::FromStr;

pub fn get_color(app: &App, name: &str) -> Color {
    app.theme
        .palette
        .get(name)
        .and_then(|hex| Color::from_str(hex).ok())
        .unwrap_or(Color::Reset)
}

pub fn get_style(app: &App, element: &ElementStyle) -> Style {
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

pub fn status_style(app: &App, status: &str) -> Style {
    match status {
        "Succeeded" => get_style(app, &app.theme.elements.job_status.succeeded),
        "Failed" => get_style(app, &app.theme.elements.job_status.failed),
        "Submit Failed" => get_style(app, &app.theme.elements.job_status.submit_failed),
        "Pending" => get_style(app, &app.theme.elements.job_status.pending),
        "Running" => get_style(app, &app.theme.elements.job_status.running),
        "Queued" => get_style(app, &app.theme.elements.job_status.queued),
        "Blocked" => get_style(app, &app.theme.elements.job_status.blocked),
        "Submitting..." => get_style(app, &app.theme.elements.job_status.submitting),
        _ => get_style(app, &app.theme.elements.job_status.unknown),
    }
}

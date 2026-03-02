use crate::app::App;
use crate::model::JobStatus;
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

pub fn status_style(app: &App, status: &JobStatus) -> Style {
    match status {
        JobStatus::Succeeded => get_style(app, &app.theme.elements.job_status.succeeded),
        JobStatus::Failed => get_style(app, &app.theme.elements.job_status.failed),
        JobStatus::SubmitFailed => get_style(app, &app.theme.elements.job_status.submit_failed),
        JobStatus::Pending => get_style(app, &app.theme.elements.job_status.pending),
        JobStatus::Running => get_style(app, &app.theme.elements.job_status.running),
        JobStatus::Queued => get_style(app, &app.theme.elements.job_status.queued),
        JobStatus::Blocked => get_style(app, &app.theme.elements.job_status.blocked),
        JobStatus::Submitting => get_style(app, &app.theme.elements.job_status.submitting),
        JobStatus::Unknown => get_style(app, &app.theme.elements.job_status.unknown),
    }
}

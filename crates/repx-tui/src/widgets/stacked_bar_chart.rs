use crate::model::StatusCounts;
use ratatui::{
    prelude::{Buffer, Color, Rect},
    widgets::Widget,
};
use std::collections::BTreeMap;

pub struct StackedBarChart<'a> {
    pub data: &'a [StatusCounts],
    pub status_colors: &'a BTreeMap<&'static str, Color>,
}
impl Widget for StackedBarChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 1 || area.height < 1 || self.data.is_empty() {
            return;
        }

        let width = area.width as usize;
        let data_len = self.data.len();

        let resampled_data = if data_len > width {
            let mut bins = vec![StatusCounts::default(); width];
            for (i, counts) in self.data.iter().enumerate() {
                let bin_index = (i * width) / data_len;
                let bin = &mut bins[bin_index];
                bin.succeeded += counts.succeeded;
                bin.failed += counts.failed;
                bin.running += counts.running;
                bin.pending += counts.pending;
                bin.queued += counts.queued;
                bin.blocked += counts.blocked;
                bin.submitting += counts.submitting;
                bin.unknown += counts.unknown;
                bin.total += counts.total;
            }
            bins
        } else {
            let mut stretched = Vec::with_capacity(width);
            for i in 0..width {
                let original_index = if data_len > 0 {
                    (i * data_len) / width
                } else {
                    0
                };
                if let Some(data) = self.data.get(original_index) {
                    stretched.push(data.clone());
                }
            }
            stretched
        };

        for (i, counts) in resampled_data.iter().enumerate() {
            let x = area.left() + i as u16;
            if x >= area.right() {
                continue;
            }

            if counts.total == 0 {
                continue;
            }

            let status_percentages: BTreeMap<&str, f64> = BTreeMap::from([
                ("Succeeded", counts.succeeded as f64 / counts.total as f64),
                ("Failed", counts.failed as f64 / counts.total as f64),
                ("Running", counts.running as f64 / counts.total as f64),
                ("Pending", counts.pending as f64 / counts.total as f64),
                ("Queued", counts.queued as f64 / counts.total as f64),
                ("Blocked", counts.blocked as f64 / counts.total as f64),
                (
                    "Submitting...",
                    counts.submitting as f64 / counts.total as f64,
                ),
                ("Unknown", counts.unknown as f64 / counts.total as f64),
            ]);

            let mut segments: Vec<(&str, f64)> = Vec::new();
            for (status, percentage) in status_percentages.iter() {
                if *percentage > 0.0 {
                    segments.push((*status, *percentage));
                }
            }

            for y in (area.top()..area.bottom()).rev() {
                let get_color_for_pct = |pct: f64| {
                    let mut cumulative_pct = 0.0;
                    for (status, percentage) in &segments {
                        cumulative_pct += *percentage;
                        if pct < cumulative_pct {
                            return self
                                .status_colors
                                .get(status)
                                .copied()
                                .unwrap_or(Color::Reset);
                        }
                    }
                    Color::Reset
                };

                let cell_bottom_pct = (area.bottom() - 1 - y) as f64 / area.height as f64;
                let cell_top_pct = (area.bottom() - y) as f64 / area.height as f64;

                let bottom_color = get_color_for_pct(cell_bottom_pct);

                let mut first_boundary: Option<f64> = None;
                let mut cumulative_pct = 0.0;
                for (_status, percentage) in &segments {
                    cumulative_pct += *percentage;
                    if cumulative_pct > cell_bottom_pct && cumulative_pct < cell_top_pct {
                        first_boundary = Some(cumulative_pct);
                        break;
                    }
                }

                let cell = &mut buf[(x, y)];

                if let Some(boundary) = first_boundary {
                    const LOWER_BLOCKS: [&str; 7] = [" ", "▂", "▃", "▄", "▅", "▆", "▇"];
                    let top_color = get_color_for_pct(boundary);

                    let height_of_bottom_in_cell_pct =
                        (boundary - cell_bottom_pct) / (cell_top_pct - cell_bottom_pct);
                    let eights = (height_of_bottom_in_cell_pct * 8.0).round() as usize;

                    if eights == 0 {
                        if top_color != Color::Reset {
                            cell.set_symbol("█").set_fg(top_color);
                        }
                    } else if eights >= 8 {
                        if bottom_color != Color::Reset {
                            cell.set_symbol("█").set_fg(bottom_color);
                        }
                    } else {
                        cell.set_symbol(LOWER_BLOCKS[eights - 1])
                            .set_fg(bottom_color)
                            .set_bg(top_color);
                    }
                } else if bottom_color != Color::Reset {
                    cell.set_symbol("█").set_fg(bottom_color);
                }
            }
        }
    }
}

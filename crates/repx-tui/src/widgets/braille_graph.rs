use ratatui::{
    prelude::{Buffer, Color, Rect},
    widgets::Widget,
};

const BRAILLE_PATTERNS: [[&str; 5]; 5] = [
    ["⠀", "⡀", "⡄", "⡆", "⡇"],
    ["⢀", "⣀", "⣄", "⣆", "⣇"],
    ["⢠", "⣠", "⣤", "⣦", "⣧"],
    ["⢰", "⣰", "⣴", "⣶", "⣷"],
    ["⢸", "⣸", "⣼", "⣾", "⣿"],
];

const INVERTED_BRAILLE_PATTERNS: [[&str; 5]; 5] = [
    ["⠀", "⠁", "⠃", "⠇", "⡇"],
    ["⠈", "⠉", "⠋", "⠏", "⡏"],
    ["⠘", "⠙", "⠛", "⠟", "⡟"],
    ["⠸", "⠹", "⠻", "⠿", "⡿"],
    ["⢸", "⢹", "⢻", "⢿", "⣿"],
];

fn interpolate_color(c1: Color, c2: Color, t: f64) -> Color {
    if let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (c1, c2) {
        let t = t.clamp(0.0, 1.0);
        let r = ((r1 as f64) * (1.0 - t) + (r2 as f64) * t).round() as u8;
        let g = ((g1 as f64) * (1.0 - t) + (g2 as f64) * t).round() as u8;
        let b = ((b1 as f64) * (1.0 - t) + (b2 as f64) * t).round() as u8;
        Color::Rgb(r, g, b)
    } else {
        if t < 0.5 {
            c1
        } else {
            c2
        }
    }
}

#[derive(Clone, Copy)]
pub enum GraphDirection {
    Upwards,
    Downwards,
}

pub struct BrailleGraph<'a> {
    pub data: &'a [f64],
    pub max_value: f64,
    pub low_color: Color,
    pub high_color: Color,
    pub direction: GraphDirection,
}

impl<'a> Widget for BrailleGraph<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 1 || area.height == 0 || self.max_value == 0.0 || self.data.is_empty() {
            return;
        }

        let width = area.width as usize;
        let num_points = width * 2;
        let data_len = self.data.len();

        let resampled_data: Vec<f64> = if data_len > num_points {
            // Down-sample by binning and averaging
            let mut bins = vec![(0.0, 0); num_points]; // (sum, count)
            for (i, &value) in self.data.iter().enumerate() {
                let bin_index = (i * num_points) / data_len;
                bins[bin_index].0 += value;
                bins[bin_index].1 += 1;
            }
            bins.iter()
                .map(|&(sum, count)| if count > 0 { sum / count as f64 } else { 0.0 })
                .collect()
        } else {
            // Stretch or use as-is
            let mut stretched = Vec::with_capacity(num_points);
            for i in 0..num_points {
                let original_index = (i * data_len) / num_points;
                stretched.push(self.data.get(original_index).copied().unwrap_or(0.0));
            }
            stretched
        };

        const DOTS_PER_ROW: usize = 4;
        let total_dots_height = area.height as usize * DOTS_PER_ROW;

        for (chunk, column_x) in resampled_data.chunks(2).zip(area.left()..area.right()) {
            let left_value = chunk.get(0).copied().unwrap_or(0.0);
            let right_value = chunk.get(1).copied().unwrap_or(0.0);

            let left_normalized = (left_value / self.max_value).clamp(0.0, 1.0);
            let right_normalized = (right_value / self.max_value).clamp(0.0, 1.0);

            let left_total_dots = (left_normalized * (total_dots_height.saturating_sub(1)) as f64
                + 1.0)
                .round() as usize;
            let right_total_dots = (right_normalized * (total_dots_height.saturating_sub(1)) as f64
                + 1.0)
                .round() as usize;

            let column_height =
                (left_total_dots.max(right_total_dots) as f64 / DOTS_PER_ROW as f64).ceil() as u16;

            let rows = match self.direction {
                GraphDirection::Upwards => (0..column_height)
                    .map(|i| area.bottom().saturating_sub(1).saturating_sub(i))
                    .collect::<Vec<_>>(),
                GraphDirection::Downwards => (0..column_height)
                    .map(|i| area.top() + i)
                    .collect::<Vec<_>>(),
            };

            for (i, row_y) in rows.into_iter().enumerate() {
                if row_y >= area.bottom() {
                    continue;
                }
                let dots_below = i * DOTS_PER_ROW;
                let left_dots = left_total_dots.saturating_sub(dots_below).min(DOTS_PER_ROW);
                let right_dots = right_total_dots
                    .saturating_sub(dots_below)
                    .min(DOTS_PER_ROW);

                let symbol = match self.direction {
                    GraphDirection::Upwards => BRAILLE_PATTERNS[right_dots][left_dots],
                    GraphDirection::Downwards => INVERTED_BRAILLE_PATTERNS[right_dots][left_dots],
                };

                let t = match self.direction {
                    GraphDirection::Upwards => i as f64 / area.height as f64,
                    GraphDirection::Downwards => 1.0 - (i as f64 / area.height as f64),
                };

                let color = interpolate_color(self.low_color, self.high_color, t);

                buf.cell_mut((column_x, row_y))
                    .unwrap()
                    .set_symbol(symbol)
                    .set_fg(color);
            }
        }
    }
}

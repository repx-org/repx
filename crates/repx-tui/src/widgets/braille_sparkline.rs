use ratatui::{
    prelude::{Buffer, Rect, Style},
    widgets::Widget,
};

const BRAILLE_PATTERNS: [[&str; 5]; 5] = [
    ["⠀", "⡀", "⡄", "⡆", "⡇"],
    ["⢀", "⣀", "⣄", "⣆", "⣇"],
    ["⢠", "⣠", "⣤", "⣦", "⣧"],
    ["⢰", "⣰", "⣴", "⣶", "⣷"],
    ["⢸", "⣸", "⣼", "⣾", "⣿"],
];

pub struct BrailleSparkline<'a> {
    pub data: &'a [f64],
    pub max_value: f64,
    pub style: Style,
}

impl<'a> Widget for BrailleSparkline<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 1 || self.max_value == 0.0 {
            return;
        }

        const DOTS_PER_CELL: usize = 4;

        let num_points_to_render = (area.width * 2) as usize;
        let data_len = self.data.len();
        let start_index = data_len.saturating_sub(num_points_to_render);
        let data_slice = &self.data[start_index..];

        let mut current_x = area.left();

        let padding_cells = area
            .width
            .saturating_sub((data_slice.len() as f32 / 2.0).ceil() as u16);
        for _ in 0..padding_cells {
            if current_x < area.right() {
                buf.cell_mut((current_x, area.top()))
                    .unwrap()
                    .set_symbol("⠀")
                    .set_style(self.style);
                current_x += 1;
            }
        }

        for chunk in data_slice.chunks(2) {
            if current_x >= area.right() {
                break;
            }

            let left_value = chunk.get(0).copied().unwrap_or(0.0);
            let right_value = chunk.get(1).copied().unwrap_or(0.0);

            let left_normalized = (left_value / self.max_value).clamp(0.0, 1.0);
            let right_normalized = (right_value / self.max_value).clamp(0.0, 1.0);

            let left_dots = (left_normalized * DOTS_PER_CELL as f64).round() as usize;
            let right_dots = (right_normalized * DOTS_PER_CELL as f64).round() as usize;

            let symbol = BRAILLE_PATTERNS[right_dots.min(4)][left_dots.min(4)];

            buf.cell_mut((current_x, area.top()))
                .unwrap()
                .set_symbol(symbol)
                .set_style(self.style);

            current_x += 1;
        }
    }
}

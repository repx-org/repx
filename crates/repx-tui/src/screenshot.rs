use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::Buffer;

pub fn buffer_to_ansi(buf: &Buffer) -> String {
    let mut out_buf: Vec<u8> = Vec::with_capacity(buf.content.len() * 4);
    {
        let mut backend = CrosstermBackend::new(&mut out_buf);

        let cells = buf.content.iter().enumerate().map(|(i, cell)| {
            let (x, y) = buf.pos_of(i);
            (x, y, cell)
        });

        let _ = backend.draw(cells);
        let _ = Backend::flush(&mut backend);
    }

    let raw_str = String::from_utf8_lossy(&out_buf);

    render_to_lines(&raw_str, buf.area.height)
}

fn render_to_lines(raw: &str, height: u16) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut current_row: u16 = 0;
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                let mut seq = String::new();

                loop {
                    match chars.peek() {
                        Some(&c) if c.is_ascii_alphabetic() => {
                            let cmd = c;
                            chars.next();

                            if cmd == 'H' {
                                let parts: Vec<&str> = seq.split(';').collect();
                                if parts.len() == 2 {
                                    if let Ok(row) = parts[0].parse::<u16>() {
                                        let new_row = row.saturating_sub(1);
                                        while current_row < new_row {
                                            out.push('\n');
                                            current_row += 1;
                                        }
                                    }
                                }
                            } else if cmd == 'J' {
                            } else {
                                out.push('\x1b');
                                out.push('[');
                                out.push_str(&seq);
                                out.push(cmd);
                            }
                            break;
                        }
                        Some(&c) => {
                            seq.push(c);
                            chars.next();
                        }
                        None => {
                            out.push('\x1b');
                            out.push('[');
                            out.push_str(&seq);
                            break;
                        }
                    }
                }
            } else {
                out.push(ch);
            }
        } else {
            out.push(ch);
        }
    }

    while out.ends_with('\n') && out.matches('\n').count() >= height as usize {
        out.pop();
    }

    out
}

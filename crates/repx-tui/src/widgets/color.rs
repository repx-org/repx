use ratatui::prelude::Color;

struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

struct Hsl {
    h: f64,
    s: f64,
    l: f64,
}

fn rgb_to_hsl(rgb: Rgb) -> Hsl {
    let r = rgb.r as f64 / 255.0;
    let g = rgb.g as f64 / 255.0;
    let b = rgb.b as f64 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);

    let mut h = 0.0;
    let mut s;
    let l = (max + min) / 2.0;

    if max == min {
        s = 0.0;
    } else {
        let d = max - min;
        s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        if max == r {
            h = (g - b) / d + (if g < b { 6.0 } else { 0.0 });
        } else if max == g {
            h = (b - r) / d + 2.0;
        } else {
            h = (r - g) / d + 4.0;
        }
        h /= 6.0;
    }

    Hsl {
        h: h * 360.0,
        s: s * 100.0,
        l: l * 100.0,
    }
}

fn hsl_to_rgb(hsl: Hsl) -> Rgb {
    let h = hsl.h / 360.0;
    let s = hsl.s / 100.0;
    let l = hsl.l / 100.0;

    let r;
    let g;
    let b;

    if s == 0.0 {
        r = l;
        g = l;
        b = l;
    } else {
        let hue2rgb = |p: f64, q: f64, mut t: f64| -> f64 {
            if t < 0.0 {
                t += 1.0;
            }
            if t > 1.0 {
                t -= 1.0;
            }
            if t < 1.0 / 6.0 {
                return p + (q - p) * 6.0 * t;
            }
            if t < 1.0 / 2.0 {
                return q;
            }
            if t < 2.0 / 3.0 {
                return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
            }
            p
        };

        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;

        r = hue2rgb(p, q, h + 1.0 / 3.0);
        g = hue2rgb(p, q, h);
        b = hue2rgb(p, q, h - 1.0 / 3.0);
    }

    Rgb {
        r: (r * 255.0).round() as u8,
        g: (g * 255.0).round() as u8,
        b: (b * 255.0).round() as u8,
    }
}

fn desaturate(color: Color, amount: f64) -> Color {
    if let Color::Rgb(r, g, b) = color {
        let mut hsl = rgb_to_hsl(Rgb { r, g, b });
        hsl.s *= 1.0 - amount;
        let new_rgb = hsl_to_rgb(hsl);
        Color::Rgb(new_rgb.r, new_rgb.g, new_rgb.b)
    } else {
        color
    }
}

fn blend(color1: Color, color2: Color, amount: f64) -> Color {
    if let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (color1, color2) {
        Color::Rgb(
            (r1 as f64 * amount + r2 as f64 * (1.0 - amount)).round() as u8,
            (g1 as f64 * amount + g2 as f64 * (1.0 - amount)).round() as u8,
            (b1 as f64 * amount + b2 as f64 * (1.0 - amount)).round() as u8,
        )
    } else {
        color1
    }
}

pub fn muted(color: Color, bg: Color) -> Color {
    blend(desaturate(color, 0.4), bg, 0.6)
}

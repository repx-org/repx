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
    let s;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_to_hsl_pure_red() {
        let rgb = Rgb { r: 255, g: 0, b: 0 };
        let hsl = rgb_to_hsl(rgb);
        assert!((hsl.h - 0.0).abs() < 1.0);
        assert!((hsl.s - 100.0).abs() < 1.0);
        assert!((hsl.l - 50.0).abs() < 1.0);
    }

    #[test]
    fn test_rgb_to_hsl_pure_green() {
        let rgb = Rgb { r: 0, g: 255, b: 0 };
        let hsl = rgb_to_hsl(rgb);
        assert!((hsl.h - 120.0).abs() < 1.0);
        assert!((hsl.s - 100.0).abs() < 1.0);
        assert!((hsl.l - 50.0).abs() < 1.0);
    }

    #[test]
    fn test_rgb_to_hsl_pure_blue() {
        let rgb = Rgb { r: 0, g: 0, b: 255 };
        let hsl = rgb_to_hsl(rgb);
        assert!((hsl.h - 240.0).abs() < 1.0);
        assert!((hsl.s - 100.0).abs() < 1.0);
        assert!((hsl.l - 50.0).abs() < 1.0);
    }

    #[test]
    fn test_rgb_to_hsl_white() {
        let rgb = Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        let hsl = rgb_to_hsl(rgb);
        assert!((hsl.s - 0.0).abs() < 0.001);
        assert!((hsl.l - 100.0).abs() < 1.0);
    }

    #[test]
    fn test_rgb_to_hsl_black() {
        let rgb = Rgb { r: 0, g: 0, b: 0 };
        let hsl = rgb_to_hsl(rgb);
        assert!((hsl.s - 0.0).abs() < 0.001);
        assert!((hsl.l - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_rgb_to_hsl_gray() {
        let rgb = Rgb {
            r: 128,
            g: 128,
            b: 128,
        };
        let hsl = rgb_to_hsl(rgb);
        assert!((hsl.s - 0.0).abs() < 0.001);
        assert!((hsl.l - 50.0).abs() < 2.0);
    }

    #[test]
    fn test_hsl_to_rgb_pure_red() {
        let hsl = Hsl {
            h: 0.0,
            s: 100.0,
            l: 50.0,
        };
        let rgb = hsl_to_rgb(hsl);
        assert_eq!(rgb.r, 255);
        assert_eq!(rgb.g, 0);
        assert_eq!(rgb.b, 0);
    }

    #[test]
    fn test_hsl_to_rgb_pure_green() {
        let hsl = Hsl {
            h: 120.0,
            s: 100.0,
            l: 50.0,
        };
        let rgb = hsl_to_rgb(hsl);
        assert_eq!(rgb.r, 0);
        assert_eq!(rgb.g, 255);
        assert_eq!(rgb.b, 0);
    }

    #[test]
    fn test_hsl_to_rgb_pure_blue() {
        let hsl = Hsl {
            h: 240.0,
            s: 100.0,
            l: 50.0,
        };
        let rgb = hsl_to_rgb(hsl);
        assert_eq!(rgb.r, 0);
        assert_eq!(rgb.g, 0);
        assert_eq!(rgb.b, 255);
    }

    #[test]
    fn test_hsl_to_rgb_white() {
        let hsl = Hsl {
            h: 0.0,
            s: 0.0,
            l: 100.0,
        };
        let rgb = hsl_to_rgb(hsl);
        assert_eq!(rgb.r, 255);
        assert_eq!(rgb.g, 255);
        assert_eq!(rgb.b, 255);
    }

    #[test]
    fn test_hsl_to_rgb_black() {
        let hsl = Hsl {
            h: 0.0,
            s: 0.0,
            l: 0.0,
        };
        let rgb = hsl_to_rgb(hsl);
        assert_eq!(rgb.r, 0);
        assert_eq!(rgb.g, 0);
        assert_eq!(rgb.b, 0);
    }

    #[test]
    fn test_hsl_to_rgb_gray() {
        let hsl = Hsl {
            h: 0.0,
            s: 0.0,
            l: 50.0,
        };
        let rgb = hsl_to_rgb(hsl);
        assert_eq!(rgb.r, rgb.g);
        assert_eq!(rgb.g, rgb.b);
        assert!((rgb.r as i32 - 128).abs() <= 1);
    }

    #[test]
    fn test_rgb_hsl_roundtrip_arbitrary() {
        let original = Rgb {
            r: 100,
            g: 150,
            b: 200,
        };
        let hsl = rgb_to_hsl(Rgb {
            r: original.r,
            g: original.g,
            b: original.b,
        });
        let result = hsl_to_rgb(hsl);
        assert!((result.r as i32 - original.r as i32).abs() <= 1);
        assert!((result.g as i32 - original.g as i32).abs() <= 1);
        assert!((result.b as i32 - original.b as i32).abs() <= 1);
    }

    #[test]
    fn test_desaturate_fully() {
        let color = Color::Rgb(255, 0, 0);
        let result = desaturate(color, 1.0);
        if let Color::Rgb(r, g, b) = result {
            assert_eq!(r, g);
            assert_eq!(g, b);
        } else {
            panic!("Expected Rgb color");
        }
    }

    #[test]
    fn test_desaturate_zero() {
        let color = Color::Rgb(255, 0, 0);
        let result = desaturate(color, 0.0);
        if let Color::Rgb(r, g, b) = result {
            assert_eq!(r, 255);
            assert_eq!(g, 0);
            assert_eq!(b, 0);
        } else {
            panic!("Expected Rgb color");
        }
    }

    #[test]
    fn test_desaturate_partial() {
        let color = Color::Rgb(255, 0, 0);
        let result = desaturate(color, 0.5);
        if let Color::Rgb(r, g, b) = result {
            assert!(r > g && r > b);
            assert!(g > 0);
        } else {
            panic!("Expected Rgb color");
        }
    }

    #[test]
    fn test_desaturate_non_rgb_passthrough() {
        let color = Color::Blue;
        let result = desaturate(color, 0.5);
        assert_eq!(result, Color::Blue);
    }

    #[test]
    fn test_blend_full_first() {
        let color1 = Color::Rgb(255, 0, 0);
        let color2 = Color::Rgb(0, 255, 0);
        let result = blend(color1, color2, 1.0);
        if let Color::Rgb(r, g, b) = result {
            assert_eq!(r, 255);
            assert_eq!(g, 0);
            assert_eq!(b, 0);
        } else {
            panic!("Expected Rgb color");
        }
    }

    #[test]
    fn test_blend_full_second() {
        let color1 = Color::Rgb(255, 0, 0);
        let color2 = Color::Rgb(0, 255, 0);
        let result = blend(color1, color2, 0.0);
        if let Color::Rgb(r, g, b) = result {
            assert_eq!(r, 0);
            assert_eq!(g, 255);
            assert_eq!(b, 0);
        } else {
            panic!("Expected Rgb color");
        }
    }

    #[test]
    fn test_blend_half() {
        let color1 = Color::Rgb(200, 100, 0);
        let color2 = Color::Rgb(100, 200, 100);
        let result = blend(color1, color2, 0.5);
        if let Color::Rgb(r, g, b) = result {
            assert_eq!(r, 150);
            assert_eq!(g, 150);
            assert_eq!(b, 50);
        } else {
            panic!("Expected Rgb color");
        }
    }

    #[test]
    fn test_blend_non_rgb_passthrough() {
        let color1 = Color::Red;
        let color2 = Color::Rgb(0, 255, 0);
        let result = blend(color1, color2, 0.5);
        assert_eq!(result, Color::Red);
    }

    #[test]
    fn test_muted_returns_rgb() {
        let color = Color::Rgb(255, 0, 0);
        let bg = Color::Rgb(30, 30, 30);
        let result = muted(color, bg);
        assert!(matches!(result, Color::Rgb(_, _, _)));
    }

    #[test]
    fn test_muted_reduces_saturation() {
        let color = Color::Rgb(255, 0, 0);
        let bg = Color::Rgb(128, 128, 128);
        let result = muted(color, bg);
        if let Color::Rgb(r, g, _b) = result {
            assert!(r > g);
            assert!(g > 0);
            assert!(r < 255);
        } else {
            panic!("Expected Rgb color");
        }
    }

    #[test]
    fn test_muted_non_rgb_passthrough() {
        let color = Color::Cyan;
        let bg = Color::Rgb(0, 0, 0);
        let result = muted(color, bg);
        assert_eq!(result, Color::Cyan);
    }
}

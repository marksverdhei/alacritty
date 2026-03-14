//! Default color theme and color resolution.
//!
//! Resolves terminal Color values to concrete RGB for rendering.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

/// Default Alacritty dark theme colors.
pub fn default_named_color(color: NamedColor) -> Rgb {
    match color {
        NamedColor::Black => Rgb { r: 0x1d, g: 0x1f, b: 0x21 },
        NamedColor::Red => Rgb { r: 0xcc, g: 0x66, b: 0x66 },
        NamedColor::Green => Rgb { r: 0xb5, g: 0xbd, b: 0x68 },
        NamedColor::Yellow => Rgb { r: 0xf0, g: 0xc6, b: 0x74 },
        NamedColor::Blue => Rgb { r: 0x81, g: 0xa2, b: 0xbe },
        NamedColor::Magenta => Rgb { r: 0xb2, g: 0x94, b: 0xbb },
        NamedColor::Cyan => Rgb { r: 0x8a, g: 0xbe, b: 0xb7 },
        NamedColor::White => Rgb { r: 0xc5, g: 0xc8, b: 0xc6 },
        NamedColor::BrightBlack => Rgb { r: 0x96, g: 0x98, b: 0x96 },
        NamedColor::BrightRed => Rgb { r: 0xcc, g: 0x66, b: 0x66 },
        NamedColor::BrightGreen => Rgb { r: 0xb5, g: 0xbd, b: 0x68 },
        NamedColor::BrightYellow => Rgb { r: 0xf0, g: 0xc6, b: 0x74 },
        NamedColor::BrightBlue => Rgb { r: 0x81, g: 0xa2, b: 0xbe },
        NamedColor::BrightMagenta => Rgb { r: 0xb2, g: 0x94, b: 0xbb },
        NamedColor::BrightCyan => Rgb { r: 0x8a, g: 0xbe, b: 0xb7 },
        NamedColor::BrightWhite => Rgb { r: 0xff, g: 0xff, b: 0xff },
        NamedColor::Foreground => Rgb { r: 0xc5, g: 0xc8, b: 0xc6 },
        NamedColor::Background => Rgb { r: 0x1d, g: 0x1f, b: 0x21 },
        NamedColor::Cursor => Rgb { r: 0xc5, g: 0xc8, b: 0xc6 },
        _ => Rgb { r: 0xc5, g: 0xc8, b: 0xc6 },
    }
}

/// Resolve a 256-color indexed color to RGB.
fn indexed_color(index: u8) -> Rgb {
    if index < 16 {
        // Standard ANSI colors.
        let named = match index {
            0 => NamedColor::Black,
            1 => NamedColor::Red,
            2 => NamedColor::Green,
            3 => NamedColor::Yellow,
            4 => NamedColor::Blue,
            5 => NamedColor::Magenta,
            6 => NamedColor::Cyan,
            7 => NamedColor::White,
            8 => NamedColor::BrightBlack,
            9 => NamedColor::BrightRed,
            10 => NamedColor::BrightGreen,
            11 => NamedColor::BrightYellow,
            12 => NamedColor::BrightBlue,
            13 => NamedColor::BrightMagenta,
            14 => NamedColor::BrightCyan,
            15 => NamedColor::BrightWhite,
            _ => unreachable!(),
        };
        default_named_color(named)
    } else if index < 232 {
        // 6x6x6 color cube.
        let index = index as u16 - 16;
        let r_idx = index / 36;
        let g_idx = (index % 36) / 6;
        let b_idx = index % 6;
        let to_val = |i: u16| if i == 0 { 0u8 } else { (55 + 40 * i) as u8 };
        Rgb { r: to_val(r_idx), g: to_val(g_idx), b: to_val(b_idx) }
    } else {
        // Grayscale ramp.
        let val = 8 + 10 * (index - 232);
        Rgb { r: val, g: val, b: val }
    }
}

/// Resolve a terminal Color to RGB using the color palette.
pub fn resolve_color(color: &Color, colors: &Colors) -> Rgb {
    match color {
        Color::Named(name) => {
            colors[*name].unwrap_or_else(|| default_named_color(*name))
        },
        Color::Spec(rgb) => *rgb,
        Color::Indexed(idx) => {
            colors[*idx as usize].unwrap_or_else(|| indexed_color(*idx))
        },
    }
}

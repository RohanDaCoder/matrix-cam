//! Character effect renderer: downsample → tone → draw, one fused parallel pass.

pub mod atlas;
pub mod font;
pub mod fx;
pub mod grid;
pub mod renderer;
pub mod tone;

pub use font::{GLYPHS, RAMP, SHADES};
pub use grid::Grid;
pub use renderer::Renderer;
pub use tone::tone_from_levels;

use crate::config::ColorMode;

/// Map a cell colour per the active color mode (shared by renderer rows and TUI).
#[allow(clippy::too_many_arguments)]
pub fn cell_color(
    mode: ColorMode,
    ink: [u8; 3],
    _bg: [u8; 3],
    accent: [u8; 3],
    rgb: [u8; 3],
    luma: u8,
    heatcut: u8,
    sigcut: i32,
    red_dom: u8,
) -> [u8; 3] {
    match mode {
        ColorMode::Mono => ink,
        ColorMode::Signal => {
            if red_dom as i32 > sigcut { accent } else { ink }
        }
        ColorMode::Heat => {
            if luma > heatcut { accent } else { ink }
        }
        ColorMode::Duo => mix(ink, accent, luma as f32 / 255.0),
        ColorMode::Color => rgb,
    }
}

pub fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
    ]
}

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

/// Everything `cell_color` needs besides the settings palette. Bundling
/// these keeps the signature small (they travel together everywhere).
#[derive(Clone, Copy)]
pub struct CellCtx {
    pub ink: [u8; 3],
    pub accent: [u8; 3],
    pub luma: u8,
    /// Bright-above-this cells take the accent color (Heat mode).
    pub heatcut: u8,
    /// Red-dominance cutoff for Signal mode.
    pub sigcut: i32,
    /// How red-dominant this cell is (0-255).
    pub red_dom: u8,
}

/// Map a cell colour per the active color mode (shared by renderer rows and TUI).
pub fn cell_color(mode: ColorMode, ctx: CellCtx, rgb: [u8; 3]) -> [u8; 3] {
    match mode {
        ColorMode::Mono => ctx.ink,
        ColorMode::Signal => {
            if ctx.red_dom as i32 > ctx.sigcut { ctx.accent } else { ctx.ink }
        }
        ColorMode::Heat => {
            if ctx.luma > ctx.heatcut { ctx.accent } else { ctx.ink }
        }
        ColorMode::Duo => mix(ctx.ink, ctx.accent, ctx.luma as f32 / 255.0),
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

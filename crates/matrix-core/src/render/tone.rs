//! Tone curve helpers shared by the renderer and the auto controller.

use crate::config::clampf;

/// Build a 256-entry brightness/contrast LUT.
pub fn build_lut(brightness: i32, contrast: i32) -> [u8; 256] {
    let bri = brightness as f32 * 1.6;
    let c = contrast as f32 * 2.55;
    let cf = (259.0 * (c + 255.0)) / (255.0 * (259.0 - c));
    let mut lut = [0u8; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        *v = ((i as f32 - 128.0) * cf + 128.0 + bri).clamp(0.0, 255.0) as u8;
    }
    lut
}

/// Contrast/brightness that stretches [p_lo, p_hi] and parks the mean on target.
/// Returns (contrast, brightness) in the -100..=100 slider space.
pub fn tone_from_levels(p_lo: u8, p_hi: u8, mean: f32, target_mean: f32) -> (f32, f32) {
    let span = (p_hi as f32 - p_lo as f32).max(8.0);
    let cf = clampf(255.0 / span, 0.2, 12.0);
    let bri = target_mean - 128.0 - (mean - 128.0) * cf;
    let c = 255.0 * 259.0 * (cf - 1.0) / (259.0 + 255.0 * cf);
    (c / 2.55, bri / 1.6)
}

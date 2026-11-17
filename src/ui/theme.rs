//! Shared GUI theme + small text helpers.

use egui::{Color32, Visuals};

pub const RED: Color32 = Color32::from_rgb(255, 47, 47);
pub const DIMC: Color32 = Color32::from_rgb(143, 141, 133);
pub const OK: Color32 = Color32::from_rgb(61, 255, 138);
pub const WARN: Color32 = Color32::from_rgb(255, 176, 32);

pub fn visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.panel_fill = Color32::from_rgb(8, 9, 12);
    v.window_fill = Color32::from_rgb(12, 14, 18);
    v.extreme_bg_color = Color32::from_rgb(5, 6, 8);
    v.override_text_color = Some(Color32::from_rgb(201, 199, 191));
    v.selection.bg_fill = Color32::from_rgb(90, 20, 20);
    v.widgets.inactive.bg_fill = Color32::from_rgb(18, 21, 27);
    v.widgets.hovered.bg_fill = Color32::from_rgb(30, 24, 26);
    v.widgets.active.bg_fill = Color32::from_rgb(255, 47, 47);
    v
}

/// Text progress bar, e.g. `▓▓▓░░░`.
pub fn bar(v01: f32, width: usize) -> String {
    let n = ((v01.clamp(0.0, 1.0)) * width as f32).round() as usize;
    let mut s = String::with_capacity(width * 3);
    for _ in 0..n {
        s.push('▓');
    }
    for _ in n..width {
        s.push('░');
    }
    s
}

/// Unicode sparkline of a 64-bin histogram.
pub fn sparkline(hist: &[u32; 64]) -> String {
    const B: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = hist.iter().copied().max().unwrap_or(1).max(1);
    let mut s = String::new();
    for i in 0..32 {
        let v = hist[i * 2] + hist[i * 2 + 1];
        let f = (v as f32 / max as f32 * 7.0).round() as usize;
        s.push(B[f.min(7)]);
    }
    s
}

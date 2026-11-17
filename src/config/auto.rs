//! Everything the adaptive controller is allowed to do, and how hard.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoConfig {
    pub enabled: bool,
    pub levels: bool,
    pub density: bool,
    pub signal: bool,
    pub fx: bool,
    pub polarity: bool,
    pub target_mean: f32,
    pub low_pct: f32,
    pub high_pct: f32,
    pub cols_min: u32,
    pub cols_max: u32,
    pub detail_gain: f32,
    pub accent_coverage: f32,
    pub motion_glitch: f32,
    pub motion_trails: f32,
    pub noise_floor: f32,
    pub response_ms: f32,
    pub interval_ms: u32,
}

impl Default for AutoConfig {
    fn default() -> Self {
        AutoConfig {
            enabled: false,
            levels: true,
            density: true,
            signal: true,
            fx: true,
            polarity: false,
            target_mean: 128.0,
            low_pct: 2.0,
            high_pct: 98.0,
            cols_min: 90,
            cols_max: 190,
            detail_gain: 1.6,
            accent_coverage: 0.06,
            motion_glitch: 45.0,
            motion_trails: 35.0,
            noise_floor: 22.0,
            response_ms: 450.0,
            interval_ms: 120,
        }
    }
}

impl AutoConfig {
    /// Human labels for the five auto subsystems (GUI checkboxes / TUI rows).
    pub const SUBSYSTEMS: [(&str, &str); 5] = [
        ("levels", "Auto brightness"),
        ("density", "Auto detail"),
        ("signal", "Auto accent"),
        ("fx", "Auto glitch/fx"),
        ("polarity", "Auto invert"),
    ];
}

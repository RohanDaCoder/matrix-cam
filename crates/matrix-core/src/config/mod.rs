//! Settings model, presets and persistence.

pub mod auto;
pub mod settings;

pub use auto::AutoConfig;
pub use settings::{ColorMode, GlyphSet, Settings, StartOverrides, Style, PRESETS};

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserPreset {
    pub name: String,
    pub set: Settings,
}

/// Everything persisted to `%APPDATA%\matrix-cam\config.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub set: Option<Settings>,
    pub auto: AutoConfig,
    pub user_presets: Vec<UserPreset>,
    pub preview_fps: u32,
    pub tui_fps: u32,
    pub threads: u32,
    /// Retro FPS limiter: cap the output frame rate for an 8-bit feel.
    #[serde(default)]
    pub fps_limit_enabled: bool,
    #[serde(default = "default_retro_fps")]
    pub fps_limit_fps: u32,
    /// Self-updater: check GitHub for a newer release when the app starts.
    #[serde(default = "default_true")]
    pub update_on_start: bool,
}

fn default_retro_fps() -> u32 {
    12
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            set: None,
            auto: AutoConfig::default(),
            user_presets: Vec::new(),
            preview_fps: 30,
            tui_fps: 30,
            threads: 0,
            fps_limit_enabled: false,
            fps_limit_fps: default_retro_fps(),
            update_on_start: true,
        }
    }
}

impl Config {
    pub fn path(custom: Option<&str>) -> PathBuf {
        if let Some(p) = custom {
            return PathBuf::from(p);
        }
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("matrix-cam").join("config.json")
    }

    pub fn load(custom: Option<&str>) -> Config {
        let p = Self::path(custom);
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str::<Config>(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, custom: Option<&str>) -> anyhow::Result<()> {
        let p = Self::path(custom);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&p, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

pub fn clampi(v: i32, lo: i32, hi: i32) -> i32 {
    v.max(lo).min(hi)
}
pub fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}

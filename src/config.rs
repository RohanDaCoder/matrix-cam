use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Style { Dots, Ascii, Cells }
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ColorMode { Mono, Signal, Heat, Duo, Color }

pub const PRESETS: [&str; 5] = ["ghost", "paper", "term", "vhs", "magma"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub style: Style,
    pub mode: ColorMode,
    pub cols: u32,
    pub brightness: i32,
    pub contrast: i32,
    pub threshold: i32,
    pub invert: bool,
    pub mirror: bool,
    pub ink: [u8; 3],
    pub bg: [u8; 3],
    pub accent: [u8; 3],
    pub glow: u32,
    pub trails: u32,
    pub scan: u32,
    pub noise: u32,
    pub glitch: u32,
}

impl Settings {
    pub fn preset(name: &str) -> Settings {
        let mut s = Settings {
            style: Style::Dots, mode: ColorMode::Signal, cols: 150,
            brightness: -4, contrast: 22, threshold: 70, invert: false, mirror: true,
            ink: [230, 227, 218], bg: [4, 5, 7], accent: [255, 38, 38],
            glow: 6, trails: 0, scan: 16, noise: 12, glitch: 16,
        };
        match name {
            "paper" => {
                s.style = Style::Ascii; s.cols = 170; s.brightness = 4; s.contrast = 30;
                s.threshold = 64; s.invert = true;
                s.ink = [25, 26, 28]; s.bg = [241, 239, 233]; s.accent = [224, 21, 21];
                s.glow = 0; s.scan = 30; s.noise = 18; s.glitch = 30;
            }
            "term" => {
                s.style = Style::Ascii; s.mode = ColorMode::Mono; s.cols = 128;
                s.brightness = 0; s.contrast = 18; s.invert = false;
                s.ink = [65, 255, 126]; s.bg = [2, 6, 4]; s.accent = [186, 255, 208];
                s.glow = 8; s.trails = 26; s.scan = 28; s.noise = 10; s.glitch = 8;
            }
            "vhs" => {
                s.style = Style::Ascii; s.mode = ColorMode::Color; s.cols = 100;
                s.brightness = 2; s.contrast = 16; s.invert = false;
                s.ink = [255, 255, 255]; s.bg = [7, 3, 16]; s.accent = [255, 47, 158];
                s.glow = 5; s.trails = 30; s.scan = 44; s.noise = 28; s.glitch = 42;
            }
            "magma" => {
                s.style = Style::Dots; s.mode = ColorMode::Heat; s.cols = 140;
                s.brightness = -6; s.contrast = 26; s.threshold = 58; s.invert = false;
                s.ink = [138, 43, 14]; s.bg = [10, 4, 5]; s.accent = [255, 207, 63];
                s.glow = 10; s.trails = 12; s.scan = 10; s.noise = 14; s.glitch = 10;
            }
            _ => {}
        }
        s
    }
    pub fn bg_is_light(&self) -> bool {
        let b = self.bg;
        (54 * b[0] as u32 + 183 * b[1] as u32 + 19 * b[2] as u32) >> 8 > 140
    }
}

/// Everything the adaptive controller is allowed to do, and how hard.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoConfig {
    pub enabled: bool,
    pub levels: bool,          // brightness + contrast from histogram stretch
    pub density: bool,         // cols from scene detail
    pub signal: bool,          // threshold from redness distribution
    pub fx: bool,              // glitch / trails / noise from motion + exposure
    pub polarity: bool,        // invert chosen to maximise glyph coverage
    pub target_mean: f32,      // desired mean luma after tone (0..255)
    pub low_pct: f32,          // black percentile (0..20)
    pub high_pct: f32,         // white percentile (80..100)
    pub cols_min: u32,
    pub cols_max: u32,
    pub detail_gain: f32,      // how strongly detail drives density
    pub accent_coverage: f32,  // fraction of cells that may take the accent
    pub motion_glitch: f32,    // glitch at full motion (0..100)
    pub motion_trails: f32,    // trails at full motion (0..90)
    pub noise_floor: f32,      // noise in a black scene (0..100)
    pub response_ms: f32,      // smoothing time constant
    pub interval_ms: u32,      // min ms between target recomputes
}

impl Default for AutoConfig {
    fn default() -> Self {
        AutoConfig {
            enabled: false,
            levels: true, density: true, signal: true, fx: true, polarity: false,
            target_mean: 128.0, low_pct: 2.0, high_pct: 98.0,
            cols_min: 90, cols_max: 190, detail_gain: 1.6,
            accent_coverage: 0.06,
            motion_glitch: 45.0, motion_trails: 35.0, noise_floor: 22.0,
            response_ms: 450.0, interval_ms: 120,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserPreset { pub name: String, pub set: Settings }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub set: Option<Settings>,
    pub auto: AutoConfig,
    pub user_presets: Vec<UserPreset>,
    pub preview_fps: u32,
    pub tui_fps: u32,
    pub threads: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            set: None, auto: AutoConfig::default(), user_presets: Vec::new(),
            preview_fps: 30, tui_fps: 30, threads: 0,
        }
    }
}

impl Config {
    pub fn path(custom: Option<&str>) -> PathBuf {
        if let Some(p) = custom { return PathBuf::from(p); }
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
        if let Some(dir) = p.parent() { std::fs::create_dir_all(dir)?; }
        std::fs::write(&p, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

pub fn clampi(v: i32, lo: i32, hi: i32) -> i32 { v.max(lo).min(hi) }
pub fn clampf(v: f32, lo: f32, hi: f32) -> f32 { v.max(lo).min(hi) }

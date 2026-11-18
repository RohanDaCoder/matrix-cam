//! Shared application state exchanged between the pipeline thread and the UI thread.

use crate::auto::analyze::Stats;
use crate::auto::controller::AutoReadout;
use crate::config::{AutoConfig, Settings, UserPreset};
use crate::render::Grid;
use crate::updater::UpdateState;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub backend: String,
    pub cam: String,
    pub out_w: u32,
    pub out_h: u32,
    pub out_fps: u32,
    pub fps: f32,
    pub threads: usize,
    pub error: String,
}

#[derive(Clone)]
pub struct View {
    pub frame: Arc<Vec<u8>>,
    pub grid: Arc<Grid>,
    pub w: u32,
    pub h: u32,
    pub serial: u64,
}

/// Retro FPS limiter (new): cap the output frame rate for an 8-bit pixel feel.
#[derive(Clone, Debug)]
pub struct FpsLimit {
    pub enabled: bool,
    pub fps: u32,
}

impl Default for FpsLimit {
    fn default() -> Self {
        FpsLimit { enabled: false, fps: 12 }
    }
}

pub struct Inner {
    pub set: Settings,
    pub eff: Settings,
    pub auto: AutoConfig,
    pub user_presets: Vec<UserPreset>,
    pub preset_name: String,
    pub paused: bool,
    pub quit: bool,
    pub view: Option<View>,
    pub stats: Stats,
    pub readout: AutoReadout,
    pub status: Status,
    pub cfg_preview_fps: u32,
    pub cfg_tui_fps: u32,
    pub fps_limit: FpsLimit,
    /// Self-updater state, driven by the background check thread.
    pub update: UpdateState,
    /// Saved "check for updates on start" preference (persisted via Runner).
    pub cfg_update_on_start: bool,
}

pub struct Shared {
    pub inner: Mutex<Inner>,
}

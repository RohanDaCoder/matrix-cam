//! matrix-cam 2.1 — webcam → dot-matrix / ASCII character effects → virtual camera.
//!
//! Module layout:
//! - `cli`      — argument parsing and help text
//! - `config`   — settings model, presets, persistence (`config/settings.rs`, `config/auto.rs`)
//! - `state`    — the shared `Inner`/`Shared` app state exchanged between threads
//! - `render`   — the effect renderer (grid, atlas, tone, fx, glyph sets)
//! - `auto`     — live-feed analysis and the adaptive controller
//! - `pipeline` — camera → render → virtual-camera loop (platform-neutral traits,
//!                Windows camera plumbing in `platform/`)
//! - `ui`       — GUI (egui) and TUI (crossterm) front-ends
//!
//! On non-Windows targets the pipeline/auto/render stack is not reachable from
//! `main` (see `platform` stub), which makes rustc flag it as dead code even
//! though the Windows build uses all of it. Silence that case only.
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

mod auto;
mod cli;
mod config;
mod pipeline;
mod render;
mod state;
mod ui;
mod updater;

#[cfg(windows)]
mod platform;

#[cfg(not(windows))]
mod platform {
    //! Non-Windows stub: matrix-cam drives real cameras only on Windows 11.
    //! `cargo check` still works everywhere for the platform-neutral modules.

    use crate::cli::Args;
    use crate::config::{Config, Settings};
    use anyhow::{bail, Result};

    pub struct Runner;

    impl Runner {
        pub fn run(self) -> Result<()> {
            Ok(())
        }
    }

    pub fn list_devices() {
        println!("cameras: (camera access requires Windows)");
        println!("virtual-camera backends: (requires Windows)");
    }

    pub fn windows_build() -> Option<u32> {
        None
    }

    pub fn start(
        _args: Args,
        _cfg: Config,
        _set: Settings,
        _preset_name: String,
        _threads: usize,
    ) -> Result<(String, Runner)> {
        bail!("matrix-cam targets Windows 11 only — camera input and the virtual camera need Windows.")
    }
}

use anyhow::Result;
use ui::Mode;

fn main() -> Result<()> {
    let args = cli::parse()?;

    if args.list {
        platform::list_devices();
        return Ok(());
    }

    // Self-update handling: headless paths first, then the background check
    // the GUI/TUI show in their panels.
    if args.update {
        updater::cleanup_stale();
        updater::update_now(); // never returns
    }
    if args.check_update {
        std::process::exit(updater::cli_check());
    }

    if let Some(b) = platform::windows_build() {
        if b < 22000 {
            anyhow::bail!(
                "Windows 11 (build 22000+) required for the Media Foundation virtual camera; \
                 detected build {b}. Use --backend obs on older systems."
            );
        }
    }

    let mut cfg = if args.reset { config::Config::default() } else { config::Config::load(args.config.as_deref()) };
    let set = config::settings::Settings::resolve(&mut cfg, &args)?;
    let preset_name = args.preset.clone().unwrap_or_else(|| "ghost".to_string());
    let threads = cli::resolve_threads(&args, &cfg);

    let (cam_name, runner) = platform::start(args, cfg, set, preset_name, threads)?;
    println!("input camera: {cam_name}");
    runner.run()?;
    Ok(())
}

// Keep Mode referenced for non-windows builds where args.mode is unread otherwise.
#[cfg(not(windows))]
#[allow(dead_code)]
fn _mode_used(m: Mode) -> Mode {
    m
}

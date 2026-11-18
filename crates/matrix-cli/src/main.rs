//! matrix-cam (desktop) — webcam → dot-matrix / ASCII character effects →
//! Windows 11 virtual camera.
//!
//! The desktop binary is now a thin host over the shared crates:
//! - `matrix-core`  — engine, renderer, adaptive auto, settings, state
//! - `matrix-gui`  — the egui front-end (also used by the web build)
//! - this crate    — CLI parsing, Windows camera plumbing (`platform`),
//!                   the terminal UI, the self-updater
//!
//! On non-Windows targets the camera stack is not reachable from `main`
//! (see `platform` stub), which makes rustc flag it as dead code even though
//! the Windows build uses all of it. Silence that case only.
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

mod cli;
mod tui;
mod updater;

#[cfg(windows)]
mod platform;

#[cfg(not(windows))]
mod platform {
    //! Non-Windows stub: matrix-cam drives real cameras only on Windows 11.
    //! `cargo check` still works everywhere for the platform-neutral crates.

    use crate::cli::Args;
    use matrix_core::config::{Config, Settings};
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

    /// Mirrors `virtualcam::BackendKind` so the CLI parser compiles on any
    /// platform; only the Windows build constructs real variants.
    #[derive(Clone, Copy)]
    pub enum BackendKind {
        MediaFoundation,
        Obs,
        Auto,
    }
}

use anyhow::Result;
use matrix_gui::gui::{Store, UpdaterHost};

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

    let mut cfg = if args.reset {
        matrix_core::config::Config::default()
    } else {
        matrix_core::config::Config::load(args.config.as_deref())
    };
    let ov = cli::overrides(&args);
    let set = matrix_core::config::Settings::resolve(&mut cfg, &ov)?;
    let preset_name = args.preset.clone().unwrap_or_else(|| "ghost".to_string());
    let threads = cli::resolve_threads(&args, &cfg);

    let (cam_name, runner) = platform::start(args, cfg, set, preset_name, threads)?;
    println!("input camera: {cam_name}");
    runner.run()?;
    Ok(())
}

/// File-backed settings store (`%APPDATA%\matrix-cam\config.json`) handed to
/// the shared GUI.
pub struct FileStore {
    pub path: Option<String>,
}

impl Store for FileStore {
    fn load(&self) -> matrix_core::config::Config {
        matrix_core::config::Config::load(self.path.as_deref())
    }
    fn save(&self, cfg: &matrix_core::config::Config) {
        let _ = cfg.save(self.path.as_deref());
    }
}

/// Real updater host: GitHub release checks + in-place exe swap. Bridges the
/// shared GUI's `UpdaterHost` to this crate's `updater` module.
pub struct DesktopUpdater;

impl UpdaterHost for DesktopUpdater {
    fn build_label(&self) -> String {
        format!("v{}", matrix_core::update::CURRENT)
    }
    fn check_now(&self, shared: std::sync::Arc<matrix_core::state::Shared>) {
        updater::spawn_check_now(shared);
    }
    fn install(&self, shared: std::sync::Arc<matrix_core::state::Shared>) {
        updater::ui_install(shared);
    }
    fn can_install(&self) -> bool {
        true
    }
}

/// Host the shared egui front-end in a native window (the old `ui::gui::run`).
/// On exit the pipeline is asked to quit; `Runner::run` persists the config.
pub fn run_gui(shared: std::sync::Arc<matrix_core::state::Shared>, cfg_path: Option<String>) -> Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1220.0, 780.0])
            .with_min_inner_size([940.0, 560.0])
            .with_title("matrix-cam"),
        ..Default::default()
    };
    let s2 = shared.clone();
    eframe::run_native(
        "matrix-cam",
        opts,
        Box::new(move |cc| {
            matrix_gui::gui::GuiApp::style_ctx(&cc.egui_ctx);
            Ok(Box::new(matrix_gui::gui::GuiApp::new(
                s2.clone(),
                std::sync::Arc::new(FileStore { path: cfg_path.clone() }),
                std::sync::Arc::new(DesktopUpdater),
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))?;
    shared.inner.lock().unwrap().quit = true;
    Ok(())
}

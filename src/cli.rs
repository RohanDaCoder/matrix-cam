//! Command-line interface: argument parsing, help text, small resolution helpers.

use crate::config::Config;
use crate::ui::Mode;
use anyhow::{Context, Result};

#[cfg(windows)]
use crate::platform::BackendKind;

pub struct Args {
    pub mode: Mode,
    pub camera: u32,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    #[cfg(windows)]
    pub backend: BackendKind,
    pub preset: Option<String>,
    pub cols: Option<u32>,
    pub mirror: Option<bool>,
    pub threads: Option<usize>,
    pub auto: Option<bool>,
    pub config: Option<String>,
    pub reset: bool,
    pub list: bool,
    /// Self-updater: install newest release and restart.
    pub update: bool,
    /// Self-updater: print whether an update exists, exit.
    pub check_update: bool,
    /// Self-updater: skip the on-start background update check.
    pub no_update_check: bool,
    /// 8-bit retro mode: apply the retro look + enable the FPS limiter.
    pub retro: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            mode: Mode::Gui,
            camera: 0,
            width: 1280,
            height: 720,
            fps: 30.0,
            #[cfg(windows)]
            backend: BackendKind::Auto,
            preset: None,
            cols: None,
            mirror: None,
            threads: None,
            auto: None,
            config: None,
            reset: false,
            list: false,
            update: false,
            check_update: false,
            no_update_check: false,
            retro: false,
        }
    }
}

fn usage() {
    println!(
        "matrix-cam 2.1 — webcam → character effects → Windows 11 virtual camera

USAGE  matrix-cam [OPTIONS]

  --gui / --tui / --headless   front-end (default: --gui)
  --camera N        physical camera index (see --list)
  --width W  --height H  --fps F   output geometry (1280x720@30)
  --backend mf|obs|auto          virtual-camera transport
  --preset NAME     ghost | paper | term | vhs | magma | <user preset>
  --cols N          detail (column count) override
  --mirror / --no-mirror
  --threads N       render workers (default: CPU cores, max 8)
  --auto / --no-auto           start with auto adjust on/off
  --config PATH     config file (default %APPDATA%\\matrix-cam\\config.json)
  --reset           ignore saved config
  --list            list cameras + backends and exit
  --retro           8-bit retro mode: chunky look + FPS limiter (12 fps)
  --check-update    show the newest GitHub release and exit
  --update          install the newest release and restart
  --no-update-check skip the on-start update check

GUI  preview + full settings + auto panel + telemetry
TUI  ↑↓ select · ←→ adjust · 1-5 presets · a auto · Tab panel · w save · q quit"
    );
}

pub fn parse() -> Result<Args> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(f) = it.next() {
        let mut val = || -> Result<String> {
            it.next().with_context(|| format!("missing value for {f}"))
        };
        match f.as_str() {
            "--gui" => a.mode = Mode::Gui,
            "--tui" => a.mode = Mode::Tui,
            "--headless" => a.mode = Mode::Headless,
            "--camera" => a.camera = val()?.parse()?,
            "--width" => a.width = val()?.parse()?,
            "--height" => a.height = val()?.parse()?,
            "--fps" => a.fps = val()?.parse()?,
            "--cols" => a.cols = Some(val()?.parse()?),
            "--threads" => a.threads = Some(val()?.parse()?),
            "--preset" => a.preset = Some(val()?),
            "--config" => a.config = Some(val()?),
            "--backend" => {
                #[cfg(windows)]
                {
                    a.backend = match val()?.to_lowercase().as_str() {
                        "mf" | "mediafoundation" => BackendKind::MediaFoundation,
                        "obs" => BackendKind::Obs,
                        "auto" => BackendKind::Auto,
                        o => anyhow::bail!("unknown backend '{o}'"),
                    };
                }
                #[cfg(not(windows))]
                {
                    let _ = val()?;
                }
            }
            "--mirror" => a.mirror = Some(true),
            "--no-mirror" => a.mirror = Some(false),
            "--auto" => a.auto = Some(true),
            "--no-auto" => a.auto = Some(false),
            "--reset" => a.reset = true,
            "--list" => a.list = true,
            "--retro" => a.retro = true,
            "--update" => a.update = true,
            "--check-update" => a.check_update = true,
            "--no-update-check" => a.no_update_check = true,
            "-h" | "--help" => {
                usage();
                std::process::exit(0);
            }
            o => anyhow::bail!("unknown option '{o}' (try --help)"),
        }
    }
    a.width = a.width.clamp(160, 3840) & !1;
    a.height = a.height.clamp(120, 2160) & !1;
    a.fps = a.fps.clamp(5.0, 60.0);
    Ok(a)
}

/// Effective worker count: CLI flag > saved config > CPU cores, clamped to 1..=8.
pub fn resolve_threads(args: &Args, cfg: &Config) -> usize {
    args.threads
        .or(if cfg.threads > 0 { Some(cfg.threads as usize) } else { None })
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4))
        .clamp(1, 8)
}

#![cfg_attr(not(windows), allow(dead_code))]

mod auto;
mod config;
mod effect;
mod font;
mod gui;
mod tui;

use anyhow::{Context, Result};
use config::{AutoConfig, Config, Settings, UserPreset, PRESETS};
use effect::{Grid, Renderer};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution};
use nokhwa::Camera;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use virtualcam::{BackendKind, Camera as VCam, PixelFormat};

#[cfg(not(windows))]
compile_error!("matrix-cam targets Windows 11 only.");

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub backend: String, pub cam: String,
    pub out_w: u32, pub out_h: u32, pub out_fps: u32,
    pub fps: f32, pub threads: usize, pub error: String,
}

#[derive(Clone)]
pub struct View {
    pub frame: Arc<Vec<u8>>,
    pub grid: Arc<Grid>,
    pub w: u32, pub h: u32, pub serial: u64,
}

pub struct Inner {
    pub set: Settings,          // user's base values
    pub eff: Settings,          // live values after auto
    pub auto: AutoConfig,
    pub user_presets: Vec<UserPreset>,
    pub preset_name: String,
    pub paused: bool,
    pub quit: bool,
    pub view: Option<View>,
    pub stats: auto::Stats,
    pub readout: auto::AutoReadout,
    pub status: Status,
    pub cfg_preview_fps: u32,
    pub cfg_tui_fps: u32,
}

pub struct Shared { pub inner: Mutex<Inner> }

struct Args {
    mode: Mode, camera: u32, width: u32, height: u32, fps: f64,
    backend: BackendKind, preset: Option<String>, cols: Option<u32>,
    mirror: Option<bool>, threads: Option<usize>, auto: Option<bool>,
    config: Option<String>, reset: bool, list: bool,
}
#[derive(PartialEq, Clone, Copy)]
enum Mode { Gui, Tui, Headless }

fn usage() {
    println!(
"matrix-cam 2.0 — webcam → ASCII/dot-matrix effect → Windows 11 virtual camera

USAGE  matrix-cam [OPTIONS]

  --gui / --tui / --headless   front-end (default: --gui)
  --camera N        physical camera index (see --list)
  --width W  --height H  --fps F      output geometry (1280x720@30)
  --backend mf|obs|auto               virtual-camera transport
  --preset NAME     ghost | paper | term | vhs | magma | <user preset>
  --cols N          density override
  --mirror / --no-mirror
  --threads N       render workers (default: CPU cores, max 8)
  --auto / --no-auto                  start with adaptive mode on/off
  --config PATH     config file (default %APPDATA%\\matrix-cam\\config.json)
  --reset           ignore saved config
  --list            list cameras + backends and exit

GUI  preview + full settings + auto panel + telemetry
TUI  ↑↓ select · ←→ adjust · 1-5 presets · a auto · Tab panel · w save · q quit");
}

fn parse() -> Result<Args> {
    let mut a = Args {
        mode: Mode::Gui, camera: 0, width: 1280, height: 720, fps: 30.0,
        backend: BackendKind::Auto, preset: None, cols: None, mirror: None,
        threads: None, auto: None, config: None, reset: false, list: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(f) = it.next() {
        let mut val = || -> Result<String> { it.next().with_context(|| format!("missing value for {f}")) };
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
            "--backend" => a.backend = match val()?.to_lowercase().as_str() {
                "mf" | "mediafoundation" => BackendKind::MediaFoundation,
                "obs" => BackendKind::Obs,
                "auto" => BackendKind::Auto,
                o => anyhow::bail!("unknown backend '{o}'"),
            },
            "--mirror" => a.mirror = Some(true),
            "--no-mirror" => a.mirror = Some(false),
            "--auto" => a.auto = Some(true),
            "--no-auto" => a.auto = Some(false),
            "--reset" => a.reset = true,
            "--list" => a.list = true,
            "-h" | "--help" => { usage(); std::process::exit(0); }
            o => anyhow::bail!("unknown option '{o}' (try --help)"),
        }
    }
    a.width = a.width.clamp(160, 3840) & !1;
    a.height = a.height.clamp(120, 2160) & !1;
    a.fps = a.fps.clamp(5.0, 60.0);
    Ok(a)
}

fn windows_build() -> Option<u32> {
    let out = std::process::Command::new("cmd").args(["/C", "ver"]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    let mut parts = s.split(|c: char| !(c.is_ascii_digit() || c == '.'));
    while let Some(p) = parts.next() {
        let seg: Vec<&str> = p.split('.').collect();
        if seg.len() >= 3 && seg[0] == "10" {
            if let Ok(b) = seg[2].parse::<u32>() { return Some(b); }
        }
    }
    None
}

fn open_camera(index: u32) -> Result<Camera> {
    let closest = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(
        CameraFormat::new(Resolution::new(1280, 720), FrameFormat::MJPEG, 30)));
    let mut cam = Camera::new(CameraIndex::Index(index), closest)
        .or_else(|_| Camera::new(CameraIndex::Index(index),
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate)))
        .with_context(|| format!("cannot open camera {index} — try --list"))?;
    cam.open_stream().context("camera stream failed (another app holding it?)")?;
    Ok(cam)
}

fn pipeline(
    shared: Arc<Shared>, mut cam: Camera, mut vcam: VCam,
    w: u32, h: u32, threads: usize, want_grid: bool,
) {
    let mut rend = Renderer::new(w, h, Settings::preset("ghost"), threads);
    rend.want_grid = want_grid;
    let mut analyzer = auto::Analyzer::new();
    let mut controller = auto::Auto::new();
    let mut stats = auto::Stats::default();
    let mut frames = 0u64;
    let mut fps_t = Instant::now();
    let mut err_streak = 0u32;

    loop {
        let (base, acfg, paused, quit) = {
            let g = shared.inner.lock().unwrap();
            (g.set.clone(), g.auto.clone(), g.paused, g.quit)
        };
        if quit { break; }

        let (eff, readout) = controller.apply(&base, &acfg, &stats, &analyzer, Instant::now());
        rend.apply(&eff);

        let frame = if paused {
            rend.frame_arc()
        } else {
            match cam.frame().ok().and_then(|b| b.decode_image::<RgbFormat>().ok()) {
                Some(img) => {
                    err_streak = 0;
                    let arc = rend.render(img.as_raw(), img.width() as usize, img.height() as usize);
                    stats = analyzer.analyze(&rend.grid());
                    Some(arc)
                }
                None => { err_streak += 1; rend.frame_arc() }
            }
        };

        if let Some(f) = frame {
            let serial = rend.serial;
            let grid = rend.grid();
            {
                let mut g = shared.inner.lock().unwrap();
                g.eff = eff.clone();
                g.readout = readout.clone();
                g.stats = stats.clone();
                g.view = Some(View { frame: f.clone(), grid, w, h, serial });
                g.status.error = if err_streak > 30 { "camera not delivering frames".into() } else { String::new() };
            }
            if vcam.send(&f).is_err() {
                shared.inner.lock().unwrap().status.error = "virtual camera send failed".into();
                break;
            }
        }
        vcam.sleep_until_next_frame();

        frames += 1;
        if fps_t.elapsed() >= Duration::from_secs(1) {
            let fps = frames as f32 / fps_t.elapsed().as_secs_f32();
            shared.inner.lock().unwrap().status.fps = fps;
            frames = 0; fps_t = Instant::now();
        }
        if err_streak > 300 {
            shared.inner.lock().unwrap().status.error = "camera lost".into();
            break;
        }
    }
    let _ = cam.stop_stream();
    let _ = vcam.close();
}

fn main() -> Result<()> {
    let args = parse()?;

    if args.list {
        println!("cameras:");
        match nokhwa::query(nokhwa::native_api_backend().unwrap_or(nokhwa::utils::ApiBackend::MediaFoundation)) {
            Ok(l) => for c in l { println!("  [{}] {}", c.index(), c.human_name()); },
            Err(e) => println!("  query failed: {e}"),
        }
        println!("virtual-camera backends:");
        for b in virtualcam::available_backends() { println!("  {b:?}"); }
        return Ok(());
    }

    if let Some(b) = windows_build() {
        if b < 22000 {
            anyhow::bail!("Windows 11 (build 22000+) required for the Media Foundation virtual camera; detected build {b}. Use --backend obs on older systems.");
        }
    }

    let mut cfg = if args.reset { Config::default() } else { Config::load(args.config.as_deref()) };
    let mut set = cfg.set.clone().unwrap_or_else(|| Settings::preset("ghost"));
    let mut preset_name = "ghost".to_string();
    if let Some(p) = &args.preset {
        if let Some(up) = cfg.user_presets.iter().find(|u| &u.name == p) {
            set = up.set.clone(); preset_name = p.clone();
        } else if PRESETS.contains(&p.as_str()) {
            set = Settings::preset(p); preset_name = p.clone();
        } else {
            anyhow::bail!("unknown preset '{p}'");
        }
    }
    if let Some(c) = args.cols { set.cols = c.clamp(40, 220); }
    if let Some(m) = args.mirror { set.mirror = m; }
    if let Some(a) = args.auto { cfg.auto.enabled = a; }

    let threads = args.threads
        .or(if cfg.threads > 0 { Some(cfg.threads as usize) } else { None })
        .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4))
        .clamp(1, 8);

    let cam = open_camera(args.camera)?;
    let res = cam.resolution();
    let cam_name = nokhwa::query(nokhwa::native_api_backend().unwrap_or(nokhwa::utils::ApiBackend::MediaFoundation)).ok()
        .and_then(|l| l.into_iter().find(|c| c.index() == &CameraIndex::Index(args.camera)))
        .map(|c| c.human_name().to_string())
        .unwrap_or_else(|| format!("camera {}", args.camera));
    let _ = res;

    let vcam = VCam::builder(args.width, args.height, args.fps)
        .format(PixelFormat::RGB)
        .backend(args.backend)
        .build()
        .context("virtual camera failed to start — run install.bat once from the virtualcam repo (README), or use --backend obs")?;
    let backend = vcam.backend().to_string();

    let shared = Arc::new(Shared {
        inner: Mutex::new(Inner {
            eff: set.clone(), set, auto: cfg.auto.clone(),
            user_presets: cfg.user_presets.clone(), preset_name,
            paused: false, quit: false, view: None,
            stats: auto::Stats::default(), readout: auto::AutoReadout::default(),
            status: Status { backend, cam: cam_name, out_w: args.width, out_h: args.height,
                             out_fps: args.fps as u32, fps: 0.0, threads, error: String::new() },
            cfg_preview_fps: cfg.preview_fps.max(5),
            cfg_tui_fps: cfg.tui_fps.max(5),
        }),
    });

    let want_grid = args.mode == Mode::Tui;
    let ps = shared.clone();
    let (w, h) = (args.width, args.height);
    let worker = std::thread::Builder::new().name("pipeline".into())
        .spawn(move || pipeline(ps, cam, vcam, w, h, threads, want_grid))?;

    match args.mode {
        Mode::Gui => gui::run(shared.clone(), args.config.clone())?,
        Mode::Tui => tui::run(shared.clone(), args.config.clone())?,
        Mode::Headless => {
            println!("headless — press q to quit");
            crossterm::terminal::enable_raw_mode()?;
            loop {
                if crossterm::event::poll(Duration::from_millis(200))? {
                    if let crossterm::event::Event::Key(k) = crossterm::event::read()? {
                        if matches!(k.code, crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc) { break; }
                    }
                }
                if shared.inner.lock().unwrap().quit { break; }
            }
            crossterm::terminal::disable_raw_mode()?;
            shared.inner.lock().unwrap().quit = true;
        }
    }

    shared.inner.lock().unwrap().quit = true;
    let _ = worker.join();

    {
        let g = shared.inner.lock().unwrap();
        cfg.set = Some(g.set.clone());
        cfg.auto = g.auto.clone();
        cfg.user_presets = g.user_presets.clone();
        cfg.threads = threads as u32;
    }
    cfg.save(args.config.as_deref())?;
    println!("saved config → {}", Config::path(args.config.as_deref()).display());
    Ok(())
}

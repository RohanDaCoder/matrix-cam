//! Windows-only plumbing: physical camera in, virtual camera out, device listing.
//! Everything else in the crate is platform neutral.

use crate::cli::Args;
use crate::config::Config;
use crate::config::settings::Settings;
use crate::pipeline::{self, FrameSink, FrameSource};
use crate::state::{FpsLimit, Inner, Shared, Status};
use crate::ui::{self, Mode};
use anyhow::{Context, Result};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution};
use nokhwa::Camera;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use virtualcam::{Camera as VCam, PixelFormat};

/// Virtual-camera transport selection (public for the CLI parser).
pub use virtualcam::BackendKind;

/// What `main` gets back: the camera name plus a callable that runs the UI loop
/// and finishes with config persistence.
pub struct Runner {
    shared: Arc<Shared>,
    cfg: Config,
    cfg_path: Option<String>,
    mode: Mode,
    threads: usize,
}

impl Runner {
    pub fn run(self) -> Result<()> {
        let Runner { shared, cfg, cfg_path, mode, threads } = self;
        match mode {
            Mode::Gui => crate::ui::gui::run(shared.clone(), cfg_path.clone())?,
            Mode::Tui => crate::ui::tui::run(shared.clone(), cfg_path.clone())?,
            Mode::Headless => {
                println!("headless — press q to quit");
                crossterm::terminal::enable_raw_mode()?;
                loop {
                    if crossterm::event::poll(Duration::from_millis(200))? {
                        if let crossterm::event::Event::Key(k) = crossterm::event::read()? {
                            if matches!(k.code, crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc) {
                                break;
                            }
                        }
                    }
                    if shared.inner.lock().unwrap().quit {
                        break;
                    }
                }
                crossterm::terminal::disable_raw_mode()?;
                shared.inner.lock().unwrap().quit = true;
            }
        }

        shared.inner.lock().unwrap().quit = true;

        let mut cfg = cfg;
        {
            let g = shared.inner.lock().unwrap();
            cfg.set = Some(g.set.clone());
            cfg.auto = g.auto.clone();
            cfg.user_presets = g.user_presets.clone();
            cfg.threads = threads as u32;
            cfg.fps_limit_enabled = g.fps_limit.enabled;
            cfg.fps_limit_fps = g.fps_limit.fps;
        }
        cfg.save(cfg_path.as_deref())?;
        println!("saved config → {}", Config::path(cfg_path.as_deref()).display());
        Ok(())
    }
}

pub fn list_devices() {
    println!("cameras:");
    match nokhwa::query(nokhwa::native_api_backend().unwrap_or(nokhwa::utils::ApiBackend::MediaFoundation)) {
        Ok(l) => {
            for c in l {
                println!("  [{}] {}", c.index(), c.human_name());
            }
        }
        Err(e) => println!("  query failed: {e}"),
    }
    println!("virtual-camera backends:");
    for b in virtualcam::available_backends() {
        println!("  {b:?}");
    }
}

pub fn windows_build() -> Option<u32> {
    let out = std::process::Command::new("cmd").args(["/C", "ver"]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    let mut parts = s.split(|c: char| !(c.is_ascii_digit() || c == '.'));
    while let Some(p) = parts.next() {
        let seg: Vec<&str> = p.split('.').collect();
        if seg.len() >= 3 && seg[0] == "10" {
            if let Ok(b) = seg[2].parse::<u32>() {
                return Some(b);
            }
        }
    }
    None
}

fn open_camera(index: u32) -> Result<Camera> {
    let closest = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(CameraFormat::new(
        Resolution::new(1280, 720),
        FrameFormat::MJPEG,
        30,
    )));
    let mut cam = Camera::new(CameraIndex::Index(index), closest).or_else(|_| {
        Camera::new(
            CameraIndex::Index(index),
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate),
        )
    }).with_context(|| format!("cannot open camera {index} — try --list"))?;
    cam.open_stream()
        .context("camera stream failed (another app holding it?)")?;
    Ok(cam)
}

/// nokhwa camera adapter for the pipeline.
struct CamSource {
    cam: Camera,
    buf: Vec<u8>,
}

impl FrameSource for CamSource {
    fn next_frame(&mut self) -> Option<(Vec<u8>, usize, usize)> {
        let b = self.cam.frame().ok()?;
        let img = b.decode_image::<RgbFormat>().ok()?;
        let (w, h) = (img.width() as usize, img.height() as usize);
        self.buf.clear();
        self.buf.extend_from_slice(img.as_raw());
        Some((std::mem::take(&mut self.buf), w, h))
    }
}

/// virtualcam adapter for the pipeline.
struct VCamSink {
    vcam: VCam,
}

impl FrameSink for VCamSink {
    fn send(&mut self, frame: &[u8]) -> bool {
        self.vcam.send(frame).is_ok()
    }
    fn wait(&mut self) {
        self.vcam.sleep_until_next_frame();
    }
}

/// Open the camera + virtual camera, spawn the pipeline, and hand back a `Runner`.
pub fn start(
    args: Args,
    mut cfg: Config,
    set: Settings,
    preset_name: String,
    threads: usize,
) -> Result<(String, Runner)> {
    let cam = open_camera(args.camera)?;
    let cam_name = nokhwa::query(nokhwa::native_api_backend().unwrap_or(nokhwa::utils::ApiBackend::MediaFoundation))
        .ok()
        .and_then(|l| l.into_iter().find(|c| c.index() == &CameraIndex::Index(args.camera)))
        .map(|c| c.human_name().to_string())
        .unwrap_or_else(|| format!("camera {}", args.camera));

    let vcam = VCam::builder(args.width, args.height, args.fps)
        .format(PixelFormat::RGB)
        .backend(args.backend)
        .build()
        .context("virtual camera failed to start — run install.bat once from the virtualcam repo (README), or use --backend obs")?;
    let backend = vcam.backend().to_string();

    let fps_limit = FpsLimit { enabled: cfg.fps_limit_enabled, fps: cfg.fps_limit_fps.max(1) };
    let shared = Arc::new(Shared {
        inner: Mutex::new(Inner {
            eff: set.clone(),
            set,
            auto: cfg.auto.clone(),
            user_presets: cfg.user_presets.clone(),
            preset_name,
            paused: false,
            quit: false,
            view: None,
            stats: Default::default(),
            readout: Default::default(),
            status: Status {
                backend,
                cam: cam_name.clone(),
                out_w: args.width,
                out_h: args.height,
                out_fps: args.fps as u32,
                fps: 0.0,
                threads,
                error: String::new(),
            },
            cfg_preview_fps: cfg.preview_fps.max(5),
            cfg_tui_fps: cfg.tui_fps.max(5),
            fps_limit,
        }),
    });

    let want_grid = args.mode == Mode::Tui;
    pipeline::spawn(
        shared.clone(),
        Box::new(CamSource { cam, buf: Vec::new() }),
        Box::new(VCamSink { vcam }),
        args.width,
        args.height,
        threads,
        want_grid,
    );

    let runner = Runner { shared, cfg, cfg_path: args.config.clone(), mode: args.mode, threads };
    Ok((cam_name, runner))
}

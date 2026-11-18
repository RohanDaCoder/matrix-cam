#![cfg(target_arch = "wasm32")]
//! matrix-cam web — the same egui front-end and effect engine, compiled to
//! wasm and driving the browser's camera.
//!
//! Differences from the desktop host, all isolated here:
//! - camera: `navigator.mediaDevices.getUserMedia` + a hidden `<video>` +
//!   a 2D canvas that yields RGB frames to `FrameSource`
//! - persistence: `localStorage["matrix-cam-config"]` instead of
//!   `%APPDATA%\matrix-cam\config.json`
//! - updater: no-op host — the site is re-deployed on release, so the web
//!   build always runs the newest version
//! - engine: `Engine::tick` runs inline once per repaint (wasm has no
//!   std threads), which is exactly what the browser event model wants
//!
//! Everything else — the settings panel, presets, auto adjust, retro look,
//! retro FPS limiter, telemetry — is the shared `matrix-gui` + `matrix-core`
//! code the Windows build runs.

use matrix_core::config::{Config, Settings};
use matrix_core::engine::Engine;
use matrix_core::pipeline::FrameSource;
use matrix_core::state::{FpsLimit, Inner, Shared, Status};
use matrix_core::time::Instant;
use matrix_gui::gui::{GuiApp, Store, UpdaterHost};
use std::sync::{Arc, Mutex};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const OUT_W: u32 = 1280;
const OUT_H: u32 = 720;

fn main() {
    // wasm logging goes to the browser console
    console_error_panic_hook::set_once();
    let _ = log::set_logger(&WasmLogger);
    log::set_max_level(log::LevelFilter::Warn);

    // state: same `Shared`/`Inner` model as the desktop (built once, shared
    // with WebApp through the OnceLock)
    let shared = SHARED.get_or_init(build_shared).clone();

    // the camera needs a user gesture in most browsers; the start button
    // lives in the egui UI, so start paused with the camera pending
    shared.inner.lock().unwrap().paused = true;

    let web = eframe::WebOptions::default();
    let document = web_sys::window().unwrap().document().unwrap();
    let canvas = document
        .get_element_by_id("matrix-cam")
        .expect("index.html must contain <canvas id=\"matrix-cam\">")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("element #matrix-cam must be a canvas");
    wasm_bindgen_futures::spawn_local(async move {
        eframe::WebRunner::new()
            .start(canvas, web, Box::new(|cc| {
                GuiApp::style_ctx(&cc.egui_ctx);
                Ok(Box::new(WebApp::new()))
            }))
            .await
            .expect("failed to start eframe");
    });
}

/// The web host: shared GUI + per-repaint engine tick + lazy camera start.
struct WebApp {
    gui: GuiApp,
    engine: Engine,
    camera: WebCamera,
    started: bool,
}

impl WebApp {
    fn new() -> Self {
        let store = Arc::new(LocalStore);
        let host = Arc::new(WebUpdater);
        // the shared state was created in main(); the GUI's Arc is passed
        // through a thread-local handoff — simplest correct route here is a
        // second construction guarded by a global OnceLock.
        let shared = SHARED.get_or_init(|| build_shared()).clone();
        let gui = GuiApp::new(shared.clone(), store, host);
        // wasm renders on a single thread; want_grid is TUI-only
        let engine = Engine::new(OUT_W, OUT_H, 1, false);
        let camera = WebCamera::new();
        // camera start needs the first user gesture: hook the pause button
        WebApp { gui, engine, camera, started: false }
    }
}

impl eframe::App for WebApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // re-assert the dark theme every frame (egui's web runner can load
        // its own default visuals after the app-creator closure ran)
        GuiApp::style_ctx(ctx);
        // first frame after a gesture: kick off getUserMedia once
        if !self.started && ctx.input(|i| i.raw.events.iter().any(|e| matches!(e, egui::Event::PointerButton { .. } | egui::Event::Key { .. }))) {
            self.started = true;
            self.camera.request();
        }
        // engine tick: the web equivalent of the desktop pipeline loop
        if self.started {
            let shared = self.gui.shared.clone();
            let cam = &mut self.camera;
            // retro FPS limiter: skip ticks that come too early
            self.engine.tick(&shared, cam, Instant::now());
        }
        // delegate the whole UI to the shared front-end
        self.gui.update(ctx, _frame);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.camera.stop();
    }
}

// ---------------------------------------------------------------------------
// camera
// ---------------------------------------------------------------------------

/// Browser camera: getUserMedia → hidden <video> → 2D canvas → RGB buffer.
struct WebCamera {
    video: Option<web_sys::HtmlVideoElement>,
    ctx: Option<web_sys::CanvasRenderingContext2d>,
    buf: Vec<u8>,
    pending: bool,
}

impl WebCamera {
    fn new() -> Self {
        WebCamera { video: None, ctx: None, buf: Vec::new(), pending: false }
    }

    /// Fire the async getUserMedia request (idempotent-ish: a second call
    /// while pending is ignored; a failure surfaces in `Inner::status.error`).
    fn request(&mut self) {
        if self.pending || self.video.is_some() {
            return;
        }
        self.pending = true;
        let video = create_hidden_video();
        let v = video.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match get_user_media().await {
                Ok(stream) => {
                    v.set_src_object(Some(&stream));
                    // autoplay muted (user already gestured)
                    v.set_autoplay(true);
                    v.set_muted(true);
                    let _ = v.play();
                }
                Err(e) => {
                    log::warn!("getUserMedia failed: {:?}", e);
                    set_camera_error("camera permission denied (allow camera access and reload)");
                }
            }
            CAMERA_PENDING.store(false, std::sync::atomic::Ordering::Relaxed);
        });
        self.video = Some(video);
        let _ = self.ctx; // canvas created lazily in next_frame
    }

    fn stop(&mut self) {
        if let Some(v) = self.video.take() {
            v.set_src_object(None);
        }
    }
}

static CAMERA_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SHARED: std::sync::OnceLock<Arc<Shared>> = std::sync::OnceLock::new();

fn build_shared() -> Arc<Shared> {
    // executed only if main()'s Arc was not reachable; same model as main()
    let cfg = LocalStore.load();
    let set = cfg.set.clone().unwrap_or_else(|| Settings::preset("ghost"));
    let fps_limit = FpsLimit { enabled: cfg.fps_limit_enabled, fps: cfg.fps_limit_fps.max(1) };
    Arc::new(Shared {
        inner: Mutex::new(Inner {
            eff: set.clone(),
            set,
            auto: cfg.auto.clone(),
            user_presets: cfg.user_presets.clone(),
            preset_name: "ghost".into(),
            paused: false,
            quit: false,
            view: None,
            stats: Default::default(),
            readout: Default::default(),
            status: Status {
                backend: "web".into(),
                cam: "browser camera".into(),
                out_w: OUT_W,
                out_h: OUT_H,
                out_fps: 30,
                fps: 0.0,
                threads: 1,
                error: String::new(),
            },
            cfg_preview_fps: cfg.preview_fps.max(5),
            cfg_tui_fps: cfg.tui_fps.max(5),
            fps_limit,
            update: matrix_core::update::UpdateState::NotChecked,
            cfg_update_on_start: false,
        }),
    })
}

fn set_camera_error(msg: &str) {
    if let Some(sh) = SHARED.get() {
        sh.inner.lock().unwrap().status.error = msg.to_string();
    }
}

fn create_hidden_video() -> web_sys::HtmlVideoElement {
    let doc = web_sys::window().unwrap().document().unwrap();
    let v = doc.create_element("video").unwrap().dyn_into::<web_sys::HtmlVideoElement>().unwrap();
    v.set_attribute("style", "display:none").ok();
    v.set_attribute("playsinline", "true").ok();
    v.set_attribute("autoplay", "true").ok();
    v.set_attribute("muted", "true").ok();
    doc.body().unwrap().append_child(&v).ok();
    v
}

async fn get_user_media() -> Result<web_sys::MediaStream, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let nav = window.navigator();
    let md = nav.media_devices().map_err(|_| JsValue::from_str("no media devices (needs https or localhost)"))?;
    let cons = web_sys::MediaStreamConstraints::new();
    cons.set_video(&JsValue::from_bool(true));
    cons.set_audio(&JsValue::from_bool(false));
    let promise = md.get_user_media_with_constraints(&cons)?;
    let stream = wasm_bindgen_futures::JsFuture::from(promise).await?;
    let stream = stream.dyn_into::<web_sys::MediaStream>()?;
    Ok(stream)
}

impl FrameSource for WebCamera {
    fn next_frame(&mut self) -> Option<(Vec<u8>, usize, usize)> {
        let v = self.video.as_ref()?;
        if v.ready_state() < 2 || v.video_width() == 0 {
            return None;
        }
        let (w, h) = (v.video_width(), v.video_height());
        // canvas lazily sized to the camera resolution
        let ctx = {
            if self.ctx.is_none() {
                let doc = web_sys::window().unwrap().document().unwrap();
                let c = doc.create_element("canvas").unwrap().dyn_into::<web_sys::HtmlCanvasElement>().unwrap();
                c.set_width(w);
                c.set_height(h);
                let ctx = c.get_context("2d").unwrap().unwrap().dyn_into::<web_sys::CanvasRenderingContext2d>().unwrap();
                self.ctx = Some(ctx);
            }
            self.ctx.as_ref().unwrap()
        };
        let _canvas = ctx.canvas();
        ctx.draw_image_with_html_video_element(&v, 0.0, 0.0).ok()?;
        // getImageData → RGBA; engine wants RGB
        let img = ctx.get_image_data(0.0, 0.0, w as f64, h as f64).ok()?;
        let data = img.data();
        let n = (w as usize) * (h as usize);
        self.buf.clear();
        let raw = data.to_vec();
        self.buf.reserve(n * 3);
        for px in raw.chunks_exact(4) {
            self.buf.push(px[0]);
            self.buf.push(px[1]);
            self.buf.push(px[2]);
        }
        Some((std::mem::take(&mut self.buf), w as usize, h as usize))
    }
}

// ---------------------------------------------------------------------------
// persistence + updater host
// ---------------------------------------------------------------------------

/// localStorage-backed settings store, JSON-compatible with the desktop
/// config file (same serde model both sides).
struct LocalStore;

impl Store for LocalStore {
    fn load(&self) -> Config {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|ls| ls.get_item("matrix-cam-config").ok().flatten())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, cfg: &Config) {
        let ok = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|ls| serde_json::to_string(cfg).ok().and_then(|s| ls.set_item("matrix-cam-config", &s).ok()));
        if ok.is_none() {
            log::warn!("localStorage unavailable — settings not saved");
        }
    }
}

/// Web updater host: no buttons (the site re-deploys on release).
struct WebUpdater;

impl UpdaterHost for WebUpdater {
    fn build_label(&self) -> String {
        format!("web v{}", matrix_core::update::CURRENT)
    }
    fn check_now(&self, _shared: Arc<Shared>) {}
    fn install(&self, _shared: Arc<Shared>) {}
    fn can_install(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// tiny console logger
// ---------------------------------------------------------------------------

struct WasmLogger;

impl log::Log for WasmLogger {
    fn enabled(&self, md: &log::Metadata) -> bool {
        md.level() <= log::Level::Warn
    }
    fn log(&self, rec: &log::Record) {
        if self.enabled(rec.metadata()) {
            web_sys::console::warn_1(&format!("{}: {}", rec.level(), rec.args()).into());
        }
    }
    fn flush(&self) {}
}

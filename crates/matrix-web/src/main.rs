#![cfg(target_arch = "wasm32")]
//! matrix-cam web — the same egui front-end and effect engine, compiled to
//! wasm and driving the browser's camera.
//!
//! Differences from the desktop host, all isolated here:
//! - camera: `navigator.mediaDevices.getUserMedia` + an off-screen
//!   `<video>` + a 2D canvas that yields RGB frames to `FrameSource`
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
//!
//! Mobile notes (the black-preview class of bugs on Android):
//! - the `<video>` must stay *attached and visible*, just 1px off the
//!   corner: `display:none` (and most `visibility` tricks) make Android
//!   Chrome stop decoding frames, so `videoWidth` never becomes nonzero;
//! - autoplay can be rejected on Android even after the permission grant,
//!   so `play()` is retried and also re-driven when a frame is expected
//!   but the element is still paused;
//! - `paused` starts `true` (the camera needs a gesture) and is cleared
//!   automatically on the FIRST real camera frame, so the user never has
//!   to find the resume button after accepting the permission dialog;
//! - if the first camera fails, one retry with the other facing runs
//!   automatically before the user sees an error.

use matrix_core::config::{Config, Settings};
use matrix_core::engine::Engine;
use matrix_core::pipeline::FrameSource;
use matrix_core::report;
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

    // crash capture: the default panic hook prints to console only — chain a
    // hook that ALSO records the panic (message + backtrace) so the GUI can
    // show its "something went wrong" popup with everything needed to fix
    // the bug, instead of a silently frozen canvas.
    install_panic_capture();
    // JS-side errors (exceptions in event handlers, promise rejections) come
    // through window.onerror / onunhandledrejection; the wasm module cannot
    // see them otherwise.
    install_js_error_capture();
    // report context: everything a fix needs to know about the environment
    report::set_context(|c| {
        c.host = "web".into();
        c.version = format!("v{}", matrix_core::update::CURRENT);
        c.platform = platform_line();
        let _ = c;
    });
    report::log_line("boot: wasm module starting");

    // state: same `Shared`/`Inner` model as the desktop (built once, shared
    // with WebApp through the OnceLock)
    let shared = SHARED.get_or_init(build_shared).clone();

    // the camera needs a user gesture in most browsers; the app un-pauses
    // itself on the first delivered camera frame (see Engine::tick callers)
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
        let shared = SHARED.get_or_init(build_shared).clone();
        let gui = GuiApp::new(shared.clone(), store, host);
        // wasm renders on a single thread; want_grid is TUI-only
        let engine = Engine::new(OUT_W, OUT_H, 1, false);
        let camera = WebCamera::new();
        // camera start needs the first user gesture: hook the pause button
        WebApp { gui, engine, camera, started: false }
    }

    /// The gesture that lets us call getUserMedia: egui translates touch into
    /// `PointerButton` on desktop AND mobile, but keep `Touch`/`Text` matches
    /// too — some Android WebViews deliver raw touches before the pointer
    /// abstraction kicks in.
    fn gesture_seen(&self, ctx: &egui::Context) -> bool {
        ctx.input(|i| {
            i.raw.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::PointerButton { .. }
                        | egui::Event::Key { .. }
                        | egui::Event::Touch { .. }
                        | egui::Event::Text { .. }
                )
            })
        })
    }
}

impl eframe::App for WebApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // re-assert the dark theme every frame (egui's web runner can load
        // its own default visuals after the app-creator closure ran)
        GuiApp::style_ctx(ctx);
        // first gesture: kick off getUserMedia once
        if !self.started && self.gesture_seen(ctx) {
            self.started = true;
            self.camera.request(None);
        }
        // engine tick: the web equivalent of the desktop pipeline loop.
        // GUARDED: a panic inside the render work (the glyph-index crash
        // class) must not unwind through eframe's repaint callback — that
        // kills the whole rAF loop and the app freezes forever with no
        // popup. Catch it, report it, keep the UI alive to show it.
        if self.started {
            self.camera.drive();
            let shared = self.gui.shared.clone();
            let cam = &mut self.camera;
            if let Err(msg) = self.engine.tick_guarded(&shared, cam, Instant::now()) {
                web_sys::console::error_1(&format!("engine tick panicked: {msg}").into());
                self.started = false; // stop ticking; the popup shows the report
            }
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

/// Browser camera: getUserMedia → off-screen `<video>` → 2D canvas → RGB buffer.
struct WebCamera {
    video: Option<web_sys::HtmlVideoElement>,
    ctx: Option<web_sys::CanvasRenderingContext2d>,
    buf: Vec<u8>,
    pending: bool,
    failed_once: bool,
}

impl WebCamera {
    fn new() -> Self {
        WebCamera { video: None, ctx: None, buf: Vec::new(), pending: false, failed_once: false }
    }

    /// Fire the async getUserMedia request. `facing` overrides the
    /// `facingMode` constraint — used for the automatic retry with the
    /// other camera when the first one fails on Android.
    fn request(&mut self, facing: Option<&str>) {
        if self.pending || self.video.is_some() {
            return;
        }
        self.pending = true;
        let video = create_video();
        let v = video.clone();
        let failed_once = self.failed_once;
        let facing = facing.map(|f| f.to_string());
        wasm_bindgen_futures::spawn_local(async move {
            match get_user_media(facing.as_deref()).await {
                Ok(stream) => {
                    let track0 = stream.get_tracks().get(0);
                    let label = js_sys::Reflect::get(&track0, &wasm_bindgen::JsValue::from_str("label"))
                        .ok()
                        .and_then(|l| l.as_string())
                        .unwrap_or_default();
                    matrix_core::report::set_context(|c| c.camera = label.clone());
                    matrix_core::report::log_line(format!("camera opened: {}", if label.is_empty() { "(unknown device)" } else { &label }));
                    v.set_src_object(Some(&stream));
                    // autoplay muted — the user already gestured; if Android
                    // still rejects it, `drive()` re-issues play() on the
                    // next repaint until frames actually flow.
                    v.set_autoplay(true);
                    v.set_muted(true);
                    v.set_loop(false);
                    let _ = v.play();
                }
                Err(e) => {
                    log::warn!("getUserMedia failed: {:?}", e);
                    // First failure on a phone is often "no front camera
                    // available" (or a transient AbortError while the
                    // permission dialog is up): retry once with the other
                    // facing before reporting an error.
                    if !failed_once {
                        Camera::RETRY.with(|r| r.set(true));
                    } else {
                        set_camera_error("camera unavailable (allow camera access and reload)");
                    }
                }
            }
            CAMERA_PENDING.store(false, std::sync::atomic::Ordering::Relaxed);
        });
        self.video = Some(video);
    }

    /// Per-repaint camera upkeep: retry `play()` if the element stalled
    /// (Android can reject the initial muted autoplay), and run the
    /// automatic other-facing retry after a failure.
    fn drive(&mut self) {
        if let Some(v) = &self.video {
            if v.paused() && v.ready_state() >= 1 {
                let _ = v.play();
            }
            // decoding for real? un-pause the app exactly once. Keyed off
            // the element itself (not frames pulled through the engine) so
            // it works even while `Inner::paused` still blocks rendering.
            if v.ready_state() >= 2 && v.video_width() > 0 {
                if let Some(sh) = SHARED.get() {
                    let mut g = sh.inner.lock().unwrap();
                    if g.paused && g.status.cam == "browser camera" {
                        g.paused = false;
                    }
                }
            }
        }
        if !self.pending && self.video.is_none() {
            let retry = Camera::RETRY.with(|r| r.replace(false));
            if retry {
                self.failed_once = true;
                self.request(Some("environment"));
            }
        }
    }

    fn stop(&mut self) {
        if let Some(v) = self.video.take() {
            if let Some(s) = v.src_object() {
                for t in s.get_tracks() {
                    if let Ok(mt) = t.dyn_into::<web_sys::MediaStreamTrack>() {
                        mt.stop();
                    }
                }
            }
        }
    }
}

/// Tiny per-camera state parking (the retry flag hops from the async
/// closure back into `drive()` without fighting the borrow checker).
struct Camera;
impl Camera {
    thread_local! {
        pub static RETRY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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

/// The camera element. NOT `display:none`: Android Chrome suspends decode
/// for hidden video elements, which reads as "permission granted but
/// black". A 1px, opacity-0 element pinned to the corner decodes normally
/// and stays invisible.
fn create_video() -> web_sys::HtmlVideoElement {
    let doc = web_sys::window().unwrap().document().unwrap();
    let v = doc.create_element("video").unwrap().dyn_into::<web_sys::HtmlVideoElement>().unwrap();
    v.set_attribute(
        "style",
        "position:fixed;left:0;top:0;width:1px;height:1px;opacity:0.01;pointer-events:none;z-index:-1;",
    )
    .ok();
    v.set_attribute("playsinline", "true").ok();
    v.set_attribute("autoplay", "true").ok();
    v.set_attribute("muted", "true").ok();
    doc.body().unwrap().append_child(&v).ok();
    v
}

async fn get_user_media(facing: Option<&str>) -> Result<web_sys::MediaStream, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let nav = window.navigator();
    let md = nav.media_devices().map_err(|_| JsValue::from_str("no media devices (needs https or localhost)"))?;
    // Track constraints as a plain JS object: `{width: {ideal}, height:
    // {ideal}, facingMode}`. `ideal` (not `exact`) lets phones pick a working
    // camera/size freely, and facingMode only when retrying the other camera.
    let track = js_sys::Object::new();
    js_sys::Reflect::set(&track, &JsValue::from_str("width"), &js_val_ideal(1280))?;
    js_sys::Reflect::set(&track, &JsValue::from_str("height"), &js_val_ideal(720))?;
    if let Some(f) = facing {
        js_sys::Reflect::set(&track, &JsValue::from_str("facingMode"), &JsValue::from_str(f))?;
    }
    let cons = web_sys::MediaStreamConstraints::new();
    cons.set_video(&track);
    cons.set_audio(&JsValue::from_bool(false));
    let promise = md.get_user_media_with_constraints(&cons)?;
    let stream = wasm_bindgen_futures::JsFuture::from(promise).await?;
    let stream = stream.dyn_into::<web_sys::MediaStream>()?;
    Ok(stream)
}

/// Build the JS `{ ideal: N }` constraint value.
fn js_val_ideal(v: u32) -> JsValue {
    let o = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&o, &JsValue::from_str("ideal"), &JsValue::from_f64(v as f64));
    JsValue::from(o)
}

impl FrameSource for WebCamera {
    fn next_frame(&mut self) -> Option<(Vec<u8>, usize, usize)> {
        let v = self.video.as_ref()?;
        if v.ready_state() < 2 || v.video_width() == 0 {
            // paused or metadata-less: nudge play() (Android autoplay stall)
            if v.paused() {
                let _ = v.play();
            }
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
                // willReadFrequently: this canvas exists for getImageData
                // readback every frame — Chrome keeps it on a CPU-backed
                // surface instead of warning on each readback.
                let opts = js_sys::Object::new();
                js_sys::Reflect::set(&opts, &wasm_bindgen::JsValue::from_str("willReadFrequently"), &wasm_bindgen::JsValue::from_bool(true)).expect("set willReadFrequently");
                let ctx = c.get_context_with_context_options("2d", &opts).unwrap().unwrap().dyn_into::<web_sys::CanvasRenderingContext2d>().unwrap();
                self.ctx = Some(ctx);
            }
            self.ctx.as_ref().unwrap()
        };
        ctx.draw_image_with_html_video_element(v, 0.0, 0.0).ok()?;
        // getImageData → RGBA; engine wants RGB
        let img = ctx.get_image_data(0.0, 0.0, w as f64, h as f64).ok()?;
        let data = img.data();
        let n = (w as usize) * (h as usize);
        self.buf.clear();
        let raw = data.to_vec();
        self.buf.reserve(n * 3);
        for px in raw.as_chunks::<4>().0 {
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
// crash / error capture
// ---------------------------------------------------------------------------

/// Platform description for the report context: user agent, language and
/// viewport — the bits that matter when reproducing a mobile bug.
fn platform_line() -> String {
    let ua = web_sys::window()
        .map(|w| w.navigator().user_agent().unwrap_or_default())
        .unwrap_or_default();
    let lang = web_sys::window().map(|w| w.navigator().language().unwrap_or_default()).unwrap_or_default();
    let vp = web_sys::window()
        .map(|w| {
            let iw = w.inner_width().map(|v| v.as_f64().unwrap_or(0.0) as u32).unwrap_or(0);
            let ih = w.inner_height().map(|v| v.as_f64().unwrap_or(0.0) as u32).unwrap_or(0);
            format!("{iw}x{ih}")
        })
        .unwrap_or_default();
    format!("{ua} / {lang} / viewport {vp}")
}

/// Chain a panic hook onto the default one: the default still logs to the
/// console (via console_error_panic_hook), this one records the panic into
/// the shared report store so the egui popup can show it. Runs on the SAME
/// thread egui repaints on, so the popup appears without any cross-thread
/// trickery.
fn install_panic_capture() {
    let prior = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info
            .payload()
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| info.payload().downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "panic (no message)".into());
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        let stack = std::backtrace::Backtrace::force_capture().to_string();
        report::capture(report::Kind::Panic, msg, stack, location);
        prior(info);
    }));
}

/// `window.onerror` + `onunhandledrejection`: JS exceptions this module
/// cannot otherwise see. Registered as global closures; kept alive in a
/// leaked Vec so they live as long as the page (the app does too).
fn install_js_error_capture() {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    // window.onerror(message, source, lineno, colno, error)
    let cb = Closure::<dyn FnMut(JsValue, JsValue, i32, i32, JsValue)>::new(
        |msg: JsValue, source: JsValue, line: i32, col: i32, _err: JsValue| {
            let msg = msg.as_string().unwrap_or_else(|| "JS error".into());
            let origin = source
                .as_string()
                .map(|s| format!("{}:{}:{}", s, line, col))
                .unwrap_or_else(|| format!("js:{}:{}", line, col));
            report::capture(report::Kind::Error, msg, String::new(), origin);
        },
    );
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("onerror"), cb.as_ref());
    // window.onunhandledrejection(event.reason)
    let rj = Closure::<dyn FnMut(JsValue)>::new(|ev| {
        let reason = js_sys::Reflect::get(&ev, &JsValue::from_str("reason")).unwrap_or(JsValue::NULL);
        let stack = js_sys::Reflect::get(&reason, &JsValue::from_str("stack"))
            .ok()
            .and_then(|s| s.as_string())
            .unwrap_or_default();
        let msg = reason.as_string().unwrap_or_else(|| format!("{:?}", reason));
        report::capture(report::Kind::Error, msg, stack, "unhandledrejection");
    });
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("onunhandledrejection"), rj.as_ref());
    // keep the closures alive for the lifetime of the page
    std::mem::forget((cb, rj));
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
            let line = format!("[{}] {}", rec.level(), rec.args());
            web_sys::console::warn_1(&line.clone().into());
            // warnings land in the crash-report log ring: they usually
            // explain a later failure ("getUserMedia failed: ...").
            if rec.level() == log::Level::Warn {
                matrix_core::report::log_line(line);
            }
        }
    }
    fn flush(&self) {}
}

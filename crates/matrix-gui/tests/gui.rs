//! Integration tests for the shared GUI: the crash popup lifecycle driven
//! through a real (headless) egui context, plus the Store trait's JSON
//! round-trip through a temp-file implementation.

use matrix_gui::gui::{GuiApp, Store};
use matrix_core::config::{Config, Settings};
use matrix_core::report;
use matrix_core::state::{Inner, Shared, Status};
use std::sync::{Arc, Mutex};

/// The report store is process-global (panic hooks must reach it from
/// anywhere), so tests touching it hold this guard for their whole body.
static REPORT_LOCK: Mutex<()> = Mutex::new(());

/// Temp-file Store: same JSON model the desktop's FileStore uses.
struct TempStore {
    path: std::path::PathBuf,
}

impl Store for TempStore {
    fn load(&self) -> Config {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    fn save(&self, cfg: &Config) {
        if let Ok(json) = serde_json::to_string(cfg) {
            let _ = std::fs::write(&self.path, json);
        }
    }
}

fn app() -> (GuiApp, TempStore) {
    let shared = Arc::new(Shared {
        inner: Mutex::new(Inner {
            eff: Settings::preset("ghost"),
            set: Settings::preset("ghost"),
            auto: Default::default(),
            user_presets: Vec::new(),
            preset_name: "ghost".into(),
            paused: true,
            quit: false,
            view: None,
            stats: Default::default(),
            readout: Default::default(),
            status: Status {
                backend: "test".into(),
                cam: "test camera".into(),
                out_w: 64,
                out_h: 48,
                out_fps: 30,
                fps: 0.0,
                threads: 1,
                error: String::new(),
            },
            cfg_preview_fps: 30,
            cfg_tui_fps: 30,
            fps_limit: Default::default(),
            update: matrix_core::update::UpdateState::NotChecked,
            cfg_update_on_start: false,
        }),
    });
    let dir = std::env::temp_dir().join(format!("mc-gui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = TempStore { path: dir.join("settings.json") };
    (GuiApp::new(shared, Arc::new(TempStore { path: store.path.clone() }), Arc::new(NoopUpdater)), store)
}

struct NoopUpdater;
impl matrix_gui::gui::UpdaterHost for NoopUpdater {
    fn build_label(&self) -> String {
        "test".into()
    }
    fn check_now(&self, _s: Arc<Shared>) {}
    fn install(&self, _s: Arc<Shared>) {}
    fn can_install(&self) -> bool {
        false
    }
}

#[test]
fn crash_popup_auto_opens_on_report_and_clears() {
    let _g = REPORT_LOCK.lock().unwrap();
    report::clear();
    let (mut app, _store) = app();

    // no reports: nothing drawn
    let ctx = egui::Context::default();
    let mut drawn = false;
    ctx.run(egui::RawInput::default(), |ctx| drawn = app.crash_popup(ctx));
    assert!(!drawn, "no popup without reports");

    // a report arrives (as if the panic hook captured one)
    report::capture(report::Kind::Panic, "test panic", "frame 0: test", "test.rs:1:1");
    ctx.run(egui::RawInput::default(), |ctx| drawn = app.crash_popup(ctx));
    assert!(drawn, "popup auto-opens on the first report");

    // still open on later frames until dismissed/cleared
    ctx.run(egui::RawInput::default(), |ctx| drawn = app.crash_popup(ctx));
    assert!(drawn, "popup stays open");

    // clear all: next frame nothing is drawn
    report::clear();
    ctx.run(egui::RawInput::default(), |ctx| drawn = app.crash_popup(ctx));
    assert!(!drawn, "popup gone after clear");
    report::clear();
}

#[test]
fn crash_popup_text_contains_fix_information() {
    let _g = REPORT_LOCK.lock().unwrap();
    report::clear();
    let (mut app, _store) = app();
    report::set_context(|c| {
        c.host = "web".into();
        c.version = "v2.4.0".into();
        c.platform = "test platform".into();
    });
    report::log_line("camera opened: front");
    report::capture(report::Kind::Error, "getUserMedia denied", "  at camera (main.rs:9)", "camera.rs:9:1");

    // draw the popup; then assert the paste-text carries everything
    let ctx = egui::Context::default();
    let mut drawn = false;
    ctx.run(egui::RawInput::default(), |ctx| drawn = app.crash_popup(ctx));
    assert!(drawn);
    let text = report::full_text();
    assert!(text.contains("message: getUserMedia denied"), "message");
    assert!(text.contains("at camera (main.rs:9)"), "stack trace");
    assert!(text.contains("origin:  camera.rs:9:1"), "origin");
    assert!(text.contains("host:    web"), "host context");
    assert!(text.contains("version: v2.4.0"), "version context");
    assert!(text.contains("platform: test platform"), "platform context");
    assert!(text.contains("recent log:"), "log section");
    assert!(text.contains("camera opened: front"), "log content");
    report::clear();
}

#[test]
fn settings_store_round_trip() {
    let (app, store) = app();
    // same model as the GUI's save button: current settings + presets
    let mut cfg = store.load();
    let g = app.shared.inner.lock().unwrap();
    let mut set = g.set.clone();
    set.glow = 7;
    set.ink = [12, 200, 90];
    cfg.set = Some(set);
    cfg.user_presets.push(matrix_core::config::UserPreset { name: "look A".into(), set: Settings::preset("magma") });
    drop(g);
    store.save(&cfg);

    let back = store.load();
    let set = back.set.expect("settings survived");
    assert_eq!(set.glow, 7);
    assert_eq!(set.ink, [12, 200, 90]);
    assert_eq!(back.user_presets.len(), 1);
    assert_eq!(back.user_presets[0].name, "look A");
}

#[test]
fn store_load_with_missing_file_gives_defaults() {
    let store = TempStore { path: std::env::temp_dir().join(format!("mc-missing-{}", std::process::id())) };
    let cfg = store.load();
    assert!(cfg.set.is_none());
    assert_eq!(cfg.preview_fps, 30);
}

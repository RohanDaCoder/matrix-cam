//! Integration tests for the v2.5 control-deck GUI: the crash dialog
//! lifecycle through a real (headless) egui context, the custom widgets
//! (segmented control clicks, deck-slider drag geometry, toggle) driven
//! with synthesized pointer input, and the Store JSON round-trip.

use matrix_gui::gui::{GuiApp, Store};
use matrix_gui::widgets;
use matrix_core::config::{Config, Settings, Style};
use matrix_core::report;
use matrix_core::state::{Inner, Shared, Status};
use std::sync::{Arc, Mutex};

/// The report store is process-global; tests touching it hold this guard.
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

// ---------------------------------------------------------------------------
// crash dialog lifecycle
// ---------------------------------------------------------------------------

#[test]
fn crash_popup_auto_opens_on_report_and_clears() {
    let _g = REPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    report::clear();
    let (mut app, _store) = app();

    let ctx = egui::Context::default();
    let mut drawn = false;
    ctx.run(egui::RawInput::default(), |ctx| drawn = app.crash_popup(ctx));
    assert!(!drawn, "no dialog without reports");

    report::capture(report::Kind::Panic, "test panic", "frame 0: test", "test.rs:1:1");
    ctx.run(egui::RawInput::default(), |ctx| drawn = app.crash_popup(ctx));
    assert!(drawn, "dialog auto-opens on the first report");

    ctx.run(egui::RawInput::default(), |ctx| drawn = app.crash_popup(ctx));
    assert!(drawn, "dialog stays open");

    report::clear();
    ctx.run(egui::RawInput::default(), |ctx| drawn = app.crash_popup(ctx));
    assert!(!drawn, "dialog gone after clear");
    report::clear();
}

#[test]
fn crash_popup_text_contains_fix_information() {
    let _g = REPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    report::clear();
    let (mut app, _store) = app();
    report::set_context(|c| {
        c.host = "web".into();
        c.version = "v2.5.0".into();
        c.platform = "test platform".into();
    });
    report::log_line("camera opened: front");
    report::capture(report::Kind::Error, "getUserMedia denied", "  at camera (main.rs:9)", "camera.rs:9:1");

    let ctx = egui::Context::default();
    let mut drawn = false;
    ctx.run(egui::RawInput::default(), |ctx| drawn = app.crash_popup(ctx));
    assert!(drawn);
    let text = report::full_text();
    assert!(text.contains("message: getUserMedia denied"), "message");
    assert!(text.contains("at camera (main.rs:9)"), "stack trace");
    assert!(text.contains("origin:  camera.rs:9:1"), "origin");
    assert!(text.contains("host:    web"), "host context");
    assert!(text.contains("version: v2.5.0"), "version context");
    assert!(text.contains("platform: test platform"), "platform context");
    assert!(text.contains("recent log:"), "log section");
    assert!(text.contains("camera opened: front"), "log content");
    report::clear();
}

// ---------------------------------------------------------------------------
// custom widgets: segmented + slider + toggle driven with real input
// ---------------------------------------------------------------------------

/// Warm up with a plain frame (widgets register for hit-testing), then a
/// second frame carrying the click at `pos`. egui hit-tests against the
/// previous frame's layout, so a cold single-frame click can never land.
fn click_at(ctx: &egui::Context, pos: egui::Pos2, mut f: impl FnMut(&mut egui::Ui)) {
    ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| f(ui));
    });
    let mut raw = egui::RawInput::default();
    raw.events = vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    ];
    ctx.run(raw, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| f(ui));
    });
}

#[test]
fn segmented_control_selects_clicked_option() {
    let ctx = egui::Context::default();
    let mut style = Style::Dots;
    let mut hits = 0;
    // warm-up + probe several x positions across the panel
    for x in [40.0, 140.0, 240.0, 340.0] {
        click_at(&ctx, egui::pos2(x, 30.0), |ui| {
            if widgets::segmented(ui, "style", &Style::ALL, |s| s.label().to_string(), &mut style).is_some() {
                hits += 1;
            }
        });
    }
    assert!(hits >= 1, "at least one segment click registered");
    assert_ne!(style, Style::Dots, "selection moved off the default");
}

#[test]
fn deck_slider_drag_updates_value() {
    let ctx = egui::Context::default();
    let mut v: u32 = 40;
    let eff = v;
    let mut track_rect = egui::Rect::NOTHING;
    // warm-up frame records where the track landed (probe via cursor delta
    // is fragile; instead mirror the layout math: label 92 + margin, then
    // track up to 320 wide, 10 tall, vertically centered in a 30px row at
    // the top of the panel (y 8..38 → track center y ≈ 23).
    ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            widgets::deck_slider_u32(ui, "detail", "tip", &mut v, eff, false, 40..=220);
            track_rect = egui::Rect::from_min_max(
                egui::pos2(ui.min_rect().left() + 94.0, 19.0),
                egui::pos2(ui.min_rect().left() + 94.0 + 320.0, 29.0),
            );
        });
    });
    // drag across the track across SEPARATE frames (a drag only registers
    // while the button stays pressed between frames): down → move → move → up
    let y = track_rect.center().y;
    let x0 = track_rect.left() + 20.0;
    let x1 = track_rect.left() + 300.0;
    let frame = |events, v: &mut u32| {
        let mut raw = egui::RawInput::default();
        raw.events = events;
        ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                widgets::deck_slider_u32(ui, "detail", "tip", v, *v, false, 40..=220);
            });
        });
    };
    frame(
        vec![
            egui::Event::PointerMoved(egui::pos2(x0, y)),
            egui::Event::PointerButton {
                pos: egui::pos2(x0, y),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        &mut v,
    );
    frame(
        vec![egui::Event::PointerMoved(egui::pos2((x0 + x1) / 2.0, y))],
        &mut v,
    );
    frame(
        vec![
            egui::Event::PointerMoved(egui::pos2(x1, y)),
            egui::Event::PointerButton {
                pos: egui::pos2(x1, y),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ],
        &mut v,
    );
    assert!(v > 40, "slider drag changed the value (now {v})");
}

#[test]
fn toggle_flips_on_click() {
    let ctx = egui::Context::default();
    let mut on = false;
    click_at(&ctx, egui::pos2(200.0, 30.0), |ui| {
        widgets::toggle(ui, "invert", "tip", &mut on);
    });
    assert!(on, "toggle flipped");
}

// ---------------------------------------------------------------------------
// store round-trip (unchanged behavior, re-verified against the new crate)
// ---------------------------------------------------------------------------

#[test]
fn settings_store_round_trip() {
    let (app, store) = app();
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


// ---------------------------------------------------------------------------
// fps limiter: set_fps_limit live + persist + the popover driven by clicks
// ---------------------------------------------------------------------------

#[test]
fn fps_limit_set_applies_live_and_persists() {
    let (app, store) = app();
    app.set_fps_limit(Some(8));
    {
        let g = app.shared.inner.lock().unwrap();
        assert!(g.fps_limit.enabled, "limit enabled");
        assert_eq!(g.fps_limit.fps, 8, "fps stored");
    }
    let cfg = store.load();
    assert!(cfg.fps_limit_enabled, "persisted: enabled");
    assert_eq!(cfg.fps_limit_fps, 8, "persisted: fps");

    // lift the limit
    app.set_fps_limit(None);
    {
        let g = app.shared.inner.lock().unwrap();
        assert!(!g.fps_limit.enabled, "limit lifted");
    }
    let cfg = store.load();
    assert!(!cfg.fps_limit_enabled, "persisted: lifted");

    // clamp out-of-range values
    app.set_fps_limit(Some(999));
    assert_eq!(app.shared.inner.lock().unwrap().fps_limit.fps, 60, "clamped high");
    app.set_fps_limit(Some(0));
    assert_eq!(app.shared.inner.lock().unwrap().fps_limit.fps, 1, "clamped low");
}

#[test]
fn fps_chip_click_opens_popover_and_preset_applies() {
    let (mut app, store) = app();
    let ctx = egui::Context::default();
    GuiApp::style_ctx(&ctx);
    // warm-up so the top-bar chips register for hit-testing
    ctx.run(egui::RawInput::default(), |ctx| app.top_bar_for_test(ctx));

    // one click at the fps chip (y ~15 in the bar; sweep x to find it)
    let mut opened = false;
    for x in [170.0, 185.0, 199.0, 215.0, 230.0] {
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)));
        raw.events = vec![
            egui::Event::PointerMoved(egui::pos2(x, 15.0)),
            egui::Event::PointerButton { pos: egui::pos2(x, 15.0), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
            egui::Event::PointerButton { pos: egui::pos2(x, 15.0), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
        ];
        ctx.run(raw, |ctx| app.top_bar_for_test(ctx));
        if app.fps_popover_open() {
            opened = true;
            break;
        }
    }
    assert!(opened, "fps chip click opened the limiter popover");

    // a PLAIN frame first: the popover's squares must exist in the widget
    // list for a full frame before a click can hit-test them
    ctx.run(egui::RawInput::default(), |ctx| app.top_bar_for_test(ctx));

    // click the first preset square (8 fps) — interior of its rect
    let mut raw = egui::RawInput::default();
    raw.screen_rect = Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)));
    raw.events = vec![
        egui::Event::PointerMoved(egui::pos2(215.0, 105.0)),
        egui::Event::PointerButton { pos: egui::pos2(215.0, 105.0), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
        egui::Event::PointerButton { pos: egui::pos2(215.0, 105.0), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
    ];
    ctx.run(raw, |ctx| app.top_bar_for_test(ctx));
    {
        let g = app.shared.inner.lock().unwrap();
        assert!(g.fps_limit.enabled, "preset click enabled the limit");
        assert_eq!(g.fps_limit.fps, 8, "preset click set the 8 fps square");
    }
    // popover closed after choosing a preset
    assert!(!app.fps_popover_open(), "popover closed after preset");

    // persisted
    let cfg = store.load();
    assert!(cfg.fps_limit_enabled, "preset persisted: enabled");
    assert_eq!(cfg.fps_limit_fps, 8, "preset persisted: fps");

    // re-open and lift via OFF
    for x in [170.0, 185.0, 199.0, 215.0, 230.0] {
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)));
        raw.events = vec![
            egui::Event::PointerMoved(egui::pos2(x, 15.0)),
            egui::Event::PointerButton { pos: egui::pos2(x, 15.0), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
            egui::Event::PointerButton { pos: egui::pos2(x, 15.0), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
        ];
        ctx.run(raw, |ctx| app.top_bar_for_test(ctx));
        if app.fps_popover_open() {
            break;
        }
    }
    assert!(app.fps_popover_open(), "popover re-opened");
    ctx.run(egui::RawInput::default(), |ctx| app.top_bar_for_test(ctx));
    // OFF row sits above the squares (y ~ 60-80, x ~ 209+)
    let mut raw = egui::RawInput::default();
    raw.screen_rect = Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)));
    raw.events = vec![
        egui::Event::PointerMoved(egui::pos2(260.0, 75.0)),
        egui::Event::PointerButton { pos: egui::pos2(260.0, 75.0), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
        egui::Event::PointerButton { pos: egui::pos2(260.0, 75.0), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
    ];
    ctx.run(raw, |ctx| app.top_bar_for_test(ctx));
    {
        let g = app.shared.inner.lock().unwrap();
        assert!(!g.fps_limit.enabled, "OFF lifted the limit");
    }
    let cfg = store.load();
    assert!(!cfg.fps_limit_enabled, "lift persisted");
}

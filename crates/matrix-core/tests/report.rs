//! Integration tests for the crash/error report store: capture paths,
//! context, the rolling log ring, and the rendered report text (the thing
//! users paste into issues — it must contain everything needed to fix a
//! bug: message, stack, origin, environment and the recent log).

use matrix_core::report::{self, Context, Kind};
use std::sync::Mutex;

/// The report store is process-global (panic hooks must reach it from
/// anywhere), so every test in this file holds this guard for its whole
/// body — parallel test threads otherwise race on the shared state
/// (exactly what the Windows CI run caught).
static STORE_LOCK: Mutex<()> = Mutex::new(());

// The report store is global state; tests reset it so they are order-safe.
fn reset() {
    report::clear();
}

#[test]
fn capture_stores_message_stack_origin() {
    let _store = STORE_LOCK.lock().unwrap();
    reset();
    report::capture(Kind::Panic, "index out of bounds", "frame 0: render\nframe 1: tick", "renderer.rs:42:9");
    let reps = report::reports_snapshot();
    assert_eq!(reps.len(), 1);
    assert_eq!(reps[0].kind, Kind::Panic);
    assert_eq!(reps[0].message, "index out of bounds");
    assert_eq!(reps[0].origin, "renderer.rs:42:9");
    assert!(reps[0].stack.contains("render"));
    reset();
}

#[test]
fn context_travels_into_the_report() {
    let _store = STORE_LOCK.lock().unwrap();
    reset();
    report::set_context(|c| {
        c.host = "web".into();
        c.version = "v2.4.0".into();
        c.platform = "Android 14 / Chrome 130".into();
        c.camera = "front camera".into();
        *c = Context { host: c.host.clone(), version: c.version.clone(), platform: c.platform.clone(), camera: c.camera.clone(), extra: vec![("url".into(), "https://x/matrix-cam/".into())] };
    });
    report::capture(Kind::Error, "getUserMedia failed", "", "camera request");
    let text = report::full_text();
    assert!(text.contains("host:    web"), "host in report");
    assert!(text.contains("version: v2.4.0"), "version in report");
    assert!(text.contains("platform: Android 14"), "platform in report");
    assert!(text.contains("camera:  front camera"), "camera in report");
    assert!(text.contains("url: https://x/matrix-cam/"), "extra k/v in report");
    assert!(text.contains("message: getUserMedia failed"), "message in report");
    reset();
}

#[test]
fn log_ring_keeps_most_recent_lines() {
    let _store = STORE_LOCK.lock().unwrap();
    reset();
    for i in 0..80 {
        report::log_line(format!("line {i}"));
    }
    let snap = report::log_snapshot();
    assert_eq!(snap.len(), 64, "ring capacity is 64");
    // oldest kept = line 16 (80 - 64), newest = line 79
    assert_eq!(snap[0], "line 16");
    assert_eq!(snap[63], "line 79");
    reset();
}

#[test]
fn capture_also_appends_to_log_ring() {
    let _store = STORE_LOCK.lock().unwrap();
    reset();
    report::capture(Kind::Warning, "camera hiccup", "", "engine");
    let log = report::log_snapshot();
    assert_eq!(log.len(), 1);
    assert!(log[0].contains("[warning] camera hiccup"));
    assert!(log[0].contains("@ engine"));
    reset();
}

#[test]
fn report_cap_keeps_most_recent() {
    let _store = STORE_LOCK.lock().unwrap();
    reset();
    for i in 0..40 {
        report::capture(Kind::Error, format!("e{i}"), "", "");
    }
    let reps = report::reports_snapshot();
    assert_eq!(reps.len(), 32, "report cap is 32");
    assert_eq!(reps[0].message, "e8", "oldest dropped (40-32=8)");
    assert_eq!(reps[31].message, "e39");
    reset();
}

#[test]
fn to_text_includes_stack_and_log() {
    let _store = STORE_LOCK.lock().unwrap();
    reset();
    report::set_context(|c| c.host = "windows-desktop".into());
    report::log_line("boot: desktop app starting");
    report::capture(Kind::Panic, "unwrap on None", "  frame 0: core\n  frame 1: gui", "gui.rs:9:31");
    let reps = report::reports_snapshot();
    let text = reps[0].to_text(&report::log_snapshot());
    assert!(text.contains("panic report"), "kind header");
    assert!(text.contains("message: unwrap on None"));
    assert!(text.contains("stack trace:"));
    assert!(text.contains("frame 0: core"), "stack frames included");
    assert!(text.contains("recent log:"));
    assert!(text.contains("boot: desktop app starting"), "log lines included");
    reset();
}

#[test]
fn count_and_clear() {
    let _store = STORE_LOCK.lock().unwrap();
    reset();
    assert_eq!(report::count(), 0);
    report::capture(Kind::Error, "a", "", "");
    report::capture(Kind::Error, "b", "", "");
    assert_eq!(report::count(), 2);
    report::clear();
    assert_eq!(report::count(), 0);
    assert!(report::full_text().is_empty());
    reset();
}

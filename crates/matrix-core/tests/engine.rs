//! Integration tests for the core engine: the render pipeline contract
//! (frames flow, view published, serials advance) and the regression for
//! the Android black-preview class of bugs — a paused engine must NOT pull
//! frames, which is exactly why auto-resume keyed on pulled frames could
//! never fire.

use matrix_core::engine::Engine;
use matrix_core::pipeline::{FrameSink, FrameSource, PipelineSpec};
use matrix_core::state::{Inner, Shared, Status};
use matrix_core::time::Instant;
use std::sync::{Arc, Mutex};

/// Tests that touch the process-global report store hold this guard for
/// their whole body (parallel test threads otherwise race on it).
static REPORT_LOCK: Mutex<()> = Mutex::new(());

/// Deterministic camera: emits `n` distinct frames, then optionally dries up.
struct FakeCam {
    frames_left: usize,
    width: usize,
    height: usize,
    tick: u8,
}

impl FakeCam {
    fn frames(n: usize) -> Self {
        FakeCam { frames_left: n, width: 8, height: 6, tick: 0 }
    }
}

impl FrameSource for FakeCam {
    fn next_frame(&mut self) -> Option<(Vec<u8>, usize, usize)> {
        if self.frames_left == 0 {
            return None;
        }
        self.frames_left -= 1;
        self.tick = self.tick.wrapping_add(40);
        let px = vec![self.tick; self.width * self.height * 3];
        Some((px, self.width, self.height))
    }
}

fn shared(paused: bool) -> Arc<Shared> {
    Arc::new(Shared {
        inner: Mutex::new(Inner {
            eff: matrix_core::config::Settings::preset("ghost"),
            set: matrix_core::config::Settings::preset("ghost"),
            auto: Default::default(),
            user_presets: Vec::new(),
            preset_name: "ghost".into(),
            paused,
            quit: false,
            view: None,
            stats: Default::default(),
            readout: Default::default(),
            status: Status {
                backend: "test".into(),
                cam: "fake camera".into(),
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
    })
}

#[test]
fn frames_flow_and_view_publishes() {
    let sh = shared(false);
    let mut eng = Engine::new(64, 48, 1, false);
    let mut cam = FakeCam::frames(3);
    let now = Instant::now();
    for _ in 0..3 {
        assert!(eng.tick(&sh, &mut cam, now));
    }
    let g = sh.inner.lock().unwrap();
    let v = g.view.as_ref().expect("view must be published after ticks");
    assert_eq!(v.w, 64);
    assert_eq!(v.h, 48);
    assert_eq!(v.serial, 3, "serial must advance once per rendered frame");
    assert!(g.status.error.is_empty(), "no error while frames flow");
}

#[test]
fn paused_engine_never_pulls_frames() {
    // THE Android regression: `Inner::paused` starts true (camera needs a
    // gesture) and a paused engine renders no new frame — so anything keyed
    // on "frames pulled through the engine" can never fire while paused.
    // This pins that contract: paused ⇒ source untouched.
    let sh = shared(true);
    let mut eng = Engine::new(64, 48, 1, false);
    let mut cam = FakeCam::frames(5);
    let now = Instant::now();
    for _ in 0..5 {
        assert!(eng.tick(&sh, &mut cam, now));
    }
    // the source still has all 5 frames: none were consumed while paused
    assert_eq!(cam.frames_left, 5, "paused engine must not pull frames");
    // and no view was published
    assert!(sh.inner.lock().unwrap().view.is_none());
}

#[test]
fn quit_request_stops_the_engine() {
    let sh = shared(false);
    sh.inner.lock().unwrap().quit = true;
    let mut eng = Engine::new(64, 48, 1, false);
    let mut cam = FakeCam::frames(1);
    assert!(!eng.tick(&sh, &mut cam, Instant::now()), "tick must return false on quit");
}

#[test]
fn camera_lost_after_long_drought_captures_report() {
    let _g = REPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // err_streak > 300 is the fatal path; camera-lost must surface BOTH in
    // status.error AND as a captured report (that is what the crash popup
    // shows). The capture fires once per streak, not every tick.
    matrix_core::report::clear();
    let sh = shared(false);
    let mut eng = Engine::new(64, 48, 1, false);
    let mut dry = FakeCam::frames(0); // never yields
    let mut t0 = Instant::now();
    for _ in 0..305 {
        eng.tick(&sh, &mut dry, t0);
        t0 = t0 + matrix_core::time::Duration::from_millis(16);
    }
    assert_eq!(sh.inner.lock().unwrap().status.error, "camera lost");
    let reps = matrix_core::report::reports_snapshot();
    assert_eq!(reps.len(), 1, "exactly one report per streak (not per tick)");
    assert!(reps[0].message.contains("camera lost"));
    assert!(reps[0].stack.contains("err_streak"), "stack context explains the streak");
    matrix_core::report::clear();
}

/// Collecting sink for the pipeline test.
struct VecSink(Vec<Vec<u8>>);
impl FrameSink for VecSink {
    fn send(&mut self, frame: &[u8]) -> bool {
        self.0.push(frame.to_vec());
        true
    }
}

/// Fake clock: controllable time, no real sleeping. `sleep` advances the
/// fake instant (interior mutability keeps the trait's &self signature).
struct ManualClock {
    t: Mutex<Instant>,
}
impl matrix_core::pipeline::Clock for ManualClock {
    fn now(&self) -> Instant {
        *self.t.lock().unwrap()
    }
    fn sleep(&self, d: matrix_core::time::Duration) {
        let mut t = self.t.lock().unwrap();
        *t = *t + d;
    }
}

#[test]
fn pipeline_renders_until_quit() {
    // The desktop render loop, driven with a manual clock and a finite
    // camera: after the frames dry up, set quit and run must return.
    let sh = shared(false);
    let spec = PipelineSpec { w: 64, h: 48, threads: 1, want_grid: false };
    let sh2 = sh.clone();
    let handle = std::thread::spawn(move || {
        let cam = FakeCam::frames(2);
        let sink = VecSink(Vec::new());
        let clock = ManualClock { t: Mutex::new(Instant::now()) };
        // give the drought time to be just a hiccup (not fatal), then quit
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            sh2.inner.lock().unwrap().quit = true;
        });
        matrix_core::pipeline::run(sh, Box::new(cam), Box::new(sink), spec, clock);
        // frames rendered before quit: view published at least once
    });
    handle.join().expect("pipeline thread panicked");
}


/// A FrameSource that panics — the crash-report guarantee: tick_guarded
/// catches it, captures a report, and neither the engine nor the caller
/// unwinds.
struct ExplodingCam;

impl FrameSource for ExplodingCam {
    fn next_frame(&mut self) -> Option<(Vec<u8>, usize, usize)> {
        panic!("exploding camera (test)");
    }
}

#[test]
fn guarded_tick_catches_panic_and_captures_report() {
    let _g = REPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    matrix_core::report::clear();
    let sh = shared(false);
    let mut eng = Engine::new(64, 48, 1, false);
    let mut cam = ExplodingCam;
    let now = Instant::now();

    // plain tick would panic straight through — guarded returns Err instead
    let r = eng.tick_guarded(&sh, &mut cam, now);
    let msg = r.expect_err("panic must surface as Err");
    assert!(msg.contains("exploding camera"), "message preserved: {msg}");

    // the report store has the full report (popup reads this)
    let reps = matrix_core::report::reports_snapshot();
    assert_eq!(reps.len(), 1, "exactly one report");
    assert_eq!(reps[0].kind, matrix_core::report::Kind::Panic);
    assert!(reps[0].message.contains("exploding camera"));
    assert!(!reps[0].stack.is_empty(), "backtrace captured");
    assert!(reps[0].origin.contains("guarded"), "origin marks the guarded tick");

    // engine requested quit so the loop stops cleanly
    assert!(sh.inner.lock().unwrap().quit, "quit requested after caught panic");

    // and the engine is still usable (a later guarded tick just returns the quit)
    let r2 = eng.tick_guarded(&sh, &mut cam, now);
    assert_eq!(r2, Ok(false), "subsequent tick honors quit — no re-panic");
    matrix_core::report::clear();
}

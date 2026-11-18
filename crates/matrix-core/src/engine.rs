//! The per-tick effect engine: render one frame from the source and publish
//! it into `Inner`. The desktop pipeline calls `tick` from its worker
//! thread; the web build calls `tick` once per repaint from the eframe
//! callback. All the shared logic — effective settings, analysis, stats,
//! FPS accounting, capture-error streaks — lives here exactly once.

use crate::auto::analyze::Analyzer;
use crate::auto::controller::Auto;
use crate::config::Settings;
use crate::pipeline::FrameSource;
use crate::render::Renderer;
use crate::state::{Shared, View};
use crate::time::Instant;
use std::sync::Arc;

/// One step of the effect chain. Returns `false` when the app asked to quit
/// (`Inner::quit`) — the caller decides what that means (thread exit,
/// eframe exit).
pub struct Engine {
    pub rend: Renderer,
    analyzer: Analyzer,
    controller: Auto,
    stats: crate::auto::analyze::Stats,
    frames: u64,
    fps_t: Instant,
    err_streak: u32,
}

impl Engine {
    pub fn new(w: u32, h: u32, threads: usize, want_grid: bool) -> Self {
        Engine {
            rend: Renderer::new(w, h, Settings::preset("ghost"), threads),
            analyzer: Analyzer::new(),
            controller: Auto::new(),
            stats: Default::default(),
            frames: 0,
            fps_t: Instant::now(),
            err_streak: 0,
        }
        .with_grid(want_grid)
    }

    /// `want_grid` = a front-end reads the cell grid (TUI); costs a snapshot.
    pub fn with_grid(mut self, want: bool) -> Self {
        self.rend.want_grid = want;
        self
    }

    /// Latest published frame without rendering (paused view / error path).
    pub fn frame_arc(&self) -> Option<Arc<Vec<u8>>> {
        self.rend.frame_arc()
    }

    pub fn tick(&mut self, shared: &Arc<Shared>, src: &mut dyn FrameSource, now: Instant) -> bool {
        let Engine { rend, controller, analyzer, stats, frames, fps_t, err_streak } = self;
        tick_inner(rend, controller, analyzer, stats, frames, fps_t, err_streak, shared, src, now)
    }

    /// `tick`, but a panic inside the frame work is caught instead of
    /// unwinding into the caller — the caller's loop (rAF on wasm, the
    /// pipeline thread on desktop) keeps running and the GUI stays alive.
    /// The panic (message + backtrace) is captured as a report so the
    /// crash popup can show it; the engine then stops itself via quit so
    /// it does not re-panic every subsequent frame. A rendering bug must
    /// cost the user a popup, not the app.
    ///
    /// `Err(msg)` is returned when the tick panicked; the report store
    /// already holds the full report.
    pub fn tick_guarded(&mut self, shared: &Arc<Shared>, src: &mut dyn FrameSource, now: Instant) -> Result<bool, String> {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.tick(shared, src, now)));
        match outcome {
            Ok(v) => Ok(v),
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic (no message)".into());
                let stack = std::backtrace::Backtrace::force_capture().to_string();
                crate::report::capture(crate::report::Kind::Panic, msg.clone(), stack, "engine tick (guarded)");
                // stop the render loop cleanly; the GUI shows the popup
                if let Ok(mut g) = shared.inner.lock() {
                    g.quit = true;
                }
                Err(msg)
            }
        }
    }
}

/// The actual per-tick work, as a free function over the engine's
/// destructured parts (disjoint borrows — the reason it is not a method).
#[allow(clippy::too_many_arguments)]
fn tick_inner(
    rend: &mut Renderer,
    controller: &mut Auto,
    analyzer: &mut Analyzer,
    stats: &mut crate::auto::analyze::Stats,
    frames: &mut u64,
    fps_t: &mut Instant,
    err_streak: &mut u32,
    shared: &Arc<Shared>,
    src: &mut dyn FrameSource,
    now: Instant,
) -> bool {
        let (base, acfg, paused, quit) = {
            let g = shared.inner.lock().unwrap();
            (g.set.clone(), g.auto.clone(), g.paused, g.quit)
        };
        if quit {
            return false;
        }

        let (eff, readout) = controller.apply(&base, &acfg, stats, analyzer, now);
        rend.apply(&eff);

        let frame = if paused {
            rend.frame_arc()
        } else {
            match src.next_frame() {
                Some((px, sw, sh)) => {
                    *err_streak = 0;
                    let arc = rend.render(&px, sw, sh);
                    *stats = analyzer.analyze(&rend.grid());
                    Some(arc)
                }
                None => {
                    *err_streak += 1;
                    rend.frame_arc()
                }
            }
        };

        let (w, h) = rend.dims();
        if let Some(f) = frame {
            let serial = rend.serial;
            let grid = rend.grid();
            {
                let mut g = shared.inner.lock().unwrap();
                g.eff = eff.clone();
                g.readout = readout.clone();
                g.stats = stats.clone();
                g.view = Some(View { frame: f.clone(), grid, w, h, serial });
                g.status.error = if *err_streak > 30 {
                    "camera not delivering frames".into()
                } else {
                    String::new()
                };
            }
        }

        *frames += 1;
        if now - *fps_t >= crate::time::Duration::from_secs(1) {
            let fps = *frames as f32 / (now - *fps_t).as_secs_f32();
            shared.inner.lock().unwrap().status.fps = fps;
            *frames = 0;
            *fps_t = now;
        }
        // Long capture failure: surface it, but only the desktop pipeline
        // treats it as fatal (the web camera can recover after a tab
        // refocus, so `tick` keeps running).
        if *err_streak > 300 {
            // capture once per streak (300 -> 301 transitions), so the
            // crash popup has the report but the log ring isn't flooded
            if *err_streak == 301 {
                let backend = shared.inner.lock().map(|g| g.status.cam.clone()).unwrap_or_default();
                crate::report::capture(
                    crate::report::Kind::Error,
                    "camera lost: the capture device stopped delivering frames",
                    format!("err_streak = {} consecutive failed next_frame() calls", err_streak),
                    format!("engine tick / source: {backend}"),
                );
            }
            shared.inner.lock().unwrap().status.error = "camera lost".into();
        }
    true
}

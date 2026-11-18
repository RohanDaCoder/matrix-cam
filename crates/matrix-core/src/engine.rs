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
use crate::state::{Inner, Shared, View};
use crate::time::Instant;
use std::sync::{Arc, Mutex};

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
        let (base, acfg, paused, quit) = {
            let g = shared.inner.lock().unwrap();
            (g.set.clone(), g.auto.clone(), g.paused, g.quit)
        };
        if quit {
            return false;
        }

        let (eff, readout) = self.controller.apply(&base, &acfg, &self.stats, &self.analyzer, now);
        self.rend.apply(&eff);

        let frame = if paused {
            self.rend.frame_arc()
        } else {
            match src.next_frame() {
                Some((px, sw, sh)) => {
                    self.err_streak = 0;
                    let arc = self.rend.render(&px, sw, sh);
                    self.stats = self.analyzer.analyze(&self.rend.grid());
                    Some(arc)
                }
                None => {
                    self.err_streak += 1;
                    self.rend.frame_arc()
                }
            }
        };

        let (w, h) = self.rend.dims();
        if let Some(f) = frame {
            let serial = self.rend.serial;
            let grid = self.rend.grid();
            {
                let mut g = shared.inner.lock().unwrap();
                g.eff = eff.clone();
                g.readout = readout.clone();
                g.stats = self.stats.clone();
                g.view = Some(View { frame: f.clone(), grid, w, h, serial });
                g.status.error = if self.err_streak > 30 {
                    "camera not delivering frames".into()
                } else {
                    String::new()
                };
            }
        }

        self.frames += 1;
        if now - self.fps_t >= crate::time::Duration::from_secs(1) {
            let fps = self.frames as f32 / (now - self.fps_t).as_secs_f32();
            shared.inner.lock().unwrap().status.fps = fps;
            self.frames = 0;
            self.fps_t = now;
        }
        // Long capture failure: surface it, but only the desktop pipeline
        // treats it as fatal (the web camera can recover after a tab
        // refocus, so `tick` keeps running).
        if self.err_streak > 300 {
            shared.inner.lock().unwrap().status.error = "camera lost".into();
        }
        true
    }
}

#[allow(dead_code)]
fn _mutex_import(_: &Mutex<Inner>) {}

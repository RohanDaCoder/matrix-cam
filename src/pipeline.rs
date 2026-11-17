//! The render loop: pull frames from the source, render, publish, push to the sink.
//! Works on any platform; the Windows camera/virtual-camera pair lives in `platform`.

use crate::auto::analyze::Analyzer;
use crate::auto::controller::Auto;
use crate::render::Renderer;
use crate::state::{Inner, Shared, View};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Something that yields camera RGB frames (nokhwa camera, test source, ...).
pub trait FrameSource {
    /// Returns (pixels, width, height) or None on a capture hiccup.
    fn next_frame(&mut self) -> Option<(Vec<u8>, usize, usize)>;
}

/// Something that accepts rendered RGB frames (virtual camera, ...).
pub trait FrameSink {
    fn send(&mut self, frame: &[u8]) -> bool;
    /// Block until the next frame slot (virtual-camera pacing), if any.
    fn wait(&mut self) {}
}

/// Timing source abstraction so the retro FPS limiter can be tested deterministically.
pub trait Clock {
    fn now(&self) -> Instant;
    fn sleep(&self, d: Duration);
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Run the pipeline until `Inner::quit` is set. `C: Clock` is `SystemClock` in
/// production; tests can inject a fake clock.
pub fn run<C: Clock>(
    shared: Arc<Shared>,
    mut src: Box<dyn FrameSource + Send>,
    mut sink: Box<dyn FrameSink + Send>,
    w: u32,
    h: u32,
    threads: usize,
    want_grid: bool,
    clock: C,
) {
    let mut rend = Renderer::new(w, h, crate::config::Settings::preset("ghost"), threads);
    rend.want_grid = want_grid;
    let mut analyzer = Analyzer::new();
    let mut controller = Auto::new();
    let mut stats = crate::auto::analyze::Stats::default();
    let mut frames = 0u64;
    let mut fps_t = clock.now();
    let mut err_streak = 0u32;
    let mut next_frame_at = clock.now();
    let mut limited = false;

    loop {
        let (base, acfg, paused, quit, fps_limit) = {
            let g = shared.inner.lock().unwrap();
            (g.set.clone(), g.auto.clone(), g.paused, g.quit, g.fps_limit.clone())
        };
        if quit {
            break;
        }

        // retro FPS limiter: recompute pacing when the setting changes
        if fps_limit.enabled != limited {
            limited = fps_limit.enabled;
            next_frame_at = clock.now();
        }

        if limited {
            let now = clock.now();
            if now < next_frame_at {
                let wait = next_frame_at - now;
                // chunked sleep so setting changes take effect quickly
                let step = wait.min(Duration::from_millis(4));
                clock.sleep(step);
                continue;
            }
            let frame_time = Duration::from_nanos(1_000_000_000 / fps_limit.fps.max(1) as u64);
            next_frame_at = now + frame_time;
        }

        let (eff, readout) = controller.apply(&base, &acfg, &stats, &analyzer, clock.now());
        rend.apply(&eff);

        let frame = if paused {
            rend.frame_arc()
        } else {
            match src.next_frame() {
                Some((px, sw, sh)) => {
                    err_streak = 0;
                    let arc = rend.render(&px, sw, sh);
                    stats = analyzer.analyze(&rend.grid());
                    Some(arc)
                }
                None => {
                    err_streak += 1;
                    rend.frame_arc()
                }
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
            if !sink.send(&f) {
                shared.inner.lock().unwrap().status.error = "virtual camera send failed".into();
                break;
            }
        }
        sink.wait();

        frames += 1;
        if clock.now() - fps_t >= Duration::from_secs(1) {
            let fps = frames as f32 / (clock.now() - fps_t).as_secs_f32();
            shared.inner.lock().unwrap().status.fps = fps;
            frames = 0;
            fps_t = clock.now();
        }
        if err_streak > 300 {
            shared.inner.lock().unwrap().status.error = "camera lost".into();
            break;
        }
    }
}

/// Convenience wrapper used by `platform` (real clock).
pub fn spawn(
    shared: Arc<Shared>,
    src: Box<dyn FrameSource + Send>,
    sink: Box<dyn FrameSink + Send>,
    w: u32,
    h: u32,
    threads: usize,
    want_grid: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("pipeline".into())
        .spawn(move || run(shared, src, sink, w, h, threads, want_grid, SystemClock))
        .expect("spawn pipeline thread")
}

#[allow(dead_code)]
fn _mutex_import(_: &Mutex<Inner>) {}

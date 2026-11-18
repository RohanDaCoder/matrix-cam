//! The desktop render loop: pull frames from the source, render (through
//! the shared `engine`), publish, push to the sink. Works on any native
//! platform; the Windows camera/virtual-camera pair lives in `matrix-cli`'s
//! `platform`, and the web build uses `engine` directly per tick.

use crate::engine::Engine;
use crate::state::Shared;
use crate::time::{Duration, Instant};
use std::sync::Arc;

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

/// Pipeline shape: output size, worker threads, whether to keep the cell
/// grid (TUI reuses it) — the parts that travel together into `run`.
#[derive(Clone, Copy)]
pub struct PipelineSpec {
    pub w: u32,
    pub h: u32,
    pub threads: usize,
    pub want_grid: bool,
}

/// Run the pipeline until `Inner::quit` is set (or the sink dies). `C: Clock`
/// is `SystemClock` in production; tests can inject a fake clock. The actual
/// per-frame work is `engine::Engine::tick` — the same code the web build runs.
pub fn run<C: Clock>(
    shared: Arc<Shared>,
    mut src: Box<dyn FrameSource + Send>,
    mut sink: Box<dyn FrameSink + Send>,
    spec: PipelineSpec,
    clock: C,
) {
    let PipelineSpec { w, h, threads, want_grid } = spec;
    let mut eng = Engine::new(w, h, threads, want_grid);
    let mut next_frame_at = clock.now();
    let mut limited = false;

    loop {
        let fps_limit = {
            let g = shared.inner.lock().unwrap();
            if g.quit {
                break;
            }
            g.fps_limit.clone()
        };

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

        // guarded: a panic in the render work is reported (crash popup)
        // instead of tearing down the pipeline thread with an unwinding
        // poison; the loop exits cleanly through quit.
        match eng.tick_guarded(&shared, src.as_mut(), clock.now()) {
            Ok(false) => break,
            Ok(true) => {}
            Err(_) => break, // panic captured + reported; stop the loop
        }

        // push the just-published frame into the virtual camera
        let view = { shared.inner.lock().unwrap().view.clone() };
        if let Some(v) = view {
            if !sink.send(&v.frame) {
                shared.inner.lock().unwrap().status.error = "virtual camera send failed".into();
                break;
            }
        }
        sink.wait();

        if shared.inner.lock().unwrap().status.error == "camera lost" {
            break;
        }
    }
}

/// Convenience wrapper used by `platform` (real clock).
pub fn spawn(
    shared: Arc<Shared>,
    src: Box<dyn FrameSource + Send>,
    sink: Box<dyn FrameSink + Send>,
    spec: PipelineSpec,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("pipeline".into())
        .spawn(move || run(shared, src, sink, spec, SystemClock))
        .expect("spawn pipeline thread")
}

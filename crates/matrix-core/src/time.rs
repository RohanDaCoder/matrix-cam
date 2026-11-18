//! `std::time::Instant` panics on wasm32 ("time not implemented"), so the
//! engine code uses this alias: the real thing on every native target and
//! `web_time` (performance.now) in the browser.

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Duration, Instant};

#[cfg(target_arch = "wasm32")]
pub use web_time::{Duration, Instant};

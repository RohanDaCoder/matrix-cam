//! Live-feed analysis + adaptive controller.
//! Analysis runs on the already-downsampled grid (~10-20k cells), so it is nearly free.

pub mod analyze;
pub mod controller;

pub use analyze::{Analyzer, Stats};
pub use controller::{Auto, AutoReadout};

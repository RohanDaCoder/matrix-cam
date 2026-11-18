//! Platform-neutral matrix-cam engine: settings/presets, character-effect
//! renderer, adaptive controller, shared app state, the thread pipeline
//! (desktop) and the tick engine (web/embedders), plus the update-state
//! model shared by every front-end.
//!
//! This crate has no UI and no argument parsing: hosts inject startup
//! overrides through `config::StartOverrides` and drive rendering through
//! `pipeline` (worker thread) or `engine` (per-tick).

pub mod auto;
pub mod config;
pub mod engine;
pub mod pipeline;
pub mod render;
pub mod state;
pub mod time;
pub mod update;

pub use state::Shared;

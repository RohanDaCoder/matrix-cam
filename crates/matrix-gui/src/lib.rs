//! The egui front-end shared by the Windows desktop app and the web build:
//! same preview, same settings panel, same theme. Host-specific bits
//! (settings persistence, update flow) sit behind the `Store` and
//! `UpdaterHost` traits in `gui.rs`.

pub mod gui;
pub mod theme;
pub mod widgets;

pub use gui::{GuiApp, Store, UpdaterHost};

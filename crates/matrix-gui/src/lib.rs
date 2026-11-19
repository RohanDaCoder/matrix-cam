//! The v2.5 "control deck" front-end shared by the Windows desktop app and
//! the web build: one chrome bar, a tabbed settings dock, a full-bleed
//! preview stage with corner telemetry, and an alert-stack crash dialog.
//! Host-specific seams (`Store`, `UpdaterHost`) are unchanged from v2.4.

pub mod gui;
pub mod theme;
pub mod widgets;

pub use gui::{GuiApp, Store, UpdaterHost};

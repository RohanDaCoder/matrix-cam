//! UI front-ends: egui GUI and crossterm TUI.

pub mod gui;
pub mod theme;
pub mod tui;
pub mod widgets;

#[derive(PartialEq, Clone, Copy)]
pub enum Mode {
    Gui,
    Tui,
    Headless,
}

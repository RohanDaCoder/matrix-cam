//! Reusable setting widgets for the GUI.

use crate::ui::theme::RED;
use egui::{Checkbox, Color32, Slider, Ui};

/// Slider that shows the live auto value (read-only, red `A` marker) when the
/// adaptive controller owns the field, and the user's base value otherwise.
fn slider_2speed_i32(
    ui: &mut Ui,
    label: &str,
    tooltip: &str,
    base: &mut i32,
    eff: i32,
    owned: bool,
    r: std::ops::RangeInclusive<i32>,
) {
    if owned {
        let mut shown = eff;
        ui.horizontal(|ui| {
            ui.add_enabled(false, Slider::new(&mut shown, r).text(label));
            ui.colored_label(RED, "A");
        });
    } else {
        ui.add(Slider::new(base, r).text(label));
    }
    ui.response().on_hover_text(tooltip.to_string());
}

fn slider_2speed_u32(
    ui: &mut Ui,
    label: &str,
    tooltip: &str,
    base: &mut u32,
    eff: u32,
    owned: bool,
    r: std::ops::RangeInclusive<u32>,
) {
    if owned {
        let mut shown = eff;
        ui.horizontal(|ui| {
            ui.add_enabled(false, Slider::new(&mut shown, r).text(label));
            ui.colored_label(RED, "A");
        });
    } else {
        ui.add(Slider::new(base, r).text(label));
    }
    ui.response().on_hover_text(tooltip.to_string());
}

pub struct SliderSpec<'a> {
    pub label: &'a str,
    pub tooltip: &'a str,
}

/// Public typed wrappers.
pub fn slider_i32(ui: &mut Ui, label: &str, tooltip: &str, base: &mut i32, eff: i32, owned: bool, r: std::ops::RangeInclusive<i32>) {
    slider_2speed_i32(ui, label, tooltip, base, eff, owned, r);
}
pub fn slider_u32(ui: &mut Ui, label: &str, tooltip: &str, base: &mut u32, eff: u32, owned: bool, r: std::ops::RangeInclusive<u32>) {
    slider_2speed_u32(ui, label, tooltip, base, eff, owned, r);
}

/// Simple label + slider with tooltip (no auto variant).
pub fn plain_u32(ui: &mut Ui, label: &str, tooltip: &str, v: &mut u32, r: std::ops::RangeInclusive<u32>) {
    ui.add(Slider::new(v, r).text(label));
    ui.response().on_hover_text(tooltip.to_string());
}
pub fn plain_f32(ui: &mut Ui, label: &str, tooltip: &str, v: &mut f32, r: std::ops::RangeInclusive<f32>) {
    ui.add(Slider::new(v, r).text(label));
    ui.response().on_hover_text(tooltip.to_string());
}

/// Checkbox bound to auto: read-only with an `A` marker when auto owns it.
pub fn checkbox_auto(ui: &mut Ui, label: &str, tooltip: &str, base: &mut bool, eff: bool, owned: bool) {
    if owned {
        let mut shown = eff;
        ui.horizontal(|ui| {
            ui.add_enabled(false, Checkbox::new(&mut shown, label));
            ui.colored_label(RED, "A");
        });
    } else {
        ui.add(Checkbox::new(base, label));
    }
    ui.response().on_hover_text(tooltip.to_string());
}

/// Color swatch + per-channel sliders with a small preview.
pub fn color_row(ui: &mut Ui, label: &str, tooltip: &str, c: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.label(format!("{label:<7}"));
        ui.colored_label(Color32::from_rgb(c[0], c[1], c[2]), "███");
        ui.add(Slider::new(&mut c[0], 0..=255).text("r"));
    });
    ui.horizontal(|ui| {
        ui.label("       ");
        ui.add(Slider::new(&mut c[1], 0..=255).text("g"));
        ui.add(Slider::new(&mut c[2], 0..=255).text("b"));
    });
    ui.response().on_hover_text(tooltip.to_string());
}

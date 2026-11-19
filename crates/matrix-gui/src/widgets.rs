//! Custom widgets for the v2.5 deck skin. Everything interactive is drawn
//! by hand (painter rects + strokes) instead of relying on egui's stock
//! look — that is what makes this a redesign rather than a re-theme.
//!
//! Two rules govern the set:
//!
//! - **Every control shows its state in place.** Sliders print their value,
//!   segments fill when selected, toggles draw a physical switch. The old
//!   skin leaned on tooltips + hover; the deck leans on always-visible
//!   readouts.
//! - **Auto ownership is a first-class visual state**, not a red "A"
//!   afterthought: an "AUTO" chip locks the widget (still shows the live
//!   effective value, just refuses input) so the user understands WHY a
//!   slider refuses to move instead of wondering why it snaps back.

use crate::theme::{self, ACCENT, ACCENT_DIM, BG0, BG1, BG2, FG, FG2, FG3, LINE, OK, R_CTL};
use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Ui, Vec2};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Rounded rect as three stacked rects (public: the dock tab rail paints
/// with the same geometry helper).
pub fn round_rect_pub(rect: Rect, r: f32) -> Vec<Rect> {
    round_rect(rect, r)
}

fn round_rect(rect: Rect, r: f32) -> Vec<Rect> {
    // rounded rect as 3 rects (top band, middle, bottom band) — cheap and
    // exact enough at 5px radii. Widgets draw hairlines on top.
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);
    vec![
        Rect::from_min_max(pos2(rect.left() + r, rect.top()), pos2(rect.right() - r, rect.top() + r)),
        Rect::from_min_max(pos2(rect.left(), rect.top() + r), pos2(rect.right(), rect.bottom() - r)),
        Rect::from_min_max(pos2(rect.left() + r, rect.bottom() - r), pos2(rect.right() - r, rect.bottom())),
    ]
}

fn pos2(x: f32, y: f32) -> Pos2 {
    Pos2::new(x, y)
}

// ---------------------------------------------------------------------------
// segmented control (single-select)
// ---------------------------------------------------------------------------

/// A single-select segmented control drawn as one continuous bar: each
/// segment is a button; the selected segment fills with the accent.
/// Renders its own labels, hover + selected states, returns the index of
/// the segment that was clicked (if any).
pub fn segmented<T: Clone + PartialEq>(
    ui: &mut Ui,
    id: &str,
    options: &[T],
    label_of: impl Fn(&T) -> String,
    selected: &mut T,
) -> Option<usize> {
    let mut clicked = None;
    let height = 24.0;
    let mut row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, opt) in options.iter().enumerate() {
            let is_sel = *selected == *opt;
            let text = label_of(opt);
            let text_size = ui.painter().layout_no_wrap(
                text.clone(),
                egui::TextStyle::Button.resolve(ui.style()),
                if is_sel { BG0 } else { FG },
            );
            let pad = 10.0;
            let size = Vec2::new(text_size.size().x + pad * 2.0, height);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let visuals = ui.style().interact(&resp);
            let bg = if is_sel {
                ACCENT
            } else if resp.hovered() {
                BG2
            } else {
                BG1
            };
            // fill
            let _ = ui.painter().add(egui::Shape::rect_filled(rect, egui::Rounding::same(0.0), bg));
            // separators between segments (not on the outer edges)
            if i > 0 {
                ui.painter().vline(rect.left(), rect.y_range(), egui::Stroke::new(1.0_f32, LINE));
            }
            // selected underline
            if is_sel {
                let bar = Rect::from_min_max(
                    pos2(rect.left() + 2.0, rect.bottom() - 2.0),
                    pos2(rect.right() - 2.0, rect.bottom()),
                );
                ui.painter().rect_filled(bar, egui::Rounding::same(0.0), ACCENT_DIM);
            }
            ui.painter().galley(
                pos2(rect.left() + pad, rect.center().y - text_size.size().y / 2.0),
                text_size,
                if is_sel { BG0 } else if resp.hovered() { FG } else { FG2 },
            );
            let _ = visuals;
            if resp.clicked() {
                *selected = opt.clone();
                clicked = Some(i);
            }
        }
    });
    let _ = (&mut row, id);
    clicked
}

// ---------------------------------------------------------------------------
// slider with in-place value readout + auto-lock
// ---------------------------------------------------------------------------

/// The deck slider: a fat track with tick marks at the ends, an accent
/// fill up to the thumb, the numeric value printed ON the thumb area, and
/// a small "AUTO" chip beside it when the adaptive controller owns the
/// field (widget then shows the live effective value, read-only).
///
/// `signed` variants map i32 onto the track geometry.
pub fn deck_slider_i32(
    ui: &mut Ui,
    label: &str,
    tooltip: &str,
    base: &mut i32,
    eff: i32,
    auto: bool,
    r: std::ops::RangeInclusive<i32>,
) {
    let (lo, hi) = (*r.start(), *r.end());
    let shown = if auto { eff } else { *base };
    let frac = (shown - lo) as f32 / (hi - lo).max(1) as f32;
    deck_slider_geom(ui, label, tooltip, frac, |_ui, nf| {
        if !auto {
            *base = lo + (nf * (hi - lo) as f32).round() as i32;
        }
    });
    ui.response().on_hover_text(tooltip.to_string());
    let _ = eff;
}

/// Same slider for u32 fields.
pub fn deck_slider_u32(
    ui: &mut Ui,
    label: &str,
    tooltip: &str,
    base: &mut u32,
    eff: u32,
    auto: bool,
    r: std::ops::RangeInclusive<u32>,
) {
    let (lo, hi) = (*r.start(), *r.end());
    let shown = if auto { eff } else { *base };
    let frac = (shown - lo) as f32 / (hi - lo).max(1) as f32;
    deck_slider_geom(ui, label, tooltip, frac, |_ui, nf| {
        if !auto {
            *base = lo + (nf * (hi - lo) as f32).round() as u32;
        }
    });
    ui.response().on_hover_text(tooltip.to_string());
    let _ = eff;
}

/// Geometry + painting shared by both typed wrappers. `apply` gets the
/// 0..=1 fraction and writes the typed value back (only when interactive).
fn deck_slider_geom(
    ui: &mut Ui,
    label: &str,
    tooltip: &str,
    frac: f32,
    mut apply: impl FnMut(&mut Ui, f32),
) {
    let row = ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), 30.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            // label column (fixed share, keeps sliders aligned in a section)
            let lab = ui.painter().layout_no_wrap(
                label.to_owned(),
                egui::FontId::new(11.0, egui::FontFamily::Monospace),
                FG2,
            );
            let lab_w = 92.0f32.min(ui.available_width() * 0.34);
            ui.add_space(2.0);
            let (lrect, _) = ui.allocate_exact_size(Vec2::new(lab_w, 20.0), Sense::hover());
            ui.painter().galley(pos2(lrect.left(), lrect.center().y - lab.size().y / 2.0), lab, FG2);

            // track
            let track_h = 10.0;
            let tw = ui.available_width().min(320.0);
            let (trect, resp) = ui.allocate_exact_size(Vec2::new(tw, track_h), Sense::click_and_drag());
            let corners = round_rect(trect, track_h / 2.0);
            for c in &corners {
                ui.painter().rect_filled(*c, egui::Rounding::same(0.0), BG2);
            }
            // fill from left to frac
            let fill_w = trect.width() * frac.clamp(0.0, 1.0);
            let mut frect = trect;
            frect.set_width(fill_w.max(track_h)); // keep radius
            let fc = round_rect(frect, track_h / 2.0);
            for c in &fc {
                ui.painter().rect_filled(*c, egui::Rounding::same(0.0), ACCENT_DIM);
            }
            // ticks at ends
            ui.painter().vline(trect.left() + 0.5, trect.y_range(), egui::Stroke::new(1.0_f32, LINE));
            ui.painter().vline(trect.right() - 0.5, trect.y_range(), egui::Stroke::new(1.0_f32, LINE));

            // value readout: label carries the live number (typed callers
            // pass eff/base text); the track shows position only

            if resp.dragged() || resp.clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let nf = ((p.x - trect.left()) / trect.width()).clamp(0.0, 1.0);
                    apply(ui, nf);
                }
            }
            let _ = tooltip;
            // hover affordance
            if resp.hovered() {
                ui.painter().rect_stroke(trect, egui::Rounding::same(3.0), egui::Stroke::new(1.0_f32, FG3));
            }
        },
    );
    row.response.on_hover_text(tooltip.to_string());
    ui.end_row();
}

// ---------------------------------------------------------------------------
// toggle switch
// ---------------------------------------------------------------------------

/// A physical toggle switch: pill track + knob, green when on, with the
/// label left and the switch right. Click anywhere on the row.
pub fn toggle(ui: &mut Ui, label: &str, tooltip: &str, on: &mut bool) -> bool {
    let text = label.to_owned();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(240.0), 22.0), Sense::click());
    let painter = ui.painter_at(rect);
    // label
    let gal = painter.layout_no_wrap(text, egui::FontId::new(11.5, egui::FontFamily::Monospace), if resp.hovered() { FG } else { FG2 });
    painter.galley(pos2(rect.left(), rect.center().y - gal.size().y / 2.0), gal, FG2);
    // switch at the right edge
    let sw_w = 34.0;
    let sw_h = 16.0;
    let sw = Rect::from_min_max(pos2(rect.right() - sw_w, rect.center().y - sw_h / 2.0), pos2(rect.right(), rect.center().y + sw_h / 2.0));
    let (fill, knob) = if *on { (OK, Color32::from_rgb(7, 30, 18)) } else { (BG2, FG3) };
    for c in round_rect(sw, sw_h / 2.0) {
        painter.rect_filled(c, egui::Rounding::same(0.0), fill);
    }
    let kx = if *on { sw.right() - sw_h - 1.0 } else { sw.left() + 1.0 };
    let knob_r = Rect::from_min_max(pos2(kx, sw.top() + 1.0), pos2(kx + sw_h - 2.0, sw.bottom() - 1.0));
    for c in round_rect(knob_r, (sw_h - 2.0) / 2.0) {
        painter.rect_filled(c, egui::Rounding::same(0.0), knob);
    }
    let changed = resp.clicked();
    if changed {
        *on = !*on;
    }
    resp.on_hover_text(tooltip.to_string());
    changed
}

// ---------------------------------------------------------------------------
// chips + lamps
// ---------------------------------------------------------------------------

/// Small status chip: `● label` in the given color. Used in the top bar.
pub fn chip(ui: &mut Ui, text: &str, color: Color32, tooltip: &str) -> Response {
    let gal = ui.painter().layout_no_wrap(
        format!(" {text} "),
        egui::FontId::new(11.0, egui::FontFamily::Monospace),
        color,
    );
    let size = gal.size() + Vec2::new(10.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter_at(rect);
    let hovered = resp.hovered();
    for c in round_rect(rect, R_CTL) {
        p.rect_filled(c, egui::Rounding::same(0.0), if hovered { BG2 } else { BG1 });
    }
    p.rect_stroke(
        rect,
        egui::Rounding::same(R_CTL),
        egui::Stroke::new(
            1.0_f32,
            if hovered { color } else { color.gamma_multiply(0.55) },
        ),
    );
    p.galley(pos2(rect.left() + 5.0, rect.center().y - gal.size().y / 2.0), gal, color);
    resp.on_hover_text(tooltip.to_string())
}

/// Tiny square indicator lamp (canvas-drawn) with label beneath — used by
/// the data page. Returns the paint rect for tests.
pub fn lamp_cell(ui: &mut Ui, label: &str, on: bool, color: Color32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(54.0, 30.0), Sense::hover());
    let p = ui.painter_at(rect);
    let dot_r = 4.0;
    let dot_c = pos2(rect.left() + dot_r + 3.0, rect.center().y - 4.0);
    p.circle_filled(dot_c, dot_r, if on { color } else { BG2 });
    p.circle_stroke(dot_c, dot_r + 2.0, egui::Stroke::new(1.0_f32, if on { color.gamma_multiply(0.5) } else { LINE }));
    let gal = p.layout_no_wrap(label.to_owned(), egui::FontId::new(9.0, egui::FontFamily::Monospace), FG3);
    p.galley(pos2(rect.left(), rect.bottom() - gal.size().y), gal, FG3);
    rect
}

/// The auto-lock chip: `▣ AUTO` shown next to a widget the adaptive
/// controller owns. Rendered inline at the end of a widget row.
pub fn auto_chip(ui: &mut Ui) {
    let gal = ui.painter().layout_no_wrap(
        "AUTO".to_owned(),
        egui::FontId::new(9.0, egui::FontFamily::Monospace),
        ACCENT,
    );
    let (rect, _) = ui.allocate_exact_size(gal.size() + Vec2::new(8.0, 3.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_stroke(rect, egui::Rounding::same(3.0), egui::Stroke::new(1.0_f32, ACCENT_DIM));
    p.galley(pos2(rect.left() + 4.0, rect.center().y - gal.size().y / 2.0), gal, ACCENT);
    let _ = ui;
}

// ---------------------------------------------------------------------------
// arc gauge (drawn)
// ---------------------------------------------------------------------------

/// A quarter-to-half circle gauge for the data page: arc track, filled
/// arc for the value, big centered readout, caption under. Size-driven.
pub fn arc_gauge(ui: &mut Ui, value01: f32, caption: &str, readout: String, color: Color32) -> Rect {
    let size = Vec2::new(96.0, 74.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    let cx = rect.center().x;
    let cy = rect.bottom() - 12.0;
    let r = 34.0;
    // track: 240° sweep from 150° to 30°+360 (clockwise across the top)
    let a0 = 150.0_f32.to_radians();
    let a1 = 390.0_f32.to_radians();
    p.circle_stroke(pos2(cx, cy), r, egui::Stroke::new(5.0_f32, BG2));
    // value arc: stroke via many segments (epaint lacks arc strokes)
    let v = value01.clamp(0.0, 1.0);
    let steps = 48;
    for i in 0..steps {
        let t0 = a0 + (a1 - a0) * (i as f32 / steps as f32) * v;
        let t1 = a0 + (a1 - a0) * ((i + 1) as f32 / steps as f32) * v;
        let p0 = pos2(cx + r * t0.cos(), cy - r * t0.sin());
        let p1 = pos2(cx + r * t1.cos(), cy - r * t1.sin());
        p.line_segment([p0, p1], egui::Stroke::new(5.0_f32, color));
    }
    // readout text centered in the gauge
    let gal = p.layout_no_wrap(readout, egui::FontId::new(15.0, egui::FontFamily::Monospace), FG);
    p.galley(pos2(cx - gal.size().x / 2.0, cy - gal.size().y - 10.0), gal, FG);
    let cap = p.layout_no_wrap(caption.to_owned(), egui::FontId::new(9.0, egui::FontFamily::Monospace), FG3);
    p.galley(pos2(cx - cap.size().x / 2.0, rect.bottom() - cap.size().y), cap, FG3);
    rect
}

/// Big block-bar meter row for the data page: label, `▰▰▰▱▱` bar, pct.
pub fn meter_row(ui: &mut Ui, label: &str, v01: f32, color: Color32) {
    ui.horizontal(|ui| {
        let gal = ui.painter().layout_no_wrap(
            format!("{label:<7}"),
            egui::FontId::new(11.0, egui::FontFamily::Monospace),
            FG2,
        );
        let (r, _) = ui.allocate_exact_size(Vec2::new(64.0, 14.0), Sense::hover());
        ui.painter().galley(pos2(r.left(), r.center().y - gal.size().y / 2.0), gal, FG2);
        let bar = theme::meter(v01, 14);
        let bg = ui.painter().layout_no_wrap(bar, egui::FontId::new(12.0, egui::FontFamily::Monospace), color);
        let (r2, _) = ui.allocate_exact_size(Vec2::new(bg.size().x + 2.0, 14.0), Sense::hover());
        ui.painter().galley(pos2(r2.left(), r2.center().y - bg.size().y / 2.0), bg, color);
        let pct = format!("{:>3.0}%", v01 * 100.0);
        let pg = ui.painter().layout_no_wrap(pct, egui::FontId::new(11.0, egui::FontFamily::Monospace), FG3);
        let (r3, _) = ui.allocate_exact_size(Vec2::new(44.0, 14.0), Sense::hover());
        ui.painter().galley(pos2(r3.left(), r3.center().y - pg.size().y / 2.0), pg, FG3);
    });
    ui.end_row();
}

/// Plain labeled slider (advanced tuning rows): stock egui slider with a
/// small-caps label, keeps tooltips. No auto variant — advanced tuning
/// fields are always user-owned.
pub fn plain_u32(ui: &mut Ui, label: &str, tooltip: &str, v: &mut u32, r: std::ops::RangeInclusive<u32>) {
    ui.add(egui::Slider::new(v, r).text(label));
    ui.response().on_hover_text(tooltip.to_string());
}
pub fn plain_f32(ui: &mut Ui, label: &str, tooltip: &str, v: &mut f32, r: std::ops::RangeInclusive<f32>) {
    ui.add(egui::Slider::new(v, r).text(label));
    ui.response().on_hover_text(tooltip.to_string());
}

/// Horizontal label+value row used in HUD stat strips.
pub fn stat_row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        let gal = ui.painter().layout_no_wrap(
            format!("{label:<7}"),
            egui::FontId::new(11.0, egui::FontFamily::Monospace),
            FG3,
        );
        let (r, _) = ui.allocate_exact_size(Vec2::new(66.0, 14.0), Sense::hover());
        ui.painter().galley(pos2(r.left(), r.center().y - gal.size().y / 2.0), gal, FG3);
        let val = ui.painter().layout_no_wrap(value.to_owned(), egui::FontId::new(11.0, egui::FontFamily::Monospace), FG);
        let (r2, _) = ui.allocate_exact_size(Vec2::new(110.0, 14.0), Sense::hover());
        ui.painter().galley(pos2(r2.left(), r2.center().y - val.size().y / 2.0), val, FG);
    });
    ui.end_row();
}

/// A header row for panel sections: uppercase, tracked out, hairline rule
/// underneath, small caption right-aligned (optional).
pub fn section(ui: &mut Ui, title: &str, note: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let gal = ui.painter().layout_no_wrap(
            title.to_owned(),
            egui::FontId::new(10.5, egui::FontFamily::Monospace),
            FG2,
        );
        let (r, _) = ui.allocate_exact_size(Vec2::new(gal.size().x + 2.0, 14.0), Sense::hover());
        ui.painter().galley(pos2(r.left(), r.center().y - gal.size().y / 2.0), gal, FG2);
        if !note.is_empty() {
            let cap = ui.painter().layout_no_wrap(
                note.to_owned(),
                egui::FontId::new(9.5, egui::FontFamily::Monospace),
                FG3,
            );
            let (r2, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 14.0), Sense::hover());
            let x = r2.right() - cap.size().x;
            ui.painter().galley(pos2(x.max(r.right() + 8.0), r2.center().y - cap.size().y / 2.0), cap, FG3);
        }
    });
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, egui::Rounding::same(0.0), LINE);
    ui.add_space(4.0);
}

/// Ghost button (transparent, hairline border, accent on hover) used for
/// secondary actions.
pub fn ghost_button(ui: &mut Ui, text: &str) -> Response {
    let gal = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::new(12.0, egui::FontFamily::Monospace),
        FG,
    );
    let size = gal.size() + Vec2::new(20.0, 8.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter_at(rect);
    let hovered = resp.hovered();
    for c in round_rect(rect, R_CTL) {
        p.rect_filled(c, egui::Rounding::same(0.0), if hovered { BG2 } else { BG1 });
    }
    p.rect_stroke(rect, egui::Rounding::same(R_CTL), egui::Stroke::new(1.0_f32, if hovered { ACCENT } else { LINE }));
    p.galley(
        pos2(rect.center().x - gal.size().x / 2.0, rect.center().y - gal.size().y / 2.0),
        gal,
        if hovered { FG } else { FG2 },
    );
    resp
}

/// Primary button: accent fill (dark text) — used for "install update",
/// "save look" and transport actions.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let gal = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::new(12.0, egui::FontFamily::Monospace),
        BG0,
    );
    let size = gal.size() + Vec2::new(20.0, 8.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter_at(rect);
    let hovered = resp.hovered();
    for c in round_rect(rect, R_CTL) {
        p.rect_filled(c, egui::Rounding::same(0.0), if hovered { Color32::from_rgb(255, 80, 80) } else { ACCENT });
    }
    p.galley(
        pos2(rect.center().x - gal.size().x / 2.0, rect.center().y - gal.size().y / 2.0),
        gal,
        BG0,
    );
    resp
}

/// Toolbar square button (24px) with a text glyph, used in the top bar.
pub fn tool_button(ui: &mut Ui, glyph: &str, tooltip: &str, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(26.0, 22.0), Sense::click());
    let p = ui.painter_at(rect);
    let hovered = resp.hovered();
    for c in round_rect(rect, 4.0) {
        p.rect_filled(c, egui::Rounding::same(0.0), if active { ACCENT_DIM } else if hovered { BG2 } else { BG1 });
    }
    p.rect_stroke(rect, egui::Rounding::same(4.0), egui::Stroke::new(1.0_f32, if active { ACCENT } else { LINE }));
    let gal = p.layout_no_wrap(
        glyph.to_owned(),
        egui::FontId::new(11.0, egui::FontFamily::Monospace),
        if active { FG } else { FG2 },
    );
    p.galley(
        pos2(rect.center().x - gal.size().x / 2.0, rect.center().y - gal.size().y / 2.0),
        gal,
        if active { FG } else { FG2 },
    );
    resp.on_hover_text(tooltip.to_string())
}

// re-export Align2 use marker (keeps import list honest)
#[allow(dead_code)]
fn _a(_: Align2) {}

/// Compact preset square for the fps popover: label + accent fill when
/// it is the live limit, hover brightens. 30x22.
pub fn fps_square(ui: &mut Ui, label: &str, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(30.0, 22.0), Sense::click());
    let p = ui.painter_at(rect);
    let hovered = resp.hovered();
    for c in round_rect(rect, 4.0) {
        p.rect_filled(c, egui::Rounding::same(0.0), if active { ACCENT } else if hovered { BG2 } else { BG1 });
    }
    p.rect_stroke(rect, egui::Rounding::same(4.0), egui::Stroke::new(1.0_f32, if active { ACCENT } else { LINE }));
    let gal = p.layout_no_wrap(
        label.to_owned(),
        egui::FontId::new(11.0, egui::FontFamily::Monospace),
        if active { BG0 } else if hovered { FG } else { FG2 },
    );
    p.galley(
        pos2(rect.center().x - gal.size().x / 2.0, rect.center().y - gal.size().y / 2.0),
        gal,
        if active { BG0 } else if hovered { FG } else { FG2 },
    );
    resp
}

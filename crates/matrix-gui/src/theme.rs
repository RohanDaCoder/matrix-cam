//! Design tokens for the v2.5 "control deck" skin, and the egui `Visuals`
//! built from them.
//!
//! The palette is a deliberate inversion of the old red-on-black hacker
//! skin: the chrome around the preview stays quiet and dark (near-black
//! panels, warm gray text) so the preview — and ONE accent — carry the
//! color. The accent stays signal-red; it is the brand and the alert color
//! at once, which means every use of it must be sparse and meaningful:
//! the recording dot, active tabs, selected segments, alert rows.
//!
//! All other tokens exist so widget code never hardcodes a color: names
//! (not raw RGB) travel through the code, and changing the skin means
//! changing this file only.

use egui::{Color32, Visuals};

// ---------------------------------------------------------------------------
// palette
// ---------------------------------------------------------------------------

/// Brand / alert / "active" accent. Sparse by contract: LIVE dot, active
/// tab underline, selected segment fill, crash headers.
pub const ACCENT: Color32 = Color32::from_rgb(255, 47, 47);
/// Soft accent for glows and borders around active things.
pub const ACCENT_DIM: Color32 = Color32::from_rgb(122, 28, 28);
/// Healthy / LIVE state. The only green in the skin.
pub const OK: Color32 = Color32::from_rgb(61, 255, 138);
/// Warnings (paused, limited fps, pending update).
pub const WARN: Color32 = Color32::from_rgb(255, 176, 32);

/// Panel background: the deck surface.
pub const BG0: Color32 = Color32::from_rgb(7, 8, 10);
/// Raised surfaces: top bar, popups, one step above BG0.
pub const BG1: Color32 = Color32::from_rgb(13, 15, 18);
/// Inset surfaces: slider tracks, text fields, readouts.
pub const BG2: Color32 = Color32::from_rgb(20, 23, 27);
/// Hairline separators / widget borders.
pub const LINE: Color32 = Color32::from_rgb(33, 36, 41);

/// Primary text.
pub const FG: Color32 = Color32::from_rgb(210, 208, 200);
/// Secondary text (labels, units, hints).
pub const FG2: Color32 = Color32::from_rgb(130, 128, 120);
/// Disabled / ghost text.
pub const FG3: Color32 = Color32::from_rgb(84, 82, 76);

// ---------------------------------------------------------------------------
// geometry tokens (kept here so widgets + layout share one scale)
// ---------------------------------------------------------------------------

/// Interactive corner radius: segmented controls, buttons, chips.
pub const R_CTL: f32 = 5.0;
/// Card / panel corner radius.
pub const R_CARD: f32 = 8.0;

// ---------------------------------------------------------------------------
// egui wiring
// ---------------------------------------------------------------------------

/// The egui visuals for the skin: near-black chrome, hairline widgets,
/// accent-driven selection.
pub fn visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.panel_fill = BG0;
    v.window_fill = BG1;
    v.extreme_bg_color = BG0;
    v.override_text_color = Some(FG);
    v.faint_bg_color = BG2;
    v.hyperlink_color = OK;
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
        w.rounding = egui::Rounding::same(R_CTL);
        w.fg_stroke = egui::Stroke::new(1.0_f32, FG);
    }

    v
}

/// Install the bundled mono font (DejaVu Sans Mono — full coverage of the
/// block / box-drawing glyphs the HUD uses) as the UI font, with a compact
/// type scale for the deck chrome. Text styles are set through the Style
/// so both families resolve to the same face.
pub fn font_defs(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "deck".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/DejaVuSansMono.ttf")),
    );
    // primary for BOTH families: mono is the deck identity, and the
    // proportional family resolves here too so nothing falls back.
    for fam in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
        fonts.families.entry(fam).or_default().insert(0, "deck".into());
    }
    ctx.set_fonts(fonts);

    // type scale (all mono — the deck is instrument-panel by design):
    // captions 10.5 / body 13 / buttons 13 / headings 15
    ctx.style_mut(|st| {
        st.text_styles = [
            (egui::TextStyle::Small, egui::FontId::new(10.5, egui::FontFamily::Monospace)),
            (egui::TextStyle::Body, egui::FontId::new(13.0, egui::FontFamily::Monospace)),
            (egui::TextStyle::Button, egui::FontId::new(13.0, egui::FontFamily::Monospace)),
            (egui::TextStyle::Monospace, egui::FontId::new(13.0, egui::FontFamily::Monospace)),
            (egui::TextStyle::Heading, egui::FontId::new(15.0, egui::FontFamily::Monospace)),
        ]
        .into_iter()
        .collect();
    });
}

// ---------------------------------------------------------------------------
// text gizmos (drawn as text so they stay crisp at any dpi)
// ---------------------------------------------------------------------------

/// Horizontal meter made of blocks, e.g. `▰▰▰▱▱▱`. 0.0-1.0 in.
pub fn meter(v01: f32, width: usize) -> String {
    let n = ((v01.clamp(0.0, 1.0)) * width as f32).round() as usize;
    let mut s = String::with_capacity(width * 4);
    for i in 0..width {
        s.push(if i < n { '▰' } else { '▱' });
    }
    s
}

/// Unicode sparkline over a 64-bin histogram.
pub fn sparkline(hist: &[u32; 64]) -> String {
    const B: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = hist.iter().copied().max().unwrap_or(1).max(1);
    let mut s = String::new();
    for i in 0..32 {
        let v = hist[i * 2] + hist[i * 2 + 1];
        let f = (v as f32 / max as f32 * 7.0).round() as usize;
        s.push(B[f.min(7)]);
    }
    s
}

/// Vertical red/green "signal lamp" text: ● when on, ○ when off.
pub fn lamp(on: bool) -> &'static str {
    if on { "●" } else { "○" }
}

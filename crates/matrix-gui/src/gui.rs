//! matrix-cam v2.5 "control deck" front-end, shared by the Windows desktop
//! app and the web build.
//!
//! Redesigned from scratch around one idea: the app is an instrument
//! panel around a full-bleed preview, not a settings form next to a
//! video. Concretely:
//!
//! - **One chrome bar.** identity + telemetry chips on the left,
//!   transport (pause / panel / telemetry / issues) on the right. The old
//!   two-row top bar is gone; on phones the chips reflow into a second
//!   line only when they must.
//! - **The settings panel is a dock** with a tab rail (LOOK / COLOR / FX /
//!   CONTROL / DATA). Each tab is one screenful of related controls —
//!   the old 6-section single scroll is dead. The dock auto-hides on
//!   narrow screens and opens as an overlay sheet over the preview.
//! - **Everything shows state in place**: the LIVE/PAUSED lamp, fps and
//!   resolution chips, auto-locked widgets with AUTO chips, arc gauges
//!   and block meters on the DATA tab.
//! - **Crash reports are a modal** styled as an alert card stack — header
//!   strip, one card per report, actions at the bottom.
//!
//! Host seams are unchanged: `Store` (config persistence) and
//! `UpdaterHost` (release flow) are implemented by matrix-cli (file +
//! real updater) and matrix-web (localStorage + no-op). The constructor
//! signature `GuiApp::new(shared, store, updater)` and the
//! `style_ctx(ctx)` call are identical to v2.4, so both hosts run
//! unmodified.

use matrix_core::config::{AutoConfig, ColorMode, Config, GlyphSet, Settings, Style, UserPreset, PRESETS};
use matrix_core::report;
use matrix_core::state::{Inner, Shared};
use matrix_core::time::Duration;
use matrix_core::update::UpdateState;
use crate::theme::{self, ACCENT, ACCENT_DIM, BG0, BG1, BG2, FG, FG2, FG3, LINE, OK, WARN};
use crate::widgets;
use eframe::egui;
use std::sync::Arc;

/// Where the settings JSON lives. Implemented by the desktop (file) and the
/// web (localStorage) hosts.
pub trait Store: Send + Sync {
    fn load(&self) -> Config;
    fn save(&self, cfg: &Config);
}

/// How the running app talks to the release/update machinery.
pub trait UpdaterHost: Send + Sync {
    fn build_label(&self) -> String;
    fn check_now(&self, shared: Arc<Shared>);
    fn install(&self, shared: Arc<Shared>);
    fn can_install(&self) -> bool;
}

/// Dock tabs. One concept per tab; order mirrors the signal chain
/// (look → color → effects → control → data).
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Look,
    Color,
    Fx,
    Control,
    Data,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::Look, Tab::Color, Tab::Fx, Tab::Control, Tab::Data];
    fn label(self) -> &'static str {
        match self {
            Tab::Look => "LOOK",
            Tab::Color => "COLOR",
            Tab::Fx => "FX",
            Tab::Control => "CONTROL",
            Tab::Data => "DATA",
        }
    }
}

pub struct GuiApp {
    pub shared: Arc<Shared>,
    store: Arc<dyn Store>,
    updater: Arc<dyn UpdaterHost>,
    tex: Option<egui::TextureHandle>,
    tex_serial: u64,
    new_name: String,
    show_stats: bool,
    /// None until the first frame reveals the screen size; then
    /// Some(open). Narrow screens default closed (dock = overlay sheet).
    show_settings: Option<bool>,
    tab: Tab,
    show_crash: bool,
    crash_seen: bool,
}

impl GuiApp {
    pub fn new(shared: Arc<Shared>, store: Arc<dyn Store>, updater: Arc<dyn UpdaterHost>) -> Self {
        GuiApp {
            shared,
            store,
            updater,
            tex: None,
            tex_serial: 0,
            new_name: String::new(),
            show_stats: true,
            show_settings: None,
            tab: Tab::Look,
            show_crash: false,
            crash_seen: false,
        }
    }

    /// The shared egui frame setup: deck visuals + bundled mono font.
    pub fn style_ctx(ctx: &egui::Context) {
        ctx.set_visuals(theme::visuals());
        theme::font_defs(ctx);
    }

    fn save(&self) {
        let g = self.shared.inner.lock().unwrap();
        let mut c = self.store.load();
        c.set = Some(g.set.clone());
        c.auto = g.auto.clone();
        c.user_presets = g.user_presets.clone();
        self.store.save(&c);
    }
}

// ---------------------------------------------------------------------------
// the frame
// ---------------------------------------------------------------------------

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let repaint_ms = {
            let g = self.shared.inner.lock().unwrap();
            (1000 / g.cfg_preview_fps.max(5)) as u64
        };
        ctx.request_repaint_after(Duration::from_millis(repaint_ms));

        if self.show_settings.is_none() {
            let narrow = ctx.screen_rect().width() < 700.0;
            self.show_settings = Some(!narrow);
        }

        self.top_bar(ctx);
        self.crash_popup(ctx);

        let shared = self.shared.clone();
        let narrow = ctx.screen_rect().width() < 700.0;
        if self.show_settings == Some(true) {
            // The dock: fixed column on desktop, overlay sheet on phones.
            egui::SidePanel::right("dock")
                .resizable(true)
                .default_width(if narrow { 320.0 } else { 380.0 })
                .frame(egui::Frame::none().fill(BG0).inner_margin(egui::Margin::same(0.0)))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let mut g = shared.inner.lock().unwrap();
                        self.dock(ui, &mut g);
                    });
                });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG0).inner_margin(egui::Margin::same(0.0)))
            .show(ctx, |ui| {
                self.stage(ui);
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        Self::save(self);
        self.shared.inner.lock().unwrap().quit = true;
    }
}

impl GuiApp {
    // -------------------------------------------------------------------
    // top command bar
    // -------------------------------------------------------------------
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("bar")
            .frame(
                egui::Frame::none()
                    .fill(BG1)
                    .stroke(egui::Stroke::new(1.0_f32, LINE))
                    .inner_margin(egui::Margin::symmetric(12.0, 7.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    // identity: the glyph mark + wordmark
                    let mark = ui.painter().layout_no_wrap(
                        "▮▮".to_owned(),
                        egui::FontId::new(15.0, egui::FontFamily::Monospace),
                        ACCENT,
                    );
                    let (mr, _) = ui.allocate_exact_size(egui::vec2(mark.size().x + 2.0, 18.0), egui::Sense::hover());
                    ui.painter().galley(egui::pos2(mr.left(), mr.center().y - mark.size().y / 2.0), mark, ACCENT);
                    let word = ui.painter().layout_no_wrap(
                        "matrix-cam".to_owned(),
                        egui::FontId::new(14.0, egui::FontFamily::Monospace),
                        FG,
                    );
                    let (wr, _) = ui.allocate_exact_size(egui::vec2(word.size().x + 2.0, 18.0), egui::Sense::hover());
                    ui.painter().galley(egui::pos2(wr.left(), wr.center().y - word.size().y / 2.0), word, FG);
                    ui.spacing_mut().item_spacing.x = 6.0;

                    // telemetry chips (left cluster)
                    let (backend, w, h, fps, cam, err) = {
                        let g = self.shared.inner.lock().unwrap();
                        let s = &g.status;
                        (s.backend.clone(), s.out_w, s.out_h, s.fps, s.cam.clone(), s.error.clone())
                    };
                    widgets::chip(ui, &format!("{w}×{h}"), FG2, "Output resolution").on_hover_text("output size");
                    widgets::chip(ui, &format!("{fps:.1} fps"), if fps < 10.0 { WARN } else { FG2 }, "Measured preview frame rate");
                    let (paused, auto, retro) = {
                        let g = self.shared.inner.lock().unwrap();
                        (g.paused, g.auto.enabled, g.fps_limit.enabled)
                    };
                    // the state lamp: LIVE / PAUSED — the primary indicator
                    let lamp = if paused { "❚❚ PAUSED" } else { "● LIVE" };
                    let lc = if paused { WARN } else { OK };
                    let lr = widgets::chip(ui, lamp, lc, if paused { "Frame source halted" } else { "Preview streaming" });
                    if lr.clicked() {
                        let mut g = self.shared.inner.lock().unwrap();
                        g.paused = !g.paused;
                    }
                    if auto {
                        widgets::chip(ui, "AUTO", ACCENT, "Adaptive tuning is active");
                    }
                    if retro {
                        let fpsx = self.shared.inner.lock().unwrap().fps_limit.fps;
                        widgets::chip(ui, &format!("RETRO {fpsx}"), WARN, "Frame rate limited (retro look)");
                    }
                    if !err.is_empty() {
                        widgets::chip(ui, "ERR", ACCENT, &err);
                    }
                    let n = report::count();
                    if n > 0 {
                        let cr = widgets::chip(ui, &format!("⚠ {n}"), WARN, "Captured issues — click for the report");
                        if cr.clicked() {
                            self.show_crash = true;
                        }
                    }
                    widgets::chip(ui, &backend, FG3, "Rendering backend (the GUI always renders with glow)");
                    widgets::chip(ui, &cam, FG3, "Capture input");

                    // right cluster: transport buttons
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let dock_open = self.show_settings == Some(true);
                        if widgets::tool_button(ui, "▤", if dock_open { "hide the settings dock" } else { "open the settings dock" }, dock_open).clicked() {
                            self.show_settings = Some(!dock_open);
                        }
                        if widgets::tool_button(ui, "∿", "show / hide telemetry HUD on the preview", self.show_stats).clicked() {
                            self.show_stats = !self.show_stats;
                        }
                        if widgets::tool_button(ui, "❚❚", if paused { "resume the frame source" } else { "pause the frame source" }, paused).clicked() {
                            let mut g = self.shared.inner.lock().unwrap();
                            g.paused = !g.paused;
                        }
                    });
                });
            });
    }

    // -------------------------------------------------------------------
    // preview stage (central panel)
    // -------------------------------------------------------------------
    fn stage(&mut self, ui: &mut egui::Ui) {
        let view = { self.shared.inner.lock().unwrap().view.clone() };
        let stage = ui.available_rect_before_wrap();
        match view {
            Some(v) => {
                if self.tex_serial != v.serial || self.tex.is_none() {
                    let img = egui::ColorImage::from_rgb([v.w as usize, v.h as usize], &v.frame);
                    self.tex = Some(ui.ctx().load_texture("preview", img, egui::TextureOptions::LINEAR));
                    self.tex_serial = v.serial;
                }
                if let Some(t) = &self.tex {
                    let avail = stage.size();
                    let sc = (avail.x / v.w as f32).min(avail.y / v.h as f32).max(0.01);
                    let size = egui::vec2(v.w as f32 * sc, v.h as f32 * sc);
                    let origin = stage.min + ((avail - size) * 0.5).max(egui::Vec2::ZERO);
                    let rect = egui::Rect::from_min_size(origin, size);
                    // letterbox field is BG0 (blends with the deck); the frame
                    // itself gets a hairline ring so its edges read as cut
                    ui.painter().rect_stroke(rect, egui::Rounding::same(0.0), egui::Stroke::new(1.0_f32, LINE));
                    ui.painter().image(
                        t.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                if self.show_stats {
                    self.stage_hud(ui, stage, &v);
                }
            }
            None => {
                // boot state: quiet centered prompt, no chrome noise
                let gal = ui.painter().layout_no_wrap(
                    "· waiting for signal ·".to_owned(),
                    egui::FontId::new(13.0, egui::FontFamily::Monospace),
                    FG3,
                );
                let c = stage.center();
                ui.painter().galley(egui::pos2(c.x - gal.size().x / 2.0, c.y - gal.size().y / 2.0), gal, FG3);
            }
        }
    }

    /// Corner telemetry HUD drawn ON the preview stage (bottom-left): three
    /// quiet rows — cells/threads, mean/clip, detail/motion.
    fn stage_hud(&mut self, ui: &mut egui::Ui, stage: egui::Rect, _v: &matrix_core::state::View) {
        let (st, threads, cells) = {
            let g = self.shared.inner.lock().unwrap();
            let cells = g.stats.cells;
            (g.stats.clone(), g.status.threads, cells)
        };
        let p = ui.painter_at(stage);
        let lines = [
            format!("cells {cells:>6}  threads {threads}"),
            format!("mean  {:5.1}  clip {:.0}/{:.0}", st.mean, st.clip_low * 100.0, st.clip_high * 100.0),
            format!("det   {:3.0}%  mot {:3.0}%", st.detail * 100.0, st.motion * 100.0),
        ];
        let mut y = stage.bottom() - 8.0 - lines.len() as f32 * 14.0;
        // quiet backing plate so the HUD reads over bright frames
        let plate = egui::Rect::from_min_max(
            egui::pos2(stage.left() + 8.0, y - 4.0),
            egui::pos2(stage.left() + 8.0 + 220.0, stage.bottom() - 8.0),
        );
        p.rect_filled(plate, egui::Rounding::same(4.0), BG0.gamma_multiply(0.55));
        for l in lines {
            let gal = p.layout_no_wrap(l, egui::FontId::new(10.5, egui::FontFamily::Monospace), FG2);
            p.galley(egui::pos2(stage.left() + 12.0, y), gal, FG2);
            y += 14.0;
        }
    }

    // -------------------------------------------------------------------
    // the dock (settings)
    // -------------------------------------------------------------------
    fn dock(&mut self, ui: &mut egui::Ui, g: &mut Inner) {
        // tab rail: the five segments + the presets row live above the fold
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let mut new_tab = None;
            for (i, t) in Tab::ALL.iter().enumerate() {
                let sel = self.tab == *t;
                let gal = ui.painter().layout_no_wrap(
                    t.label().to_owned(),
                    egui::FontId::new(11.0, egui::FontFamily::Monospace),
                    if sel { BG0 } else { FG2 },
                );
                let size = egui::vec2(gal.size().x + 22.0, 26.0);
                let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                let p = ui.painter_at(rect);
                let hovered = resp.hovered();
                for c in widgets::round_rect_pub(rect, 0.0) {
                    p.rect_filled(c, egui::Rounding::same(0.0), if sel { ACCENT } else if hovered { BG2 } else { BG1 });
                }
                p.galley(
                    egui::pos2(rect.center().x - gal.size().x / 2.0, rect.center().y - gal.size().y / 2.0),
                    gal,
                    if sel { BG0 } else { FG2 },
                );
                if resp.clicked() {
                    new_tab = Some(*t);
                }
                let _ = i;
            }
            if let Some(t) = new_tab {
                self.tab = t;
            }
            ui.add_space(4.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::tool_button(ui, "⟲", "reset the current preset", false).clicked() {
                    let name = g.preset_name.clone();
                    let mirror = g.set.mirror;
                    g.set = Settings::preset(if PRESETS.contains(&name.as_str()) { &name } else { "ghost" });
                    g.set.mirror = mirror;
                }
                if widgets::tool_button(ui, "⤓", "save the current look as a preset", false).clicked() {
                    let mut c = self.store.load();
                    c.set = Some(g.set.clone());
                    c.auto = g.auto.clone();
                    c.user_presets = g.user_presets.clone();
                    self.store.save(&c);
                }
            });
        });
        ui.add_space(2.0);
        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().rect_filled(r, egui::Rounding::same(0.0), LINE);

        // presets row (always visible — switching looks is the most common
        // action and deserves zero clicks of navigation)
        ui.add_space(8.0);
        widgets::section(ui, "PRESETS", "built-in looks + your saved ones");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for p in PRESETS {
                let sel = g.preset_name == *p;
                let label = if sel { format!("▸ {p}") } else { p.to_string() };
                if widgets::ghost_button(ui, &label).clicked() {
                    let mirror = g.set.mirror;
                    g.set = Settings::preset(p);
                    g.set.mirror = mirror;
                    g.preset_name = p.to_string();
                }
            }
            for n in g.user_presets.iter().map(|p| p.name.clone()).collect::<Vec<_>>() {
                let sel = g.preset_name == n;
                let label = if sel { format!("▸ {n}") } else { n.clone() };
                if widgets::ghost_button(ui, &label).clicked() {
                    if let Some(p) = g.user_presets.iter().find(|p| p.name == n) {
                        g.set = p.set.clone();
                        g.preset_name = n.clone();
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            let mut name = self.new_name.clone();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .desired_width(140.0)
                    .hint_text("name this look"),
            );
            if resp.changed() {
                self.new_name = name;
            }
            if widgets::primary_button(ui, "+ SAVE LOOK").clicked() && !self.new_name.trim().is_empty() {
                let nm = self.new_name.trim().to_string();
                let set = g.eff.clone();
                g.user_presets.retain(|p| p.name != nm);
                if g.user_presets.len() >= 12 {
                    g.user_presets.remove(0);
                }
                g.user_presets.push(UserPreset { name: nm.clone(), set });
                g.preset_name = nm;
                self.new_name.clear();
            }
        });

        // body by tab
        ui.add_space(6.0);
        match self.tab {
            Tab::Look => self.tab_look(ui, g),
            Tab::Color => self.tab_color(ui, g),
            Tab::Fx => self.tab_fx(ui, g),
            Tab::Control => self.tab_control(ui, g),
            Tab::Data => self.tab_data(ui, g),
        }
    }

    // -------------------------------------------------------------------
    // LOOK tab
    // -------------------------------------------------------------------
    fn tab_look(&mut self, ui: &mut egui::Ui, g: &mut Inner) {
        widgets::section(ui, "SHAPE STYLE", "how each cell is drawn");
        let mut style = g.set.style;
        if widgets::segmented(ui, "style", &Style::ALL, |s| s.label().to_string(), &mut style).is_some() {
            g.set.style = style;
            g.preset_name = "custom".into();
        }

        widgets::section(ui, "CHARACTERS", "text style only");
        let mut glyphs = g.set.glyphs;
        if widgets::segmented(ui, "glyphs", &GlyphSet::ALL, |s| s.label().to_string(), &mut glyphs).is_some() {
            g.set.glyphs = glyphs;
            g.preset_name = "custom".into();
        }
        widgets::toggle(ui, "reverse ramp", "Flips the ramp: bright areas get the sparse glyphs.", &mut g.set.reverse);
        widgets::toggle(ui, "mirror", "Flip left-right, like a real mirror.", &mut g.set.mirror);

        widgets::section(ui, "RETRO", "one-click 8-bit look");
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, " APPLY 8-BIT ").clicked() {
                g.set.apply_retro();
                g.preset_name = "custom".into();
            }
            if widgets::ghost_button(ui, "retro fps limit").clicked() {
                g.fps_limit.enabled = true;
            }
        });

        let eff = g.eff.clone();
        let auto_density = g.auto.enabled && g.auto.density;
        widgets::deck_slider_u32(
            ui,
            "detail",
            "How many columns the image is split into. More = sharper but busier.",
            &mut g.set.cols,
            eff.cols,
            auto_density,
            40..=220,
        );
        if auto_density {
            widgets::auto_chip(ui);
        }
    }

    // -------------------------------------------------------------------
    // COLOR tab
    // -------------------------------------------------------------------
    fn tab_color(&mut self, ui: &mut egui::Ui, g: &mut Inner) {
        widgets::section(ui, "COLOR MODE", "how cells are painted");
        let mut mode = g.set.mode;
        if widgets::segmented(ui, "mode", &ColorMode::ALL, |m| m.label().to_string(), &mut mode).is_some() {
            g.set.mode = mode;
            g.preset_name = "custom".into();
        }
        let tips = [
            "One flat color for everything.",
            "Reddish areas pop in the accent color.",
            "Bright areas take the accent color.",
            "Blend between main color and accent.",
            "Keep the camera's real colors.",
        ];
        let _ = tips;

        widgets::section(ui, "PICTURE", "tone");
        let eff = g.eff.clone();
        let auto_levels = g.auto.enabled && g.auto.levels;
        widgets::deck_slider_i32(ui, "brightness", "Overall lightness.", &mut g.set.brightness, eff.brightness, auto_levels, -100..=100);
        if auto_levels {
            widgets::auto_chip(ui);
        }
        widgets::deck_slider_i32(ui, "contrast", "Distance between darkest and brightest parts.", &mut g.set.contrast, eff.contrast, auto_levels, -100..=100);
        if auto_levels {
            widgets::auto_chip(ui);
        }
        let auto_signal = g.auto.enabled && g.auto.signal;
        widgets::deck_slider_i32(ui, "accent amt", "How much of the image takes the accent color.", &mut g.set.threshold, eff.threshold, auto_signal, 0..=100);
        if auto_signal {
            widgets::auto_chip(ui);
        }
        let auto_polarity = g.auto.enabled && g.auto.polarity;
        ui.horizontal(|ui| {
            if auto_polarity {
                let mut shown = eff.invert;
                ui.add_enabled(false, egui::Checkbox::new(&mut shown, "invert"));
                widgets::auto_chip(ui);
            } else {
                widgets::toggle(ui, "invert", "Swap light and dark, like a film negative.", &mut g.set.invert);
            }
        });

        widgets::section(ui, "PALETTE", "main / background / accent");
        for (label, tip, slot) in [
            ("main", "Base color of the characters/dots.", 0),
            ("background", "Color of empty cells.", 1),
            ("accent", "Highlight color for hotspots.", 2),
        ] {
            let c: &mut [u8; 3] = match slot {
                0 => &mut g.set.ink,
                1 => &mut g.set.bg,
                _ => &mut g.set.accent,
            };
            let mut rgbf = [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0];
            ui.horizontal(|ui| {
                let gal = ui.painter().layout_no_wrap(
                    format!("{label:<11}"),
                    egui::FontId::new(11.0, egui::FontFamily::Monospace),
                    FG2,
                );
                let (r, _) = ui.allocate_exact_size(egui::vec2(92.0, 18.0), egui::Sense::hover());
                ui.painter().galley(egui::pos2(r.left(), r.center().y - gal.size().y / 2.0), gal, FG2);
                ui.color_edit_button_rgb(&mut rgbf).on_hover_text(tip.to_string());
            });
            *c = [
                (rgbf[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                (rgbf[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                (rgbf[2] * 255.0).round().clamp(0.0, 255.0) as u8,
            ];
        }
    }

    // -------------------------------------------------------------------
    // FX tab
    // -------------------------------------------------------------------
    fn tab_fx(&mut self, ui: &mut egui::Ui, g: &mut Inner) {
        widgets::section(ui, "GLOW & TRAILS", "");
        let eff = g.eff.clone();
        widgets::deck_slider_u32(ui, "glow", "Soft halo around bright cells.", &mut g.set.glow, eff.glow, false, 0..=24);
        let auto_fx = g.auto.enabled && g.auto.fx;
        widgets::deck_slider_u32(ui, "trails", "Previous frames fade out slowly, like phosphor.", &mut g.set.trails, eff.trails, auto_fx, 0..=90);
        if auto_fx {
            widgets::auto_chip(ui);
        }

        widgets::section(ui, "CRT", "scanlines / static / glitch");
        widgets::deck_slider_u32(ui, "scanlines", "Dark horizontal lines, CRT style.", &mut g.set.scan, eff.scan, false, 0..=100);
        widgets::deck_slider_u32(ui, "static", "Random speckles, like an old TV.", &mut g.set.noise, eff.noise, auto_fx, 0..=100);
        widgets::deck_slider_u32(ui, "glitch", "Random band shifts and color tears.", &mut g.set.glitch, eff.glitch, auto_fx, 0..=100);
    }

    // -------------------------------------------------------------------
    // CONTROL tab (auto + fps limit + updates)
    // -------------------------------------------------------------------
    fn tab_control(&mut self, ui: &mut egui::Ui, g: &mut Inner) {
        widgets::section(ui, "ADAPTIVE", "let matrix-cam tune itself");
        widgets::toggle(ui, "auto adjust", "Watch the picture and tune settings for you.", &mut g.auto.enabled);

        ui.horizontal(|ui| {
            ui.label("auto may own:");
            let _ = ui;
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            let mut fields = [
                ("levels", "Keeps the picture neither too dark nor too bright.", &mut g.auto.levels),
                ("detail", "More detail in busy scenes, calmer in simple ones.", &mut g.auto.density),
                ("accent", "Keeps only a small fraction of cells accented.", &mut g.auto.signal),
                ("glitch/fx", "Glitch and trails follow motion; noise fills dark scenes.", &mut g.auto.fx),
                ("invert", "Picks invert automatically so the most glyphs are visible.", &mut g.auto.polarity),
            ];
            for (label, tip, field) in &mut fields {
                widgets::toggle(ui, label, tip, field);
            }
        });

        ui.add_space(6.0);
        egui::CollapsingHeader::new("advanced tuning")
            .default_open(false)
            .id_salt("adv-auto")
            .show(ui, |ui| {
                let a: &mut AutoConfig = &mut g.auto;
                widgets::plain_f32(ui, "target mean", "Mean brightness auto aims for (0-255).", &mut a.target_mean, 40.0..=215.0);
                widgets::plain_f32(ui, "black point %", "Percentile treated as pure black.", &mut a.low_pct, 0.0..=20.0);
                widgets::plain_f32(ui, "white point %", "Percentile treated as pure white.", &mut a.high_pct, 80.0..=100.0);
                widgets::plain_u32(ui, "detail min", "Fewest columns auto will use.", &mut a.cols_min, 40..=200);
                widgets::plain_u32(ui, "detail max", "Most columns auto will use.", &mut a.cols_max, 60..=220);
                widgets::plain_f32(ui, "detail response", "How strongly scene detail drives columns.", &mut a.detail_gain, 0.2..=4.0);
                widgets::plain_f32(ui, "accent coverage", "Fraction of cells allowed to take accent.", &mut a.accent_coverage, 0.0..=0.35);
                widgets::plain_f32(ui, "motion → glitch", "Glitch amount at full motion.", &mut a.motion_glitch, 0.0..=100.0);
                widgets::plain_f32(ui, "motion → trails", "Trails amount at full motion.", &mut a.motion_trails, 0.0..=90.0);
                widgets::plain_f32(ui, "darkness → noise", "Noise shown in a fully dark scene.", &mut a.noise_floor, 0.0..=100.0);
                widgets::plain_f32(ui, "smoothing (ms)", "How fast auto values move; bigger = calmer.", &mut a.response_ms, 40.0..=3000.0);
                widgets::plain_u32(ui, "recompute every", "How often auto reconsiders targets (ms).", &mut a.interval_ms, 16..=1000);
            });

        // fps limiter
        widgets::section(ui, "FRAME LIMIT", "retro chop");
        ui.horizontal(|ui| {
            let mut on = g.fps_limit.enabled;
            if widgets::toggle(ui, "limit fps", "Cap the output frame rate for an 8-bit / retro feel.", &mut on) {
                g.fps_limit.enabled = on;
            }
            ui.add_enabled(
                g.fps_limit.enabled,
                egui::DragValue::new(&mut g.fps_limit.fps).range(1..=60).suffix(" fps").speed(1),
            )
            .on_hover_text("Type or drag a number, 1-60. Try 8-15 for pixel-game vibes.");
        });

        // updates
        widgets::section(ui, "UPDATES", "");
        ui.label(format!("this build: {}", self.updater.build_label()));
        let upd = g.update.clone();
        ui.monospace(upd.label());
        if self.updater.can_install() {
            match &upd {
                UpdateState::Available { version, notes, size } => {
                    ui.separator();
                    ui.label(format!("new version {version} ({:.1} MB) — release notes:", *size as f32 / 1_000_000.0));
                    egui::ScrollArea::vertical().id_salt("upd-notes").max_height(140.0).show(ui, |ui| {
                        ui.monospace(notes.trim());
                    });
                    if widgets::primary_button(ui, "⬇ DOWNLOAD & RESTART").clicked() {
                        let shared = self.shared.clone();
                        let host = self.updater.clone();
                        std::thread::spawn(move || host.install(shared));
                    }
                }
                UpdateState::Latest { .. } | UpdateState::NotChecked | UpdateState::Checking
                    if widgets::ghost_button(ui, "⟳ check now").clicked() =>
                {
                    g.update = UpdateState::Checking;
                    let shared = self.shared.clone();
                    let host = self.updater.clone();
                    std::thread::spawn(move || host.check_now(shared));
                }
                _ => {}
            }
            ui.checkbox(&mut g.cfg_update_on_start, "check on start")
                .on_hover_text("Look for a newer release each start (never auto-installs). Disable here or with --no-update-check.");
        }
        if widgets::ghost_button(ui, "reset auto to defaults").clicked() {
            g.auto = AutoConfig { enabled: g.auto.enabled, ..Default::default() };
        }
    }

    // -------------------------------------------------------------------
    // DATA tab (telemetry + auto readout)
    // -------------------------------------------------------------------
    fn tab_data(&mut self, ui: &mut egui::Ui, g: &mut Inner) {
        widgets::section(ui, "SIGNAL", "luma histogram (64 bins → 32 bars)");
        ui.monospace(theme::sparkline(&g.stats.hist));

        widgets::section(ui, "LEVELS", "");
        ui.horizontal(|ui| {
            widgets::arc_gauge(ui, g.stats.mean / 255.0, "mean", format!("{:5.1}", g.stats.mean), FG);
            widgets::arc_gauge(ui, g.stats.clip_high, "clip hi", format!("{:.0}%", g.stats.clip_high * 100.0), WARN);
            widgets::arc_gauge(ui, g.stats.clip_low, "clip lo", format!("{:.0}%", g.stats.clip_low * 100.0), WARN);
        });

        widgets::section(ui, "SCENE", "");
        widgets::meter_row(ui, "detail", g.stats.detail, OK);
        widgets::meter_row(ui, "motion", g.stats.motion, OK);
        widgets::meter_row(ui, "red dom", g.stats.red_frac, ACCENT);
        widgets::stat_row(ui, "cells", &format!("{}", g.stats.cells));
        widgets::stat_row(ui, "threads", &format!("{}", g.status.threads));

        let ro = g.readout.clone();
        if ro.active {
            widgets::section(ui, "AUTO", &ro.note);
            widgets::stat_row(ui, "black", &format!("{}", ro.black));
            widgets::stat_row(ui, "white", &format!("{}", ro.white));
        }
    }
}

// ---------------------------------------------------------------------------
// crash dialog
// ---------------------------------------------------------------------------

impl GuiApp {
    /// The crash / error dialog, redesigned as an alert stack: full-width
    /// header strip, one card per report, footer with copy / dismiss /
    /// clear. Auto-opens on the first report and re-opens for new ones.
    /// Returns `true` when drawn (tests assert on this).
    pub fn crash_popup(&mut self, ctx: &egui::Context) -> bool {
        if report::count() > 0 {
            if !self.show_crash && !self.crash_seen {
                self.show_crash = true;
            }
            self.crash_seen = true;
        } else if self.crash_seen {
            self.crash_seen = false;
            self.show_crash = false;
        }
        if !self.show_crash {
            return false;
        }
        let n = report::count();
        let mut dismiss = false;
        let mut clear_all = false;
        let mut copied = false;

        egui::Window::new("report")
            .title_bar(false)
            .collapsible(false)
            .resizable(true)
            .default_width(600.0)
            .default_height(440.0)
            .frame(egui::Frame::none().fill(BG1).stroke(egui::Stroke::new(1.0_f32, ACCENT_DIM)).inner_margin(egui::Margin::same(0.0)))
            .show(ctx, |ui| {
                // header strip
                let (hr, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::hover());
                let p = ui.painter_at(hr);
                p.rect_filled(hr, egui::Rounding::same(0.0), ACCENT_DIM);
                let t = p.layout_no_wrap(
                    format!("⚠ ISSUES CAPTURED — {n}"),
                    egui::FontId::new(13.0, egui::FontFamily::Monospace),
                    ACCENT,
                );
                p.galley(egui::pos2(hr.left() + 12.0, hr.center().y - t.size().y / 2.0), t, ACCENT);
                let sub = p.layout_no_wrap(
                    "everything needed to fix this is below — copy it into a GitHub issue".to_owned(),
                    egui::FontId::new(10.0, egui::FontFamily::Monospace),
                    FG2,
                );
                p.galley(egui::pos2(hr.right() - sub.size().x - 10.0, hr.center().y - sub.size().y / 2.0), sub, FG2);

                ui.add_space(4.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_space(6.0);
                    let reps = report::reports_snapshot();
                    let log = report::log_snapshot();
                    for (i, r) in reps.iter().enumerate() {
                        // one card per report
                        let (card, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width() - 16.0, 96.0),
                            egui::Sense::hover(),
                        );
                        let cp = ui.painter_at(card);
                        cp.rect_filled(card.shrink(4.0), egui::Rounding::same(6.0), BG0);
                        cp.rect_stroke(card.shrink(4.0), egui::Rounding::same(6.0), egui::Stroke::new(1.0_f32, LINE));
                        let kind_color = if r.kind == matrix_core::report::Kind::Warning { WARN } else { ACCENT };
                        let head = cp.layout_no_wrap(
                            format!("{} #{}", r.kind.label().to_uppercase(), i + 1),
                            egui::FontId::new(11.0, egui::FontFamily::Monospace),
                            kind_color,
                        );
                        cp.galley(egui::pos2(card.left() + 10.0, card.top() + 8.0), head, kind_color);
                        let msg = cp.layout_no_wrap(
                            r.message.clone(),
                            egui::FontId::new(11.5, egui::FontFamily::Monospace),
                            FG,
                        );
                        cp.galley(egui::pos2(card.left() + 10.0, card.top() + 26.0), msg, FG);
                        // stack preview: first 3 lines
                        let stack: Vec<&str> = r.stack.lines().take(3).collect();
                        let s = stack.join("\n");
                        let sg = cp.layout_no_wrap(
                            s,
                            egui::FontId::new(9.5, egui::FontFamily::Monospace),
                            FG3,
                        );
                        cp.galley(egui::pos2(card.left() + 10.0, card.top() + 44.0), sg, FG3);
                        ui.add_space(96.0);
                    }
                    if !log.is_empty() {
                        widgets::section(ui, "RECENT LOG", "");
                        ui.monospace(log.join("\n"));
                    }
                });

                // footer actions
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if widgets::primary_button(ui, "⧉ COPY REPORT").clicked() {
                        let text = report::full_text();
                        ui.ctx().copy_text(text);
                        copied = true;
                    }
                    if widgets::ghost_button(ui, "dismiss").clicked() {
                        dismiss = true;
                    }
                    if widgets::ghost_button(ui, "clear all").clicked() {
                        clear_all = true;
                    }
                });
            });

        let _ = copied;
        if dismiss {
            self.show_crash = false;
        }
        if clear_all {
            report::clear();
            self.show_crash = false;
            self.crash_seen = false;
        }
        true
    }
}

// keep unused-import lints quiet for tokens used only by some tabs
#[allow(dead_code)]
fn _unused() {
    let _ = (BG2, FG, FG2, FG3, LINE, OK, WARN, ACCENT, ACCENT_DIM, BG0, BG1);
}

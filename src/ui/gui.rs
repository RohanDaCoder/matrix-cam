//! egui front-end: live preview + renamed, grouped settings + auto panel + telemetry.

use crate::config::{AutoConfig, ColorMode, Config, GlyphSet, Settings, Style, UserPreset, PRESETS};
use crate::state::{Inner, Shared};
use crate::ui::theme::{self, DIMC, OK, RED, WARN};
use crate::ui::widgets;
use crate::updater::UpdateState;
use eframe::egui;
use egui::{Color32, RichText};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn run(shared: Arc<Shared>, cfg_path: Option<String>) -> anyhow::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1220.0, 780.0])
            .with_min_inner_size([940.0, 560.0])
            .with_title("matrix-cam"),
        ..Default::default()
    };
    let s2 = shared.clone();
    eframe::run_native(
        "matrix-cam",
        opts,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(theme::visuals());
            cc.egui_ctx.style_mut(|st| st.override_text_style = Some(egui::TextStyle::Monospace));
            Ok(Box::new(GuiApp::new(s2.clone(), cfg_path.clone())))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))?;
    shared.inner.lock().unwrap().quit = true;
    Ok(())
}

struct GuiApp {
    shared: Arc<Shared>,
    cfg_path: Option<String>,
    tex: Option<egui::TextureHandle>,
    tex_serial: u64,
    new_name: String,
    show_stats: bool,
}

impl GuiApp {
    fn new(shared: Arc<Shared>, cfg_path: Option<String>) -> Self {
        GuiApp { shared, cfg_path, tex: None, tex_serial: 0, new_name: String::new(), show_stats: true }
    }

    fn save(&self) {
        let g = self.shared.inner.lock().unwrap();
        let mut c = Config::load(self.cfg_path.as_deref());
        c.set = Some(g.set.clone());
        c.auto = g.auto.clone();
        c.user_presets = g.user_presets.clone();
        let _ = c.save(self.cfg_path.as_deref());
    }
}

/// Section header.
fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.colored_label(RED, RichText::new(title).size(14.0));
    ui.add_space(2.0);
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let repaint_ms = {
            let g = self.shared.inner.lock().unwrap();
            (1000 / g.cfg_preview_fps.max(5)) as u64
        };
        ctx.request_repaint_after(Duration::from_millis(repaint_ms));

        // ---------- top bar ----------
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(RED, RichText::new("▚ MATRIX_CAM").size(16.0));
                let g = self.shared.inner.lock().unwrap();
                ui.label(format!(
                    "{}  {}x{}@{}  {:.1} fps",
                    g.status.backend, g.status.out_w, g.status.out_h, g.status.out_fps, g.status.fps
                ));
                ui.separator();
                ui.colored_label(
                    if g.paused { WARN } else { OK },
                    if g.paused { "❚❚ PAUSED" } else { "● LIVE" },
                );
                if g.auto.enabled {
                    ui.colored_label(RED, "AUTO");
                }
                if g.fps_limit.enabled {
                    ui.colored_label(WARN, format!("RETRO {}fps", g.fps_limit.fps));
                }
                ui.separator();
                ui.checkbox(&mut { self.show_stats }, "telemetry");
            });
            ui.horizontal(|ui| {
                let mut g = self.shared.inner.lock().unwrap();
                if ui.button(if g.paused { "▶ resume" } else { "❚❚ pause" }).clicked() {
                    g.paused = !g.paused;
                }
                if ui.button("💾 save").clicked() {
                    let mut c = Config::load(self.cfg_path.as_deref());
                    c.set = Some(g.set.clone());
                    c.auto = g.auto.clone();
                    c.user_presets = g.user_presets.clone();
                    let _ = c.save(self.cfg_path.as_deref());
                }
                if ui.button("⟲ reset preset").clicked() {
                    let name = g.preset_name.clone();
                    let mirror = g.set.mirror;
                    g.set = Settings::preset(if PRESETS.contains(&name.as_str()) { &name } else { "ghost" });
                    g.set.mirror = mirror;
                }
                ui.separator();
                ui.colored_label(DIMC, format!("in: {}", g.status.cam));
                if !g.status.error.is_empty() {
                    ui.colored_label(RED, &g.status.error);
                }
            });
        });

        // ---------- settings ----------
        // Clone the Arc so the MutexGuard does not borrow `self` while the
        // panel builder needs `&mut self` for the preset-name text box.
        let shared = self.shared.clone();
        egui::SidePanel::right("settings")
            .resizable(true)
            .default_width(370.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut g = shared.inner.lock().unwrap();
                    self.settings_panel(ui, &mut g);
                });
            });

        // ---------- preview ----------
        egui::CentralPanel::default().show(ctx, |ui| {
            let view = { self.shared.inner.lock().unwrap().view.clone() };
            if let Some(v) = view {
                if self.tex_serial != v.serial || self.tex.is_none() {
                    let img = egui::ColorImage::from_rgb([v.w as usize, v.h as usize], &v.frame);
                    self.tex = Some(ctx.load_texture("preview", img, egui::TextureOptions::LINEAR));
                    self.tex_serial = v.serial;
                }
                if let Some(t) = &self.tex {
                    let avail = ui.available_size();
                    let sc = (avail.x / v.w as f32).min(avail.y / v.h as f32).max(0.01);
                    let size = egui::vec2(v.w as f32 * sc, v.h as f32 * sc);
                    let pad = ((avail - size) * 0.5).max(egui::Vec2::ZERO);
                    ui.add_space(pad.y);
                    ui.horizontal(|ui| {
                        ui.add_space(pad.x);
                        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                        ui.painter().image(
                            t.id(),
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    });
                }
            } else {
                ui.centered_and_justified(|ui| ui.colored_label(DIMC, "· waiting for signal ·"));
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        Self::save(self);
        self.shared.inner.lock().unwrap().quit = true;
    }
}

impl GuiApp {
    /// The redesigned settings panel (renamed + grouped + tooltips).
    fn settings_panel(&mut self, ui: &mut egui::Ui, g: &mut Inner) {
        let auto_on = g.auto.enabled;
        let eff = g.eff.clone();
        let auto_levels = auto_on && g.auto.levels;
        let auto_density = auto_on && g.auto.density;
        let auto_signal = auto_on && g.auto.signal;
        let auto_fx = auto_on && g.auto.fx;
        let auto_polarity = auto_on && g.auto.polarity;

        // ===== PRESETS =====
        section(ui, "PRESETS");
        ui.horizontal_wrapped(|ui| {
            for (i, p) in PRESETS.iter().enumerate() {
                if ui
                    .selectable_label(g.preset_name == *p, format!("{} {}", i + 1, p))
                    .on_hover_text("Built-in look. Number keys 1-5 switch too.")
                    .clicked()
                {
                    let mirror = g.set.mirror;
                    g.set = Settings::preset(p);
                    g.set.mirror = mirror;
                    g.preset_name = p.to_string();
                }
            }
        });
        if !g.user_presets.is_empty() {
            ui.horizontal_wrapped(|ui| {
                let names: Vec<String> = g.user_presets.iter().map(|p| p.name.clone()).collect();
                for n in names {
                    if ui.selectable_label(g.preset_name == n, &n).clicked() {
                        if let Some(p) = g.user_presets.iter().find(|p| p.name == n) {
                            g.set = p.set.clone();
                            g.preset_name = n.clone();
                        }
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.new_name);
            if ui.button("+ save look").clicked() && !self.new_name.trim().is_empty() {
                let name = self.new_name.trim().to_string();
                let set = g.eff.clone();
                g.user_presets.retain(|p| p.name != name);
                if g.user_presets.len() >= 12 {
                    g.user_presets.remove(0);
                }
                g.user_presets.push(UserPreset { name: name.clone(), set });
                g.preset_name = name;
                self.new_name.clear();
            }
        });

        // ===== LOOK (style + character settings) =====
        section(ui, "LOOK");
        ui.label("Shape style");
        ui.horizontal(|ui| {
            for s in Style::ALL {
                if ui
                    .selectable_label(g.set.style == s, s.label())
                    .on_hover_text("How each cell is drawn: dots, text glyphs or solid blocks.")
                    .clicked()
                {
                    g.set.style = s;
                    g.preset_name = "custom".into();
                }
            }
        });
        ui.horizontal(|ui| {
            if ui
                .button("🕹 8-bit retro")
                .on_hover_text("One-click 8-bit look: pixel blocks, halved detail, strong scanlines/static and phosphor trails. Turn on the retro FPS limiter below for the full effect.")
                .clicked()
            {
                g.set.apply_retro();
                g.preset_name = "custom".into();
            }
            if ui
                .button("⏱ retro FPS: on")
                .on_hover_text("Cap the output frame rate (12 fps default) for the choppy retro-game feel. Adjust the exact rate in the RETRO FPS section.")
                .clicked()
            {
                g.fps_limit.enabled = true;
            }
        });
        widgets::slider_u32(
            ui,
            "Detail",
            "How many columns the image is split into. More = sharper but busier.",
            &mut g.set.cols,
            eff.cols,
            auto_density,
            40..=220,
        );

        // ---- character settings ----
        ui.add_space(4.0);
        ui.label("Characters (text style)");
        egui::ComboBox::from_id_salt("glyphset")
            .selected_text(g.set.glyphs.label())
            .show_ui(ui, |ui| {
                for gs in GlyphSet::ALL {
                    ui.selectable_value(&mut g.set.glyphs, gs, gs.label())
                        .on_hover_text(format!("Ramp: {}", gs.ramp().trim()));
                }
            });
        ui.checkbox(&mut g.set.reverse, "Reverse character order")
            .on_hover_text("Flips the ramp: bright areas get the empty glyphs instead of dense ones.");
        ui.separator();

        // ===== PICTURE (tone) =====
        section(ui, "PICTURE");
        widgets::slider_i32(
            ui,
            "Brightness",
            "Overall lightness of the image.",
            &mut g.set.brightness,
            eff.brightness,
            auto_levels,
            -100..=100,
        );
        widgets::slider_i32(
            ui,
            "Contrast",
            "Distance between the darkest and brightest parts.",
            &mut g.set.contrast,
            eff.contrast,
            auto_levels,
            -100..=100,
        );
        widgets::slider_i32(
            ui,
            "Accent amount",
            "How much of the image is picked out in the accent color (hotspot / bright-goes-accent modes).",
            &mut g.set.threshold,
            eff.threshold,
            auto_signal,
            0..=100,
        );
        widgets::checkbox_auto(
            ui,
            "Invert",
            "Swap light and dark, like a film negative.",
            &mut g.set.invert,
            eff.invert,
            auto_polarity,
        );
        ui.checkbox(&mut g.set.mirror, "Mirror")
            .on_hover_text("Flip left-right, like a real mirror.");
        ui.separator();

        // ===== COLORS =====
        section(ui, "COLORS");
        egui::ComboBox::from_id_salt("colormode")
            .selected_text(g.set.mode.label())
            .show_ui(ui, |ui| {
                for (m, tip) in [
                    (ColorMode::Mono, "One flat color for everything."),
                    (ColorMode::Signal, "Reddish areas pop in the accent color."),
                    (ColorMode::Heat, "Bright areas take the accent color."),
                    (ColorMode::Duo, "Blend between main color and accent."),
                    (ColorMode::Color, "Keep the camera's real colors."),
                ] {
                    ui.selectable_value(&mut g.set.mode, m, m.label()).on_hover_text(tip);
                }
            });
        widgets::color_row(ui, "Main", "Base color of the characters/dots.", &mut g.set.ink);
        widgets::color_row(ui, "Background", "Color of empty cells.", &mut g.set.bg);
        widgets::color_row(ui, "Accent", "Highlight color for hotspots / bright areas.", &mut g.set.accent);
        ui.separator();

        // ===== EFFECTS =====
        section(ui, "EFFECTS");
        widgets::slider_u32(
            ui,
            "Glow",
            "Soft halo around bright cells.",
            &mut g.set.glow,
            eff.glow,
            false,
            0..=24,
        );
        widgets::slider_u32(
            ui,
            "Ghost trails",
            "Previous frames fade out slowly, like phosphor.",
            &mut g.set.trails,
            eff.trails,
            auto_fx,
            0..=90,
        );
        widgets::slider_u32(
            ui,
            "Scanlines",
            "Dark horizontal lines, CRT style.",
            &mut g.set.scan,
            eff.scan,
            false,
            0..=100,
        );
        widgets::slider_u32(
            ui,
            "Static / noise",
            "Random speckles, like an old TV.",
            &mut g.set.noise,
            eff.noise,
            auto_fx,
            0..=100,
        );
        widgets::slider_u32(
            ui,
            "Glitch",
            "Random band shifts and color tears.",
            &mut g.set.glitch,
            eff.glitch,
            auto_fx,
            0..=100,
        );

        // ===== RETRO FPS =====
        section(ui, "RETRO FPS");
        ui.horizontal(|ui| {
            ui.checkbox(&mut g.fps_limit.enabled, "Limit frame rate")
                .on_hover_text("Cap the output frame rate for an 8-bit / retro game feel.");
            let fps = &mut g.fps_limit.fps;
            ui.add_enabled(
                g.fps_limit.enabled,
                egui::DragValue::new(fps)
                    .range(1..=60)
                    .suffix(" fps")
                    .speed(1),
            )
            .on_hover_text("Type or drag a number, 1-60. Try 8-15 for pixel-game vibes.");
        });
        ui.separator();

        // ===== AUTO ADJUST =====
        section(ui, "AUTO ADJUST");
        ui.checkbox(&mut g.auto.enabled, "Enable auto adjust")
            .on_hover_text("Let matrix-cam watch the picture and tune settings for you.");
        ui.label("What auto may control:");
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut g.auto.levels, "Auto brightness")
                .on_hover_text("Keeps the picture neither too dark nor too bright.");
            ui.checkbox(&mut g.auto.density, "Auto detail")
                .on_hover_text("More detail in busy scenes, calmer in simple ones.");
            ui.checkbox(&mut g.auto.signal, "Auto accent")
                .on_hover_text("Keeps only a small fraction of cells accented.");
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut g.auto.fx, "Auto glitch/fx")
                .on_hover_text("Glitch and trails follow motion; noise fills dark scenes.");
            ui.checkbox(&mut g.auto.polarity, "Auto invert")
                .on_hover_text("Picks invert automatically so the most glyphs are visible.");
        });

        ui.collapsing("Advanced auto tuning", |ui| {
            let a: &mut AutoConfig = &mut g.auto;
            widgets::plain_f32(ui, "Target brightness", "Mean brightness auto aims for (0-255).", &mut a.target_mean, 40.0..=215.0);
            widgets::plain_f32(ui, "Black point %", "Percentile treated as pure black.", &mut a.low_pct, 0.0..=20.0);
            widgets::plain_f32(ui, "White point %", "Percentile treated as pure white.", &mut a.high_pct, 80.0..=100.0);
            widgets::plain_u32(ui, "Detail min", "Fewest columns auto will use.", &mut a.cols_min, 40..=200);
            widgets::plain_u32(ui, "Detail max", "Most columns auto will use.", &mut a.cols_max, 60..=220);
            widgets::plain_f32(ui, "Detail responsiveness", "How strongly scene detail drives the column count.", &mut a.detail_gain, 0.2..=4.0);
            widgets::plain_f32(ui, "Accent coverage", "Fraction of cells allowed to take the accent color.", &mut a.accent_coverage, 0.0..=0.35);
            widgets::plain_f32(ui, "Motion → glitch", "Glitch amount at full motion.", &mut a.motion_glitch, 0.0..=100.0);
            widgets::plain_f32(ui, "Motion → trails", "Trails amount at full motion.", &mut a.motion_trails, 0.0..=90.0);
            widgets::plain_f32(ui, "Darkness → noise", "Noise shown in a fully dark scene.", &mut a.noise_floor, 0.0..=100.0);
            widgets::plain_f32(ui, "Smoothing (ms)", "How fast auto values move; bigger = calmer.", &mut a.response_ms, 40.0..=3000.0);
            widgets::plain_u32(ui, "Recompute every (ms)", "How often auto reconsiders its targets.", &mut a.interval_ms, 16..=1000);
        });
        if ui.button("Reset auto to defaults").clicked() {
            g.auto = AutoConfig { enabled: g.auto.enabled, ..Default::default() };
        }

        // ===== UPDATES =====
        section(ui, "UPDATES");
        let upd = g.update.clone();
        ui.label(format!("this build: v{}", crate::updater::CURRENT));
        ui.monospace(upd.label());
        match &upd {
            UpdateState::Available { version, notes, size } => {
                ui.separator();
                ui.label(format!(
                    "new version {version} ({:.1} MB) — release notes:",
                    *size as f32 / 1_000_000.0
                ));
                egui::ScrollArea::vertical()
                    .id_salt("upd-notes")
                    .max_height(140.0)
                    .show(ui, |ui| ui.monospace(notes.trim()));
                if ui.button("⬇ download & restart").clicked() {
                    let shared = self.shared.clone();
                    std::thread::spawn(move || crate::updater::ui_install(shared));
                }
            }
            UpdateState::Latest { .. } | UpdateState::NotChecked | UpdateState::Checking => {
                if ui
                    .button("⟳ check now")
                    .on_hover_text("Ask GitHub for a newer matrix-cam release right now.")
                    .clicked()
                {
                    g.update = crate::updater::UpdateState::Checking;
                    let shared = self.shared.clone();
                    std::thread::spawn(move || {
                        crate::updater::spawn_check_now(shared);
                    });
                }
            }
            _ => {}
        }
        ui.checkbox(&mut g.cfg_update_on_start, "Check for updates on start")
            .on_hover_text("Look for a newer release each time matrix-cam starts (never auto-installs). Disable here or with --no-update-check.");

        // ===== TELEMETRY =====
        if self.show_stats {
            section(ui, "FEED TELEMETRY");
            let st = g.stats.clone();
            let ro = g.readout.clone();
            ui.monospace(theme::sparkline(&st.hist));
            ui.monospace(format!("mean {:5.1}  clip {:.0}%/{:.0}%", st.mean, st.clip_low * 100.0, st.clip_high * 100.0));
            ui.monospace(format!("detail {} {:3.0}%", theme::bar(st.detail, 12), st.detail * 100.0));
            ui.monospace(format!("motion {} {:3.0}%", theme::bar(st.motion, 12), st.motion * 100.0));
            ui.monospace(format!("red    {} {:3.0}%", theme::bar(st.red_frac, 12), st.red_frac * 100.0));
            if ro.active {
                ui.colored_label(OK, format!("auto: {}", ro.note));
                ui.monospace(format!("black {} → white {}", ro.black, ro.white));
            }
            ui.monospace(format!("cells {}  threads {}", st.cells, g.status.threads));
        }
    }
}

#[allow(dead_code)]
fn _mutex_import(_: &Mutex<Inner>) {}

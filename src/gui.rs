//! egui front-end: live preview + full settings + auto-mode panel + feed telemetry.
use crate::config::{AutoConfig, ColorMode, Config, Settings, Style, PRESETS};
use crate::Shared;
use egui::{Color32, RichText};
use std::sync::Arc;
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
            cc.egui_ctx.set_visuals(theme());
            cc.egui_ctx.style_mut(|st| st.override_text_style = Some(egui::TextStyle::Monospace));
            Ok(Box::new(GuiApp::new(s2.clone(), cfg_path.clone())))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))?;
    shared.inner.lock().unwrap().quit = true;
    Ok(())
}

fn theme() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(8, 9, 12);
    v.window_fill = Color32::from_rgb(12, 14, 18);
    v.extreme_bg_color = Color32::from_rgb(5, 6, 8);
    v.override_text_color = Some(Color32::from_rgb(201, 199, 191));
    v.selection.bg_fill = Color32::from_rgb(90, 20, 20);
    v.widgets.inactive.bg_fill = Color32::from_rgb(18, 21, 27);
    v.widgets.hovered.bg_fill = Color32::from_rgb(30, 24, 26);
    v.widgets.active.bg_fill = Color32::from_rgb(255, 47, 47);
    v
}

const RED: Color32 = Color32::from_rgb(255, 47, 47);
const DIMC: Color32 = Color32::from_rgb(143, 141, 133);
const OK: Color32 = Color32::from_rgb(61, 255, 138);

fn bar(v01: f32, width: usize) -> String {
    let n = ((v01.clamp(0.0, 1.0)) * width as f32).round() as usize;
    let mut s = String::with_capacity(width * 3);
    for _ in 0..n { s.push('▓'); }
    for _ in n..width { s.push('░'); }
    s
}
fn sparkline(hist: &[u32; 64]) -> String {
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

fn swatch(ui: &mut egui::Ui, c: [u8; 3]) {
    ui.colored_label(Color32::from_rgb(c[0], c[1], c[2]), "███");
}

fn color_row(ui: &mut egui::Ui, label: &str, c: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.label(format!("{label:<7}"));
        swatch(ui, *c);
        ui.add(egui::Slider::new(&mut c[0], 0..=255).text("r"));
    });
    ui.horizontal(|ui| {
        ui.label("       ");
        ui.add(egui::Slider::new(&mut c[1], 0..=255).text("g"));
        ui.add(egui::Slider::new(&mut c[2], 0..=255).text("b"));
    });
}

/// Slider that shows the live auto value (disabled) when auto owns the field.
fn s_i32(ui: &mut egui::Ui, label: &str, base: &mut i32, eff: i32, owned: bool, r: std::ops::RangeInclusive<i32>) {
    if owned {
        let mut shown = eff;
        ui.horizontal(|ui| {
            ui.add_enabled(false, egui::Slider::new(&mut shown, r).text(label));
            ui.colored_label(RED, "A");
        });
    } else {
        ui.add(egui::Slider::new(base, r).text(label));
    }
}
fn s_u32(ui: &mut egui::Ui, label: &str, base: &mut u32, eff: u32, owned: bool, r: std::ops::RangeInclusive<u32>) {
    if owned {
        let mut shown = eff;
        ui.horizontal(|ui| {
            ui.add_enabled(false, egui::Slider::new(&mut shown, r).text(label));
            ui.colored_label(RED, "A");
        });
    } else {
        ui.add(egui::Slider::new(base, r).text(label));
    }
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
                ui.label(format!("{}  {}x{}@{}  {:.1} fps", g.status.backend,
                                 g.status.out_w, g.status.out_h, g.status.out_fps, g.status.fps));
                ui.separator();
                ui.colored_label(if g.paused { Color32::from_rgb(255, 176, 32) } else { OK },
                                 if g.paused { "❚❚ PAUSED" } else { "● LIVE" });
                if g.auto.enabled { ui.colored_label(RED, "AUTO"); }
                ui.separator();
                ui.checkbox(&mut { self.show_stats }, "telemetry");
            });
            // re-do the two buttons with real effects (kept outside the borrow above)
            ui.horizontal(|ui| {
                let mut g = self.shared.inner.lock().unwrap();
                if ui.button(if g.paused { "▶ resume" } else { "❚❚ pause" }).clicked() { g.paused = !g.paused; }
                if ui.button("💾 save").clicked() {
                    let mut c = Config::load(self.cfg_path.as_deref());
                    c.set = Some(g.set.clone()); c.auto = g.auto.clone(); c.user_presets = g.user_presets.clone();
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
                if !g.status.error.is_empty() { ui.colored_label(RED, &g.status.error); }
            });
        });

        // ---------- settings ----------
        egui::SidePanel::right("settings").resizable(true).default_width(360.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut g = self.shared.inner.lock().unwrap();
                let auto_on = g.auto.enabled;
                let eff = g.eff.clone();
                let auto_levels = auto_on && g.auto.levels;
                let auto_density = auto_on && g.auto.density;
                let auto_signal = auto_on && g.auto.signal;
                let auto_fx = auto_on && g.auto.fx;

                ui.colored_label(RED, "PRESETS");
                ui.horizontal_wrapped(|ui| {
                    for (i, p) in PRESETS.iter().enumerate() {
                        if ui.selectable_label(g.preset_name == *p, format!("{} {}", i + 1, p)).clicked() {
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
                    if ui.button("+ save").clicked() && !self.new_name.trim().is_empty() {
                        let name = self.new_name.trim().to_string();
                        let set = g.eff.clone();
                        g.user_presets.retain(|p| p.name != name);
                        if g.user_presets.len() >= 12 { g.user_presets.remove(0); }
                        g.user_presets.push(crate::config::UserPreset { name: name.clone(), set });
                        g.preset_name = name;
                        self.new_name.clear();
                    }
                });
                ui.separator();

                ui.colored_label(RED, "RENDER");
                ui.horizontal(|ui| {
                    ui.label("style ");
                    for (s, n) in [(Style::Dots, "dots"), (Style::Ascii, "ascii"), (Style::Cells, "cells")] {
                        if ui.selectable_label(g.set.style == s, n).clicked() { g.set.style = s; g.preset_name = "custom".into(); }
                    }
                });
                s_u32(ui, "density", &mut g.set.cols, eff.cols, auto_density, 40..=220);
                ui.separator();

                ui.colored_label(RED, "TONE");
                s_i32(ui, "brightness", &mut g.set.brightness, eff.brightness, auto_levels, -100..=100);
                s_i32(ui, "contrast", &mut g.set.contrast, eff.contrast, auto_levels, -100..=100);
                s_i32(ui, "threshold", &mut g.set.threshold, eff.threshold, auto_signal, 0..=100);
                if auto_on && g.auto.polarity {
                    ui.horizontal(|ui| { ui.add_enabled(false, egui::Checkbox::new(&mut { eff.invert }, "invert")); ui.colored_label(RED, "A"); });
                } else {
                    ui.checkbox(&mut g.set.invert, "invert");
                }
                ui.checkbox(&mut g.set.mirror, "mirror");
                ui.separator();

                ui.colored_label(RED, "COLOR");
                egui::ComboBox::from_label("mode")
                    .selected_text(format!("{:?}", g.set.mode))
                    .show_ui(ui, |ui| {
                        for (m, n) in [(ColorMode::Mono, "mono"), (ColorMode::Signal, "red signal"),
                                       (ColorMode::Heat, "threshold"), (ColorMode::Duo, "duotone"),
                                       (ColorMode::Color, "full color")] {
                            ui.selectable_value(&mut g.set.mode, m, n);
                        }
                    });
                color_row(ui, "ink", &mut g.set.ink);
                color_row(ui, "paper", &mut g.set.bg);
                color_row(ui, "signal", &mut g.set.accent);
                ui.separator();

                ui.colored_label(RED, "FX");
                s_u32(ui, "glow", &mut g.set.glow, eff.glow, false, 0..=24);
                s_u32(ui, "trails", &mut g.set.trails, eff.trails, auto_fx, 0..=90);
                s_u32(ui, "scanlines", &mut g.set.scan, eff.scan, false, 0..=100);
                s_u32(ui, "noise", &mut g.set.noise, eff.noise, auto_fx, 0..=100);
                s_u32(ui, "glitch", &mut g.set.glitch, eff.glitch, auto_fx, 0..=100);
                ui.separator();

                // ---------- AUTO ----------
                ui.colored_label(RED, "AUTO ADJUST");
                ui.checkbox(&mut g.auto.enabled, "enable adaptive mode");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut g.auto.levels, "levels");
                    ui.checkbox(&mut g.auto.density, "density");
                    ui.checkbox(&mut g.auto.signal, "signal");
                });
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut g.auto.fx, "fx");
                    ui.checkbox(&mut g.auto.polarity, "polarity");
                });
                egui::Grid::new("autogrid").num_columns(2).show(ui, |ui| {
                    let a: &mut AutoConfig = &mut g.auto;
                    ui.label("target mean"); ui.add(egui::Slider::new(&mut a.target_mean, 40.0..=215.0)); ui.end_row();
                    ui.label("black pct");   ui.add(egui::Slider::new(&mut a.low_pct, 0.0..=20.0)); ui.end_row();
                    ui.label("white pct");   ui.add(egui::Slider::new(&mut a.high_pct, 80.0..=100.0)); ui.end_row();
                    ui.label("cols min");    ui.add(egui::Slider::new(&mut a.cols_min, 40..=200)); ui.end_row();
                    ui.label("cols max");    ui.add(egui::Slider::new(&mut a.cols_max, 60..=220)); ui.end_row();
                    ui.label("detail gain"); ui.add(egui::Slider::new(&mut a.detail_gain, 0.2..=4.0)); ui.end_row();
                    ui.label("accent %");    ui.add(egui::Slider::new(&mut a.accent_coverage, 0.0..=0.35)); ui.end_row();
                    ui.label("motion→glitch"); ui.add(egui::Slider::new(&mut a.motion_glitch, 0.0..=100.0)); ui.end_row();
                    ui.label("motion→trails"); ui.add(egui::Slider::new(&mut a.motion_trails, 0.0..=90.0)); ui.end_row();
                    ui.label("noise floor"); ui.add(egui::Slider::new(&mut a.noise_floor, 0.0..=100.0)); ui.end_row();
                    ui.label("response ms"); ui.add(egui::Slider::new(&mut a.response_ms, 40.0..=3000.0)); ui.end_row();
                    ui.label("interval ms"); ui.add(egui::Slider::new(&mut a.interval_ms, 16..=1000)); ui.end_row();
                });
                if ui.button("reset auto defaults").clicked() { g.auto = AutoConfig { enabled: g.auto.enabled, ..Default::default() }; }

                if self.show_stats {
                    ui.separator();
                    ui.colored_label(RED, "FEED TELEMETRY");
                    let st = g.stats.clone();
                    let ro = g.readout.clone();
                    ui.monospace(sparkline(&st.hist));
                    ui.monospace(format!("mean {:5.1}  clip {:.0}%/{:.0}%", st.mean, st.clip_low * 100.0, st.clip_high * 100.0));
                    ui.monospace(format!("detail {} {:3.0}%", bar(st.detail, 12), st.detail * 100.0));
                    ui.monospace(format!("motion {} {:3.0}%", bar(st.motion, 12), st.motion * 100.0));
                    ui.monospace(format!("red    {} {:3.0}%", bar(st.red_frac, 12), st.red_frac * 100.0));
                    if ro.active {
                        ui.colored_label(OK, format!("auto: {}", ro.note));
                        ui.monospace(format!("black {} → white {}", ro.black, ro.white));
                    }
                    ui.monospace(format!("cells {}  threads {}", st.cells, g.status.threads));
                }
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
                            t.id(), rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    });
                }
            } else {
                ui.centered_and_justified(|ui| { ui.colored_label(DIMC, "· waiting for signal ·"); });
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        Self::save(self);
        self.shared.inner.lock().unwrap().quit = true;
    }
}

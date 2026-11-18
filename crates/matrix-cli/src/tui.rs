//! Truecolor terminal front-end. Reuses the renderer's cell grid directly —
//! no second render pass. Row-level dirty tracking keeps writes minimal.

use matrix_core::config::{clampf, clampi, AutoConfig, ColorMode, GlyphSet, Settings, Style, Config, PRESETS};
use matrix_core::render::font;
use matrix_core::state::{Inner, Shared};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::style::Print;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Get = fn(&Settings, &Settings, &AutoConfig, &FpsLimitRef) -> String;
type Adj = fn(&mut Settings, &mut AutoConfig, &mut FpsLimitMut, i32);
struct Row {
    name: &'static str,
    hint: &'static str,
    get: Get,
    adj: Adj,
    auto: bool,
    head: bool,
}

/// Read-only view of the retro FPS limit for `get` closures.
pub struct FpsLimitRef<'a> {
    pub enabled: bool,
    pub fps: u32,
    _marker: std::marker::PhantomData<&'a ()>,
}

/// Mutable view of the retro FPS limit for `adj` closures.
pub struct FpsLimitMut<'a> {
    pub enabled: &'a mut bool,
    pub fps: &'a mut u32,
}

fn r(name: &'static str, hint: &'static str, get: Get, adj: Adj, auto: bool) -> Row {
    Row { name, hint, get, adj, auto, head: false }
}
fn head(name: &'static str) -> Row {
    Row { name, hint: "", get: |_, _, _, _| String::new(), adj: |_, _, _, _| {}, auto: false, head: true }
}

fn fps_ref<'a>(enabled: bool, fps: u32) -> FpsLimitRef<'a> {
    FpsLimitRef { enabled, fps, _marker: std::marker::PhantomData }
}

fn rows() -> Vec<Row> {
    vec![
        head("LOOK"),
        r("shape", "dots / text / blocks", |s, _, _, _| s.style.label().into(), |s, _, _, d| {
            s.style = match (s.style, d >= 0) {
                (Style::Dots, true) => Style::Ascii,
                (Style::Ascii, true) => Style::Cells,
                (Style::Cells, true) => Style::Dots,
                (Style::Dots, false) => Style::Cells,
                (Style::Ascii, false) => Style::Dots,
                (Style::Cells, false) => Style::Ascii,
            };
        }, false),
        r("detail", "columns 40-220", |_, e, _, _| format!("{}", e.cols), |s, _, _, d| s.cols = clampi(s.cols as i32 + d * 4, 40, 220) as u32, true),
        r("characters", "glyph set for text style", |s, _, _, _| s.glyphs.label().into(), |s, _, _, d| {
            let i = GlyphSet::ALL.iter().position(|g| *g == s.glyphs).unwrap_or(0);
            let n = GlyphSet::ALL.len();
            s.glyphs = if d >= 0 { GlyphSet::ALL[(i + 1) % n] } else { GlyphSet::ALL[(i + n - 1) % n] };
        }, false),
        r("reverse chars", "flip the glyph ramp", |s, _, _, _| format!("{}", s.reverse), |s, _, _, _| s.reverse = !s.reverse, false),
        head("PICTURE"),
        r("brightness", "-100..100", |_, e, _, _| format!("{:+}", e.brightness), |s, _, _, d| s.brightness = clampi(s.brightness + d * 5, -100, 100), true),
        r("contrast", "-100..100", |_, e, _, _| format!("{:+}", e.contrast), |s, _, _, d| s.contrast = clampi(s.contrast + d * 5, -100, 100), true),
        r("accent amount", "0..100", |_, e, _, _| format!("{}", e.threshold), |s, _, _, d| s.threshold = clampi(s.threshold + d * 5, 0, 100), true),
        r("invert", "film negative", |s, _, _, _| format!("{}", s.invert), |s, _, _, _| s.invert = !s.invert, false),
        r("mirror", "flip left-right", |s, _, _, _| format!("{}", s.mirror), |s, _, _, _| s.mirror = !s.mirror, false),
        head("COLORS"),
        r("color mode", "single / hotspots / ...", |s, _, _, _| s.mode.label().into(), |s, _, _, d| {
            s.mode = match (s.mode, d >= 0) {
                (ColorMode::Mono, true) => ColorMode::Signal,
                (ColorMode::Signal, true) => ColorMode::Heat,
                (ColorMode::Heat, true) => ColorMode::Duo,
                (ColorMode::Duo, true) => ColorMode::Color,
                (ColorMode::Color, true) => ColorMode::Mono,
                (ColorMode::Mono, false) => ColorMode::Color,
                (ColorMode::Signal, false) => ColorMode::Mono,
                (ColorMode::Heat, false) => ColorMode::Signal,
                (ColorMode::Duo, false) => ColorMode::Heat,
                (ColorMode::Color, false) => ColorMode::Duo,
            };
        }, false),
        r("main color", "base color", |s, _, _, _| hex(s.ink), |s, _, _, d| tint(&mut s.ink, d), false),
        r("background", "empty-cell color", |s, _, _, _| hex(s.bg), |s, _, _, d| tint(&mut s.bg, d), false),
        r("accent color", "highlight color", |s, _, _, _| hex(s.accent), |s, _, _, d| tint(&mut s.accent, d), false),
        head("EFFECTS"),
        r("glow", "halo 0-24", |s, _, _, _| format!("{}", s.glow), |s, _, _, d| s.glow = clampi(s.glow as i32 + d * 2, 0, 24) as u32, false),
        r("ghost trails", "phosphor fade 0-90", |_, e, _, _| format!("{}", e.trails), |s, _, _, d| s.trails = clampi(s.trails as i32 + d * 5, 0, 90) as u32, true),
        r("scanlines", "CRT lines 0-100", |s, _, _, _| format!("{}", s.scan), |s, _, _, d| s.scan = clampi(s.scan as i32 + d * 5, 0, 100) as u32, false),
        r("static", "noise 0-100", |_, e, _, _| format!("{}", e.noise), |s, _, _, d| s.noise = clampi(s.noise as i32 + d * 5, 0, 100) as u32, true),
        r("glitch", "band tears 0-100", |_, e, _, _| format!("{}", e.glitch), |s, _, _, d| s.glitch = clampi(s.glitch as i32 + d * 5, 0, 100) as u32, true),
        head("RETRO FPS"),
        r("limit fps", "cap frame rate", |_, _, _, f| format!("{}", f.enabled), |_, _, f, _| *f.enabled = !*f.enabled, false),
        r("fps value", "1-60", |_, _, _, f| format!("{}", f.fps), |_, _, f, d| *f.fps = clampi(*f.fps as i32 + d, 1, 60) as u32, false),
        head("AUTO"),
        r("auto adjust", "master switch", |_, _, a, _| format!("{}", a.enabled), |_, a, _, _| a.enabled = !a.enabled, false),
        r("auto brightness", "levels", |_, _, a, _| format!("{}", a.levels), |_, a, _, _| a.levels = !a.levels, false),
        r("auto detail", "density", |_, _, a, _| format!("{}", a.density), |_, a, _, _| a.density = !a.density, false),
        r("auto accent", "signal", |_, _, a, _| format!("{}", a.signal), |_, a, _, _| a.signal = !a.signal, false),
        r("auto glitch/fx", "fx", |_, _, a, _| format!("{}", a.fx), |_, a, _, _| a.fx = !a.fx, false),
        r("auto invert", "polarity", |_, _, a, _| format!("{}", a.polarity), |_, a, _, _| a.polarity = !a.polarity, false),
        r("target brightness", "0-255", |_, _, a, _| format!("{:.0}", a.target_mean), |_, a, _, d| a.target_mean = clampf(a.target_mean + d as f32 * 5.0, 40.0, 215.0), false),
        r("black point %", "0-20", |_, _, a, _| format!("{:.1}", a.low_pct), |_, a, _, d| a.low_pct = clampf(a.low_pct + d as f32, 0.0, 20.0), false),
        r("white point %", "80-100", |_, _, a, _| format!("{:.1}", a.high_pct), |_, a, _, d| a.high_pct = clampf(a.high_pct + d as f32, 80.0, 100.0), false),
        r("detail min", "columns", |_, _, a, _| format!("{}", a.cols_min), |_, a, _, d| a.cols_min = clampi(a.cols_min as i32 + d * 5, 40, 200) as u32, false),
        r("detail max", "columns", |_, _, a, _| format!("{}", a.cols_max), |_, a, _, d| a.cols_max = clampi(a.cols_max as i32 + d * 5, 60, 220) as u32, false),
        r("detail resp.", "gain 0.2-4", |_, _, a, _| format!("{:.2}", a.detail_gain), |_, a, _, d| a.detail_gain = clampf(a.detail_gain + d as f32 * 0.1, 0.2, 4.0), false),
        r("accent coverage", "fraction", |_, _, a, _| format!("{:.1}", a.accent_coverage * 100.0), |_, a, _, d| a.accent_coverage = clampf(a.accent_coverage + d as f32 * 0.01, 0.0, 0.35), false),
        r("motion glitch", "0-100", |_, _, a, _| format!("{:.0}", a.motion_glitch), |_, a, _, d| a.motion_glitch = clampf(a.motion_glitch + d as f32 * 5.0, 0.0, 100.0), false),
        r("motion trails", "0-90", |_, _, a, _| format!("{:.0}", a.motion_trails), |_, a, _, d| a.motion_trails = clampf(a.motion_trails + d as f32 * 5.0, 0.0, 90.0), false),
        r("dark noise", "0-100", |_, _, a, _| format!("{:.0}", a.noise_floor), |_, a, _, d| a.noise_floor = clampf(a.noise_floor + d as f32 * 5.0, 0.0, 100.0), false),
        r("smoothing ms", "40-3000", |_, _, a, _| format!("{:.0}", a.response_ms), |_, a, _, d| a.response_ms = clampf(a.response_ms + d as f32 * 50.0, 40.0, 3000.0), false),
        r("recompute ms", "16-1000", |_, _, a, _| format!("{}", a.interval_ms), |_, a, _, d| a.interval_ms = clampi(a.interval_ms as i32 + d * 20, 16, 1000) as u32, false),
    ]
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}
fn tint(c: &mut [u8; 3], d: i32) {
    for k in 0..3 {
        c[k] = clampi(c[k] as i32 + d * 8, 0, 255) as u8;
    }
}

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        let mut o = std::io::stdout();
        let _ = execute!(o, Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

pub fn run(shared: Arc<Shared>, cfg_path: Option<String>) -> anyhow::Result<()> {
    let mut out = BufWriter::with_capacity(1 << 18, std::io::stdout());
    enable_raw_mode()?;
    execute!(out, EnterAlternateScreen, Hide)?;
    let _g = Guard;

    let table = rows();
    let mut sel: usize = 1;
    let mut panel = true;
    let mut prev_sig: Vec<u64> = Vec::new();
    let mut last_size = (0u16, 0u16);
    let mut force = true;
    let mut last = Instant::now();

    loop {
        let (fps, quit) = {
            let g = shared.inner.lock().unwrap();
            (g.cfg_tui_fps.max(5), g.quit)
        };
        if quit {
            break;
        }
        let frame_time = Duration::from_micros(1_000_000 / fps as u64);
        let wait = frame_time.saturating_sub(last.elapsed());
        if event::poll(wait)? {
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                let mut g = shared.inner.lock().unwrap();
                let mut dirty = false;
                match k.code {
                    KeyCode::Char('q') => g.quit = true,
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => g.quit = true,
                    KeyCode::Esc => g.quit = true,
                    KeyCode::Tab => {
                        panel = !panel;
                        force = true;
                    }
                    KeyCode::Up => {
                        sel = sel.saturating_sub(1);
                        if table[sel].head {
                            sel = sel.saturating_sub(1);
                        }
                    }
                    KeyCode::Down => {
                        sel = (sel + 1).min(table.len() - 1);
                        if table[sel].head {
                            sel = (sel + 1).min(table.len() - 1);
                        }
                    }
                    KeyCode::Left => {
                        let inner = &mut *g;
                        let fps = &mut inner.fps_limit;
                        (table[sel].adj)(&mut inner.set, &mut inner.auto, &mut FpsLimitMut { enabled: &mut fps.enabled, fps: &mut fps.fps }, -1);
                        dirty = true;
                    }
                    KeyCode::Right => {
                        let inner = &mut *g;
                        let fps = &mut inner.fps_limit;
                        (table[sel].adj)(&mut inner.set, &mut inner.auto, &mut FpsLimitMut { enabled: &mut fps.enabled, fps: &mut fps.fps }, 1);
                        dirty = true;
                    }
                    KeyCode::Char('p') => g.paused = !g.paused,
                    KeyCode::Char('a') => g.auto.enabled = !g.auto.enabled,
                    KeyCode::Char('m') => g.set.mirror = !g.set.mirror,
                    KeyCode::Char('i') => g.set.invert = !g.set.invert,
                    KeyCode::Char('f') => g.fps_limit.enabled = !g.fps_limit.enabled,
                    KeyCode::Char('r') => {
                        g.set.apply_retro();
                        g.preset_name = "custom".into();
                        force = true;
                    }
                    KeyCode::Char('u') => {
                        g.update = matrix_core::update::UpdateState::Checking;
                        let shared2 = shared.clone();
                        drop(g);
                        crate::updater::spawn_check_now(shared2);
                        continue;
                    }
                    KeyCode::Char('U') => {
                        let shared2 = shared.clone();
                        drop(g);
                        crate::updater::ui_install(shared2); // may restart the process
                        continue;
                    }
                    KeyCode::Char(d @ '1'..='5') => {
                        let name = PRESETS[(d as u8 - b'1') as usize];
                        let mirror = g.set.mirror;
                        g.set = Settings::preset(name);
                        g.set.mirror = mirror;
                        g.preset_name = name.into();
                        force = true;
                    }
                    KeyCode::Char(d @ '6'..='9') => {
                        let i = (d as u8 - b'6') as usize;
                        if let Some(p) = g.user_presets.get(i).cloned() {
                            g.set = p.set;
                            g.preset_name = p.name;
                            force = true;
                        }
                    }
                    KeyCode::Char('W') => {
                        let name = format!("user{}", g.user_presets.len() + 1);
                        let up = matrix_core::config::UserPreset { name: name.clone(), set: g.eff.clone() };
                        g.user_presets.push(up);
                        g.preset_name = name;
                    }
                    KeyCode::Char('w') => {
                        let mut c = Config::load(cfg_path.as_deref());
                        c.set = Some(g.set.clone());
                        c.auto = g.auto.clone();
                        c.user_presets = g.user_presets.clone();
                        let _ = c.save(cfg_path.as_deref());
                    }
                    _ => {}
                }
                if dirty {
                    g.preset_name = "custom".into();
                }
            }
        }
        last = Instant::now();

        let (cols, rows_n) = crossterm::terminal::size()?;
        if (cols, rows_n) != last_size {
            last_size = (cols, rows_n);
            force = true;
            prev_sig.clear();
        }
        if cols < 40 || rows_n < 12 {
            queue!(out, MoveTo(0, 0), Print("terminal too small — resize\r\n"))?;
            out.flush()?;
            continue;
        }
        let panel_w: u16 = if panel { 34 } else { 0 };
        let pw = cols.saturating_sub(panel_w + 1);
        let ph = rows_n.saturating_sub(2);

        let (view, set, eff, auto, stats, ro, status, paused, preset, fps_enabled, fps_val, upd) = {
            let g = shared.inner.lock().unwrap();
            (
                g.view.clone(),
                g.set.clone(),
                g.eff.clone(),
                g.auto.clone(),
                g.stats.clone(),
                g.readout.clone(),
                g.status.clone(),
                g.paused,
                g.preset_name.clone(),
                g.fps_limit.enabled,
                g.fps_limit.fps,
                g.update.clone(),
            )
        };

        // ---- preview from the shared cell grid ----
        if let Some(v) = view {
            let g = &v.grid;
            if g.w > 0 && g.h > 0 {
                if prev_sig.len() != ph as usize {
                    prev_sig = vec![0; ph as usize];
                    force = true;
                }
                let ascii = set.style == Style::Ascii;
                let ramp = font::ramp(set.glyphs);
                let mut line = String::with_capacity(pw as usize * 24);
                for ty in 0..ph {
                    let gy = (ty as usize * g.h / ph as usize).min(g.h - 1);
                    let mut sig: u64 = 1469598103934665603;
                    let mut last_col = [255u8, 255, 255];
                    let mut have_col = false;
                    line.clear();
                    for tx in 0..pw {
                        let gx = (tx as usize * g.w / pw as usize).min(g.w - 1);
                        let ci = gy * g.w + gx;
                        let idx = g.idx[ci] as usize;
                        let col = g.col[ci];
                        let ch = if ascii {
                            let i = if set.reverse { ramp.len() - 1 - idx.min(ramp.len() - 1) } else { idx.min(ramp.len() - 1) };
                            ramp[i] as char
                        } else {
                            let lv = if set.style == Style::Dots { idx * 5 / 32 } else { idx * 5 / 16 };
                            *font::SHADES.get(lv.min(4)).unwrap_or(&' ')
                        };
                        sig = sig
                            .wrapping_mul(1099511628211)
                            ^ (idx as u64)
                            ^ ((col[0] as u64) << 16)
                            ^ ((col[1] as u64) << 8)
                            ^ col[2] as u64;
                        if !have_col || col != last_col {
                            line.push_str(&format!("\x1b[38;2;{};{};{}m", col[0], col[1], col[2]));
                            last_col = col;
                            have_col = true;
                        }
                        line.push(ch);
                    }
                    let si = ty as usize;
                    if force || prev_sig[si] != sig {
                        prev_sig[si] = sig;
                        queue!(out, MoveTo(0, ty), Print(&line))?;
                    }
                }
            }
        }

        // ---- settings panel ----
        if panel {
            let x = cols - panel_w;
            let mut y = 0u16;
            let put = |out: &mut BufWriter<std::io::Stdout>, y: &mut u16, s: String| -> anyhow::Result<()> {
                if *y < rows_n {
                    queue!(out, MoveTo(x, *y), Print(format!("\x1b[0m{:<w$}", s, w = panel_w as usize)))?;
                    *y += 1;
                }
                Ok(())
            };
            put(&mut out, &mut y, format!("\x1b[38;2;255;47;47m▚ MATRIX_CAM  {}", preset))?;
            put(&mut out, &mut y, format!("{} {}x{} {:.0}fps", status.backend, status.out_w, status.out_h, status.fps))?;
            let mut state = if paused { "❚❚ paused".into() } else { "● live".into() };
            if fps_enabled {
                state = format!("{}  retro {}fps", state, fps_val);
            }
            put(&mut out, &mut y, state)?;
            if !matches!(upd, matrix_core::update::UpdateState::NotChecked) {
                put(&mut out, &mut y, upd.label())?;
            }
            let scroll = sel.saturating_sub((rows_n as usize).saturating_sub(10));
            for (i, row) in table.iter().enumerate().skip(scroll) {
                if y >= rows_n - 1 {
                    break;
                }
                if row.head {
                    put(&mut out, &mut y, format!("\x1b[38;2;255;47;47m── {} ──", row.name))?;
                } else {
                    let fr = fps_ref(fps_enabled, fps_val);
                    let val = (row.get)(&set, &eff, &auto, &fr);
                    let mark = if row.auto && auto.enabled { "A" } else { " " };
                    let sel_mark = if i == sel { "\x1b[7m" } else { "" };
                    put(&mut out, &mut y, format!("{}{:<14}{:>15} {}{}", sel_mark, row.name, val, mark, if i == sel { "\x1b[0m" } else { "" }))?;
                }
            }
            let st = &stats;
            if y < rows_n - 1 {
                put(&mut out, &mut y, format!("\x1b[38;2;143;141;133mmean {:3.0} det {:2.0}% mot {:2.0}%", st.mean, st.detail * 100.0, st.motion * 100.0))?;
            }
            if ro.active && y < rows_n - 1 {
                put(&mut out, &mut y, format!("\x1b[38;2;61;255;138mauto {}→{} {}", ro.black, ro.white, ro.note))?;
            }
        }

        queue!(
            out,
            MoveTo(0, rows_n - 1),
            Print(format!(
                "\x1b[0m{:<w$}",
                "↑↓ select  ←→ adjust  1-5 presets  6-9 user  a auto  f retro-fps  r 8-bit look  u check-update  U install  p pause  Tab panel  w save  W new look  q quit",
                w = cols as usize
            ))
        )?;
        out.flush()?;
        force = false;
    }

    {
        let g = shared.inner.lock().unwrap();
        let mut c = Config::load(cfg_path.as_deref());
        c.set = Some(g.set.clone());
        c.auto = g.auto.clone();
        c.user_presets = g.user_presets.clone();
        let _ = c.save(cfg_path.as_deref());
    }
    Ok(())
}

fn fps_mut(inner: &mut Inner) -> FpsLimitMut<'_> {
    FpsLimitMut { enabled: &mut inner.fps_limit.enabled, fps: &mut inner.fps_limit.fps }
}

#[allow(dead_code)]
fn _imports(_: &Mutex<Inner>, _: &Arc<Shared>) {}

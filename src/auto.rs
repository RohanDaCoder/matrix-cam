//! Live-feed analysis + adaptive controller.
//! Analysis runs on the already-downsampled grid (~10-20k cells), so it is nearly free.
//! Targets are recomputed on an interval; values glide toward them every frame (no flicker).
use crate::config::{clampf, clampi, AutoConfig, Settings, Style};
use crate::effect::{tone_from_levels, Grid};
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct Stats {
    pub mean: f32,
    pub p_lo: u8,
    pub p_hi: u8,
    pub detail: f32,
    pub motion: f32,
    pub red_frac: f32,
    pub clip_low: f32,
    pub clip_high: f32,
    pub hist: [u32; 64],
    pub cells: u32,
}
impl Default for Stats {
    fn default() -> Self {
        Stats { mean: 0.0, p_lo: 0, p_hi: 255, detail: 0.0, motion: 0.0, red_frac: 0.0,
                clip_low: 0.0, clip_high: 0.0, hist: [0; 64], cells: 0 }
    }
}

pub struct Analyzer {
    prev: Vec<u8>,
    hist: [u32; 256],
    red_hist: [u32; 256],
}
impl Analyzer {
    pub fn new() -> Self { Analyzer { prev: Vec::new(), hist: [0; 256], red_hist: [0; 256] } }

    pub fn analyze(&mut self, g: &Grid) -> Stats {
        self.hist = [0; 256];
        self.red_hist = [0; 256];
        let n = g.raw.len();
        if n == 0 { return Stats::default(); }
        let mut sum = 0u64;
        for i in 0..n {
            let v = g.raw[i];
            self.hist[v as usize] += 1;
            self.red_hist[g.red[i] as usize] += 1;
            sum += v as u64;
        }
        let mean = sum as f32 / n as f32;

        // detail: mean neighbour gradient, normalised
        let mut acc = 0u64;
        let mut cnt = 0u64;
        if g.w > 1 {
            for y in 0..g.h {
                let row = y * g.w;
                for x in 0..g.w - 1 {
                    acc += (g.raw[row + x] as i32 - g.raw[row + x + 1] as i32).unsigned_abs() as u64;
                    cnt += 1;
                }
            }
        }
        if g.h > 1 {
            for y in 0..g.h - 1 {
                for x in 0..g.w {
                    acc += (g.raw[y * g.w + x] as i32 - g.raw[(y + 1) * g.w + x] as i32).unsigned_abs() as u64;
                    cnt += 1;
                }
            }
        }
        let detail = if cnt == 0 { 0.0 } else { clampf(acc as f32 / cnt as f32 / 48.0, 0.0, 1.0) };

        // motion vs previous grid
        let motion = if self.prev.len() == n {
            let mut m = 0u64;
            for i in 0..n { m += (g.raw[i] as i32 - self.prev[i] as i32).unsigned_abs() as u64; }
            clampf(m as f32 / n as f32 / 40.0, 0.0, 1.0)
        } else { 0.0 };
        self.prev.clear();
        self.prev.extend_from_slice(&g.raw);

        let mut red = 0u64;
        for i in 21..256 { red += self.red_hist[i] as u64; }

        let mut hist = [0u32; 64];
        for i in 0..256 { hist[i / 4] += self.hist[i]; }

        let clip_low = (self.hist[0] + self.hist[1] + self.hist[2]) as f32 / n as f32;
        let clip_high = (self.hist[253] + self.hist[254] + self.hist[255]) as f32 / n as f32;

        Stats {
            mean, p_lo: 0, p_hi: 255, detail, motion,
            red_frac: red as f32 / n as f32,
            clip_low, clip_high, hist, cells: n as u32,
        }
    }

    pub fn percentile(&self, p: f32) -> u8 {
        let total: u64 = self.hist.iter().map(|&x| x as u64).sum();
        if total == 0 { return 0; }
        let want = (total as f32 * clampf(p, 0.0, 100.0) / 100.0) as u64;
        let mut acc = 0u64;
        for (i, &h) in self.hist.iter().enumerate() {
            acc += h as u64;
            if acc >= want { return i as u8; }
        }
        255
    }

    /// Smallest redness cut so that at most `coverage` of cells exceed it.
    pub fn red_cut(&self, coverage: f32) -> u8 {
        let total: u64 = self.red_hist.iter().map(|&x| x as u64).sum();
        if total == 0 { return 255; }
        let budget = (total as f32 * clampf(coverage, 0.0, 1.0)) as u64;
        let mut acc = 0u64;
        for i in (0..256).rev() {
            acc += self.red_hist[i] as u64;
            if acc > budget { return (i as u8).saturating_add(1); }
        }
        0
    }

    /// Fraction of cells that would produce a visible glyph for a given polarity.
    pub fn coverage(&self, lut: &[u8; 256], invert: bool, levels: usize) -> f32 {
        let mut on = 0u64;
        let mut tot = 0u64;
        for i in 0..256 {
            let lv = lut[i];
            let v = if invert { 255 - lv } else { lv };
            if (v as usize * levels) / 256 > 0 { on += self.hist[i] as u64; }
            tot += self.hist[i] as u64;
        }
        if tot == 0 { 0.0 } else { on as f32 / tot as f32 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AutoReadout {
    pub active: bool,
    pub black: u8,
    pub white: u8,
    pub accent_pct: f32,
    pub note: String,
}

pub struct Auto {
    prev: Instant,
    last_target: Instant,
    t_bri: f32, t_con: f32, t_cols: f32, t_thr: f32,
    t_glitch: f32, t_trails: f32, t_noise: f32,
    c_bri: f32, c_con: f32, c_cols: f32, c_thr: f32,
    c_glitch: f32, c_trails: f32, c_noise: f32,
    polarity: bool,
    inited: bool,
    lut: [u8; 256],
}

impl Auto {
    pub fn new() -> Self {
        let now = Instant::now();
        Auto {
            prev: now, last_target: now - std::time::Duration::from_secs(1),
            t_bri: 0.0, t_con: 0.0, t_cols: 150.0, t_thr: 70.0,
            t_glitch: 0.0, t_trails: 0.0, t_noise: 0.0,
            c_bri: 0.0, c_con: 0.0, c_cols: 150.0, c_thr: 70.0,
            c_glitch: 0.0, c_trails: 0.0, c_noise: 0.0,
            polarity: false, inited: false, lut: [0; 256],
        }
    }

    fn seed(&mut self, s: &Settings) {
        self.c_bri = s.brightness as f32; self.t_bri = self.c_bri;
        self.c_con = s.contrast as f32; self.t_con = self.c_con;
        self.c_cols = s.cols as f32;   self.t_cols = self.c_cols;
        self.c_thr = s.threshold as f32; self.t_thr = self.c_thr;
        self.c_glitch = s.glitch as f32; self.t_glitch = self.c_glitch;
        self.c_trails = s.trails as f32; self.t_trails = self.c_trails;
        self.c_noise = s.noise as f32;   self.t_noise = self.c_noise;
        self.polarity = s.invert;
        self.inited = true;
    }

    pub fn apply(
        &mut self, base: &Settings, cfg: &AutoConfig, st: &Stats, an: &Analyzer, now: Instant,
    ) -> (Settings, AutoReadout) {
        let mut eff = base.clone();
        let dt = (now - self.prev).as_secs_f32().clamp(0.0, 0.25);
        self.prev = now;
        if !cfg.enabled {
            self.inited = false;
            return (eff, AutoReadout { active: false, note: "manual".into(), ..Default::default() });
        }
        if !self.inited { self.seed(base); }

        let levels = match base.style { Style::Ascii => 11, Style::Dots => 32, Style::Cells => 16 };
        let mut note = Vec::new();
        let mut black = 0u8;
        let mut white = 255u8;
        let mut accent_pct = 0.0f32;

        // ---- recompute targets on interval ----
        if (now - self.last_target).as_millis() as u32 >= cfg.interval_ms.max(16) {
            self.last_target = now;

            if cfg.levels && st.cells > 0 {
                black = an.percentile(cfg.low_pct);
                white = an.percentile(cfg.high_pct);
                let target = if self.polarity { 255.0 - cfg.target_mean } else { cfg.target_mean };
                let (c, b) = tone_from_levels(black, white, st.mean, target);
                self.t_con = clampf(c, -100.0, 100.0);
                self.t_bri = clampf(b, -100.0, 100.0);
                note.push("levels");
            }
            if cfg.density {
                let d = clampf(st.detail * cfg.detail_gain, 0.0, 1.0);
                let lo = cfg.cols_min.min(cfg.cols_max) as f32;
                let hi = cfg.cols_min.max(cfg.cols_max) as f32;
                // quantise to 4 so the mask atlas is not rebuilt every frame
                let raw = lo + (hi - lo) * d;
                self.t_cols = ((raw / 4.0).round() * 4.0).clamp(40.0, 220.0);
                note.push("density");
            }
            if cfg.signal && base.mode == crate::config::ColorMode::Signal {
                let cut = an.red_cut(cfg.accent_coverage) as f32;
                self.t_thr = clampf(100.0 - (cut - 12.0) / 1.1, 0.0, 100.0);
                accent_pct = cfg.accent_coverage * 100.0;
                note.push("signal");
            }
            if cfg.fx {
                let m = st.motion;
                self.t_glitch = clampf(cfg.motion_glitch * m.powf(0.7), 0.0, 100.0);
                self.t_trails = clampf(cfg.motion_trails * m, 0.0, 90.0);
                self.t_noise = clampf(cfg.noise_floor * (1.0 - st.mean / 255.0), 0.0, 100.0);
                note.push("fx");
            }
            if cfg.polarity && st.cells > 0 {
                let bri = self.c_bri * 1.6;
                let c = self.c_con * 2.55;
                let cf = (259.0 * (c + 255.0)) / (255.0 * (259.0 - c));
                for i in 0..256 {
                    self.lut[i] = ((i as f32 - 128.0) * cf + 128.0 + bri).clamp(0.0, 255.0) as u8;
                }
                let a = an.coverage(&self.lut, false, levels);
                let b = an.coverage(&self.lut, true, levels);
                // hysteresis: only flip when clearly better
                if (!self.polarity && b > a + 0.08) || (self.polarity && a > b + 0.08) {
                    self.polarity = !self.polarity;
                }
                note.push("polarity");
            }
        }

        // ---- glide every frame ----
        let tau = (cfg.response_ms / 1000.0).max(0.02);
        let a = 1.0 - (-dt / tau).exp();
        let glide = |cur: &mut f32, tgt: f32| { *cur += (tgt - *cur) * a; };

        if cfg.levels { glide(&mut self.c_bri, self.t_bri); glide(&mut self.c_con, self.t_con);
            eff.brightness = clampi(self.c_bri.round() as i32, -100, 100);
            eff.contrast = clampi(self.c_con.round() as i32, -100, 100);
        } else { self.c_bri = base.brightness as f32; self.c_con = base.contrast as f32; }

        if cfg.density { glide(&mut self.c_cols, self.t_cols);
            eff.cols = clampi(self.c_cols.round() as i32, 40, 220) as u32;
        } else { self.c_cols = base.cols as f32; }

        if cfg.signal { glide(&mut self.c_thr, self.t_thr);
            eff.threshold = clampi(self.c_thr.round() as i32, 0, 100);
        } else { self.c_thr = base.threshold as f32; }

        if cfg.fx {
            glide(&mut self.c_glitch, self.t_glitch);
            glide(&mut self.c_trails, self.t_trails);
            glide(&mut self.c_noise, self.t_noise);
            eff.glitch = clampi(self.c_glitch.round() as i32, 0, 100) as u32;
            eff.trails = clampi(self.c_trails.round() as i32, 0, 90) as u32;
            eff.noise = clampi(self.c_noise.round() as i32, 0, 100) as u32;
        } else {
            self.c_glitch = base.glitch as f32;
            self.c_trails = base.trails as f32;
            self.c_noise = base.noise as f32;
        }

        if cfg.polarity { eff.invert = self.polarity; } else { self.polarity = base.invert; }

        (eff, AutoReadout {
            active: true, black, white, accent_pct,
            note: if note.is_empty() { "idle".into() } else { note.join(" · ") },
        })
    }
}

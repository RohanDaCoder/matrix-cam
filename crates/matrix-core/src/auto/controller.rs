//! The adaptive controller: turns analyzer stats into smooth settings targets.

use crate::auto::analyze::{Analyzer, Stats};
use crate::config::{clampf, clampi, AutoConfig, ColorMode, Settings};
use crate::render::tone::tone_from_levels;
use crate::time::Instant;

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
    t_bri: f32,
    t_con: f32,
    t_cols: f32,
    t_thr: f32,
    t_glitch: f32,
    t_trails: f32,
    t_noise: f32,
    c_bri: f32,
    c_con: f32,
    c_cols: f32,
    c_thr: f32,
    c_glitch: f32,
    c_trails: f32,
    c_noise: f32,
    polarity: bool,
    inited: bool,
    lut: [u8; 256],
}

impl Auto {
    pub fn new() -> Self {
        let now = Instant::now();
        // `now - 1s` underflows web_time (performance.now starts near 0 on a
        // fresh page), which would panic; saturate instead — the first target
        // recompute fires immediately either way.
        let last_target = now.checked_sub(crate::time::Duration::from_secs(1)).unwrap_or(now);
        Auto {
            prev: now,
            last_target,
            t_bri: 0.0,
            t_con: 0.0,
            t_cols: 150.0,
            t_thr: 70.0,
            t_glitch: 0.0,
            t_trails: 0.0,
            t_noise: 0.0,
            c_bri: 0.0,
            c_con: 0.0,
            c_cols: 150.0,
            c_thr: 70.0,
            c_glitch: 0.0,
            c_trails: 0.0,
            c_noise: 0.0,
            polarity: false,
            inited: false,
            lut: [0; 256],
        }
    }

    fn seed(&mut self, s: &Settings) {
        self.c_bri = s.brightness as f32;
        self.t_bri = self.c_bri;
        self.c_con = s.contrast as f32;
        self.t_con = self.c_con;
        self.c_cols = s.cols as f32;
        self.t_cols = self.c_cols;
        self.c_thr = s.threshold as f32;
        self.t_thr = self.c_thr;
        self.c_glitch = s.glitch as f32;
        self.t_glitch = self.c_glitch;
        self.c_trails = s.trails as f32;
        self.t_trails = self.c_trails;
        self.c_noise = s.noise as f32;
        self.t_noise = self.c_noise;
        self.polarity = s.invert;
        self.inited = true;
    }

    pub fn apply(
        &mut self,
        base: &Settings,
        cfg: &AutoConfig,
        st: &Stats,
        an: &Analyzer,
        now: Instant,
    ) -> (Settings, AutoReadout) {
        let mut eff = base.clone();
        let dt = (now - self.prev).as_secs_f32().clamp(0.0, 0.25);
        self.prev = now;
        if !cfg.enabled {
            self.inited = false;
            return (eff, AutoReadout { active: false, note: "manual".into(), ..Default::default() });
        }
        if !self.inited {
            self.seed(base);
        }

        let levels = base.style.glyph_count();
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
            if cfg.signal && base.mode == ColorMode::Signal {
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
                self.lut = crate::render::tone::build_lut(self.c_bri as i32, self.c_con as i32);
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
        let glide = |cur: &mut f32, tgt: f32| {
            *cur += (tgt - *cur) * a;
        };

        if cfg.levels {
            glide(&mut self.c_bri, self.t_bri);
            glide(&mut self.c_con, self.t_con);
            eff.brightness = clampi(self.c_bri.round() as i32, -100, 100);
            eff.contrast = clampi(self.c_con.round() as i32, -100, 100);
        } else {
            self.c_bri = base.brightness as f32;
            self.c_con = base.contrast as f32;
        }

        if cfg.density {
            glide(&mut self.c_cols, self.t_cols);
            eff.cols = clampi(self.c_cols.round() as i32, 40, 220) as u32;
        } else {
            self.c_cols = base.cols as f32;
        }

        if cfg.signal {
            glide(&mut self.c_thr, self.t_thr);
            eff.threshold = clampi(self.c_thr.round() as i32, 0, 100) as u32 as i32;
        } else {
            self.c_thr = base.threshold as f32;
        }

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

        if cfg.polarity {
            eff.invert = self.polarity;
        } else {
            self.polarity = base.invert;
        }

        (
            eff,
            AutoReadout {
                active: true,
                black,
                white,
                accent_pct,
                note: if note.is_empty() { "idle".into() } else { note.join(" · ") },
            },
        )
    }
}

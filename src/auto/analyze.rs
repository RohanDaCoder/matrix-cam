//! Grid statistics: histogram, detail, motion, redness.

use crate::config::clampf;
use crate::render::Grid;

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
        Stats {
            mean: 0.0,
            p_lo: 0,
            p_hi: 255,
            detail: 0.0,
            motion: 0.0,
            red_frac: 0.0,
            clip_low: 0.0,
            clip_high: 0.0,
            hist: [0; 64],
            cells: 0,
        }
    }
}

pub struct Analyzer {
    prev: Vec<u8>,
    hist: [u32; 256],
    red_hist: [u32; 256],
}

impl Analyzer {
    pub fn new() -> Self {
        Analyzer { prev: Vec::new(), hist: [0; 256], red_hist: [0; 256] }
    }

    pub fn analyze(&mut self, g: &Grid) -> Stats {
        self.hist = [0; 256];
        self.red_hist = [0; 256];
        let n = g.raw.len();
        if n == 0 {
            return Stats::default();
        }
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
            for i in 0..n {
                m += (g.raw[i] as i32 - self.prev[i] as i32).unsigned_abs() as u64;
            }
            clampf(m as f32 / n as f32 / 40.0, 0.0, 1.0)
        } else {
            0.0
        };
        self.prev.clear();
        self.prev.extend_from_slice(&g.raw);

        let mut red = 0u64;
        for i in 21..256 {
            red += self.red_hist[i] as u64;
        }

        let mut hist = [0u32; 64];
        for i in 0..256 {
            hist[i / 4] += self.hist[i];
        }

        let clip_low = (self.hist[0] + self.hist[1] + self.hist[2]) as f32 / n as f32;
        let clip_high = (self.hist[253] + self.hist[254] + self.hist[255]) as f32 / n as f32;

        Stats {
            mean,
            p_lo: 0,
            p_hi: 255,
            detail,
            motion,
            red_frac: red as f32 / n as f32,
            clip_low,
            clip_high,
            hist,
            cells: n as u32,
        }
    }

    pub fn percentile(&self, p: f32) -> u8 {
        let total: u64 = self.hist.iter().map(|&x| x as u64).sum();
        if total == 0 {
            return 0;
        }
        let want = (total as f32 * clampf(p, 0.0, 100.0) / 100.0) as u64;
        let mut acc = 0u64;
        for (i, &h) in self.hist.iter().enumerate() {
            acc += h as u64;
            if acc >= want {
                return i as u8;
            }
        }
        255
    }

    /// Smallest redness cut so that at most `coverage` of cells exceed it.
    pub fn red_cut(&self, coverage: f32) -> u8 {
        let total: u64 = self.red_hist.iter().map(|&x| x as u64).sum();
        if total == 0 {
            return 255;
        }
        let budget = (total as f32 * clampf(coverage, 0.0, 1.0)) as u64;
        let mut acc = 0u64;
        for i in (0..256).rev() {
            acc += self.red_hist[i] as u64;
            if acc > budget {
                return (i as u8).saturating_add(1);
            }
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
            if (v as usize * levels) / 256 > 0 {
                on += self.hist[i] as u64;
            }
            tot += self.hist[i] as u64;
        }
        if tot == 0 { 0.0 } else { on as f32 / tot as f32 }
    }
}

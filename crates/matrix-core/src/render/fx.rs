//! Post-pass screen effects: glitch bands, scanlines, noise.

use crate::render::grid::Grid;

pub struct Rng(pub u64);

impl Rng {
    pub fn new() -> Self {
        // Seed from the platform clock. `Instant` is the time alias (web_time
        // on wasm, std elsewhere); a nonzero fallback keeps the stream alive
        // if the clock is unavailable.
        let n = crate::time::Instant::now()
            .elapsed()
            .as_nanos() as u64;
        Rng(n | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / 16_777_216.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

/// Apply glitch / scanline / noise to a finished frame in place.
pub fn apply(
    buf: &mut [u8],
    w: usize,
    h: usize,
    set: &crate::config::Settings,
    rowtmp: &mut Vec<u8>,
    rng: &mut Rng,
    _grid: &Grid,
) {
    let accent = set.accent;
    if set.glitch > 0 && rng.f() < set.glitch as f32 * 0.004 + 0.01 {
        let bands = 1 + rng.below(3);
        for _ in 0..bands {
            let bh = (2 + (h as f32 * (0.02 + rng.f() * 0.09)) as usize).min(h);
            let y0 = rng.below(h - bh + 1);
            let dx = rng.below(set.glitch as usize + 1) as i32 - set.glitch as i32 / 2;
            if dx == 0 {
                continue;
            }
            for y in y0..y0 + bh {
                let row = &mut buf[y * w * 3..(y + 1) * w * 3];
                rowtmp.resize(row.len(), 0);
                rowtmp.copy_from_slice(row);
                for x in 0..w {
                    let sx = ((x as i32 - dx).rem_euclid(w as i32)) as usize;
                    row[x * 3..x * 3 + 3].copy_from_slice(&rowtmp[sx * 3..sx * 3 + 3]);
                }
            }
        }
        if rng.f() < 0.4 {
            let y = rng.below(h);
            let row = &mut buf[y * w * 3..(y + 1) * w * 3];
            for px in row.chunks_exact_mut(3) {
                px[0] = ((px[0] as u16 + accent[0] as u16) / 2) as u8;
                px[1] = ((px[1] as u16 + accent[1] as u16) / 2) as u8;
                px[2] = ((px[2] as u16 + accent[2] as u16) / 2) as u8;
            }
        }
    }
    if set.scan > 0 {
        let f = (256.0 * (1.0 - set.scan as f32 / 100.0 * 0.55)) as u32;
        let mut y = 2;
        while y < h {
            let row = &mut buf[y * w * 3..(y + 1) * w * 3];
            for b in row.iter_mut() {
                *b = ((*b as u32 * f) >> 8) as u8;
            }
            y += 3;
        }
    }
    if set.noise > 0 {
        let count = set.noise as usize * w * h / 3000;
        let amp = 8 + set.noise as i32 / 2;
        for _ in 0..count {
            let p = rng.below(w * h) * 3;
            let d = if rng.below(2) == 0 { amp } else { -amp };
            for k in 0..3 {
                buf[p + k] = (buf[p + k] as i32 + d).clamp(0, 255) as u8;
            }
        }
    }
}

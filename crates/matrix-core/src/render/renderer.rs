//! The frame renderer: camera RGB in → effect frame out.

use crate::config::{ColorMode, Settings, Style};
use crate::render::atlas::{self, Atlas, Key};
use crate::render::fx::{self, Rng};
use crate::render::grid::Grid;
use crate::render::tone;
use crate::render::{cell_color, mix};
use std::sync::Arc;

pub struct Renderer {
    w: usize,
    h: usize,
    set: Settings,
    atlas: Option<Atlas>,
    grid: Grid,
    pool: [Option<Arc<Vec<u8>>>; 2],
    slot: usize,
    rowtmp: Vec<u8>,
    rng: Rng,
    lut: [u8; 256],
    threads: usize,
    pub want_grid: bool,
    last_grid: Arc<Grid>,
    pub serial: u64,
}

impl Renderer {
    pub fn new(w: u32, h: u32, set: Settings, threads: usize) -> Self {
        Renderer {
            w: w as usize,
            h: h as usize,
            set,
            atlas: None,
            grid: Grid::new(),
            pool: [None, None],
            slot: 0,
            rowtmp: vec![0; w as usize * 3],
            rng: Rng::new(),
            lut: [0; 256],
            threads: threads.max(1),
            want_grid: false,
            last_grid: Arc::new(Grid::new()),
            serial: 0,
        }
    }

    pub fn apply(&mut self, s: &Settings) {
        self.set = s.clone();
    }

    pub fn grid(&self) -> Arc<Grid> {
        self.last_grid.clone()
    }

    /// Output dimensions (set at construction; the engine reads them to
    /// publish `View` frames). Rounded to even numbers like the CLI does.
    pub fn dims(&self) -> (u32, u32) {
        (self.w as u32, self.h as u32)
    }

    /// Last published frame without re-rendering (used while paused / on capture hiccups).
    pub fn frame_arc(&self) -> Option<Arc<Vec<u8>>> {
        self.pool.iter().find_map(|p| p.clone())
    }

    fn acquire(&mut self) -> Vec<u8> {
        let need = self.w * self.h * 3;
        for k in 0..2 {
            let i = (self.slot + k) % 2;
            match self.pool[i].take() {
                Some(arc) => match Arc::try_unwrap(arc) {
                    Ok(mut v) => {
                        v.resize(need, 0);
                        self.slot = i;
                        return v;
                    }
                    Err(arc) => self.pool[i] = Some(arc),
                },
                None => {
                    self.slot = i;
                    return vec![0; need];
                }
            }
        }
        vec![0; need]
    }

    fn ensure_atlas(&mut self) {
        let key = Key(self.w, self.h, self.set.cols, self.set.style, self.set.glow, self.set.glyphs, self.set.reverse);
        if let Some(a) = &self.atlas {
            if a.key == key {
                return;
            }
        }
        self.atlas = Some(atlas::build(
            self.w, self.h, self.set.cols, self.set.style, self.set.glow, self.set.glyphs, self.set.reverse,
        ));
    }

    pub fn render(&mut self, src: &[u8], sw: usize, sh: usize) -> Arc<Vec<u8>> {
        self.ensure_atlas();
        self.lut = tone::build_lut(self.set.brightness, self.set.contrast);
        let (cw, chh, gw, gh) = {
            let a = self.atlas.as_ref().unwrap();
            (a.cw, a.chh, a.gw, a.gh)
        };
        self.grid.ensure(gw, gh);
        let mut buf = self.acquire();

        // background / phosphor persistence
        let s = &self.set;
        let t = (s.trails as f32 / 100.0 * 0.94 * 256.0) as u32;
        if t > 0 {
            let inv = 256 - t;
            for px in buf.chunks_exact_mut(3) {
                px[0] = ((px[0] as u32 * t + s.bg[0] as u32 * inv) >> 8) as u8;
                px[1] = ((px[1] as u32 * t + s.bg[1] as u32 * inv) >> 8) as u8;
                px[2] = ((px[2] as u32 * t + s.bg[2] as u32 * inv) >> 8) as u8;
            }
        } else {
            for px in buf.chunks_exact_mut(3) {
                px.copy_from_slice(&s.bg);
            }
        }

        // centre-crop to output aspect
        let out_ar = self.w as f64 / self.h as f64;
        let crop = if sw == 0 || sh == 0 {
            (1, 1, 0, 0)
        } else if (sw as f64 / sh as f64) > out_ar {
            let c = ((sh as f64 * out_ar) as usize).max(1);
            (c, sh, (sw - c) / 2, 0)
        } else {
            let c = ((sw as f64 / out_ar) as usize).max(1);
            (sw, c, 0, (sh - c) / 2)
        };

        let threads = self.threads.min(gh).max(1);
        let rows_per = (gh + threads - 1) / threads;
        let (w, h) = (self.w, self.h);
        let atlas_ref = self.atlas.as_ref().unwrap();
        let set = &self.set;
        let lut = &self.lut;
        let serial = self.serial;

        {
            let mut out_rest: &mut [u8] = &mut buf[..];
            let mut raw_rest: &mut [u8] = &mut self.grid.raw[..];
            let mut red_rest: &mut [u8] = &mut self.grid.red[..];
            let mut rgb_rest: &mut [[u8; 3]] = &mut self.grid.rgb[..];
            let mut idx_rest: &mut [u8] = &mut self.grid.idx[..];
            let mut col_rest: &mut [[u8; 3]] = &mut self.grid.col[..];
            let mut jobs = Vec::with_capacity(threads);
            for t in 0..threads {
                let gy0 = t * rows_per;
                let gy1 = ((t + 1) * rows_per).min(gh);
                if gy0 >= gy1 {
                    break;
                }
                let n = gy1 - gy0;
                let px = (n * chh).min(h - gy0 * chh);
                let (o, r) = out_rest.split_at_mut(px * w * 3);
                out_rest = r;
                let cells = n * gw;
                let (a1, r1) = raw_rest.split_at_mut(cells);
                raw_rest = r1;
                let (a2, r2) = red_rest.split_at_mut(cells);
                red_rest = r2;
                let (a3, r3) = rgb_rest.split_at_mut(cells);
                rgb_rest = r3;
                let (a4, r4) = idx_rest.split_at_mut(cells);
                idx_rest = r4;
                let (a5, r5) = col_rest.split_at_mut(cells);
                col_rest = r5;
                jobs.push((gy0, gy1, px, o, a1, a2, a3, a4, a5));
            }
            // wasm has no std threads: render rows inline on this thread.
            // (threads.max(1) already guarantees `jobs` has at least one job
            // on native, so the two paths produce identical output.)
            #[cfg(target_arch = "wasm32")]
            for job in jobs {
                let (gy0, gy1, px, o, rw, rd, rgb, ix, cl) = job;
                rows_job(
                    gy0, gy1, px, o, rw, rd, rgb, ix, cl, src, sw, crop, gw, cw, chh, w,
                    atlas_ref, set, lut, serial,
                );
            }
            #[cfg(not(target_arch = "wasm32"))]
            std::thread::scope(|sc| {
                for (k, job) in jobs.into_iter().enumerate() {
                    sc.spawn(move || {
                        let (gy0, gy1, px, o, rw, rd, rgb, ix, cl) = job;
                        rows_job(
                            gy0, gy1, px, o, rw, rd, rgb, ix, cl, src, sw, crop, gw, cw, chh, w,
                            atlas_ref, set, lut,
                            serial ^ (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
                        );
                    });
                }
            });
        }

        fx::apply(&mut buf, w, h, s, &mut self.rowtmp, &mut self.rng, &self.grid);
        if self.want_grid {
            self.last_grid = Arc::new(self.grid.snapshot());
        }
        self.serial += 1;
        let arc = Arc::new(buf);
        self.pool[self.slot] = Some(arc.clone());
        arc
    }
}

#[allow(clippy::too_many_arguments)]
fn rows_job(
    gy0: usize,
    gy1: usize,
    pxh: usize,
    out: &mut [u8],
    raw: &mut [u8],
    red: &mut [u8],
    rgb: &mut [[u8; 3]],
    idxs: &mut [u8],
    cols: &mut [[u8; 3]],
    src: &[u8],
    sw: usize,
    crop: (usize, usize, usize, usize),
    gw: usize,
    cw: usize,
    chh: usize,
    w: usize,
    atlas: &Atlas,
    set: &Settings,
    lut: &[u8; 256],
    seed: u64,
) {
    let (cropw, croph, cx0, cy0) = crop;
    let gh = atlas.gh;
    let mirror = set.mirror;
    let inv = set.invert;
    let sigcut = ((100 - set.threshold) as f32 * 1.1 + 12.0) as i32;
    let heatcut = (set.threshold as f32 * 2.55) as u8;
    let dim = mix(set.bg, set.ink, 0.28);
    let glitchp = set.glitch as f32 * 0.0004;
    let mut rng = Rng(seed | 1);
    let levels = set.style.glyph_count();

    for gy in gy0..gy1 {
        let ly = gy - gy0;
        let oy = ly * chh;
        let sy0 = cy0 + gy * croph / gh;
        let sy1 = (cy0 + ((gy + 1) * croph / gh)).min(cy0 + croph).max(sy0 + 1);
        for gx in 0..gw {
            let ci = ly * gw + gx;
            let sx0 = gx * cropw / gw;
            let sx1 = ((gx + 1) * cropw / gw).min(cropw).max(sx0 + 1);
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for y in sy0..sy1 {
                for x in sx0..sx1 {
                    let xx = if mirror { cropw - 1 - x } else { x };
                    let i = (y * sw + cx0 + xx) * 3;
                    if i + 2 < src.len() {
                        r += src[i] as u32;
                        g += src[i + 1] as u32;
                        b += src[i + 2] as u32;
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            let cr = (r / n) as u8;
            let cg = (g / n) as u8;
            let cb = (b / n) as u8;
            let lraw = ((54 * cr as u32 + 183 * cg as u32 + 19 * cb as u32) >> 8) as u8;
            let rd = cr.saturating_sub(cg.max(cb));
            raw[ci] = lraw;
            red[ci] = rd;
            let cell_rgb = [cr, cg, cb];
            rgb[ci] = cell_rgb;

            let lv = lut[lraw as usize];
            let v = if inv { 255 - lv } else { lv };

            let mut col = cell_color(
                set.mode,
                set.ink,
                set.bg,
                set.accent,
                toned_rgb(cell_rgb, lut, inv),
                v,
                heatcut,
                sigcut,
                rd,
            );
            let mut idx = (v as usize * levels) / 256;
            if idx >= levels {
                idx = levels - 1;
            }
            let corrupt = glitchp > 0.0 && rng.f() < glitchp;
            if corrupt {
                col = set.accent;
                if set.style == Style::Ascii {
                    idx = 1 + rng.below(levels - 1);
                }
            }
            idxs[ci] = idx as u8;
            cols[ci] = col;

            let ox = gx * cw;
            match set.style {
                Style::Ascii => {
                    if idx == 0 {
                        continue;
                    }
                    if set.glow > 0 {
                        atlas::blit(out, w, pxh, ox + 1, oy + 1, &atlas.glyphs[idx], dim);
                    }
                    atlas::blit(out, w, pxh, ox, oy, &atlas.glyphs[idx], col);
                }
                Style::Dots => {
                    if v < 8 {
                        continue;
                    }
                    if set.glow > 0 {
                        atlas::blit(out, w, pxh, ox, oy, &atlas.halos[idx], dim);
                    }
                    atlas::blit(out, w, pxh, ox, oy, &atlas.dots[idx], col);
                }
                Style::Cells => {
                    if v < 8 {
                        continue;
                    }
                    let li = (idx * 16) / 32;
                    atlas::blit(out, w, pxh, ox, oy, &atlas.cells[li.min(15)], col);
                }
            }
        }
    }
}

/// Apply the tone LUT (and polarity) to a cell's own colour for full-color mode.
fn toned_rgb(rgb: [u8; 3], lut: &[u8; 256], inv: bool) -> [u8; 3] {
    let (mut a, mut b, mut c) = (lut[rgb[0] as usize], lut[rgb[1] as usize], lut[rgb[2] as usize]);
    if inv {
        a = 255 - a;
        b = 255 - b;
        c = 255 - c;
    }
    [a, b, c]
}

// re-export for convenience
pub use crate::render::tone::tone_from_levels;

// silence unused warnings for ColorMode import used by cell_color docs
#[allow(unused)]
fn _color_mode_used(_: ColorMode) {}

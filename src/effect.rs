//! Camera-only renderer. One fused parallel pass per frame: downsample -> tone -> draw.
use crate::config::{clampf, ColorMode, Settings, Style};
use crate::font::GLYPHS;
use std::sync::Arc;

pub struct Grid {
    pub w: usize, pub h: usize,
    pub raw: Vec<u8>,        // pre-tone luma (analysis input)
    pub red: Vec<u8>,        // red dominance
    pub rgb: Vec<[u8; 3]>,   // sampled cell colour
    pub idx: Vec<u8>,        // ramp / level index
    pub col: Vec<[u8; 3]>,   // final cell colour
}
impl Grid {
    fn new() -> Self { Grid { w: 0, h: 0, raw: vec![], red: vec![], rgb: vec![], idx: vec![], col: vec![] } }
    fn ensure(&mut self, w: usize, h: usize) {
        if self.w == w && self.h == h { return; }
        let n = w * h;
        self.w = w; self.h = h;
        self.raw = vec![0; n]; self.red = vec![0; n];
        self.rgb = vec![[0; 3]; n]; self.idx = vec![0; n]; self.col = vec![[0; 3]; n];
    }
    pub fn snapshot(&self) -> Grid {
        Grid { w: self.w, h: self.h, raw: self.raw.clone(), red: self.red.clone(),
               rgb: self.rgb.clone(), idx: self.idx.clone(), col: self.col.clone() }
    }
}

pub struct Rng(pub u64);
impl Rng {
    pub fn new() -> Self {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64).unwrap_or(0x9E37_79B9);
        Rng(n | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0; x ^= x >> 12; x ^= x << 25; x ^= x >> 27;
        self.0 = x; x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn f(&mut self) -> f32 { (self.next() >> 40) as f32 / 16_777_216.0 }
    fn below(&mut self, n: usize) -> usize { if n == 0 { 0 } else { (self.next() % n as u64) as usize } }
}

type Span = (i16, i16, i16);
struct Mask { spans: Vec<Span> }
fn mask_from(cw: usize, ch: usize, pred: impl Fn(usize, usize) -> bool) -> Mask {
    let mut spans = Vec::new();
    for y in 0..ch {
        let mut x = 0;
        while x < cw {
            if pred(x, y) {
                let x0 = x;
                while x < cw && pred(x, y) { x += 1; }
                spans.push((y as i16, x0 as i16, x as i16));
            } else { x += 1; }
        }
    }
    Mask { spans }
}
fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [ (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
      (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
      (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8 ]
}
fn blit(out: &mut [u8], w: usize, h: usize, ox: usize, oy: usize, m: &Mask, col: [u8; 3]) {
    for &(dy, x0, x1) in &m.spans {
        let y = oy + dy as usize;
        if y >= h { continue; }
        let a = ox + x0 as usize;
        if a >= w { continue; }
        let b = (ox + x1 as usize).min(w);
        let row = &mut out[(y * w + a) * 3..(y * w + b) * 3];
        for px in row.chunks_exact_mut(3) { px.copy_from_slice(&col); }
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
struct Key(usize, usize, u32, Style, u32);
struct Atlas {
    key: Key, cw: usize, chh: usize, gw: usize, gh: usize,
    glyphs: Vec<Mask>, dots: Vec<Mask>, halos: Vec<Mask>, cells: Vec<Mask>,
}

pub struct Renderer {
    w: usize, h: usize,
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
            w: w as usize, h: h as usize, set,
            atlas: None, grid: Grid::new(),
            pool: [None, None], slot: 0,
            rowtmp: vec![0; w as usize * 3],
            rng: Rng::new(), lut: [0; 256],
            threads: threads.max(1),
            want_grid: false,
            last_grid: Arc::new(Grid::new()),
            serial: 0,
        }
    }
    pub fn apply(&mut self, s: &Settings) { self.set = s.clone(); }
    pub fn grid(&self) -> Arc<Grid> { self.last_grid.clone() }

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
                    Ok(mut v) => { v.resize(need, 0); self.slot = i; return v; }
                    Err(arc) => { self.pool[i] = Some(arc); }
                },
                None => { self.slot = i; return vec![0; need]; }
            }
        }
        vec![0; need]
    }

    fn ensure_atlas(&mut self) {
        let key = Key(self.w, self.h, self.set.cols, self.set.style, self.set.glow);
        if let Some(a) = &self.atlas { if a.key == key { return; } }
        let cw = (self.w / self.set.cols.max(1) as usize).max(2);
        let chh = if self.set.style == Style::Ascii { cw * 2 } else { cw };
        let gw = self.w / cw;
        let gh = self.h / chh;
        let mut glyphs = Vec::new();
        for g in GLYPHS.iter() {
            let g = *g;
            glyphs.push(mask_from(cw, chh, move |x, y| (g[y * 7 / chh] >> (4 - x * 5 / cw)) & 1 == 1));
        }
        let mut dots = Vec::new();
        let mut halos = Vec::new();
        let c = (cw as f32 - 1.0) / 2.0;
        let glow = self.set.glow;
        for l in 0..32 {
            let v = (l as f32 + 1.0) / 32.0;
            let r = (0.10 + 0.90 * v) * cw as f32 * 0.52;
            let r2 = r * r;
            dots.push(mask_from(cw, cw, move |x, y| {
                let dx = x as f32 - c; let dy = y as f32 - c; dx * dx + dy * dy <= r2
            }));
            let hr = r + 1.0 + glow as f32 / 8.0;
            let hr2 = hr * hr;
            halos.push(mask_from(cw, cw, move |x, y| {
                let dx = x as f32 - c; let dy = y as f32 - c; dx * dx + dy * dy <= hr2
            }));
        }
        let mut cells = Vec::new();
        for l in 0..16 {
            let v = (l as f32 + 1.0) / 16.0;
            let side = (cw as f32 * v.sqrt() * 0.96) as usize;
            let off = (cw - side) / 2;
            cells.push(mask_from(cw, cw, move |x, y| x >= off && x < off + side && y >= off && y < off + side));
        }
        self.atlas = Some(Atlas { key, cw, chh, gw, gh, glyphs, dots, halos, cells });
    }

    fn build_lut(&mut self) {
        let bri = self.set.brightness as f32 * 1.6;
        let c = self.set.contrast as f32 * 2.55;
        let cf = (259.0 * (c + 255.0)) / (255.0 * (259.0 - c));
        for i in 0..256 {
            self.lut[i] = ((i as f32 - 128.0) * cf + 128.0 + bri).clamp(0.0, 255.0) as u8;
        }
    }

    pub fn render(&mut self, src: &[u8], sw: usize, sh: usize) -> Arc<Vec<u8>> {
        self.ensure_atlas();
        self.build_lut();
        let (cw, chh, gw, gh) = { let a = self.atlas.as_ref().unwrap(); (a.cw, a.chh, a.gw, a.gh) };
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
            for px in buf.chunks_exact_mut(3) { px.copy_from_slice(&s.bg); }
        }

        // centre-crop to output aspect
        let out_ar = self.w as f64 / self.h as f64;
        let crop = if sw == 0 || sh == 0 {
            (1, 1, 0, 0)
        } else if (sw as f64 / sh as f64) > out_ar {
            let c = ((sh as f64 * out_ar) as usize).max(1); (c, sh, (sw - c) / 2, 0)
        } else {
            let c = ((sw as f64 / out_ar) as usize).max(1); (sw, c, 0, (sh - c) / 2)
        };

        let threads = self.threads.min(gh).max(1);
        let rows_per = (gh + threads - 1) / threads;
        let (w, h) = (self.w, self.h);
        let atlas = self.atlas.as_ref().unwrap();
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
                if gy0 >= gy1 { break; }
                let n = gy1 - gy0;
                let px = (n * chh).min(h - gy0 * chh);
                let (o, r) = out_rest.split_at_mut(px * w * 3); out_rest = r;
                let cells = n * gw;
                let (a1, r1) = raw_rest.split_at_mut(cells); raw_rest = r1;
                let (a2, r2) = red_rest.split_at_mut(cells); red_rest = r2;
                let (a3, r3) = rgb_rest.split_at_mut(cells); rgb_rest = r3;
                let (a4, r4) = idx_rest.split_at_mut(cells); idx_rest = r4;
                let (a5, r5) = col_rest.split_at_mut(cells); col_rest = r5;
                jobs.push((gy0, gy1, px, o, a1, a2, a3, a4, a5));
            }
            std::thread::scope(|sc| {
                for (k, job) in jobs.into_iter().enumerate() {
                    sc.spawn(move || {
                        let (gy0, gy1, px, o, rw, rd, rgb, ix, cl) = job;
                        rows_job(gy0, gy1, px, o, rw, rd, rgb, ix, cl,
                                 src, sw, sh, crop, gw, cw, chh, w,
                                 atlas, set, lut,
                                 serial ^ (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
                    });
                }
            });
        }

        self.fx(&mut buf);
        if self.want_grid { self.last_grid = Arc::new(self.grid.snapshot()); }
        self.serial += 1;
        let arc = Arc::new(buf);
        self.pool[self.slot] = Some(arc.clone());
        arc
    }

    fn fx(&mut self, buf: &mut [u8]) {
        let s = self.set.clone();
        let (w, h) = (self.w, self.h);
        if s.glitch > 0 && self.rng.f() < s.glitch as f32 * 0.004 + 0.01 {
            let bands = 1 + self.rng.below(3);
            for _ in 0..bands {
                let bh = (2 + (h as f32 * (0.02 + self.rng.f() * 0.09)) as usize).min(h);
                let y0 = self.rng.below(h - bh + 1);
                let dx = self.rng.below(s.glitch as usize + 1) as i32 - s.glitch as i32 / 2;
                if dx == 0 { continue; }
                for y in y0..y0 + bh {
                    let row = &mut buf[y * w * 3..(y + 1) * w * 3];
                    self.rowtmp.copy_from_slice(row);
                    for x in 0..w {
                        let sx = ((x as i32 - dx).rem_euclid(w as i32)) as usize;
                        row[x * 3..x * 3 + 3].copy_from_slice(&self.rowtmp[sx * 3..sx * 3 + 3]);
                    }
                }
            }
            if self.rng.f() < 0.4 {
                let y = self.rng.below(h);
                let row = &mut buf[y * w * 3..(y + 1) * w * 3];
                for px in row.chunks_exact_mut(3) {
                    px[0] = ((px[0] as u16 + s.accent[0] as u16) / 2) as u8;
                    px[1] = ((px[1] as u16 + s.accent[1] as u16) / 2) as u8;
                    px[2] = ((px[2] as u16 + s.accent[2] as u16) / 2) as u8;
                }
            }
        }
        if s.scan > 0 {
            let f = (256.0 * (1.0 - s.scan as f32 / 100.0 * 0.55)) as u32;
            let mut y = 2;
            while y < h {
                let row = &mut buf[y * w * 3..(y + 1) * w * 3];
                for b in row.iter_mut() { *b = ((*b as u32 * f) >> 8) as u8; }
                y += 3;
            }
        }
        if s.noise > 0 {
            let count = s.noise as usize * w * h / 3000;
            let amp = 8 + s.noise as i32 / 2;
            for _ in 0..count {
                let p = self.rng.below(w * h) * 3;
                let d = if self.rng.below(2) == 0 { amp } else { -amp };
                for k in 0..3 { buf[p + k] = (buf[p + k] as i32 + d).clamp(0, 255) as u8; }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rows_job(
    gy0: usize, gy1: usize, pxh: usize,
    out: &mut [u8], raw: &mut [u8], red: &mut [u8],
    rgb: &mut [[u8; 3]], idxs: &mut [u8], cols: &mut [[u8; 3]],
    src: &[u8], sw: usize, _sh: usize, crop: (usize, usize, usize, usize),
    gw: usize, cw: usize, chh: usize, w: usize,
    atlas: &Atlas, set: &Settings, lut: &[u8; 256], seed: u64,
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
    let levels = match set.style { Style::Ascii => 11, Style::Dots => 32, Style::Cells => 16 };

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
                        r += src[i] as u32; g += src[i + 1] as u32; b += src[i + 2] as u32; n += 1;
                    }
                }
            }
            let n = n.max(1);
            let cr = (r / n) as u8; let cg = (g / n) as u8; let cb = (b / n) as u8;
            let lraw = ((54 * cr as u32 + 183 * cg as u32 + 19 * cb as u32) >> 8) as u8;
            let rd = cr.saturating_sub(cg.max(cb));
            raw[ci] = lraw; red[ci] = rd; rgb[ci] = [cr, cg, cb];

            let lv = lut[lraw as usize];
            let v = if inv { 255 - lv } else { lv };

            let mut col = set.ink;
            match set.mode {
                ColorMode::Mono => {}
                ColorMode::Signal => { if rd as i32 > sigcut { col = set.accent; } }
                ColorMode::Heat => { if v > heatcut { col = set.accent; } }
                ColorMode::Duo => { col = mix(set.ink, set.accent, v as f32 / 255.0); }
                ColorMode::Color => {
                    let (mut a, mut b2, mut c2) = (lut[cr as usize], lut[cg as usize], lut[cb as usize]);
                    if inv { a = 255 - a; b2 = 255 - b2; c2 = 255 - c2; }
                    col = [a, b2, c2];
                }
            }
            let mut idx = (v as usize * levels) / 256;
            if idx >= levels { idx = levels - 1; }
            let corrupt = glitchp > 0.0 && rng.f() < glitchp;
            if corrupt {
                col = set.accent;
                if set.style == Style::Ascii { idx = 1 + rng.below(levels - 1); }
            }
            idxs[ci] = idx as u8;
            cols[ci] = col;

            let ox = gx * cw;
            match set.style {
                Style::Ascii => {
                    if idx == 0 { continue; }
                    if set.glow > 0 { blit(out, w, pxh, ox + 1, oy + 1, &atlas.glyphs[idx], dim); }
                    blit(out, w, pxh, ox, oy, &atlas.glyphs[idx], col);
                }
                Style::Dots => {
                    if v < 8 { continue; }
                    if set.glow > 0 { blit(out, w, pxh, ox, oy, &atlas.halos[idx], dim); }
                    blit(out, w, pxh, ox, oy, &atlas.dots[idx], col);
                }
                Style::Cells => {
                    if v < 8 { continue; }
                    let li = (idx * 16) / 32;
                    blit(out, w, pxh, ox, oy, &atlas.cells[li.min(15)], col);
                }
            }
        }
    }
}

pub fn tone_from_levels(p_lo: u8, p_hi: u8, mean: f32, target_mean: f32) -> (f32, f32) {
    let span = (p_hi as f32 - p_lo as f32).max(8.0);
    let cf = clampf(255.0 / span, 0.2, 12.0);
    let bri = target_mean - 128.0 - (mean - 128.0) * cf;
    let c = 255.0 * 259.0 * (cf - 1.0) / (259.0 + 255.0 * cf);
    (c / 2.55, bri / 1.6)
}

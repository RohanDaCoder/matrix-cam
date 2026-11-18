//! Mask atlases: cached glyph / dot / cell sprites keyed by geometry + options.

use crate::config::{GlyphSet, Style};
use crate::render::font;

pub type Span = (i16, i16, i16);

pub struct Mask {
    pub spans: Vec<Span>,
}

pub fn mask_from(cw: usize, ch: usize, pred: impl Fn(usize, usize) -> bool) -> Mask {
    let mut spans = Vec::new();
    for y in 0..ch {
        let mut x = 0;
        while x < cw {
            if pred(x, y) {
                let x0 = x;
                while x < cw && pred(x, y) {
                    x += 1;
                }
                spans.push((y as i16, x0 as i16, x as i16));
            } else {
                x += 1;
            }
        }
    }
    Mask { spans }
}

pub fn blit(out: &mut [u8], w: usize, h: usize, ox: usize, oy: usize, m: &Mask, col: [u8; 3]) {
    for &(dy, x0, x1) in &m.spans {
        let y = oy + dy as usize;
        if y >= h {
            continue;
        }
        let a = ox + x0 as usize;
        if a >= w {
            continue;
        }
        let b = (ox + x1 as usize).min(w);
        let row = &mut out[(y * w + a) * 3..(y * w + b) * 3];
        for px in row.as_chunks_mut::<3>().0 {
            px.copy_from_slice(&col);
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub struct Key(pub usize, pub usize, pub u32, pub Style, pub u32, pub GlyphSet, pub bool);

pub struct Atlas {
    pub key: Key,
    pub cw: usize,
    pub chh: usize,
    pub gw: usize,
    pub gh: usize,
    pub glyphs: Vec<Mask>,
    pub dots: Vec<Mask>,
    pub halos: Vec<Mask>,
    pub cells: Vec<Mask>,
}

/// Build a full atlas for the given geometry / style / glow / character set.
pub fn build(w: usize, h: usize, cols: u32, style: Style, glow: u32, glyphs: GlyphSet, reverse: bool) -> Atlas {
    let cw = (w / cols.max(1) as usize).max(2);
    let chh = if style == Style::Ascii { cw * 2 } else { cw };
    let gw = w / cw;
    let gh = h / chh;

    let mut table: Vec<[u8; 7]> = font::glyph_table(glyphs).to_vec();
    if reverse {
        table.reverse();
    }

    let mut glyph_masks = Vec::with_capacity(table.len());
    for g in &table {
        let g = *g;
        glyph_masks.push(mask_from(cw, chh, move |x, y| {
            (g[y * 7 / chh] >> (4 - x * 5 / cw)) & 1 == 1
        }));
    }

    let mut dots = Vec::new();
    let mut halos = Vec::new();
    let c = (cw as f32 - 1.0) / 2.0;
    for l in 0..32 {
        let v = (l as f32 + 1.0) / 32.0;
        let r = (0.10 + 0.90 * v) * cw as f32 * 0.52;
        let r2 = r * r;
        dots.push(mask_from(cw, cw, move |x, y| {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            dx * dx + dy * dy <= r2
        }));
        let hr = r + 1.0 + glow as f32 / 8.0;
        let hr2 = hr * hr;
        halos.push(mask_from(cw, cw, move |x, y| {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            dx * dx + dy * dy <= hr2
        }));
    }

    let mut cells = Vec::new();
    for l in 0..16 {
        let v = (l as f32 + 1.0) / 16.0;
        let side = (cw as f32 * v.sqrt() * 0.96) as usize;
        let off = (cw - side) / 2;
        cells.push(mask_from(cw, cw, move |x, y| x >= off && x < off + side && y >= off && y < off + side));
    }

    Atlas { key: Key(w, h, cols, style, glow, glyphs, reverse), cw, chh, gw, gh, glyphs: glyph_masks, dots, halos, cells }
}

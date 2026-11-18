
//! Regression: rendering with EVERY (style x glyphset) combination must not
//! panic — the "len is 5 but the index is 8" crash was Ascii + Minimal.

use matrix_core::config::{GlyphSet, Settings, Style};
use matrix_core::render::Renderer;

#[test]
fn every_style_and_glyph_set_renders() {
    for style in Style::ALL {
        for glyphs in GlyphSet::ALL {
            let set = Settings { style, glyphs, ..Settings::preset("ghost") };
            let mut r = Renderer::new(320, 180, set, 1);
            // bright + dim regions so every shade index (incl. the max) is hit
            let sw = 32;
            let sh = 18;
            let mut src = vec![0u8; sw * sh * 3];
            for y in 0..sh {
                for x in 0..sw {
                    let v = ((x * 8 + y * 14) % 256) as u8;
                    let i = (y * sw + x) * 3;
                    src[i] = v;
                    src[i + 1] = (v / 2).max(30);
                    src[i + 2] = (v / 3).max(10);
                }
            }
            let out = r.render(&src, sw, sh);
            assert!(!out.is_empty());
            assert!(out.len() >= 3, "rgb frame");
        }
    }
}

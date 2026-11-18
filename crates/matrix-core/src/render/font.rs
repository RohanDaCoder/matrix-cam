//! Bitmap glyphs for the text style, one 5x7 set per character set.
//! bit 4 = leftmost pixel; rows top to bottom.

/// Classic code ramp " .:-=+*x#%@" (sparse → dense).
pub const GLYPHS_CLASSIC: [[u8; 7]; 11] = [
    [0, 0, 0, 0, 0, 0, 0],                                            // space
    [0, 0, 0, 0, 0, 0b00110, 0b00110],                                // .
    [0, 0b00110, 0b00110, 0, 0b00110, 0b00110, 0],                    // :
    [0, 0, 0, 0b01110, 0, 0, 0],                                      // -
    [0, 0, 0b11111, 0, 0b11111, 0, 0],                                // =
    [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],              // +
    [0, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0],              // *
    [0, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0],              // x
    [0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010],  // #
    [0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011],  // %
    [0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110],  // @
];

/// Simple readable set " .:-=+#@".
pub const GLYPHS_SIMPLE: [[u8; 7]; 8] = [
    [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0b00110, 0b00110],                    // .
    [0, 0b00110, 0b00110, 0, 0b00110, 0b00110, 0],        // :
    [0, 0, 0, 0b01110, 0, 0, 0],                          // -
    [0, 0, 0b11111, 0, 0b11111, 0, 0],                    // =
    [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],  // +
    [0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010], // #
    [0b01110, 0b10001, 0b10101, 0b10101, 0b10101, 0b10001, 0b01110], // @
];

/// Minimal set " .-+#".
pub const GLYPHS_MINIMAL: [[u8; 7]; 5] = [
    [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0b00110, 0b00110],        // .
    [0, 0, 0, 0b01110, 0, 0, 0],              // -
    [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0], // +
    [0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010], // #
];

/// Block shades " ░▒▓█" — density ladders made of filled rows.
pub const GLYPHS_BLOCKS: [[u8; 7]; 5] = [
    [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0b11111, 0b11111, 0b11111],              // bottom third
    [0, 0, 0b11111, 0b11111, 0b11111, 0b11111, 0b11111],  // bottom two thirds
    [0, 0b11111, 0b11111, 0b11111, 0b11111, 0b11111, 0],  // hollow frame
    [0b11111, 0b11111, 0b11111, 0b11111, 0b11111, 0b11111, 0b11111], // full
];

/// Binary set " 01".
pub const GLYPHS_BINARY: [[u8; 7]; 3] = [
    [0, 0, 0, 0, 0, 0, 0],
    [0b01110, 0b10001, 0b00011, 0b00101, 0b01001, 0b10001, 0b01110], // 0
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110], // 1
];

/// Default export kept for compatibility: the classic set.
pub use GLYPHS_CLASSIC as GLYPHS;

/// Terminal ramps per character set (block shades are non-ASCII, kept as str).
pub const RAMP_CLASSIC: &[u8] = b" .:-=+*x#%@";
pub const RAMP_SIMPLE: &[u8] = b" .:-=+#@";
pub const RAMP_MINIMAL: &[u8] = b" .-+#";
pub const RAMP_BLOCKS: &str = " .░▒▓█";
pub const RAMP_BINARY: &[u8] = b" 01";

/// Terminal shade characters for dot / block styles.
pub const SHADES: &[char] = &[' ', '░', '▒', '▓', '█'];

/// The bitmap glyph table for a character set.
use crate::config::GlyphSet;
pub fn glyph_table(set: GlyphSet) -> &'static [[u8; 7]] {
    match set {
        GlyphSet::Classic => &GLYPHS_CLASSIC,
        GlyphSet::Simple => &GLYPHS_SIMPLE,
        GlyphSet::Minimal => &GLYPHS_MINIMAL,
        GlyphSet::Blocks => &GLYPHS_BLOCKS,
        GlyphSet::Binary => &GLYPHS_BINARY,
    }
}

/// The terminal ramp for a character set (UTF-8 bytes; block set includes
/// multi-byte shade glyphs, which the TUI writes as bytes fine).
pub fn ramp(set: GlyphSet) -> &'static [u8] {
    match set {
        GlyphSet::Classic => RAMP_CLASSIC,
        GlyphSet::Simple => RAMP_SIMPLE,
        GlyphSet::Minimal => RAMP_MINIMAL,
        GlyphSet::Blocks => RAMP_BLOCKS.as_bytes(),
        GlyphSet::Binary => RAMP_BINARY,
    }
}

/// Compatibility export for the classic ramp.
pub use RAMP_CLASSIC as RAMP;

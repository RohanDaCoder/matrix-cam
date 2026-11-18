//! The user-facing effect settings, built-in presets and the new character options.

use serde::{Deserialize, Serialize};
use crate::config::Config;

/// Startup overrides injected by the host (CLI flags on desktop, defaults
/// on web). `Settings::resolve` applies them to the saved config — this is
/// the seam that keeps matrix-core free of any argument parsing.
#[derive(Clone, Debug, Default)]
pub struct StartOverrides {
    pub preset: Option<String>,
    pub cols: Option<u32>,
    pub mirror: Option<bool>,
    pub auto: Option<bool>,
    /// 8-bit retro mode (`--retro`): apply the retro look on top.
    pub retro: bool,
}

/// Which character set the text style draws with (new "character settings").
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum GlyphSet {
    #[default]
    Classic, // classic ASCII ramp: " .:-=+*x#%@"
    Simple,  // simple readable set: " .:-=+#@" style
    Minimal, // minimal 4-glyph set
    Blocks,  // unicode block shades
    Binary,  // 0/1 binary look
}

impl GlyphSet {
    pub const ALL: [GlyphSet; 5] = [GlyphSet::Classic, GlyphSet::Simple, GlyphSet::Minimal, GlyphSet::Blocks, GlyphSet::Binary];
    pub fn label(self) -> &'static str {
        match self {
            GlyphSet::Classic => "Classic code",
            GlyphSet::Simple => "Simple",
            GlyphSet::Minimal => "Minimal",
            GlyphSet::Blocks => "Block shades",
            GlyphSet::Binary => "Binary 0/1",
        }
    }
    /// Terminal ramp used by the TUI (block shades are non-ASCII UTF-8).
    pub fn ramp(self) -> &'static str {
        match self {
            GlyphSet::Classic => " .:-=+*x#%@",
            GlyphSet::Simple => " .:-=+#@",
            GlyphSet::Minimal => " .-+#",
            GlyphSet::Blocks => " .░▒▓█",
            GlyphSet::Binary => " 01",
        }
    }
    pub fn from_index(i: usize) -> GlyphSet {
        GlyphSet::ALL[i.min(GlyphSet::ALL.len() - 1)]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Style {
    Dots,
    Ascii,
    Cells,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::Dots, Style::Ascii, Style::Cells];
    pub fn label(self) -> &'static str {
        match self {
            Style::Dots => "Dot matrix",
            Style::Ascii => "Text characters",
            Style::Cells => "Pixel blocks",
        }
    }
    pub fn glyph_count(self) -> usize {
        match self {
            Style::Ascii => 11,
            Style::Dots => 32,
            Style::Cells => 16,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ColorMode {
    Mono,
    Signal,
    Heat,
    Duo,
    Color,
}

impl ColorMode {
    pub const ALL: [ColorMode; 5] = [ColorMode::Mono, ColorMode::Signal, ColorMode::Heat, ColorMode::Duo, ColorMode::Color];
    pub fn label(self) -> &'static str {
        match self {
            ColorMode::Mono => "Single color",
            ColorMode::Signal => "Highlight hotspots",
            ColorMode::Heat => "Bright goes accent",
            ColorMode::Duo => "Two-color blend",
            ColorMode::Color => "Original color",
        }
    }
}

pub const PRESETS: [&str; 5] = ["ghost", "paper", "term", "vhs", "magma"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub style: Style,
    pub mode: ColorMode,
    pub cols: u32,
    pub brightness: i32,
    pub contrast: i32,
    pub threshold: i32,
    pub invert: bool,
    pub mirror: bool,
    pub ink: [u8; 3],
    pub bg: [u8; 3],
    pub accent: [u8; 3],
    pub glow: u32,
    pub trails: u32,
    pub scan: u32,
    pub noise: u32,
    pub glitch: u32,
    /// Character settings: which glyph set the text style uses.
    #[serde(default)]
    pub glyphs: GlyphSet,
    /// Character settings: reverse the glyph ramp (dark-on-light feel).
    #[serde(default)]
    pub reverse: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings::preset("ghost")
    }
}

impl Settings {
    pub fn preset(name: &str) -> Settings {
        let mut s = Settings {
            style: Style::Dots,
            mode: ColorMode::Signal,
            cols: 150,
            brightness: -4,
            contrast: 22,
            threshold: 70,
            invert: false,
            mirror: true,
            ink: [230, 227, 218],
            bg: [4, 5, 7],
            accent: [255, 38, 38],
            glow: 6,
            trails: 0,
            scan: 16,
            noise: 12,
            glitch: 16,
            glyphs: GlyphSet::Classic,
            reverse: false,
        };
        match name {
            "paper" => {
                s.style = Style::Ascii;
                s.cols = 170;
                s.brightness = 4;
                s.contrast = 30;
                s.threshold = 64;
                s.invert = true;
                s.ink = [25, 26, 28];
                s.bg = [241, 239, 233];
                s.accent = [224, 21, 21];
                s.glow = 0;
                s.scan = 30;
                s.noise = 18;
                s.glitch = 30;
            }
            "term" => {
                s.style = Style::Ascii;
                s.mode = ColorMode::Mono;
                s.cols = 128;
                s.brightness = 0;
                s.contrast = 18;
                s.invert = false;
                s.ink = [65, 255, 126];
                s.bg = [2, 6, 4];
                s.accent = [186, 255, 208];
                s.glow = 8;
                s.trails = 26;
                s.scan = 28;
                s.noise = 10;
                s.glitch = 8;
            }
            "vhs" => {
                s.style = Style::Ascii;
                s.mode = ColorMode::Color;
                s.cols = 100;
                s.brightness = 2;
                s.contrast = 16;
                s.invert = false;
                s.ink = [255, 255, 255];
                s.bg = [7, 3, 16];
                s.accent = [255, 47, 158];
                s.glow = 5;
                s.trails = 30;
                s.scan = 44;
                s.noise = 28;
                s.glitch = 42;
            }
            "magma" => {
                s.style = Style::Dots;
                s.mode = ColorMode::Heat;
                s.cols = 140;
                s.brightness = -6;
                s.contrast = 26;
                s.threshold = 58;
                s.invert = false;
                s.ink = [138, 43, 14];
                s.bg = [10, 4, 5];
                s.accent = [255, 207, 63];
                s.glow = 10;
                s.trails = 12;
                s.scan = 10;
                s.noise = 14;
                s.glitch = 10;
            }
            _ => {}
        }
        s
    }

    /// Pick starting settings: host preset > saved settings > built-in default,
    /// then apply the cols / mirror overrides and the 8-bit retro look.
    pub fn resolve(cfg: &mut Config, ov: &StartOverrides) -> anyhow::Result<Settings> {
        let mut set = cfg.set.clone().unwrap_or_else(|| Settings::preset("ghost"));
        if let Some(p) = &ov.preset {
            if let Some(up) = cfg.user_presets.iter().find(|u| &u.name == p) {
                set = up.set.clone();
            } else if PRESETS.contains(&p.as_str()) {
                set = Settings::preset(p);
            } else {
                anyhow::bail!("unknown preset '{p}'");
            }
        }
        if let Some(c) = ov.cols {
            set.cols = c.clamp(40, 220);
        }
        if let Some(m) = ov.mirror {
            set.mirror = m;
        }
        if let Some(a) = ov.auto {
            cfg.auto.enabled = a;
        }
        if ov.retro {
            set.apply_retro();
        }
        Ok(set)
    }

    pub fn bg_is_light(&self) -> bool {
        let b = self.bg;
        (54 * b[0] as u32 + 183 * b[1] as u32 + 19 * b[2] as u32) >> 8 > 140
    }

    /// 8-bit retro look, applied on top of the current settings: chunky
    /// pixel blocks, halved detail, stronger CRT scan/static/glitch and
    /// phosphor trails. Colors are kept, so a green "term" look stays green.
    /// Used by `--retro`, the GUI *8-bit retro* button and the TUI row.
    pub fn apply_retro(&mut self) {
        self.style = Style::Cells;
        self.cols = (self.cols / 2).max(56);
        self.glow = (self.glow + 4).min(24);
        self.scan = 35.max(self.scan);
        self.noise = 18.max(self.noise);
        self.glitch = 20.max(self.glitch);
        self.trails = 25.max(self.trails);
    }
}

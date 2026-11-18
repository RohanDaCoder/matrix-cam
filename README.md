# matrix-cam 2.2 — Windows 11

Webcam → dot-matrix / character glitch effect → a real camera device. GUI **and** TUI,
live preview, plain-language settings, character-set options, a retro FPS limiter,
a one-click 8-bit look, a built-in **self-updater**, and an adaptive auto mode.
No overlays, no scenes.

## Setup (once)
1. Windows 11 build 22000+, Rust stable MSVC, VS 2022 Build Tools (C++ workload).
2. Register the virtual-camera runtime:
   ```bat
   git clone https://github.com/Jagirium/virtualcam
   cd virtualcam && install.bat
   ```
   Run unelevated; it asks for one UAC prompt.
3. Build: `cargo build --release` (or grab `matrix-cam.exe` from the latest
   GitHub Actions run artifact).

## Run
```bat
target\release\matrix-cam.exe                 :: GUI (default)
target\release\matrix-cam.exe --tui           :: terminal UI
target\release\matrix-cam.exe --headless      :: no UI
target\release\matrix-cam.exe --retro         :: 8-bit retro look + 12fps cap
target\release\matrix-cam.exe --check-update   :: newest release info
target\release\matrix-cam.exe --update        :: self-update + restart
target\release\matrix-cam.exe --list          :: devices + backends
```
Pick the **VCam** device in Teams/Zoom/OBS/Chrome.

## Project layout
```
src/
  cli.rs           argument parsing, --help
  main.rs          startup order + non-Windows stub
  config/
    settings.rs    Settings, presets, character sets (GlyphSet)
    auto.rs        AutoConfig (what auto may touch)
    mod.rs         Config (persisted JSON)
  state.rs         Shared/Inner state + Status/View/FpsLimit
  render/
    renderer.rs    fused parallel pass (downsample → tone → draw)
    atlas.rs       cached glyph/dot/cell masks
    font.rs        5x7 bitmap glyph tables per character set
    grid.rs        cell grid shared with analyzer + TUI
    tone.rs        brightness/contrast LUT
    fx.rs          glitch / scanlines / noise
  auto/
    analyze.rs     histogram, detail, motion, redness stats
    controller.rs  adaptive controller (glide + interval)
  pipeline.rs      camera → render → virtual camera loop (traits + FPS limiter)
  platform/        Windows-only nokhwa + virtualcam plumbing
  updater.rs       self-updater (releases check, download, swap, restart)
  ui/
    gui.rs         egui front-end (settings redesign)
    tui.rs         crossterm front-end
    widgets.rs     shared setting widgets (tooltips, auto-aware)
    theme.rs       colors + text gauges
```
Camera/virtual-camera crates are gated to `cfg(windows)`; everything else
type-checks on any OS with plain `cargo check`.

## Settings (renamed)
Old niche names → plain names:

| old | new |
|---|---|
| style | Shape style (Dot matrix / Text characters / Pixel blocks) |
| density / cols | Detail |
| threshold | Accent amount |
| ink / paper / signal | Main color / Background / Accent color |
| mode (mono/signal/heat/duo/color) | Color mode (Single color / Highlight hotspots / …) |
| scan | Scanlines |
| trails | Ghost trails |
| noise | Static |
| glow / glitch | Glow / Glitch |
| auto levels / density / signal / fx / polarity | Auto brightness / detail / accent / glitch-fx / invert |
| target mean / low pct / high pct | Target brightness / black point / white point |
| response ms / interval ms | Smoothing / recompute interval (under "Advanced auto tuning") |

Every control has a hover tooltip explaining what it does.

## Character settings
In **LOOK** (GUI) or the `characters` row (TUI):
- **Characters** — which glyph set the text style draws with:
  Classic code (` .:-=+*x#%@`), Simple, Minimal, Block shades (`░▒▓█`), Binary (`0`/`1`).
- **Reverse character order** — flips the ramp so bright areas get sparse glyphs.

## Retro FPS
**RETRO FPS** section (GUI) or `limit fps` + `fps value` rows (TUI, hotkey `f`
toggles). Cap the output frame rate (1–60, default 12) for that choppy 8-bit
pixel-game feel. Persisted in the config; the top bar shows `RETRO 12fps` while
active. The limiter paces the actual pipeline output, so the virtual camera
itself runs at the capped rate.

## 8-bit retro mode
One click (GUI **8-bit retro** button in LOOK, TUI `r`, CLI `--retro`):
pixel-block shape, halved detail, stronger scanlines/static/glitch and phosphor
trails — colors are kept, so a green "term" look stays green. Stack it with the
retro FPS limiter for the full early-console experience.

## Self-updater
Checks the GitHub releases of `rohandacoder/matrix-cam` and installs the newest
`matrix-cam.exe` asset in place (rename-then-swap, with rollback if the move
fails; the replaced exe is kept as `matrix-cam.exe.old` until the next start).
- On start (config `update_on_start`, on by default) a background check runs;
  the GUI **UPDATES** panel and the TUI status line show the result.
- GUI: **check now**, or **download & restart** with release notes shown first.
- TUI: `u` checks, `U` downloads + installs + restarts.
- CLI: `--check-update` prints the newest release; `--update` installs and
  restarts; `--no-update-check` skips the on-start check.
Updates only ever move forward (real semver comparison, `v`-prefix ok,
prereleases handled); a release with no `.exe` asset is never offered.

## Auto adjust
Analyses the live grid every frame (histogram, detail, motion, redness) and drives:

| subsystem | what it does |
|---|---|
| **levels** | percentile stretch → brightness/contrast, mean parked on *target brightness* |
| **density** | scene detail → column count between *detail min/max* |
| **signal** | redness distribution → accent amount so only *accent coverage* of cells go red |
| **fx** | motion → glitch + trails; darkness → static floor |
| **polarity** | picks invert by measuring which polarity yields more visible glyphs (8% hysteresis) |

Tune it with *smoothing* and *recompute interval* (Advanced auto tuning). Auto-owned
sliders show the live value with an `A` marker and are read-only; your manual base
values are preserved and return when you disable auto.

## Customisation
- Config: `%APPDATA%\matrix-cam\config.json` (or `--config PATH`), saved on exit / `w`.
- User presets ("looks"): name + **save look** in the GUI, `W` in the TUI; load with the chips or `6`–`9`.
- Every colour is three channel sliders; every FX is live.

## Performance
- One fused parallel pass per frame (downsample → tone LUT → draw), `std::thread::scope`
  workers, `--threads N` (default cores, max 8).
- Tone is a 256-entry LUT, not per-pixel float math.
- Frame buffers are `Arc` ping-pong — no per-frame allocation or copy for the preview.
- TUI reuses the renderer's cell grid (no second render) and only redraws changed rows.
- GUI repaints at `preview_fps`, uploads a texture only when the frame serial changes.
- Mask atlases rebuild only when detail/style/glow/character-set change; auto quantises detail to 4.

## CI / Releases
- `windows-build` (on push to main): native `windows-latest` MSVC build; unit
  tests first, then `cargo build --release --locked`, PE-magic verification and
  the `matrix-cam-windows-x64` artifact.
- `release` (on pushing a `v*` tag): same native build + tests, packages
  `matrix-cam.exe` + zip + sha256 and publishes the GitHub release via
  `softprops/action-gh-release` with generated release notes. The self-updater
  installs exactly these release assets.

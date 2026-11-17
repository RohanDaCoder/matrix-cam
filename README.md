# matrix-cam 2.0 — Windows 11

Webcam → dot-matrix / ASCII glitch effect → a real camera device. GUI **and** TUI,
live preview, full settings, and an adaptive auto mode. No overlays, no scenes.

## Setup (once)
1. Windows 11 build 22000+, Rust stable MSVC, VS 2022 Build Tools (C++ workload).
2. Register the virtual-camera runtime:
   ```bat
   git clone https://github.com/Jagirium/virtualcam
   cd virtualcam && install.bat
   ```
   Run unelevated; it asks for one UAC prompt.
3. Build: `cargo build --release`

## Run
```bat
target\release\matrix-cam.exe                 :: GUI (default)
target\release\matrix-cam.exe --tui           :: terminal UI
target\release\matrix-cam.exe --headless      :: no UI
target\release\matrix-cam.exe --list          :: devices + backends
```
Pick the **VCam** device in Teams/Zoom/OBS/Chrome.

## Auto adjust
Analyses the live grid every frame (histogram, detail, motion, redness) and drives:

| subsystem | what it does |
|---|---|
| **levels** | percentile stretch → brightness/contrast, mean parked on *target mean* |
| **density** | scene detail → column count between *cols min/max* |
| **signal** | redness distribution → threshold so only *accent %* of cells go red |
| **fx** | motion → glitch + trails; darkness → noise floor |
| **polarity** | picks invert by measuring which polarity yields more visible glyphs (8% hysteresis) |

Tune it with *response ms* (smoothing), *interval ms* (target recompute rate), and the
per-subsystem checkboxes. Auto-owned sliders show the live value with an `A` marker and
are read-only; your manual base values are preserved and return when you disable auto.

## Customisation
- Config: `%APPDATA%\matrix-cam\config.json` (or `--config PATH`), saved on exit / `w`.
- User presets: name + **save** in the GUI, `W` in the TUI; load with the chips or `6`–`9`.
- Every colour is three channel sliders; every FX is live.

## Performance
- One fused parallel pass per frame (downsample → tone LUT → draw), `std::thread::scope`
  workers, `--threads N` (default cores, max 8).
- Tone is a 256-entry LUT, not per-pixel float math.
- Frame buffers are `Arc` ping-pong — no per-frame allocation or copy for the preview.
- TUI reuses the renderer's cell grid (no second render) and only redraws changed rows.
- GUI repaints at `preview_fps`, uploads a texture only when the frame serial changes.
- Mask atlases rebuild only when density/style/glow change; auto quantises density to 4.

## CI
GitHub Actions builds the Windows release binary natively (workflow `windows-build`:
`windows-latest` runner, Rust stable MSVC). Download `matrix-cam.exe` from the latest
run artifact.

# matrix-cam 2.5 — Windows 11 + Web

Webcam → dot-matrix / character glitch effect → a real camera device. GUI **and** TUI,
live preview, plain-language settings, character-set options, a retro FPS limiter,
a one-click 8-bit look, a built-in **self-updater**, and an adaptive auto mode.
No overlays, no scenes.

**New in 2.3:** a multi-crate Cargo workspace (engine / shared GUI / desktop /
web) and a **web version** of the same app — try it live at
`https://rohandacoder.github.io/matrix-cam/` (camera + effects in the browser,
settings saved to localStorage). Desktop and web run the identical engine and
UI code.

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

## Project layout (Cargo workspace)
```
crates/
  matrix-core/     the engine — platform neutral, no UI, no arg parsing
    config/         Settings, presets, character sets, AutoConfig, Config JSON
    render/         renderer (fused parallel pass), atlas, font, grid, tone, fx
    auto/           analyze (histogram/detail/motion/redness) + adaptive controller
    state.rs        Shared/Inner + Status/View/FpsLimit
    pipeline.rs      camera → render → sink worker loop (traits + FPS limiter)
    engine.rs        per-tick engine — the loop body shared by desktop AND web
    time.rs          Instant/Duration alias (web_time on wasm32, std elsewhere)
    update.rs        UpdateState model + semver compare (shared UI vocabulary)
  matrix-gui/      the egui front-end — one codebase, two hosts
    gui.rs           GuiApp + `Store` (persistence seam) + `UpdaterHost` seam
    widgets.rs theme.rs
  matrix-cli/      the Windows desktop app
    main.rs          startup order, FileStore, DesktopUpdater, native window
    cli.rs tui.rs updater.rs platform.rs (nokhwa + virtualcam, cfg(windows))
  matrix-web/      the browser app (eframe wasm + trunk)
    main.rs          getUserMedia camera, localStorage Store, no-op UpdaterHost,
                     engine tick per repaint
web/index.html    trunk entry (canvas #matrix-cam)
Trunk.toml         trunk build config (wasm32, dist/, public_url /matrix-cam/)
```
The engine renders rows in `std::thread::scope` workers on desktop and inline
on a single thread on wasm — same code, `cfg`-selected path. Camera/virtual-camera
crates are gated to `cfg(windows)`; matrix-web's `main` is gated to
`cfg(target_arch = "wasm32")`. Everything type-checks on any OS with plain
`cargo check`.

## Web version
The same app compiled to WebAssembly: identical settings panel, presets,
auto adjust, retro look and FPS limiter; the preview loop runs `Engine::tick`
once per repaint instead of a worker thread.
- **URL:** `https://rohandacoder.github.io/matrix-cam/` (auto-deployed by the
  `web-deploy` workflow on every push to main).
- **Camera:** browser `getUserMedia` — click once in the window to grant
  permission (browsers require a gesture). Served over HTTPS by GitHub Pages.
- **Persistence:** settings live in `localStorage["matrix-cam-config"]` —
  the same JSON model the desktop writes to `%APPDATA%`.
- **No self-updater:** the site re-deploys on release, so the web build is
  always current.
- **Run locally:** `trunk serve` then open http://localhost:8080 (camera needs
  localhost/HTTPS).
- **Crash reports:** anything that goes wrong — a Rust panic, a JS error,
  a rejected promise, a camera that dies mid-stream — is captured with its
  message, stack trace, platform (user agent / OS build), camera name and a
  rolling log of recent warnings. A popup shows the full report with a
  copy-to-clipboard button, so every bug report pasted into an issue has
  everything needed to fix it.
- **Color picker:** the COLORS section uses a proper picker (hue/saturation
  square, RGB and hex fields) instead of three raw RGB sliders.
- **Android / iOS:** tap the page once to grant camera access — the preview
  starts automatically once the camera delivers frames. Phones start with
  the settings panel collapsed (⚙ settings shows it); the preview owns the
  full screen. If the camera is denied, the top bar shows the reason and a
  reload restarts the flow.

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
  tests first, then `cargo build --release --locked --workspace --exclude
  matrix-web`, PE-magic verification and the `matrix-cam-windows-x64` artifact.
- `web-deploy` (on push to main): ubuntu trunk build for `wasm32-unknown-unknown`,
  artifact smoke-check, then deploy `dist/` to GitHub Pages
  (`actions/configure-pages` + `actions/deploy-pages`, workflow-sourced Pages).
- `release` (on pushing a `v*` tag): same native build + tests, packages
  `matrix-cam.exe` + zip + sha256 and publishes the GitHub release via
  `softprops/action-gh-release` with generated release notes. The self-updater
  installs exactly these release assets.

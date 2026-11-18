//! Self-updater: checks GitHub releases, downloads the newest Windows binary,
//! and (on Windows) swaps the running exe in place.
//!
//! Flow: `GET /repos/rohandacoder/matrix-cam/releases/latest` → compare tags
//! with real semver rules → download the `matrix-cam.exe` asset → replace the
//! running exe (rename-then-swap: Windows locks a running executable, but a
//! running exe *can* be renamed) → restart with the same arguments.
//!
//! Everything except the actual exe swap is platform neutral, so the whole
//! module type-checks (and its parsing/comparison logic is unit-tested) on any
//! OS; the swap restarts the app only on Windows.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::sync::{Arc, Mutex};

use matrix_core::state::{Inner, Shared};

pub const OWNER: &str = "rohandacoder";
pub const REPO: &str = "matrix-cam";
/// Version of this build — reported by the desktop updater.
pub use matrix_core::update::CURRENT;
/// Update-state model + label (moved to `matrix_core::update` so the web
/// front-end can display it too).
pub use matrix_core::update::{UpdateState, is_newer};

/// The release we would install.
#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct Release {
    tag_name: Option<String>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    #[serde(default)]
    browser_download_url: Option<String>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
}

fn http_get_json(url: &str) -> Result<String> {
    let resp = agent()
        .get(url)
        .set("User-Agent", &format!("{OWNER}/{REPO}-updater"))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| anyhow::anyhow!("request failed: {e}"))?;
    let mut buf = String::new();
    resp.into_reader().read_to_string(&mut buf).context("reading response")?;
    Ok(buf)
}

/// Latest published release, or `None` when there is no release yet.
fn fetch_latest_release() -> Result<Option<Release>> {
    let url = format!("https://api.github.com/repos/{OWNER}/{REPO}/releases/latest");
    let body = http_get_json(&url)?;
    let rel: Release = serde_json::from_str(&body).context("parsing release JSON")?;
    // "no releases yet" arrives as a JSON error object without tag_name.
    if rel.tag_name.is_none() {
        return Ok(None);
    }
    Ok(Some(rel))
}

/// Check GitHub for a newer release. `Ok(None)` = already up to date.
pub fn check() -> Result<Option<ReleaseInfo>> {
    let Some(rel) = fetch_latest_release()? else {
        return Ok(None);
    };
    let tag = rel.tag_name.unwrap_or_default();
    if !is_newer(&tag, CURRENT) {
        return Ok(None);
    }
    // Prefer the raw exe asset; fall back to any .exe the release carries.
    let asset = rel
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("matrix-cam.exe"))
        .or_else(|| rel.assets.iter().find(|a| a.name.to_lowercase().ends_with(".exe")));
    let Some(a) = asset else {
        bail!("release {tag} has no .exe asset to install");
    };
    let Some(url) = &a.browser_download_url else {
        bail!("release asset '{}' has no download URL", a.name);
    };
    Ok(Some(ReleaseInfo {
        version: tag,
        notes: rel.body,
        url: url.clone(),
        size: a.size,
    }))
}

/// Download `url` to `dest`, reporting 0-100 progress through `on_progress`.
pub fn download(url: &str, dest: &Path, mut on_progress: impl FnMut(u32)) -> Result<()> {
    let resp = agent()
        .get(url)
        .set("User-Agent", &format!("{OWNER}/{REPO}-updater"))
        .call()
        .map_err(|e| anyhow::anyhow!("download failed: {e}"))?;
    let total: u64 = resp
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut reader = resp.into_reader();
    let mut file = std::io::BufWriter::new(std::fs::File::create(dest).context("creating temp file")?);
    let mut buf = vec![0u8; 64 * 1024];
    let mut done: u64 = 0;
    loop {
        let n = reader.read(&mut buf).context("download stream")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if total > 0 {
            on_progress(((done.saturating_mul(100)) / total).min(100) as u32);
        }
    }
    file.flush().context("finishing download")?;
    Ok(())
}

/// Where a downloaded exe waits before being swapped in (next to the real exe,
/// so the final move never crosses a volume).
fn staging_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("locating the running exe")?;
    Ok(exe.with_extension("exe.new"))
}

/// Remove a stale updater backup/staging file next to the exe (call at start).
pub fn cleanup_stale() {
    for ext in ["exe.new", "exe.old"] {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::fs::remove_file(exe.with_extension(ext));
        }
    }
}

/// Replace the running exe with `new_file` (Windows: rename aside, move in,
/// roll back on failure). On other platforms this is refused.
pub fn install(new_file: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().context("locating the running exe")?;
        let backup = exe.with_extension("exe.old");
        let _ = std::fs::remove_file(&backup);
        std::fs::rename(&exe, &backup).context("moving the running exe aside")?;
        match std::fs::rename(new_file, &exe) {
            Ok(()) => {
                // Keep the backup until the next start, in case the new build
                // cannot even boot (user can rename it back manually).
                Ok(())
            }
            Err(e) => {
                let _ = std::fs::rename(&backup, &exe); // roll back
                Err(anyhow::Error::new(e).context("installing the new exe"))
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = new_file;
        bail!("installing a new build only works on Windows");
    }
}

/// Re-exec ourselves with the same arguments. Never returns.
pub fn restart() -> ! {
    let exe = std::env::current_exe()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| PathBuf::from("matrix-cam.exe"));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let _ = std::process::Command::new(exe).args(args).spawn();
    std::process::exit(0);
}

/// Full headless update: check → download → install → restart. Exits the
/// process with a status code; used by `--update` and the GUI/TUI buttons.
pub fn update_now() -> ! {
    match check() {
        Err(e) => {
            eprintln!("update check failed: {e:#}");
            std::process::exit(1);
        }
        Ok(None) => {
            println!("matrix-cam {CURRENT} — already up to date");
            std::process::exit(0);
        }
        Ok(Some(rel)) => {
            println!("updating to {} …", rel.version);
            let stage = match staging_path() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{e:#}");
                    std::process::exit(1);
                }
            };
            let ver = rel.version.clone();
            if let Err(e) = download(&rel.url, &stage, |pct| {
                println!("  {pct:>3}%");
            }) {
                eprintln!("{e:#}");
                std::process::exit(1);
            }
            if let Err(e) = install(&stage) {
                eprintln!("{e:#}");
                std::process::exit(1);
            }
            println!("installed {ver} — restarting");
            restart();
        }
    }
}

/// One-shot check requested from a UI ("check now" button / TUI `u` key):
/// re-checks and fills `Inner::update`. Runs on its own thread so the UI
/// never blocks on the network.
pub fn spawn_check_now(shared: Arc<Shared>) {
    let _ = std::thread::spawn(move || {
        let state = match check() {
            Ok(None) => UpdateState::Latest { version: CURRENT.to_string() },
            Ok(Some(r)) => UpdateState::Available { version: r.version, notes: r.notes, size: r.size },
            Err(e) => UpdateState::Failed { why: format!("{e:#}") },
        };
        shared.inner.lock().unwrap().update = state;
    });
}

/// Background check thread: fills `Inner::update` so the GUI/TUI can show the
/// result without blocking. Spawns from `platform::start` when
/// `cfg.update_on_start` is on and `--no-update-check` was not given.
pub fn spawn_check(shared: Arc<Shared>) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            let state = match check() {
                Ok(None) => UpdateState::Latest { version: CURRENT.to_string() },
                Ok(Some(r)) => UpdateState::Available { version: r.version, notes: r.notes, size: r.size },
                Err(e) => UpdateState::Failed { why: format!("{e:#}") },
            };
            shared.inner.lock().unwrap().update = state;
        })
        .expect("spawn update-check thread")
}

/// Install + restart from a UI: stages the download, swaps the exe, and exits
/// the process (the new process continues the session). Status is written to
/// `Inner::update` as it progresses.
pub fn ui_install(shared: Arc<Shared>) {
    let state = { shared.inner.lock().unwrap().update.clone() };
    let UpdateState::Available { version, .. } = state else {
        return;
    };
    shared.inner.lock().unwrap().update = UpdateState::Downloading { version: version.clone(), pct: 0 };
    let stage = match staging_path() {
        Ok(p) => p,
        Err(e) => {
            shared.inner.lock().unwrap().update = UpdateState::Failed { why: format!("{e:#}") };
            return;
        }
    };
    match check() {
        Ok(Some(r)) => {
            if let Err(e) = download(&r.url, &stage, |pct| {
                shared.inner.lock().unwrap().update =
                    UpdateState::Downloading { version: version.clone(), pct };
            }) {
                shared.inner.lock().unwrap().update = UpdateState::Failed { why: format!("{e:#}") };
                return;
            }
        }
        Ok(None) => {
            shared.inner.lock().unwrap().update = UpdateState::Latest { version: CURRENT.into() };
            return;
        }
        Err(e) => {
            shared.inner.lock().unwrap().update = UpdateState::Failed { why: format!("{e:#}") };
            return;
        }
    }
    if let Err(e) = install(&stage) {
        shared.inner.lock().unwrap().update = UpdateState::Failed { why: format!("{e:#}") };
        return;
    }
    shared.inner.lock().unwrap().update = UpdateState::Ready { version };
    // Give the UI a beat to show "installed — restart to apply", then restart.
    std::thread::sleep(Duration::from_millis(1500));
    shared.inner.lock().unwrap().quit = true;
    shared.inner.lock().unwrap().update = UpdateState::Ready { version: String::new() };
    restart();
}

/// `--check-update`: print the result, exit 0/1 (1 only on network errors).
pub fn cli_check() -> i32 {
    match check() {
        Ok(None) => {
            println!("matrix-cam {CURRENT} — up to date");
            0
        }
        Ok(Some(r)) => {
            println!("matrix-cam {CURRENT} → {} available", r.version);
            if !r.notes.is_empty() {
                println!();
                println!("{}", r.notes.trim());
            }
            println!();
            println!("run `matrix-cam --update` to install and restart");
            0
        }
        Err(e) => {
            eprintln!("update check failed: {e:#}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_json_parses() {
        let body = r#"{
            "tag_name": "v2.2.0",
            "body": "notes here",
            "assets": [
                {"name": "matrix-cam-windows-x64.zip", "size": 6000000,
                 "browser_download_url": "https://example.invalid/zip"},
                {"name": "matrix-cam.exe", "size": 5972480,
                 "browser_download_url": "https://example.invalid/exe"}
            ]
        }"#;
        let rel: Release = serde_json::from_str(body).unwrap();
        assert_eq!(rel.tag_name.as_deref(), Some("v2.2.0"));
        assert_eq!(rel.assets.len(), 2);
        assert!(rel.assets[1].browser_download_url.is_some());
    }

    #[test]
    fn no_release_body_is_none() {
        let body = r#"{"message":"Not Found","documentation_url":"x"}"#;
        let rel: Release = serde_json::from_str(body).unwrap();
        assert!(rel.tag_name.is_none());
    }
}

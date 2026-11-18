//! Crash / error reporting shared by every host.
//!
//! One place assembles everything a fix needs: the error itself, the
//! backtrace/stack, the engine state at the time, and a rolling log of the
//! most recent messages (warnings included — they usually lead straight to
//! the crash). Hosts push reports from panic hooks and window.onerror-style
//! handlers; the GUI shows a popup with the report rendered as text and a
//! copy button so users can paste it into an issue.
//!
//! Deliberately dependency-free and allocation-light: the capture path may
//! itself run inside a broken process.

use std::sync::Mutex;
use std::sync::OnceLock;

/// Anything the hosts wants attached to a report verbatim.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// Host identity: "windows-desktop" / "web".
    pub host: String,
    /// App version string.
    pub version: String,
    /// Raw platform line (OS name, user agent, viewport — host-supplied).
    pub platform: String,
    /// Camera/backend info (selected camera name, backend kind).
    pub camera: String,
    /// Extra key/values the host considers useful (page URL, uptime...).
    pub extra: Vec<(String, String)>,
}

impl Context {
    pub fn kv(mut self, k: &str, v: impl Into<String>) -> Self {
        self.extra.push((k.to_string(), v.into()));
        self
    }
}

/// A captured error or crash.
#[derive(Clone, Debug)]
pub struct Report {
    /// "panic" for Rust panics, "error" for JS/window errors, "warning" for
    /// recovered-but-noteworthy conditions.
    pub kind: Kind,
    /// The error message itself.
    pub message: String,
    /// Stack trace / backtrace as text (may be empty when unavailable).
    pub stack: String,
    /// Where it came from (Rust module path, JS file:line).
    pub origin: String,
    /// Context snapshot at capture time.
    pub context: Context,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Panic,
    Error,
    Warning,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Panic => "panic",
            Kind::Error => "error",
            Kind::Warning => "warning",
        }
    }
}

/// Rolling log ring: the last `LOG_CAP` messages, newest last.
const LOG_CAP: usize = 64;

struct LogRing {
    entries: Vec<String>,
    next: usize,
}

impl LogRing {
    fn new() -> Self {
        LogRing { entries: Vec::with_capacity(LOG_CAP), next: 0 }
    }

    fn push(&mut self, line: String) {
        if self.entries.len() < LOG_CAP {
            self.entries.push(line);
        } else {
            self.entries[self.next] = line;
            self.next = (self.next + 1) % LOG_CAP;
        }
    }

    /// Entries in chronological order (oldest first). Once the ring has
    /// wrapped, slot order no longer equals time order — reorder from the
    /// write cursor.
    fn snapshot(&self) -> Vec<String> {
        if self.entries.len() < LOG_CAP {
            self.entries.clone()
        } else {
            let rotated: Vec<String> = self.entries[self.next..].to_vec();
            let head: Vec<String> = self.entries[..self.next].to_vec();
            [rotated, head].concat()
        }
    }
}

static REPORTS: OnceLock<Mutex<Vec<Report>>> = OnceLock::new();
static LOG: OnceLock<Mutex<LogRing>> = OnceLock::new();
static CONTEXT: OnceLock<Mutex<Context>> = OnceLock::new();

fn reports() -> &'static Mutex<Vec<Report>> {
    REPORTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn log_ring() -> &'static Mutex<LogRing> {
    LOG.get_or_init(|| Mutex::new(LogRing::new()))
}

/// The context hosts install once at startup (and may update later, e.g.
/// when the camera opens).
pub fn set_context(f: impl FnOnce(&mut Context)) {
    let m = CONTEXT.get_or_init(|| Mutex::new(Context::default()));
    if let Ok(mut c) = m.lock() {
        f(&mut c);
    }
}

/// Read a copy of the current context.
pub fn context() -> Context {
    let m = CONTEXT.get_or_init(|| Mutex::new(Context::default()));
    m.lock().map(|c| c.clone()).unwrap_or_default()
}

/// Append a line to the rolling log. Called by the logger sinks (warn!)
/// and hosts for notable events ("camera opened: HD Camera").
pub fn log_line(line: impl Into<String>) {
    if let Ok(mut ring) = log_ring().lock() {
        ring.push(line.into());
    }
}

/// Capture a report. This is the one entry point every error path funnels
/// through; it also leaves a log line so the ring tells the story even if
/// nobody opens the popup.
pub fn capture(kind: Kind, message: impl Into<String>, stack: impl Into<String>, origin: impl Into<String>) {
    let message = message.into();
    let stack = stack.into();
    let origin = origin.into();
    {
        let ctx = context();
        let mut line = String::with_capacity(128);
        line.push('[');
        line.push_str(kind.label());
        line.push_str("] ");
        line.push_str(&message);
        if !origin.is_empty() {
            line.push_str(" @ ");
            line.push_str(&origin);
        }
        log_line(line.clone());
        let _ = line; // also kept in reports
        let rep = Report { kind, message, stack, origin, context: ctx };
        if let Ok(mut v) = reports().lock() {
            if v.len() < 32 {
                v.push(rep);
            } else {
                // keep the most recent: drop the oldest
                v.remove(0);
                v.push(rep);
            }
        }
    }
}

/// All captured reports, oldest first.
pub fn reports_snapshot() -> Vec<Report> {
    reports().lock().map(|v| v.clone()).unwrap_or_default()
}

/// The rolling log, oldest first.
pub fn log_snapshot() -> Vec<String> {
    log_ring().lock().map(|r| r.snapshot()).unwrap_or_default()
}

/// Has anything been captured (drives the "⚠ N" indicator in the top bar)?
pub fn count() -> usize {
    reports().lock().map(|v| v.len()).unwrap_or(0)
}

/// Drop all captured reports and log lines (after "copy & dismiss").
pub fn clear() {
    if let Ok(mut v) = reports().lock() {
        v.clear();
    }
    if let Ok(mut r) = log_ring().lock() {
        *r = LogRing::new();
    }
}

impl Report {
    /// Render the report as the text users paste into an issue.
    /// Everything a fix needs: what happened, where, the stack, the state
    /// of the app, and the recent log.
    pub fn to_text(&self, log: &[String]) -> String {
        let mut s = String::with_capacity(1024);
        s.push_str(&format!("matrix-cam {} report\n", self.kind.label()));
        s.push_str("=======================\n");
        s.push_str(&format!("message: {}\n", self.message));
        if !self.origin.is_empty() {
            s.push_str(&format!("origin:  {}\n", self.origin));
        }
        let c = &self.context;
        if !c.host.is_empty() {
            s.push_str(&format!("host:    {}\n", c.host));
        }
        if !c.version.is_empty() {
            s.push_str(&format!("version: {}\n", c.version));
        }
        if !c.platform.is_empty() {
            s.push_str(&format!("platform: {}\n", c.platform));
        }
        if !c.camera.is_empty() {
            s.push_str(&format!("camera:  {}\n", c.camera));
        }
        for (k, v) in &c.extra {
            s.push_str(&format!("{k}: {v}\n"));
        }
        if !self.stack.is_empty() {
            s.push_str("\nstack trace:\n");
            for l in self.stack.lines().take(40) {
                s.push_str(&format!("  {l}\n"));
            }
        }
        if !log.is_empty() {
            s.push_str("\nrecent log:\n");
            for l in log.iter().rev().take(20) {
                s.push_str(&format!("  {l}\n"));
            }
        }
        s
    }
}

/// All captured reports rendered as one paste-ready text block.
pub fn full_text() -> String {
    let reps = reports_snapshot();
    let log = log_snapshot();
    if reps.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    for r in &reps {
        s.push_str(&r.to_text(&log));
        s.push('\n');
    }
    s
}

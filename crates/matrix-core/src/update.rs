//! Update-state model shared by every front-end (desktop GUI/TUI, web).
//!
//! The desktop app owns the actual check/download/swap logic in
//! `matrix-cli::updater`; the web build always serves the newest deployed
//! version and never installs anything, so it only shows `CURRENT`.

/// Version of this build, checked against release tags. The workspace
/// version flows to every crate, so all front-ends report the same number.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// What the user needs to know about an update check / download.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateState {
    /// No check performed yet.
    NotChecked,
    /// Check running (background thread).
    Checking,
    /// Newest release is what we already run.
    Latest { version: String },
    /// Newer release found.
    Available { version: String, notes: String, size: u64 },
    /// Asset download in progress, 0-100.
    Downloading { version: String, pct: u32 },
    /// New exe installed; restart pending.
    Ready { version: String },
    /// Check or install failed.
    Failed { why: String },
}

impl UpdateState {
    /// One-line text for the GUI panel / TUI status line.
    pub fn label(&self) -> String {
        match self {
            UpdateState::NotChecked => "not checked".into(),
            UpdateState::Checking => "checking…".into(),
            UpdateState::Latest { version } => format!("up to date ({version})"),
            UpdateState::Available { version, .. } => format!("update available: {version}"),
            UpdateState::Downloading { version, pct } => format!("downloading {version}: {pct}%"),
            UpdateState::Ready { version } => format!("{version} installed — restart to apply"),
            UpdateState::Failed { why } => format!("update failed: {why}"),
        }
    }
}

/// Semver precedence with the usual `v` prefix tolerated.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn norm(t: &str) -> &str {
        t.trim().trim_start_matches(['v', 'V'])
    }
    let (Ok(c), Ok(k)) = (
        norm(candidate).parse::<semver::Version>(),
        norm(current).parse::<semver::Version>(),
    ) else {
        // Unparseable tags are never offered as updates.
        return false;
    };
    c > k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_ordering() {
        assert!(is_newer("v2.2.0", "2.1.0"));
        assert!(is_newer("2.10.0", "v2.9.2"));
        assert!(!is_newer("v2.1.0", "2.1.0"));
        assert!(!is_newer("v2.0.9", "2.1.0"));
        // prerelease of a higher version still beats a lower release…
        assert!(is_newer("2.2.0-rc1", "2.1.0"));
        // …but never beats its own release version
        assert!(!is_newer("2.2.0-rc1", "2.2.0"));
        assert!(is_newer("2.2.0-rc2", "2.2.0-rc1")); // rc2 > rc1
    }

    #[test]
    fn junk_tags_are_not_updates() {
        assert!(!is_newer("", "2.1.0"));
        assert!(!is_newer("latest", "2.1.0"));
        assert!(!is_newer("v2", "2.1.0"));
    }

    #[test]
    fn state_labels_read_well() {
        assert_eq!(UpdateState::NotChecked.label(), "not checked");
        assert_eq!(
            UpdateState::Available { version: "v2.2.0".into(), notes: String::new(), size: 1 }.label(),
            "update available: v2.2.0"
        );
    }
}

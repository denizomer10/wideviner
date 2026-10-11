//! A unified view over the supported browsers.

use std::path::PathBuf;

use crate::chromium::Chromium;
use crate::firefox::Firefox;
use crate::sysinfo::SystemInfo;

#[derive(Debug, Clone)]
pub enum Browser {
    Chromium(Chromium),
    Firefox(Firefox),
}

impl Browser {
    pub fn name(&self) -> &str {
        match self {
            Browser::Chromium(b) => &b.name,
            Browser::Firefox(b) => &b.name,
        }
    }

    pub fn version(&self) -> &str {
        match self {
            Browser::Chromium(b) => &b.version,
            Browser::Firefox(b) => &b.version,
        }
    }

    pub fn source(&self) -> &str {
        match self {
            Browser::Chromium(b) => &b.source,
            Browser::Firefox(b) => &b.source,
        }
    }

    pub fn binary(&self) -> PathBuf {
        match self {
            Browser::Chromium(b) => b.binary.clone(),
            Browser::Firefox(b) => b.binary.clone(),
        }
    }

    pub fn widevine_version(&self) -> &str {
        match self {
            Browser::Chromium(b) => &b.widevine_version,
            Browser::Firefox(b) => &b.widevine_version,
        }
    }

    /// The profile directory, when there is one.
    pub fn profile_dir(&self) -> Option<PathBuf> {
        match self {
            Browser::Chromium(b) => b.user_data_dir.clone(),
            Browser::Firefox(b) => b.profile_dir.clone(),
        }
    }

    /// A short one-line description.
    pub fn describe(&self) -> String {
        match self {
            Browser::Chromium(b) => b.describe(),
            Browser::Firefox(b) => b.describe(),
        }
    }

    /// A secondary line for the UI.
    pub fn detail(&self) -> String {
        let engine = match self {
            Browser::Chromium(_) => "Chromium",
            Browser::Firefox(_) => "Firefox",
        };
        let mut parts = vec![engine.to_string()];
        if !self.version().is_empty() {
            parts.push(self.version().to_string());
        }
        parts.push(format!("[{}]", self.source()));
        parts.push(self.binary().to_string_lossy().to_string());
        if let Some(profile) = self.profile_dir() {
            parts.push(format!("profile: {}", profile.display()));
        }
        parts.join("  ·  ")
    }

    /// `(state text, kind)` where kind is 0 = none, 1 = installed, 2 = incomplete.
    pub fn state(&self) -> (String, i32) {
        let version = self.widevine_version();
        if !version.is_empty() {
            return (format!("Widevine {}", version), 1);
        }
        let notes = match self {
            Browser::Chromium(b) => &b.notes,
            Browser::Firefox(b) => &b.notes,
        };
        if notes.iter().any(|note| note.contains("incomplete")) {
            return ("Incomplete CDM present".into(), 2);
        }
        ("No Widevine".into(), 0)
    }
}

/// Find every supported browser on this system.
pub fn discover(si: &SystemInfo, deep: bool) -> Vec<Browser> {
    let mut browsers: Vec<Browser> = Vec::new();
    for browser in crate::chromium::discover(si, deep) {
        browsers.push(Browser::Chromium(browser));
    }
    for browser in crate::firefox::discover(si) {
        browsers.push(Browser::Firefox(browser));
    }
    browsers.sort_by_key(|browser| {
        (
            browser.name().to_string(),
            browser.binary().to_string_lossy().to_string(),
        )
    });
    browsers
}

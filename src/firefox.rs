//! Discovery of Firefox (Gecko) browsers.
//!
//! Firefox loads the Widevine CDM as a Gecko Media Plugin (GMP):
//!
//! ```text
//! <base>/gmp-widevinecdm/system-installed/
//!     manifest.json
//!     <library>
//! ```
//!
//! The GMP name is the parent directory name without the `gmp-` prefix and the
//! GMP version is the leaf directory name (`system-installed`), which is why the
//! prefs select exactly that version.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::chromium::{extract_version, run};
use crate::constants as C;
use crate::sysinfo::{self, SystemInfo};

/// A discovered Firefox (or Firefox fork) installation.
#[derive(Debug, Clone)]
pub struct Firefox {
    pub name: String,
    pub binary: PathBuf,
    pub install_dir: PathBuf,
    pub version: String,
    pub source: String,
    pub profile_dir: Option<PathBuf>,
    pub widevine_version: String,
    pub widevine_path: Option<PathBuf>,
    pub library: String,
    pub notes: Vec<String>,
}

impl Firefox {
    /// True for Flatpak/Snap apps whose files live in an immutable image.
    pub fn sandboxed(&self) -> bool {
        self.source == "flatpak" || self.source == "snap"
    }

    /// Directories that may hold the Widevine GMP, in search order.
    pub fn gmp_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(profile_gmp) = self.profile_gmp_dir() {
            dirs.push(profile_gmp);
        }
        for base in C::MOZILLA_PLUGIN_DIRS {
            dirs.push(
                PathBuf::from(base)
                    .join(C::FIREFOX_GMP_ID)
                    .join(C::FIREFOX_GMP_VERSION),
            );
        }
        dirs
    }

    /// GMP directory inside the profile.
    pub fn profile_gmp_dir(&self) -> Option<PathBuf> {
        self.profile_dir
            .as_ref()
            .map(|dir| dir.join(C::FIREFOX_GMP_ID).join(C::FIREFOX_GMP_VERSION))
    }

    /// Directory for system wide default prefs.
    pub fn pref_dir(&self) -> PathBuf {
        self.install_dir.join("defaults").join("pref")
    }

    /// Look for an already installed Widevine GMP.
    pub fn detect_widevine(&mut self) {
        for candidate in self.gmp_dirs() {
            let version = manifest_version(&candidate.join(C::CDM_MANIFEST_NAME));
            if !version.is_empty() {
                self.widevine_version = version;
                self.widevine_path = Some(candidate);
                return;
            }
        }
        self.widevine_version.clear();
        self.widevine_path = None;
    }

    pub fn describe(&self) -> String {
        let mut parts = vec![self.name.clone()];
        if !self.version.is_empty() {
            parts.push(self.version.clone());
        }
        parts.push(format!("[{}]", self.source));
        parts.join(" ")
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
fn manifest_version(path: &Path) -> String {
    let Ok(text) = fs::read_to_string(path) else {
        return String::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return String::new();
    };
    value
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn probe_version(binary: &Path) -> String {
    let Some(output) = run(&[&binary.to_string_lossy(), "--version"], 15) else {
        return String::new();
    };
    extract_version(output.trim())
}

fn package_source(path: &Path) -> String {
    let path_str = path.to_string_lossy().to_string();
    if sysinfo::which("rpm").is_some()
        && run(&["rpm", "-qf", "--qf", "%{NAME}", &path_str], 20).is_some()
    {
        return "rpm".into();
    }
    if sysinfo::which("dpkg").is_some() && run(&["dpkg", "-S", &path_str], 20).is_some() {
        return "deb".into();
    }
    "manual".into()
}

/// Heuristically decide whether `path` is a Firefox executable.
pub fn looks_like_firefox(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(metadata) = fs::metadata(path) else {
            return false;
        };
        if metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if metadata.len() == 0 {
        return false;
    }
    if metadata.len() >= 1_000_000 {
        return true;
    }
    let Ok(head) = read_head(path, 8192) else {
        return false;
    };
    if head.starts_with(b"\x7fELF") || head.starts_with(b"MZ") {
        return true;
    }
    let text = String::from_utf8_lossy(&head).to_lowercase();
    text.contains("firefox") || text.contains("moz_app_name")
}

fn read_head(path: &Path, length: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0u8; length];
    let read = file.read(&mut buffer)?;
    buffer.truncate(read);
    Ok(buffer)
}

/// Check whether a binary name looks like Firefox or one of its forks.
pub fn is_firefox_name(name: &str) -> bool {
    let lowered = name.to_lowercase();
    let lowered = lowered.trim_end_matches(".exe");
    if C::FIREFOX_BINARIES.contains(&lowered) {
        return true;
    }
    [
        "firefox",
        "librewolf",
        "waterfox",
        "icecat",
        "basilisk",
        "palemoon",
    ]
    .iter()
    .any(|keyword| lowered.contains(keyword))
}

/// Find the real Firefox installation directory.
///
/// Distribution packages install a small wrapper script in `/usr/bin` that
/// launches the real binary from the installation directory. That directory is
/// identified by its `application.ini` file.
fn resolve_install_dir(binary: &Path) -> PathBuf {
    let parent = binary.parent().unwrap_or(Path::new(".")).to_path_buf();
    if parent.join("application.ini").is_file() {
        return parent;
    }

    let name = binary
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut candidates: Vec<PathBuf> = C::FIREFOX_SCAN_DIRS.iter().map(PathBuf::from).collect();
    for libdir in [
        "/usr/lib64",
        "/usr/lib",
        "/usr/local/lib64",
        "/usr/local/lib",
        "/opt",
    ] {
        candidates.push(Path::new(libdir).join(&name));
        candidates.push(Path::new(libdir).join(name.replace("-bin", "")));
    }

    for candidate in candidates {
        if candidate.join("application.ini").is_file() {
            return candidate;
        }
    }
    parent
}

/// Search `root` for a Firefox binary.
pub fn find_binary(root: &Path, max_depth: usize) -> Option<PathBuf> {
    if !root.is_dir() {
        return None;
    }
    let base_depth = root.components().count();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if path.components().count() - base_depth < max_depth {
                    stack.push(path);
                }
            } else if is_firefox_name(&entry.file_name().to_string_lossy())
                && looks_like_firefox(&path)
            {
                return Some(path);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Profile discovery
// ---------------------------------------------------------------------------
fn profile_roots(si: &SystemInfo, source: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if si.is_linux() && source == "flatpak" {
        roots.push(
            si.home
                .join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
        );
    } else if si.is_linux() && source == "snap" {
        roots.push(si.home.join("snap/firefox/common/.mozilla/firefox"));
    }
    if si.is_windows() {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| si.home.join("AppData").join("Roaming"));
        roots.push(base.join("Mozilla").join("Firefox"));
    } else {
        roots.push(si.home.join(".mozilla").join("firefox"));
    }
    roots
}

/// Parse a `profiles.ini` style file into sections.
fn parse_ini(path: &Path) -> Vec<Vec<(String, String)>> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut sections: Vec<Vec<(String, String)>> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            let name = line[1..line.len() - 1].to_string();
            sections.push(vec![("__section__".to_string(), name)]);
        } else if let Some((key, value)) = line.split_once('=') {
            if let Some(section) = sections.last_mut() {
                section.push((key.trim().to_string(), value.trim().to_string()));
            }
        }
    }
    sections
}

fn section_value<'a>(section: &'a [(String, String)], key: &str) -> Option<&'a str> {
    section
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// Return the default Firefox profile directory, if there is one.
fn find_profile(si: &SystemInfo, source: &str) -> Option<PathBuf> {
    for root in profile_roots(si, source) {
        let ini = root.join("profiles.ini");
        if !ini.is_file() {
            continue;
        }
        let sections: Vec<Vec<(String, String)>> = parse_ini(&ini)
            .into_iter()
            .filter(|section| {
                section_value(section, "__section__")
                    .map(|name| name.starts_with("Profile"))
                    .unwrap_or(false)
            })
            .collect();

        for section in &sections {
            if section_value(section, "Default") == Some("1") {
                if let Some(path) = section_value(section, "Path") {
                    let candidate = root.join(path);
                    if candidate.is_dir() {
                        return Some(candidate);
                    }
                }
            }
        }
        for section in &sections {
            if let Some(path) = section_value(section, "Path") {
                let candidate = root.join(path);
                if candidate.is_dir() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Candidate collection
// ---------------------------------------------------------------------------
fn binary_names(si: &SystemInfo) -> Vec<String> {
    if si.is_windows() {
        C::FIREFOX_BINARIES
            .iter()
            .map(|name| format!("{}.exe", name))
            .collect()
    } else {
        C::FIREFOX_BINARIES.iter().map(|s| s.to_string()).collect()
    }
}

fn path_candidates(si: &SystemInfo) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for name in binary_names(si) {
        if let Some(found) = sysinfo::which(&name) {
            if looks_like_firefox(&found) {
                paths.push(found);
            }
        }
    }
    paths
}

fn package_candidates(si: &SystemInfo) -> Vec<PathBuf> {
    if !si.is_linux() {
        return Vec::new();
    }
    let keywords = [
        "firefox", "librewolf", "waterfox", "icecat", "basilisk", "palemoon",
    ];
    let names: HashSet<&str> = C::FIREFOX_BINARIES.iter().copied().collect();
    let mut paths = Vec::new();

    if sysinfo::which("rpm").is_some() {
        if let Some(listing) = run(&["rpm", "-qa", "--qf", "%{NAME}\n"], 30) {
            for package in listing.lines() {
                if !keywords.iter().any(|k| package.to_lowercase().contains(k)) {
                    continue;
                }
                let Some(files) = run(&["rpm", "-ql", package], 30) else {
                    continue;
                };
                for line in files.lines() {
                    let candidate = PathBuf::from(line);
                    if names.contains(candidate.file_name().unwrap_or_default().to_string_lossy().as_ref())
                        && looks_like_firefox(&candidate)
                    {
                        paths.push(candidate);
                    }
                }
            }
        }
    }

    if sysinfo::which("dpkg").is_some() {
        if let Some(listing) = run(&["dpkg-query", "-W", "-f", "${Package}\n"], 30) {
            for package in listing.lines() {
                if !keywords.iter().any(|k| package.to_lowercase().contains(k)) {
                    continue;
                }
                let Some(files) = run(&["dpkg", "-L", package], 30) else {
                    continue;
                };
                for line in files.lines() {
                    let candidate = PathBuf::from(line);
                    if names.contains(candidate.file_name().unwrap_or_default().to_string_lossy().as_ref())
                        && looks_like_firefox(&candidate)
                    {
                        paths.push(candidate);
                    }
                }
            }
        }
    }

    paths
}

fn scan_candidates(si: &SystemInfo) -> Vec<PathBuf> {
    if si.is_windows() {
        return windows_candidates();
    }
    let names: HashSet<&str> = C::FIREFOX_BINARIES.iter().copied().collect();
    let mut paths = Vec::new();
    for directory in C::FIREFOX_SCAN_DIRS {
        let base = Path::new(directory);
        if !base.is_dir() {
            continue;
        }
        for name in &names {
            let candidate = base.join(name);
            if looks_like_firefox(&candidate) {
                paths.push(candidate);
            }
        }
    }
    paths
}

fn windows_candidates() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    for variable in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
        if let Some(value) = std::env::var_os(variable) {
            bases.push(PathBuf::from(value));
        }
    }
    let mut paths = Vec::new();
    for base in bases {
        for relative in ["Mozilla Firefox", "Firefox", "LibreWolf", "Waterfox"] {
            let candidate = base.join(relative).join("firefox.exe");
            if candidate.is_file() {
                paths.push(candidate);
            }
        }
    }
    paths
}

fn flatpak_candidates(si: &SystemInfo) -> Vec<(PathBuf, String, String)> {
    if !si.flatpak {
        return Vec::new();
    }
    let Some(listing) = run(
        &[
            "flatpak",
            "list",
            "--app",
            "--columns=application,version,installation",
        ],
        30,
    ) else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for line in listing.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        let Some(app_id) = parts.first().map(|s| s.trim()) else {
            continue;
        };
        if !C::FIREFOX_FLATPAK_APPS.iter().any(|(id, _)| *id == app_id) {
            continue;
        }
        let version = parts.get(1).map(|s| s.trim().to_string()).unwrap_or_default();
        let installation = parts.get(2).map(|s| s.trim()).unwrap_or("system");

        let app_root = if installation == "user" {
            si.home
                .join(".local/share/flatpak/app")
                .join(app_id)
                .join("current/active/files")
        } else {
            PathBuf::from("/var/lib/flatpak/app")
                .join(app_id)
                .join("current/active/files")
        };
        let binary = find_binary(&app_root, 4).unwrap_or_else(|| app_root.join("firefox"));
        results.push((binary, version, app_id.to_string()));
    }
    results
}

fn snap_candidates(si: &SystemInfo) -> Vec<(PathBuf, String)> {
    if !si.snap {
        return Vec::new();
    }
    let Some(listing) = run(&["snap", "list"], 30) else {
        return Vec::new();
    };
    let mut results = Vec::new();
    for line in listing.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let Some(snap_name) = parts.first() else {
            continue;
        };
        if !C::FIREFOX_SNAP_APPS.iter().any(|(id, _)| id == snap_name) {
            continue;
        }
        let version = parts.get(1).map(|s| s.to_string()).unwrap_or_default();
        let app_root = PathBuf::from("/snap").join(snap_name).join("current");
        let binary = find_binary(&app_root, 4).unwrap_or_else(|| app_root.join("firefox"));
        results.push((binary, version));
    }
    results
}

// ---------------------------------------------------------------------------
// Browser construction
// ---------------------------------------------------------------------------
fn build(
    binary: &Path,
    si: &SystemInfo,
    source: Option<&str>,
    version: Option<String>,
    notes: Vec<String>,
) -> Option<Firefox> {
    let real = binary.canonicalize().unwrap_or_else(|_| binary.to_path_buf());
    if !real.is_file() {
        return None;
    }

    let install_dir = resolve_install_dir(&real);
    // Point at the real binary inside the installation directory so that a
    // wrapper in /usr/bin and the binary itself deduplicate.
    let mut real = real;
    let real_binary = install_dir.join(real.file_name().unwrap_or_default());
    if real_binary.is_file() {
        real = real_binary;
    }

    let source = source
        .map(|s| s.to_string())
        .unwrap_or_else(|| package_source(&real));
    let version = version.unwrap_or_else(|| probe_version(&real));

    let mut firefox = Firefox {
        name: real
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        binary: real,
        install_dir,
        version,
        source: source.clone(),
        profile_dir: find_profile(si, &source),
        widevine_version: String::new(),
        widevine_path: None,
        library: if si.lib_name().is_empty() {
            "libwidevinecdm.so".into()
        } else {
            si.lib_name().to_string()
        },
        notes,
    };
    firefox.detect_widevine();
    Some(firefox)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------
/// Find Firefox installations on this system.
pub fn discover(si: &SystemInfo) -> Vec<Firefox> {
    let mut browsers: Vec<Firefox> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut push = |browser: Option<Firefox>, browsers: &mut Vec<Firefox>| {
        if let Some(browser) = browser {
            let key = browser.binary.to_string_lossy().to_string();
            if seen.insert(key) {
                browsers.push(browser);
            }
        }
    };

    for path in path_candidates(si) {
        push(build(&path, si, None, None, Vec::new()), &mut browsers);
    }
    for path in package_candidates(si) {
        push(build(&path, si, None, None, Vec::new()), &mut browsers);
    }
    for path in scan_candidates(si) {
        push(build(&path, si, None, None, Vec::new()), &mut browsers);
    }
    for (binary, version, app_id) in flatpak_candidates(si) {
        push(
            build(
                &binary,
                si,
                Some("flatpak"),
                Some(version),
                vec![format!("Flatpak application {}", app_id)],
            ),
            &mut browsers,
        );
    }
    for (binary, version) in snap_candidates(si) {
        push(
            build(
                &binary,
                si,
                Some("snap"),
                Some(version),
                vec!["Snap package firefox".into()],
            ),
            &mut browsers,
        );
    }

    browsers.sort_by(|a, b| {
        (a.name.clone(), a.binary.clone()).cmp(&(b.name.clone(), b.binary.clone()))
    });
    browsers
}

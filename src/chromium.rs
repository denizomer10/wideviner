//! Discovery of Chromium (and Chromium-based) installations.
//!
//! Chromium can be installed in many different ways: distribution packages
//! (RPM/DEB), Flatpak, Snap, or a plain extracted archive. This module finds
//! them and works out where the Widevine CDM has to be placed for each one.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::constants as C;
use crate::sysinfo::{self, SystemInfo};

/// Directories that never contain a browser executable, even if a file with a
/// matching name lives there (cron jobs, AppArmor profiles, shell completions).
const BAD_PARENT_DIRS: &[&str] = &[
    "apparmor.d",
    "applications",
    "autostart",
    "bash_completion.d",
    "binfmt.d",
    "completion",
    "cron.d",
    "cron.daily",
    "cron.hourly",
    "cron.monthly",
    "cron.weekly",
    "dbus-1",
    "doc",
    "environment.d",
    "icons",
    "init.d",
    "locale",
    "locales",
    "logrotate.d",
    "man",
    "metainfo",
    "mime",
    "modules-load.d",
    "pam.d",
    "pixmaps",
    "polkit-1",
    "profile.d",
    "security",
    "selinux",
    "sysctl.d",
    "systemd",
    "tmpfiles.d",
    "udev",
    "xdg",
    "xdg-autostart",
];

/// A discovered Chromium installation.
#[derive(Debug, Clone)]
pub struct Chromium {
    pub name: String,
    pub binary: PathBuf,
    pub module_dir: PathBuf,
    pub source: String,
    pub version: String,
    pub user_data_dir: Option<PathBuf>,
    pub writable: bool,
    pub widevine_version: String,
    pub widevine_path: Option<PathBuf>,
    pub notes: Vec<String>,
}

impl Chromium {
    /// True for Flatpak/Snap apps whose files live in an immutable image.
    pub fn sandboxed(&self) -> bool {
        self.source == "flatpak" || self.source == "snap"
    }

    /// The actual browser binary.
    ///
    /// Distribution packages often install a small wrapper script that launches
    /// the real binary (`chrome`/`chromium`) next to it.
    pub fn executable(&self) -> PathBuf {
        for candidate in ["chrome", "chromium", "google-chrome", "Google Chrome"] {
            let path = self.module_dir.join(candidate);
            if path.is_file() {
                return path;
            }
        }
        let named = self.module_dir.join(&self.name);
        if named.is_file() {
            return named;
        }
        self.binary.clone()
    }

    /// Directory Chromium checks when the CDM is bundled with the build.
    pub fn bundled_cdm_dir(&self) -> PathBuf {
        self.module_dir.join(C::CDM_DIR_NAME)
    }

    /// Path of the component-updated CDM hint file (Linux only).
    pub fn hint_file_path(&self) -> Option<PathBuf> {
        self.user_data_dir
            .as_ref()
            .map(|dir| dir.join(C::CDM_DIR_NAME).join(C::HINT_FILE_NAME))
    }

    /// Directory used for a user-level (non-root) installation.
    pub fn user_cdm_dir(&self, version: &str) -> Option<PathBuf> {
        self.user_data_dir
            .as_ref()
            .map(|dir| dir.join(C::CDM_DIR_NAME).join(version))
    }

    /// Look for an already installed, usable Widevine CDM.
    pub fn detect_widevine(&mut self, platform: Option<&C::CdmPlatform>) {
        self.widevine_version.clear();
        self.widevine_path = None;

        let mut candidates = vec![self.bundled_cdm_dir()];
        if let Some(hint) = self.hint_file_path() {
            if let Ok(text) = fs::read_to_string(&hint) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(path) = value.get("Path").and_then(|p| p.as_str()) {
                        if !path.is_empty() {
                            candidates.push(PathBuf::from(path));
                        }
                    }
                }
            }
        }

        let mut incomplete = false;
        for cdm_dir in candidates {
            let version = usable_cdm_version(&cdm_dir, platform);
            if !version.is_empty() {
                self.widevine_version = version;
                self.widevine_path = Some(cdm_dir);
                return;
            }
            if !manifest_version(&cdm_dir.join(C::CDM_MANIFEST_NAME)).is_empty() {
                incomplete = true;
            }
        }

        if incomplete {
            self.notes
                .push("an incomplete Widevine CDM is present and will be replaced".into());
        }
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
/// Run `command` and return its stdout, or `None` on failure.
pub fn run(command: &[&str], timeout_secs: u64) -> Option<String> {
    let (program, args) = command.split_first()?;
    let mut child = Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .ok()?;

    // A simple timeout: poll until the process exits or the deadline passes.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let output = child.wait_with_output().ok()?;
                return Some(String::from_utf8_lossy(&output.stdout).to_string());
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }
}

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

/// Return the CDM version when `cdm_dir` holds a usable CDM, else `""`.
fn usable_cdm_version(cdm_dir: &Path, platform: Option<&C::CdmPlatform>) -> String {
    let version = manifest_version(&cdm_dir.join(C::CDM_MANIFEST_NAME));
    if version.is_empty() {
        return String::new();
    }
    if let Some(platform) = platform {
        let library = cdm_dir
            .join(C::CDM_PLATFORM_DIR)
            .join(platform.dir)
            .join(platform.lib);
        if !library.is_file() {
            return String::new();
        }
    }
    version
}

/// Ask a browser binary for its version.
pub fn probe_version(binary: &Path) -> String {
    let Some(output) = run(&[&binary.to_string_lossy(), "--version"], 15) else {
        return String::new();
    };
    let first_line = output.lines().next().unwrap_or("").trim().to_string();
    extract_version(&first_line)
}

/// Pull the first `x.y.z.w` style version out of `text`.
pub fn extract_version(text: &str) -> String {
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit() {
            let start = index;
            let mut dots = 0;
            while index < bytes.len()
                && (bytes[index].is_ascii_digit() || bytes[index] == '.')
            {
                if bytes[index] == '.' {
                    dots += 1;
                }
                index += 1;
            }
            if dots >= 2 {
                let candidate: String = bytes[start..index].iter().collect();
                return candidate.trim_end_matches('.').to_string();
            }
        } else {
            index += 1;
        }
    }
    text.to_string()
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

fn is_writable(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    // A directory is writable when we can create a file in it.
    let probe = path.join(".wideviner-write-probe");
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Heuristically decide whether `path` is a browser executable.
pub fn looks_like_browser(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if let Some(parent) = path.parent().and_then(|p| p.file_name()) {
        if BAD_PARENT_DIRS.contains(&parent.to_string_lossy().as_ref()) {
            return false;
        }
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
    if text.contains("chrome_wrapper") {
        return true;
    }
    (text.contains("exec") || text.contains("launch"))
        && (text.contains("chrome") || text.contains("chromium"))
}

fn read_head(path: &Path, length: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0u8; length];
    let read = file.read(&mut buffer)?;
    buffer.truncate(read);
    Ok(buffer)
}

/// Search `root` for a browser binary called `name` (or chrome/chromium).
pub fn find_binary_in(root: &Path, name: &str, max_depth: usize) -> Option<PathBuf> {
    if !root.is_dir() {
        return None;
    }
    let wanted: HashSet<String> = [name.to_string(), "chrome".to_string(), "chromium".to_string()]
        .into_iter()
        .collect();
    walk(root, max_depth, &mut |path| {
        let file_name = path.file_name()?.to_string_lossy().to_string();
        if wanted.contains(&file_name) && looks_like_browser(path) {
            Some(path.to_path_buf())
        } else {
            None
        }
    })
}

/// Walk `root` up to `max_depth` levels, returning the first match.
fn walk<F>(root: &Path, max_depth: usize, predicate: &mut F) -> Option<PathBuf>
where
    F: FnMut(&Path) -> Option<PathBuf>,
{
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
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.starts_with('.') {
                        stack.push(path);
                    }
                }
            } else if let Some(found) = predicate(&path) {
                return Some(found);
            }
        }
    }
    None
}


// ---------------------------------------------------------------------------
// Profile discovery
// ---------------------------------------------------------------------------
const PROFILE_MARKERS: &[&str] = &["Local State", "Default", "First Run", "Last Version"];

fn looks_like_profile(path: &Path) -> bool {
    path.is_dir() && PROFILE_MARKERS.iter().any(|marker| path.join(marker).exists())
}

/// Find an existing Chromium profile directory below `config_home`.
fn find_existing_profile(config_home: &Path, basename: &str) -> Option<PathBuf> {
    if basename.is_empty() || !config_home.is_dir() {
        return None;
    }
    let mut matches: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in fs::read_dir(config_home).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(basename) || name.ends_with("-headless") {
            continue;
        }
        let path = entry.path();
        if looks_like_profile(&path) {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            matches.push((modified, path));
        }
    }
    matches.sort_by(|a, b| b.0.cmp(&a.0));
    matches.into_iter().next().map(|(_, path)| path)
}

fn config_dir_names(config: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(config) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| !name.ends_with("-headless"))
        .collect()
}

/// Learn the profile directory name by running the browser once.
///
/// The browser is started with a temporary home directory, so it creates its
/// profile there and nothing touches the real one.
fn probe_profile_name(binary: &Path) -> Option<String> {
    let tmp = std::env::temp_dir().join(format!("wideviner-home-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).ok()?;

    let mut child = Command::new(binary)
        .args([
            "--headless=new",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-sync",
            "about:blank",
        ])
        .env("HOME", &tmp)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let config = tmp.join(".config");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut names: Vec<String> = Vec::new();
    while std::time::Instant::now() < deadline {
        names = config_dir_names(&config);
        if !names.is_empty() {
            break;
        }
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(&tmp);

    if names.is_empty() {
        return None;
    }
    names
        .iter()
        .find(|name| looks_like_profile(&config.join(name)))
        .cloned()
        .or_else(|| names.into_iter().next())
}

/// Work out where the browser keeps its profile.
fn resolve_user_data_dir(si: &SystemInfo, binary: &Path) -> Option<PathBuf> {
    let basename = C::chromium_profile_name(
        &binary
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
    );
    if basename.is_empty() {
        return None;
    }
    let config_home = si.config_home();

    if si.is_linux() {
        if let Some(existing) = find_existing_profile(&config_home, basename) {
            return Some(existing);
        }
        if !si.is_admin && binary.is_file() {
            if let Some(name) = probe_profile_name(binary) {
                return Some(config_home.join(name));
            }
        }
    }

    Some(config_home.join(basename))
}

// ---------------------------------------------------------------------------
// Candidate collection
// ---------------------------------------------------------------------------
fn binary_names(_si: &SystemInfo) -> Vec<String> {
    C::BROWSER_BINARIES.iter().map(|s| s.to_string()).collect()
}

fn path_candidates(si: &SystemInfo) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for name in binary_names(si) {
        if let Some(found) = sysinfo::which(&name) {
            if looks_like_browser(&found) {
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
    let keywords = ["chromium", "chrome", "brave", "vivaldi", "edge", "opera"];
    let names: HashSet<&str> = C::BROWSER_BINARIES.iter().copied().collect();
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
                        && looks_like_browser(&candidate)
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
                        && looks_like_browser(&candidate)
                    {
                        paths.push(candidate);
                    }
                }
            }
        }
    }

    paths
}

fn scan_candidates(si: &SystemInfo, deep: bool) -> Vec<PathBuf> {
    let _ = si;
    let names: HashSet<&str> = C::BROWSER_BINARIES.iter().copied().collect();
    let mut paths = Vec::new();

    let mut directories: Vec<PathBuf> = C::SCAN_DIRS.iter().map(PathBuf::from).collect();
    for pattern in C::SCAN_GLOBS {
        if let Some(matches) = glob(pattern) {
            directories.extend(matches);
        }
    }

    for directory in directories {
        if !directory.is_dir() {
            continue;
        }
        for name in &names {
            let candidate = directory.join(name);
            if looks_like_browser(&candidate) {
                paths.push(candidate);
            }
        }
    }

    if deep {
        paths.extend(deep_scan());
    }
    paths
}

/// A minimal glob for the fixed patterns we use (`/opt/*`, `/opt/*/*`, ...).
fn glob(pattern: &str) -> Option<Vec<PathBuf>> {
    let parts: Vec<&str> = pattern.split('/').filter(|p| !p.is_empty()).collect();
    let mut current = vec![PathBuf::from("/")];
    for part in parts {
        let mut next = Vec::new();
        for base in &current {
            if part == "*" {
                if let Ok(entries) = fs::read_dir(base) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            next.push(entry.path());
                        }
                    }
                }
            } else {
                let candidate = base.join(part);
                if candidate.is_dir() {
                    next.push(candidate);
                }
            }
        }
        current = next;
    }
    Some(current)
}

fn deep_scan() -> Vec<PathBuf> {
    const ROOTS: &[&str] = &["/usr", "/opt", "/usr/local", "/snap", "/var/lib/flatpak"];
    const SKIP: &[&str] = &[
        "share",
        "include",
        "src",
        "doc",
        "man",
        "locale",
        "icons",
        "firmware",
        "modules",
        "debug",
        "python3",
        "node_modules",
        "tests",
        "test",
    ];
    let names: HashSet<&str> = C::BROWSER_BINARIES.iter().copied().collect();
    let mut results = Vec::new();

    for root in ROOTS {
        let root = Path::new(root);
        if !root.is_dir() {
            continue;
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
                    let name = entry.file_name().to_string_lossy().to_string();
                    if path.components().count() - base_depth < 4
                        && !name.starts_with('.')
                        && !SKIP.contains(&name.as_str())
                    {
                        stack.push(path);
                    }
                } else if names.contains(entry.file_name().to_string_lossy().as_ref())
                    && looks_like_browser(&path)
                {
                    results.push(path);
                }
            }
        }
    }
    results
}

// ---------------------------------------------------------------------------
// Browser construction
// ---------------------------------------------------------------------------
pub fn build_browser(
    binary: &Path,
    si: &SystemInfo,
    source: Option<&str>,
    user_data_dir: Option<PathBuf>,
    module_dir: Option<PathBuf>,
    version: Option<String>,
    notes: Vec<String>,
) -> Option<Chromium> {
    let real = binary.canonicalize().unwrap_or_else(|_| binary.to_path_buf());
    if !real.is_file() {
        return None;
    }

    let module_dir = module_dir.unwrap_or_else(|| {
        real.parent().unwrap_or(Path::new(".")).to_path_buf()
    });
    let source = source
        .map(|s| s.to_string())
        .unwrap_or_else(|| package_source(&real));
    let version = version.unwrap_or_else(|| probe_version(&real));
    let user_data_dir = user_data_dir.or_else(|| resolve_user_data_dir(si, &real));

    let mut browser = Chromium {
        name: real
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        binary: real,
        module_dir,
        source,
        version,
        user_data_dir,
        writable: false,
        widevine_version: String::new(),
        widevine_path: None,
        notes,
    };
    browser.writable = is_writable(&browser.module_dir);
    browser.detect_widevine(si.cdm());
    Some(browser)
}

fn flatpak_browsers(si: &SystemInfo) -> Vec<Chromium> {
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

    let mut browsers = Vec::new();
    for line in listing.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        let Some(app_id) = parts.first().map(|s| s.trim()) else {
            continue;
        };
        let Some((_, name)) = C::FLATPAK_APPS.iter().find(|(id, _)| *id == app_id) else {
            continue;
        };
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

        let binary = find_binary_in(&app_root, name, 4).unwrap_or_else(|| app_root.join(name));
        let user_data_dir = si
            .home
            .join(".var/app")
            .join(app_id)
            .join("config")
            .join(C::chromium_profile_name(name));

        let mut browser = Chromium {
            name: (*name).to_string(),
            binary,
            module_dir: app_root,
            source: "flatpak".into(),
            version,
            user_data_dir: Some(user_data_dir),
            writable: false,
            widevine_version: String::new(),
            widevine_path: None,
            notes: vec![format!("Flatpak application {} ({})", app_id, installation)],
        };
        browser.detect_widevine(si.cdm());
        browsers.push(browser);
    }
    browsers
}

fn snap_browsers(si: &SystemInfo) -> Vec<Chromium> {
    if !si.snap {
        return Vec::new();
    }
    let Some(listing) = run(&["snap", "list"], 30) else {
        return Vec::new();
    };

    let mut browsers = Vec::new();
    for line in listing.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let Some(snap_name) = parts.first() else {
            continue;
        };
        let Some((_, name)) = C::SNAP_APPS.iter().find(|(id, _)| id == snap_name) else {
            continue;
        };
        let version = parts.get(1).map(|s| s.to_string()).unwrap_or_default();

        let app_root = PathBuf::from("/snap").join(snap_name).join("current");
        let binary = find_binary_in(&app_root, name, 4).unwrap_or_else(|| app_root.join(name));
        let user_data_dir = si
            .home
            .join("snap")
            .join(snap_name)
            .join("current/.config")
            .join(C::chromium_profile_name(name));

        let mut browser = Chromium {
            name: (*name).to_string(),
            binary,
            module_dir: app_root,
            source: "snap".into(),
            version,
            user_data_dir: Some(user_data_dir),
            writable: false,
            widevine_version: String::new(),
            widevine_path: None,
            notes: vec![format!("Snap package {}", snap_name)],
        };
        browser.detect_widevine(si.cdm());
        browsers.push(browser);
    }
    browsers
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------
/// Find Chromium installations on this system.
pub fn discover(si: &SystemInfo, deep: bool) -> Vec<Chromium> {
    let mut browsers: Vec<Chromium> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut push = |browser: Option<Chromium>, browsers: &mut Vec<Chromium>| {
        if let Some(browser) = browser {
            let key = browser.binary.to_string_lossy().to_string();
            if seen.insert(key) {
                browsers.push(browser);
            }
        }
    };

    for path in path_candidates(si) {
        push(build_browser(&path, si, None, None, None, None, Vec::new()), &mut browsers);
    }
    for path in package_candidates(si) {
        push(build_browser(&path, si, None, None, None, None, Vec::new()), &mut browsers);
    }
    for path in scan_candidates(si, deep) {
        push(build_browser(&path, si, None, None, None, None, Vec::new()), &mut browsers);
    }
    for browser in flatpak_browsers(si) {
        push(Some(browser), &mut browsers);
    }
    for browser in snap_browsers(si) {
        push(Some(browser), &mut browsers);
    }

    browsers.sort_by(|a, b| {
        (a.name.clone(), a.binary.clone()).cmp(&(b.name.clone(), b.binary.clone()))
    });
    browsers
}

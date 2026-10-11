//! Detection of the operating system, distribution and environment.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::constants as C;

/// Distribution families and the IDs that belong to them (Linux only).
const FAMILIES: &[(&str, &[&str])] = &[
    (
        "fedora",
        &["fedora", "rhel", "centos", "rocky", "almalinux", "ol"],
    ),
    (
        "debian",
        &["debian", "ubuntu", "linuxmint", "pop", "raspbian", "kali", "neon"],
    ),
    (
        "arch",
        &["arch", "manjaro", "endeavouros", "garuda", "cachyos", "artix"],
    ),
    ("suse", &["opensuse", "suse", "sles", "sled"]),
    ("alpine", &["alpine"]),
    ("gentoo", &["gentoo", "funtoo"]),
    ("void", &["void"]),
];

const PACKAGE_MANAGERS: &[&str] = &[
    "dnf",
    "apt",
    "apt-get",
    "pacman",
    "zypper",
    "apk",
    "xbps-install",
    "emerge",
];

/// Everything wideviner knows about the running system.
#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub os_name: String,
    pub os_id: String,
    pub os_like: Vec<String>,
    pub pretty_name: String,
    pub family: String,
    pub machine: String,
    pub arch: &'static str,
    pub kernel: String,
    pub glibc: String,
    pub session_type: String,
    pub desktop: String,
    pub user: String,
    pub home: PathBuf,
    pub is_admin: bool,
    pub has_sudo: bool,
    pub flatpak: bool,
    pub snap: bool,
    pub package_manager: String,
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self {
            os_name: String::new(),
            os_id: "unknown".into(),
            os_like: Vec::new(),
            pretty_name: "Unknown".into(),
            family: "unknown".into(),
            machine: String::new(),
            arch: "",
            kernel: String::new(),
            glibc: String::new(),
            session_type: String::new(),
            desktop: String::new(),
            user: String::new(),
            home: dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
            is_admin: false,
            has_sudo: false,
            flatpak: false,
            snap: false,
            package_manager: String::new(),
        }
    }
}

impl SystemInfo {
    pub fn is_linux(&self) -> bool {
        self.os_name == "linux"
    }

    pub fn is_windows(&self) -> bool {
        self.os_name == "windows"
    }

    pub fn arch_supported(&self) -> bool {
        C::cdm_platform(&self.os_name, self.arch).is_some()
    }

    /// Layout of the Widevine CDM for this platform.
    pub fn cdm(&self) -> Option<&'static C::CdmPlatform> {
        C::cdm_platform(&self.os_name, self.arch)
    }

    pub fn lib_name(&self) -> &'static str {
        self.cdm().map(|c| c.lib).unwrap_or("")
    }

    pub fn platform_dir(&self) -> &'static str {
        self.cdm().map(|c| c.dir).unwrap_or("")
    }

    /// Directory that holds browser profiles.
    pub fn config_home(&self) -> PathBuf {
        if self.is_windows() {
            if let Some(local) = env::var_os("LOCALAPPDATA") {
                return PathBuf::from(local);
            }
            return self.home.join("AppData").join("Local");
        }
        if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg);
        }
        self.home.join(".config")
    }

    pub fn arch_label(&self) -> &'static str {
        C::arch_info(self.arch)
            .map(|info| info.label)
            .unwrap_or("unknown")
    }
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------
fn read_os_release() -> Vec<(String, String)> {
    let mut data = Vec::new();
    let Ok(text) = fs::read_to_string("/etc/os-release") else {
        return data;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let mut value = value.trim().to_string();
        if value.len() >= 2 {
            let first = value.chars().next().unwrap();
            let last = value.chars().last().unwrap();
            if first == last && (first == '"' || first == '\'') {
                value = value[1..value.len() - 1].to_string();
            }
        }
        data.push((key.trim().to_string(), value));
    }
    data
}

fn os_release_value(data: &[(String, String)], key: &str) -> String {
    data.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn glibc_version() -> String {
    let Ok(output) = Command::new("ldd").arg("--version").output() else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next().unwrap_or("");
    // "ldd (Debian GLIBC 2.41-3) 2.41"
    first
        .split_whitespace()
        .last()
        .filter(|token| token.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .unwrap_or("")
        .to_string()
}

fn detect_family(os_id: &str, os_like: &[String]) -> String {
    let mut candidates: Vec<&str> = vec![os_id];
    candidates.extend(os_like.iter().map(|s| s.as_str()));
    for (family, ids) in FAMILIES {
        for candidate in &candidates {
            if ids.contains(candidate) {
                return (*family).to_string();
            }
        }
    }
    "unknown".into()
}

fn detect_package_manager() -> String {
    PACKAGE_MANAGERS
        .iter()
        .find(|manager| which(manager).is_some())
        .map(|manager| (*manager).to_string())
        .unwrap_or_default()
}

/// Look up an executable on `PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn is_admin() -> bool {
    if cfg!(windows) {
        // Writing into System32 only succeeds with administrator rights.
        let probe = Path::new(r"C:\Windows\System32\wideviner-probe.tmp");
        match fs::write(probe, b"") {
            Ok(()) => {
                let _ = fs::remove_file(probe);
                true
            }
            Err(_) => false,
        }
    } else {
        Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim() == "0")
            .unwrap_or(false)
    }
}

/// Return `(name, uid, gid, home)` of the user that invoked sudo, if any.
pub fn invoking_user() -> Option<(String, u32, u32, PathBuf)> {
    let name = env::var("SUDO_USER").ok()?;
    if name.is_empty() || name == "root" {
        return None;
    }
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 6 && fields[0] == name {
            let uid = fields[2].parse().ok()?;
            let gid = fields[3].parse().ok()?;
            return Some((name, uid, gid, PathBuf::from(fields[5])));
        }
    }
    None
}

/// Inspect the running system.
pub fn detect() -> SystemInfo {
    let mut info = SystemInfo::default();

    if cfg!(target_os = "linux") {
        info.os_name = "linux".into();
    } else if cfg!(windows) {
        info.os_name = "windows".into();
    } else {
        info.os_name = env::consts::OS.into();
    }

    if info.is_linux() {
        let release = read_os_release();
        info.os_id = {
            let id = os_release_value(&release, "ID");
            if id.is_empty() {
                "unknown".into()
            } else {
                id
            }
        };
        info.os_like = os_release_value(&release, "ID_LIKE")
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        info.pretty_name = {
            let pretty = os_release_value(&release, "PRETTY_NAME");
            if pretty.is_empty() {
                let name = os_release_value(&release, "NAME");
                if name.is_empty() {
                    "Linux".into()
                } else {
                    name
                }
            } else {
                pretty
            }
        };
        info.glibc = glibc_version();
        info.package_manager = detect_package_manager();
        info.family = detect_family(&info.os_id, &info.os_like);
        info.flatpak = which("flatpak").is_some();
        info.snap = which("snap").is_some();
    } else if info.is_windows() {
        info.os_id = "windows".into();
        info.pretty_name = format!("Windows {}", env::consts::OS);
        info.family = "windows".into();
    }

    info.machine = uname_machine();
    info.arch = C::normalize_arch(&info.machine);
    info.kernel = uname_release();

    info.session_type = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    info.desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

    info.is_admin = is_admin();
    info.has_sudo = which("sudo").is_some();

    // When running through sudo, report the invoking user rather than root.
    if let Some((name, _, _, home)) = invoking_user() {
        info.user = name;
        info.home = home;
    } else {
        info.user = env::var("USER")
            .or_else(|_| env::var("USERNAME"))
            .unwrap_or_default();
        info.home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    }

    info
}

fn uname_machine() -> String {
    Command::new("uname")
        .arg("-m")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| env::consts::ARCH.to_string())
}

fn uname_release() -> String {
    Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------
/// Return the system information as a list of `(label, value)` rows.
pub fn report(info: &SystemInfo) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();

    if info.is_linux() {
        rows.push(("Distribution".into(), info.pretty_name.clone()));
        let family = if info.package_manager.is_empty() {
            info.family.clone()
        } else {
            format!("{} [{}]", info.family, info.package_manager)
        };
        rows.push(("Family".into(), family));
    } else {
        rows.push(("Operating system".into(), info.pretty_name.clone()));
        rows.push(("Family".into(), info.family.clone()));
    }

    rows.push((
        "Architecture".into(),
        format!("{} -> {}", info.machine, info.arch_label()),
    ));
    rows.push(("Kernel".into(), info.kernel.clone()));
    if info.is_linux() {
        rows.push((
            "glibc".into(),
            if info.glibc.is_empty() {
                "unknown".into()
            } else {
                info.glibc.clone()
            },
        ));
        let session = [info.session_type.as_str(), info.desktop.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        rows.push((
            "Session".into(),
            if session.is_empty() {
                "unknown".into()
            } else {
                session
            },
        ));
    }

    rows.push((
        "User".into(),
        format!(
            "{} (admin: {}, sudo: {})",
            info.user,
            if info.is_admin { "yes" } else { "no" },
            if info.has_sudo { "yes" } else { "no" }
        ),
    ));

    if info.is_linux() {
        rows.push((
            "Flatpak".into(),
            if info.flatpak {
                "available".into()
            } else {
                "not available".into()
            },
        ));
        rows.push((
            "Snap".into(),
            if info.snap {
                "available".into()
            } else {
                "not available".into()
            },
        ));
    }

    if info.arch_supported() {
        let source = if info.is_linux() && info.arch == "amd64" {
            "Chrome package"
        } else if info.is_linux() {
            "Chrome package"
        } else {
            "Google CRX package"
        };
        rows.push((
            "Widevine source".into(),
            format!(
                "{} / {} ({})",
                info.platform_dir(),
                info.lib_name(),
                source
            ),
        ));
    }

    rows
}

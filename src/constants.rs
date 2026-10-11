//! Shared constants.

pub const APP_NAME: &str = "wideviner";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_TAGLINE: &str = "Widevine CDM installer for Chromium and Firefox";

// ---------------------------------------------------------------------------
// Widevine sources
// ---------------------------------------------------------------------------
/// Google publishes the stable Chrome `.deb` under a fixed URL that always
/// serves the current release. The Widevine CDM bundled inside it is extracted
/// and installed into the browser.
pub const CHROME_DEB_URL: &str =
    "https://dl.google.com/linux/direct/google-chrome-stable_current_{deb_arch}.deb";

/// Mozilla publishes, for every Firefox release, a manifest that lists the
/// official Google download URLs of the current Widevine CDM for each platform,
/// together with its SHA-512 hash.
pub const MOZILLA_GMP_MANIFEST: &str = "https://raw.githubusercontent.com/mozilla-firefox/firefox/refs/heads/main/toolkit/content/gmp-sources/widevinecdm.json";
pub const MOZILLA_GMP_MANIFEST_FALLBACK: &str = "https://hg.mozilla.org/mozilla-central/raw-file/tip/toolkit/content/gmp-sources/widevinecdm.json";
pub const MOZILLA_GMP_VENDOR: &str = "gmp-widevinecdm";

// ---------------------------------------------------------------------------
// Architectures
// ---------------------------------------------------------------------------
pub struct ArchInfo {
    /// Values that `uname -m` may report.
    pub machine: &'static [&'static str],
    pub label: &'static str,
    pub deb_arch: &'static str,
}

pub const ARCHES: &[(&str, ArchInfo)] = &[
    (
        "amd64",
        ArchInfo {
            machine: &["x86_64", "amd64", "x64", "em64t"],
            label: "x86_64 (amd64)",
            deb_arch: "amd64",
        },
    ),
    (
        "arm64",
        ArchInfo {
            machine: &["aarch64", "arm64", "armv8l"],
            label: "ARM64 (aarch64)",
            deb_arch: "arm64",
        },
    ),
    (
        "x86",
        ArchInfo {
            machine: &["i386", "i486", "i586", "i686", "x86", "ia32"],
            label: "x86 (32-bit)",
            deb_arch: "i386",
        },
    ),
];

pub fn arch_info(arch: &str) -> Option<&'static ArchInfo> {
    ARCHES
        .iter()
        .find(|(name, _)| *name == arch)
        .map(|(_, info)| info)
}

/// Map a `uname -m` value to one of our architectures.
pub fn normalize_arch(machine: &str) -> &'static str {
    let machine = machine.to_lowercase();
    for (name, info) in ARCHES {
        if info.machine.contains(&machine.as_str()) {
            return name;
        }
    }
    ""
}

// ---------------------------------------------------------------------------
// Widevine CDM layout per platform
// ---------------------------------------------------------------------------
pub struct CdmPlatform {
    /// Where the CDM lives inside a package.
    pub dir: &'static str,
    /// Name of the shared library.
    pub lib: &'static str,
    /// Platform key used in Mozilla's GMP manifest.
    pub mozilla_key: Option<&'static str>,
}

pub const CDM_PLATFORMS: &[(&str, &str, CdmPlatform)] = &[
    (
        "linux",
        "amd64",
        CdmPlatform {
            dir: "linux_x64",
            lib: "libwidevinecdm.so",
            mozilla_key: Some("Linux_x86_64-gcc3"),
        },
    ),
    (
        "linux",
        "arm64",
        CdmPlatform {
            dir: "linux_arm64",
            lib: "libwidevinecdm.so",
            mozilla_key: None,
        },
    ),
    (
        "windows",
        "amd64",
        CdmPlatform {
            dir: "win_x64",
            lib: "widevinecdm.dll",
            mozilla_key: Some("WINNT_x86_64-msvc"),
        },
    ),
    (
        "windows",
        "arm64",
        CdmPlatform {
            dir: "win_arm64",
            lib: "widevinecdm.dll",
            mozilla_key: Some("WINNT_aarch64-msvc-aarch64"),
        },
    ),
    (
        "windows",
        "x86",
        CdmPlatform {
            dir: "win_x86",
            lib: "widevinecdm.dll",
            mozilla_key: Some("WINNT_x86-msvc"),
        },
    ),
];

pub fn cdm_platform(os_name: &str, arch: &str) -> Option<&'static CdmPlatform> {
    CDM_PLATFORMS
        .iter()
        .find(|(os, a, _)| *os == os_name && *a == arch)
        .map(|(_, _, platform)| platform)
}

// ---------------------------------------------------------------------------
// Widevine CDM layout
// ---------------------------------------------------------------------------
pub const CDM_DIR_NAME: &str = "WidevineCdm";
pub const CDM_PLATFORM_DIR: &str = "_platform_specific";
pub const CDM_MANIFEST_NAME: &str = "manifest.json";
pub const CDM_LICENSE_NAME: &str = "LICENSE";

/// Chromium reads this JSON hint file from its user data directory to locate a
/// component-updated Widevine CDM. The mechanism only exists on Linux/ChromeOS.
pub const HINT_FILE_NAME: &str = "latest-component-updated-widevine-cdm";

/// Location of the Widevine CDM inside the Chrome `.deb` (inside `data.tar.*`).
pub const DEB_CDM_PATH: &str = "opt/google/chrome/WidevineCdm";

/// Number of bytes requested when trying to extract the CDM with a partial
/// (HTTP range) download.
pub const RANGE_PROBE_BYTES: u64 = 24 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Chromium based browsers
// ---------------------------------------------------------------------------
/// Only open source browsers are handled. Google Chrome, Microsoft Edge, Opera,
/// Vivaldi and Brave ship Widevine themselves and are deliberately ignored.
pub const BROWSER_BINARIES: &[&str] = &[
    "chromium",
    "chromium-browser",
    "chromium-browser-unstable",
    "chromium-browser-stable",
    "chromium-freeworld",
    "ungoogled-chromium",
];

/// Names that are recognised but never installed into (used for reporting).
#[allow(dead_code)]
pub const COMMERCIAL_BINARIES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "google-chrome-beta",
    "google-chrome-unstable",
    "chrome",
    "brave-browser",
    "brave",
    "vivaldi",
    "vivaldi-stable",
    "microsoft-edge",
    "microsoft-edge-stable",
    "msedge",
    "opera",
];

/// Directories that are scanned for browser binaries (Linux layout).
pub const SCAN_DIRS: &[&str] = &[
    "/usr/lib/chromium",
    "/usr/lib64/chromium",
    "/usr/lib/chromium-browser",
    "/usr/lib64/chromium-browser",
    "/usr/lib/chromium-freeworld",
    "/usr/lib64/chromium-freeworld",
    "/opt/chromium.org",
    "/opt/chromium",
    "/usr/local/lib/chromium",
    "/usr/local/lib64/chromium",
    "/usr/local/bin",
];

pub const SCAN_GLOBS: &[&str] = &["/opt/*", "/opt/*/*", "/usr/local/*", "/app/*"];

pub const FLATPAK_APPS: &[(&str, &str)] = &[("org.chromium.Chromium", "chromium")];

pub const SNAP_APPS: &[(&str, &str)] = &[("chromium", "chromium")];

/// Profile directory names, relative to the platform's configuration directory.
pub const CHROMIUM_PROFILE: &str = "chromium";

/// Return the profile directory name for a Chromium based browser.
pub fn chromium_profile_name(binary_name: &str) -> &'static str {
    let name = binary_name
        .to_lowercase()
        .trim_end_matches(".exe")
        .to_string();
    if name.starts_with("chromium") || name == "ungoogled-chromium" {
        return CHROMIUM_PROFILE;
    }
    ""
}

// ---------------------------------------------------------------------------
// Firefox (Gecko) layout
// ---------------------------------------------------------------------------
/// Firefox loads the Widevine CDM as a Gecko Media Plugin (GMP):
///
/// ```text
/// <base>/gmp-widevinecdm/system-installed/
///     manifest.json
///     <library>
/// ```
pub const FIREFOX_GMP_ID: &str = "gmp-widevinecdm";
pub const FIREFOX_GMP_VERSION: &str = "system-installed";
pub const FIREFOX_PREF_FILE: &str = "gmpwidevine.js";

pub const FIREFOX_PREFS: &[(&str, &str)] = &[
    ("media.gmp-widevinecdm.version", "\"system-installed\""),
    ("media.gmp-widevinecdm.visible", "true"),
    ("media.gmp-widevinecdm.enabled", "true"),
    ("media.gmp-widevinecdm.autoupdate", "false"),
    ("media.eme.enabled", "true"),
    (
        "media.eme.encrypted-media-encryption-scheme.enabled",
        "true",
    ),
];

pub const FIREFOX_BINARIES: &[&str] = &[
    "firefox",
    "firefox-bin",
    "firefox-esr",
    "firefox-nightly",
    "firefox-developer-edition",
    "firefox-beta",
    "librewolf",
    "waterfox",
    "waterfox-classic",
    "waterfox-current",
    "icecat",
    "gnu-icecat",
    "mullvad-browser",
];

/// Directories that are scanned for Firefox installations (Linux layout).
pub const FIREFOX_SCAN_DIRS: &[&str] = &[
    "/usr/lib64/firefox",
    "/usr/lib/firefox",
    "/usr/lib64/firefox-esr",
    "/usr/lib/firefox-esr",
    "/opt/firefox",
    "/opt/firefox-nightly",
    "/opt/firefox-developer-edition",
    "/opt/librewolf",
    "/opt/waterfox",
    "/usr/local/lib/firefox",
];

/// System wide Mozilla plugin directories (Linux layout), in search order.
pub const MOZILLA_PLUGIN_DIRS: &[&str] = &[
    "/usr/lib64/mozilla/plugins",
    "/usr/lib/mozilla/plugins",
    "/usr/local/lib64/mozilla/plugins",
    "/usr/local/lib/mozilla/plugins",
];

pub const FIREFOX_FLATPAK_APPS: &[(&str, &str)] = &[
    ("org.mozilla.firefox", "firefox"),
    ("org.mozilla.firefox.BaseApp", "firefox"),
    ("io.github.librewolf.LibreWolf", "librewolf"),
];

pub const FIREFOX_SNAP_APPS: &[(&str, &str)] = &[("firefox", "firefox")];

"""Shared constants for wideviner."""

from __future__ import annotations

APP_NAME = "wideviner"
APP_VERSION = "0.2.0"
APP_TAGLINE = "Widevine CDM installer for Chromium and Firefox"

# ---------------------------------------------------------------------------
# Widevine sources
# ---------------------------------------------------------------------------
# Google publishes the stable Chrome .deb under a fixed URL that always serves
# the current release. The Widevine CDM bundled inside it is extracted and
# installed into the browser. This is Google's own download server (not a
# third-party mirror) and the URL has been stable for many years.
CHROME_DEB_URL = (
    "https://dl.google.com/linux/direct/google-chrome-stable_current_{deb_arch}.deb"
)

# Mozilla publishes, for every Firefox release, a manifest that lists the
# official Google download URLs of the current Widevine CDM for each platform,
# together with its SHA-512 hash. Chrome and Firefox both consume exactly these
# CRX packages, so this is the official source for Windows (and it works on
# Linux too).
MOZILLA_GMP_MANIFEST = (
    "https://raw.githubusercontent.com/mozilla-firefox/firefox/refs/heads/main/"
    "toolkit/content/gmp-sources/widevinecdm.json"
)
MOZILLA_GMP_MANIFEST_FALLBACK = (
    "https://hg.mozilla.org/mozilla-central/raw-file/tip/"
    "toolkit/content/gmp-sources/widevinecdm.json"
)
MOZILLA_GMP_VENDOR = "gmp-widevinecdm"

# ---------------------------------------------------------------------------
# Architectures
# ---------------------------------------------------------------------------
# "machine" lists the values that platform.machine() may report.
ARCHES = {
    "amd64": {
        "machine": ("x86_64", "amd64", "x64", "em64t"),
        "label": "x86_64 (amd64)",
        "deb_arch": "amd64",
    },
    "arm64": {
        "machine": ("aarch64", "arm64", "armv8l"),
        "label": "ARM64 (aarch64)",
        "deb_arch": "arm64",
    },
    "x86": {
        "machine": ("i386", "i486", "i586", "i686", "x86", "ia32"),
        "label": "x86 (32-bit)",
        "deb_arch": "i386",
    },
}

# ---------------------------------------------------------------------------
# Widevine CDM layout per platform
# ---------------------------------------------------------------------------
# (operating system, architecture) -> where the CDM lives inside a package and
# what the library is called. "mozilla_key" is the platform key used in
# Mozilla's GMP manifest ("None" means there is no CRX for that platform).
CDM_PLATFORMS = {
    ("linux", "amd64"): {
        "dir": "linux_x64",
        "lib": "libwidevinecdm.so",
        "mozilla_key": "Linux_x86_64-gcc3",
    },
    ("linux", "arm64"): {
        "dir": "linux_arm64",
        "lib": "libwidevinecdm.so",
        "mozilla_key": None,
    },
    ("windows", "amd64"): {
        "dir": "win_x64",
        "lib": "widevinecdm.dll",
        "mozilla_key": "WINNT_x86_64-msvc",
    },
    ("windows", "arm64"): {
        "dir": "win_arm64",
        "lib": "widevinecdm.dll",
        "mozilla_key": "WINNT_aarch64-msvc-aarch64",
    },
    ("windows", "x86"): {
        "dir": "win_x86",
        "lib": "widevinecdm.dll",
        "mozilla_key": "WINNT_x86-msvc",
    },
}

# ---------------------------------------------------------------------------
# Widevine CDM layout
# ---------------------------------------------------------------------------
CDM_DIR_NAME = "WidevineCdm"
CDM_PLATFORM_DIR = "_platform_specific"
CDM_MANIFEST_NAME = "manifest.json"
CDM_LICENSE_NAME = "LICENSE"

# Chromium reads this JSON hint file from its user data directory to locate a
# component-updated Widevine CDM. The mechanism only exists on Linux/ChromeOS
# (see chrome/common/chrome_paths.cc), so it is not used elsewhere.
HINT_FILE_NAME = "latest-component-updated-widevine-cdm"

# Location of the Widevine CDM inside the Chrome .deb (inside data.tar.*).
DEB_CDM_PATH = "opt/google/chrome/WidevineCdm"

# Number of bytes requested when trying to extract the CDM with a partial
# (HTTP range) download. The CDM sits near the start of the archive, so this
# is enough to recover it without downloading the whole package (the library
# compresses to roughly 15 MiB). If the probe is not enough, wideviner falls
# back to the full package.
RANGE_PROBE_BYTES = 24 * 1024 * 1024

# ---------------------------------------------------------------------------
# Chromium based browsers
# ---------------------------------------------------------------------------
# Only open source browsers are handled. Browsers that already bundle Widevine
# or are distributed under a proprietary licence are deliberately ignored:
#
#   Google Chrome, Microsoft Edge, Opera, Vivaldi, Brave
#
# They ship the CDM themselves (usually through their own agreement with
# Google), so there is nothing to install and copying a second copy into them
# is both unnecessary and legally questionable. Pointing --target at one of
# them is refused as well.
BROWSER_BINARIES = (
    "chromium",
    "chromium-browser",
    "chromium-browser-unstable",
    "chromium-browser-stable",
    "chromium-freeworld",
    "ungoogled-chromium",
)

# Names that are recognised but never installed into (used for reporting).
COMMERCIAL_BINARIES = (
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
)

# Directories that are scanned for browser binaries (Linux layout).
SCAN_DIRS = (
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
)

SCAN_GLOBS = (
    "/opt/*",
    "/opt/*/*",
    "/usr/local/*",
    "/app/*",
)

FLATPAK_APPS = {
    "org.chromium.Chromium": "chromium",
}

SNAP_APPS = {
    "chromium": "chromium",
}

# Profile directory names, relative to the platform's configuration directory.
# On Linux that is $XDG_CONFIG_HOME and on Windows %LOCALAPPDATA%.
CHROMIUM_PROFILES = {
    "chromium": "chromium",
}

# ---------------------------------------------------------------------------
# Firefox (Gecko) layout
# ---------------------------------------------------------------------------
# Firefox loads the Widevine CDM as a Gecko Media Plugin (GMP). The layout is
# fixed by the browser (see dom/media/gmp/GMPParent.cpp):
#
#     <base>/gmp-widevinecdm/system-installed/
#         manifest.json
#         <library>
#
# The GMP name is the parent directory without the "gmp-" prefix and the GMP
# version is the leaf directory name, which is why the prefs select the
# version "system-installed".
FIREFOX_GMP_ID = "gmp-widevinecdm"
FIREFOX_GMP_VERSION = "system-installed"
FIREFOX_PREF_FILE = "gmpwidevine.js"

FIREFOX_PREFS = (
    ("media.gmp-widevinecdm.version", '"system-installed"'),
    ("media.gmp-widevinecdm.visible", "true"),
    ("media.gmp-widevinecdm.enabled", "true"),
    ("media.gmp-widevinecdm.autoupdate", "false"),
    ("media.eme.enabled", "true"),
    ("media.eme.encrypted-media-encryption-scheme.enabled", "true"),
)

FIREFOX_BINARIES = (
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
)

# Directories that are scanned for Firefox installations (Linux layout).
FIREFOX_SCAN_DIRS = (
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
)

# System wide Mozilla plugin directories (Linux layout), in search order.
MOZILLA_PLUGIN_DIRS = (
    "/usr/lib64/mozilla/plugins",
    "/usr/lib/mozilla/plugins",
    "/usr/local/lib64/mozilla/plugins",
    "/usr/local/lib/mozilla/plugins",
)

FIREFOX_FLATPAK_APPS = {
    "org.mozilla.firefox": "firefox",
    "org.mozilla.firefox.BaseApp": "firefox",
    "io.github.librewolf.LibreWolf": "librewolf",
}

FIREFOX_SNAP_APPS = {
    "firefox": "firefox",
}


def chromium_profile_name(binary_name: str) -> str:
    """Return the profile directory name for a Chromium based browser.

    The returned value is relative to the platform's configuration directory.
    """
    name = binary_name.lower().removesuffix(".exe")
    if name.startswith("chromium") or name == "ungoogled-chromium":
        return CHROMIUM_PROFILES["chromium"]
    return ""


def cdm_platform(os_name: str, arch: str) -> dict | None:
    """Return the CDM layout for an operating system and architecture."""
    return CDM_PLATFORMS.get((os_name, arch))


def normalize_arch(machine: str) -> str:
    """Map a platform.machine() value to one of our architectures."""
    machine = (machine or "").lower()
    for arch, info in ARCHES.items():
        if machine in info["machine"]:
            return arch
    return ""

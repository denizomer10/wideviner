# SPDX-License-Identifier: MIT
"""Discovery of Chromium (and Chromium-based) installations.

Chromium can be installed in many different ways: distribution packages
(RPM/DEB), Flatpak, Snap, or a plain extracted archive. This module finds them
on Linux and Windows and works out where the Widevine CDM has to be placed for
each one.
"""

from __future__ import annotations

import glob
import json
import os
import re
import shutil
import signal
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

from . import constants as C
from .sysinfo import SystemInfo

_VERSION_RE = re.compile(r"(\d+\.\d+\.\d+\.\d+)")

# Directories that never contain a browser executable, even if a file with a
# matching name lives there (cron jobs, AppArmor profiles, shell completions,
# ...). Distribution packages ship such files next to the real binary.
_BAD_PARENT_DIRS = {
    "apparmor.d", "applications", "autostart", "bash_completion.d", "binfmt.d",
    "completion", "cron.d", "cron.daily", "cron.hourly", "cron.monthly",
    "cron.weekly", "dbus-1", "doc", "environment.d", "icons", "init.d",
    "locale", "locales", "logrotate.d", "man", "metainfo", "mime",
    "modules-load.d", "pam.d", "pixmaps", "polkit-1", "profile.d", "security",
    "selinux", "sysctl.d", "systemd", "tmpfiles.d", "udev", "xdg",
    "xdg-autostart",
}

# Windows install locations (relative to the usual base directories), open
# source browsers only.
_WINDOWS_INSTALL_PATHS = (
    ("Chromium/Application", "chrome.exe"),
    ("Chromium", "chrome.exe"),
    ("ungoogled-chromium", "chrome.exe"),
)


@dataclass
class Browser:
    """A discovered Chromium installation."""

    name: str
    binary: Path
    module_dir: Path
    source: str = "manual"
    version: str = ""
    user_data_dir: Path | None = None
    writable: bool = False
    widevine_version: str = ""
    widevine_path: Path | None = None
    notes: list[str] = field(default_factory=list)

    # -- locations ---------------------------------------------------------
    @property
    def kind(self) -> str:
        return "chromium"

    @property
    def executable(self) -> Path:
        """The actual browser binary.

        Distribution packages often install a small wrapper script that
        launches the real binary (``chrome``/``chromium``) next to it.
        """
        for candidate in (
            "chrome", "chromium", "chrome.exe", "chromium.exe",
            "google-chrome", "Google Chrome", self.name,
        ):
            path = self.module_dir / candidate
            if path.is_file():
                return path
        return self.binary

    @property
    def bundled_cdm_dir(self) -> Path:
        """Directory Chromium checks when the CDM is bundled with the build."""
        return self.module_dir / C.CDM_DIR_NAME

    def hint_file_path(self) -> Path | None:
        """Path of the component-updated CDM hint file (Linux only)."""
        if self.user_data_dir is None:
            return None
        return self.user_data_dir / C.CDM_DIR_NAME / C.HINT_FILE_NAME

    def user_cdm_dir(self, version: str) -> Path | None:
        """Directory used for a user-level (non-root) installation."""
        if self.user_data_dir is None:
            return None
        return self.user_data_dir / C.CDM_DIR_NAME / version

    # -- state -------------------------------------------------------------
    def detect_widevine(self) -> None:
        """Look for an already installed Widevine CDM."""
        version = _manifest_version(self.bundled_cdm_dir / C.CDM_MANIFEST_NAME)
        if version:
            self.widevine_version = version
            self.widevine_path = self.bundled_cdm_dir
            return

        hint = self.hint_file_path()
        if hint is not None and hint.is_file():
            try:
                data = json.loads(hint.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                data = {}
            path = data.get("Path") if isinstance(data, dict) else None
            if isinstance(path, str) and path:
                cdm_dir = Path(path)
                version = _manifest_version(cdm_dir / C.CDM_MANIFEST_NAME)
                if version:
                    self.widevine_version = version
                    self.widevine_path = cdm_dir
                    return

        self.widevine_version = ""
        self.widevine_path = None

    def describe(self) -> str:
        parts = [self.name]
        if self.version:
            parts.append(self.version)
        parts.append(f"[{self.source}]")
        return " ".join(parts)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
def _run(command: list[str], timeout: int = 20) -> str | None:
    """Run *command* and return its stdout, or None on failure."""
    try:
        result = subprocess.run(
            command, capture_output=True, text=True, timeout=timeout, check=False
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if result.returncode != 0:
        return None
    return result.stdout


def _manifest_version(path: Path) -> str:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return ""
    version = data.get("version") if isinstance(data, dict) else None
    return version if isinstance(version, str) else ""


def _probe_version(binary: Path) -> str:
    """Ask a browser binary for its version."""
    output = _run([str(binary), "--version"], timeout=15)
    if not output:
        return ""
    first_line = output.strip().splitlines()[0] if output.strip() else ""
    match = _VERSION_RE.search(first_line)
    if match:
        return match.group(1)
    return first_line


def _package_source(path: Path) -> str:
    if shutil.which("rpm") and _run(["rpm", "-qf", "--qf", "%{NAME}", str(path)]):
        return "rpm"
    if shutil.which("dpkg") and _run(["dpkg", "-S", str(path)]):
        return "deb"
    return "manual"


def _is_writable(path: Path) -> bool:
    try:
        return path.is_dir() and os.access(path, os.W_OK)
    except OSError:
        return False


def _looks_like_browser(path: Path) -> bool:
    """Heuristically decide whether *path* is a browser executable.

    Distribution packages ship files with the same name as the browser in
    places like /etc/cron.daily or apparmor.d, so a name match alone is not
    enough.
    """
    if not path.is_file():
        return False
    if path.parent.name in _BAD_PARENT_DIRS:
        return False
    if os.name != "nt" and not os.access(path, os.X_OK):
        return False
    try:
        size = path.stat().st_size
    except OSError:
        return False
    if size == 0:
        return False
    if size >= 1_000_000:
        return True
    try:
        head = path.read_bytes()[:8192]
    except OSError:
        return False
    if head.startswith((b"\x7fELF", b"MZ", b"\xcf\xfa\xed\xfe")):
        return True
    text = head.decode("utf-8", "replace")
    if "CHROME_WRAPPER" in text:
        return True
    return bool(
        re.search(r"(exec|launch)[^\n]{0,120}(chrome|chromium)", text, re.IGNORECASE)
    )


def _is_chromium_name(path: Path) -> bool:
    """Check whether *path* is a supported (open source) browser binary.

    A plain ``chrome`` binary is the standard name of extracted Chromium
    builds, so it is accepted when the path also mentions Chromium. Google
    Chrome and the other commercial browsers are never accepted.
    """
    name = path.name.lower().removesuffix(".exe")
    if name in C.BROWSER_BINARIES:
        return True
    return name == "chrome" and "chromium" in str(path).lower()


def _find_binary_in(root: Path, name: str, max_depth: int = 4) -> Path | None:
    """Search *root* for a browser binary called *name* (or chrome/chromium)."""
    wanted = {name, "chrome", "chromium", "chrome.exe", "chromium.exe"}
    if not root.is_dir():
        return None
    base_depth = len(root.parts)
    for dirpath, dirnames, filenames in os.walk(root):
        current = Path(dirpath)
        if len(current.parts) - base_depth >= max_depth:
            dirnames[:] = []
        for filename in filenames:
            if filename in wanted:
                candidate = current / filename
                if candidate.is_file():
                    return candidate
    return None


def _versioned_module_dir(binary: Path) -> Path:
    """Return the directory that holds the browser's assets.

    Windows Chrome keeps the versioned files (and therefore WidevineCdm) in an
    ``Application\\<version>`` subdirectory, while plain Chromium builds and
    every other platform keep them next to the binary.
    """
    parent = binary.parent
    if os.name != "nt":
        return parent
    candidates: list[tuple[tuple[int, ...], Path]] = []
    try:
        for entry in parent.iterdir():
            if not (
                entry.is_dir()
                and re.fullmatch(r"\d+(\.\d+){2,}", entry.name)
                and (
                    (entry / "chrome.dll").is_file()
                    or (entry / "WidevineCdm").is_dir()
                )
            ):
                continue
            candidates.append((_numeric_key(entry.name), entry))
    except OSError:
        return parent
    if candidates:
        candidates.sort(key=lambda item: item[0])
        return candidates[-1][1]
    return parent


def _numeric_key(value: str) -> tuple[int, ...]:
    return tuple(int(part) for part in re.findall(r"\d+", value))


# ---------------------------------------------------------------------------
# Profile discovery
# ---------------------------------------------------------------------------
_PROFILE_MARKERS = ("Local State", "Default", "First Run", "Last Version")


def _looks_like_profile(path: Path) -> bool:
    """Check whether *path* is a Chromium profile directory."""
    if not path.is_dir():
        return False
    for marker in _PROFILE_MARKERS:
        try:
            if (path / marker).exists():
                return True
        except OSError:
            continue
    return False


def _find_existing_profile(config_home: Path, basename: str) -> Path | None:
    """Find an existing Chromium profile directory below *config_home*."""
    if not basename or not config_home.is_dir():
        return None
    matches: list[Path] = []
    try:
        for entry in config_home.iterdir():
            if not entry.name.startswith(basename):
                continue
            if entry.name.endswith("-headless"):
                continue
            if _looks_like_profile(entry):
                matches.append(entry)
    except OSError:
        return None
    if not matches:
        return None
    try:
        matches.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    except OSError:
        pass
    return matches[0]


def _config_dir_names(config: Path) -> list[str]:
    """Return the profile directory names below *config*."""
    if not config.is_dir():
        return []
    names: list[str] = []
    try:
        for entry in config.iterdir():
            if entry.is_dir() and not entry.name.endswith("-headless"):
                names.append(entry.name)
    except OSError:
        return []
    return names


def _terminate(process: subprocess.Popen) -> None:
    """Stop a browser process and its children after probing."""
    if process.poll() is not None:
        return
    if os.name == "nt":
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
        return
    try:
        os.killpg(os.getpgid(process.pid), signal.SIGTERM)
    except (OSError, ProcessLookupError):
        process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(os.getpgid(process.pid), signal.SIGKILL)
        except (OSError, ProcessLookupError):
            process.kill()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass


def _probe_profile_name(binary: Path, timeout: int = 30) -> str | None:
    """Learn the profile directory name by running the browser once.

    The browser is started with a temporary home directory, so it creates its
    profile there and nothing touches the real one. It is terminated as soon
    as the directory appears.
    """
    import tempfile
    import time

    try:
        tmp = tempfile.mkdtemp(prefix="wideviner-home-")
    except OSError:
        return None

    result: str | None = None
    try:
        env = dict(os.environ)
        env["HOME"] = tmp
        env["USERPROFILE"] = tmp
        for key in ("XDG_CONFIG_HOME", "CHROME_CONFIG_HOME", "CHROME_USER_DATA_DIR"):
            env.pop(key, None)
        try:
            process = subprocess.Popen(
                [
                    str(binary),
                    "--headless=new",
                    "--no-first-run",
                    "--no-default-browser-check",
                    "--disable-background-networking",
                    "--disable-component-update",
                    "--disable-sync",
                    "about:blank",
                ],
                env=env,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=os.name != "nt",
            )
        except OSError:
            return None

        if os.name == "nt":
            config = Path(tmp) / "AppData" / "Local"
        else:
            config = Path(tmp) / ".config"

        names: list[str] = []
        deadline = time.monotonic() + timeout
        try:
            while time.monotonic() < deadline:
                names = _config_dir_names(config)
                if names or process.poll() is not None:
                    break
                time.sleep(0.25)
        finally:
            _terminate(process)

        if names:
            for name in names:
                if _looks_like_profile(config / name):
                    result = name
                    break
            else:
                result = names[0]
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    return result


def _resolve_user_data_dir(
    si: SystemInfo, binary: Path, *, probe: bool = True
) -> Path | None:
    """Work out where the browser keeps its profile.

    On Linux the directory name carries a build specific suffix
    ("chromium-clang" for the official Chromium Linux builds), so an existing
    profile is used when one is found and the browser is asked otherwise.
    """
    basename = C.chromium_profile_name(binary.name)
    if not basename:
        return None
    config_home = si.config_home

    if si.is_linux:
        existing = _find_existing_profile(config_home, basename)
        if existing is not None:
            return existing
        if probe and not si.is_admin and binary.is_file():
            name = _probe_profile_name(binary)
            if name:
                return config_home / name

    return config_home / basename


# ---------------------------------------------------------------------------
# Candidate collection
# ---------------------------------------------------------------------------
def _binary_names(si: SystemInfo) -> tuple[str, ...]:
    if si.is_windows:
        return tuple(f"{name}.exe" for name in C.BROWSER_BINARIES)
    return C.BROWSER_BINARIES


def _path_candidates(si: SystemInfo) -> list[Path]:
    paths: list[Path] = []
    for name in _binary_names(si):
        found = shutil.which(name)
        if found:
            candidate = Path(found)
            if _looks_like_browser(candidate):
                paths.append(candidate)
    return paths


def _package_candidates(si: SystemInfo) -> list[Path]:
    """Ask the package manager for browser packages and their files."""
    if not si.is_linux:
        return []
    paths: list[Path] = []
    keywords = ("chromium", "chrome", "brave", "vivaldi", "edge", "opera")
    names = set(C.BROWSER_BINARIES)

    if shutil.which("rpm"):
        listing = _run(["rpm", "-qa", "--qf", "%{NAME}\n"])
        if listing:
            for package in listing.splitlines():
                if not any(keyword in package.lower() for keyword in keywords):
                    continue
                files = _run(["rpm", "-ql", package])
                if not files:
                    continue
                for line in files.splitlines():
                    candidate = Path(line)
                    if candidate.name in names and _looks_like_browser(candidate):
                        paths.append(candidate)

    if shutil.which("dpkg"):
        listing = _run(["dpkg-query", "-W", "-f", "${Package}\n"])
        if listing:
            for package in listing.splitlines():
                if not any(keyword in package.lower() for keyword in keywords):
                    continue
                files = _run(["dpkg", "-L", package])
                if not files:
                    continue
                for line in files.splitlines():
                    candidate = Path(line)
                    if candidate.name in names and _looks_like_browser(candidate):
                        paths.append(candidate)

    return paths


def _windows_candidates() -> list[Path]:
    """Look for browsers in the usual Windows install locations."""
    bases: list[Path] = []
    for variable in ("PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA", "PROGRAMDATA"):
        value = os.environ.get(variable)
        if value:
            bases.append(Path(value))
    paths: list[Path] = []
    for base in bases:
        for relative, executable in _WINDOWS_INSTALL_PATHS:
            candidate = base / relative / executable
            if candidate.is_file():
                paths.append(candidate)
    return paths


def _scan_candidates(si: SystemInfo, deep: bool = False) -> list[Path]:
    """Scan well-known directories for browser binaries."""
    if si.is_windows:
        return _windows_candidates()

    paths: list[Path] = []
    names = set(C.BROWSER_BINARIES)

    directories = [Path(d) for d in C.SCAN_DIRS]
    for pattern in C.SCAN_GLOBS:
        directories.extend(Path(p) for p in glob.glob(pattern))

    for directory in directories:
        if not directory.is_dir():
            continue
        for name in names:
            candidate = directory / name
            if _looks_like_browser(candidate):
                paths.append(candidate)

    if deep:
        paths.extend(_deep_scan())
    return paths


def _deep_scan(
    roots: tuple[str, ...] = ("/usr", "/opt", "/usr/local", "/snap", "/var/lib/flatpak"),
    max_depth: int = 4,
) -> list[Path]:
    """Walk the usual installation roots looking for browser binaries."""
    names = set(C.BROWSER_BINARIES)
    skip = {
        "share", "include", "src", "doc", "man", "locale", "icons", "firmware",
        "modules", "debug", "python3", "node_modules", "tests", "test",
    }
    results: list[Path] = []
    for root in roots:
        root_path = Path(root)
        if not root_path.is_dir():
            continue
        base_depth = len(root_path.parts)
        for dirpath, dirnames, filenames in os.walk(root_path):
            current = Path(dirpath)
            if len(current.parts) - base_depth >= max_depth:
                dirnames[:] = []
            else:
                dirnames[:] = [
                    d for d in dirnames if d not in skip and not d.startswith(".")
                ]
            for filename in filenames:
                if filename in names:
                    candidate = current / filename
                    if _looks_like_browser(candidate):
                        results.append(candidate)
    return results


def _target_paths(target: str) -> list[Path]:
    """Resolve a user supplied --target value to browser binaries."""
    path = Path(target).expanduser()
    if path.is_file():
        return [path] if _is_chromium_name(path) else []
    if path.is_dir():
        for name in C.BROWSER_BINARIES:
            candidate = path / name
            if candidate.is_file():
                return [candidate]
        found = _find_binary_in(path, path.name)
        if found is not None and _is_chromium_name(found):
            return [found]
    return []


# ---------------------------------------------------------------------------
# Browser construction
# ---------------------------------------------------------------------------
def _build_browser(
    binary: Path,
    si: SystemInfo,
    *,
    source: str | None = None,
    user_data_dir: Path | None = None,
    module_dir: Path | None = None,
    version: str = "",
    notes: list[str] | None = None,
) -> Browser | None:
    try:
        real = binary.resolve()
    except OSError:
        real = binary
    if not real.is_file():
        return None

    if module_dir is None:
        module_dir = _versioned_module_dir(real)
    if source is None:
        source = _package_source(real)
    if not version:
        version = _probe_version(real)
    if user_data_dir is None:
        user_data_dir = _resolve_user_data_dir(si, real)

    browser = Browser(
        name=real.name,
        binary=real,
        module_dir=module_dir,
        source=source,
        version=version,
        user_data_dir=user_data_dir,
        notes=list(notes or []),
    )
    browser.writable = _is_writable(module_dir)
    browser.detect_widevine()
    return browser


def _flatpak_browsers(si: SystemInfo) -> list[Browser]:
    if not si.flatpak:
        return []
    listing = _run(
        ["flatpak", "list", "--app", "--columns=application,version,installation"]
    )
    if not listing:
        return []

    browsers: list[Browser] = []
    for line in listing.splitlines():
        parts = line.split("\t")
        if not parts:
            continue
        app_id = parts[0].strip()
        if app_id not in C.FLATPAK_APPS:
            continue
        version = parts[1].strip() if len(parts) > 1 else ""
        installation = parts[2].strip() if len(parts) > 2 else "system"
        name = C.FLATPAK_APPS[app_id]

        if installation == "user":
            app_root = (
                Path.home() / ".local/share/flatpak/app" / app_id
                / "current/active/files"
            )
        else:
            app_root = Path("/var/lib/flatpak/app") / app_id / "current/active/files"

        binary = _find_binary_in(app_root, name) or (app_root / name)
        user_data_dir = (
            si.home / ".var/app" / app_id / "config" / C.chromium_profile_name(name)
        )

        browser = Browser(
            name=name,
            binary=binary,
            module_dir=app_root,
            source="flatpak",
            version=version,
            user_data_dir=user_data_dir,
            notes=[f"Flatpak application {app_id} ({installation})"],
        )
        browser.writable = False
        browser.detect_widevine()
        browsers.append(browser)
    return browsers


def _snap_browsers(si: SystemInfo) -> list[Browser]:
    if not si.snap:
        return []
    listing = _run(["snap", "list"])
    if not listing:
        return []

    browsers: list[Browser] = []
    for line in listing.splitlines()[1:]:
        parts = line.split()
        if not parts:
            continue
        snap_name = parts[0]
        if snap_name not in C.SNAP_APPS:
            continue
        version = parts[1] if len(parts) > 1 else ""
        name = C.SNAP_APPS[snap_name]

        app_root = Path("/snap") / snap_name / "current"
        binary = _find_binary_in(app_root, name) or (app_root / name)
        user_data_dir = (
            si.home / "snap" / snap_name / "current/.config"
            / C.chromium_profile_name(name)
        )

        browser = Browser(
            name=name,
            binary=binary,
            module_dir=app_root,
            source="snap",
            version=version,
            user_data_dir=user_data_dir,
            notes=[f"Snap package {snap_name}"],
        )
        browser.writable = False
        browser.detect_widevine()
        browsers.append(browser)
    return browsers


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------
def discover(
    si: SystemInfo, *, deep: bool = False, target: str | None = None
) -> list[Browser]:
    """Find Chromium installations on this system."""
    browsers: list[Browser] = []
    seen: set[str] = set()

    def push(browser: Browser | None) -> None:
        if browser is None:
            return
        key = str(browser.binary)
        if key in seen:
            return
        seen.add(key)
        browsers.append(browser)

    if target:
        for path in _target_paths(target):
            push(_build_browser(path, si))
    else:
        for path in _path_candidates(si):
            push(_build_browser(path, si))
        for path in _package_candidates(si):
            push(_build_browser(path, si))
        for path in _scan_candidates(si, deep=deep):
            push(_build_browser(path, si))
        for browser in _flatpak_browsers(si):
            push(browser)
        for browser in _snap_browsers(si):
            push(browser)

    browsers.sort(key=lambda b: (b.name, str(b.binary)))
    return browsers

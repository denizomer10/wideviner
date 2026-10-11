# SPDX-License-Identifier: MIT
"""Discovery and installation support for Firefox (Gecko) browsers.

Firefox loads the Widevine CDM as a Gecko Media Plugin (GMP). The layout is
fixed by the browser (see ``dom/media/gmp/GMPParent.cpp``):

    <base>/gmp-widevinecdm/system-installed/
        manifest.json
        <library>            libwidevinecdm.so / .dylib / widevinecdm.dll

* the GMP name is the parent directory name without the ``gmp-`` prefix,
* the GMP version is the leaf directory name (``system-installed``),

which is why the prefs select exactly that version:

    pref("media.gmp-widevinecdm.version", "system-installed");

Firefox discovers additional plugin directories through the ``MOZ_GMP_PATH``
environment variable (a list separated by ``:`` on Unix and ``;`` on Windows).
On Linux that is set through profile.d/environment.d snippets - the same way
Fedora ships OpenH264 - and on Windows through the user's environment in the
registry. The profile itself is always used as well, so a user level
installation works everywhere without administrator rights.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

from . import constants as C
from .sysinfo import SystemInfo

_VERSION_RE = re.compile(r"(\d+(?:\.\d+)+)")


@dataclass
class Firefox:
    """A discovered Firefox (or Firefox fork) installation."""

    name: str
    binary: Path
    install_dir: Path
    version: str = ""
    source: str = "manual"
    profile_dir: Path | None = None
    widevine_version: str = ""
    widevine_path: Path | None = None
    library: str = "libwidevinecdm.so"
    notes: list[str] = field(default_factory=list)

    # -- locations ---------------------------------------------------------
    @property
    def kind(self) -> str:
        return "firefox"

    @property
    def sandboxed(self) -> bool:
        """True for Flatpak/Snap apps whose files live in an immutable image.

        A sandboxed Firefox cannot see the host's Mozilla plugin directories,
        so the GMP has to go into its own profile.
        """
        return self.source in ("flatpak", "snap")

    def gmp_dirs(self) -> list[Path]:
        """Directories that may hold the Widevine GMP, in search order."""
        dirs: list[Path] = []
        profile_gmp = self.profile_gmp_dir()
        if profile_gmp is not None:
            dirs.append(profile_gmp)
        for base in C.MOZILLA_PLUGIN_DIRS:
            dirs.append(Path(base) / C.FIREFOX_GMP_ID / C.FIREFOX_GMP_VERSION)
        return dirs

    def profile_gmp_dir(self) -> Path | None:
        """GMP directory inside the profile."""
        if self.profile_dir is None:
            return None
        return self.profile_dir / C.FIREFOX_GMP_ID / C.FIREFOX_GMP_VERSION

    def pref_dir(self) -> Path:
        """Directory for system wide default prefs."""
        return self.install_dir / "defaults" / "pref"

    # -- state -------------------------------------------------------------
    def detect_widevine(self) -> None:
        for candidate in self.gmp_dirs():
            version = _manifest_version(candidate / C.CDM_MANIFEST_NAME)
            if version:
                self.widevine_version = version
                self.widevine_path = candidate
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
    output = _run([str(binary), "--version"], timeout=15)
    if not output:
        return ""
    match = _VERSION_RE.search(output)
    return match.group(1) if match else output.strip().splitlines()[0]


def _package_source(path: Path) -> str:
    if shutil.which("rpm") and _run(["rpm", "-qf", "--qf", "%{NAME}", str(path)]):
        return "rpm"
    if shutil.which("dpkg") and _run(["dpkg", "-S", str(path)]):
        return "deb"
    return "manual"


def _looks_like_firefox(path: Path) -> bool:
    """Heuristically decide whether *path* is a Firefox executable."""
    if not path.is_file():
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
    if head.startswith((b"\x7fELF", b"MZ")):
        return True
    text = head.decode("utf-8", "replace")
    return "firefox" in text.lower() or "MOZ_APP_NAME" in text


def _is_firefox_name(name: str) -> bool:
    """Check whether a binary name looks like Firefox or one of its forks."""
    lowered = name.lower().removesuffix(".exe")
    if lowered in C.FIREFOX_BINARIES:
        return True
    return any(
        keyword in lowered
        for keyword in (
            "firefox", "librewolf", "waterfox", "icecat", "basilisk", "palemoon",
        )
    )


def _resolve_install_dir(binary: Path) -> Path:
    """Find the real Firefox installation directory.

    Distribution packages install a small wrapper script in /usr/bin that
    launches the real binary from the installation directory. That directory
    is identified by its ``application.ini`` file.
    """
    if (binary.parent / "application.ini").is_file():
        return binary.parent

    name = binary.name
    candidates = [Path(d) for d in C.FIREFOX_SCAN_DIRS]
    for libdir in (
        "/usr/lib64", "/usr/lib", "/usr/local/lib64", "/usr/local/lib", "/opt",
    ):
        candidates.append(Path(libdir) / name)
        candidates.append(Path(libdir) / name.replace("-bin", ""))

    for candidate in candidates:
        if (candidate / "application.ini").is_file():
            return candidate
    return binary.parent


def _find_binary(root: Path, max_depth: int = 4) -> Path | None:
    if not root.is_dir():
        return None
    base_depth = len(root.parts)
    for dirpath, dirnames, filenames in os.walk(root):
        current = Path(dirpath)
        if len(current.parts) - base_depth >= max_depth:
            dirnames[:] = []
        for filename in filenames:
            if _is_firefox_name(filename):
                candidate = current / filename
                if _looks_like_firefox(candidate):
                    return candidate
    return None


# ---------------------------------------------------------------------------
# Profile discovery
# ---------------------------------------------------------------------------
def _profile_roots(si: SystemInfo, source: str) -> list[Path]:
    roots: list[Path] = []
    if si.is_linux and source == "flatpak":
        roots.append(si.home / ".var/app/org.mozilla.firefox/.mozilla/firefox")
    elif si.is_linux and source == "snap":
        roots.append(si.home / "snap/firefox/common/.mozilla/firefox")
    if si.is_windows:
        appdata = os.environ.get("APPDATA")
        base = Path(appdata) if appdata else si.home / "AppData" / "Roaming"
        roots.append(base / "Mozilla" / "Firefox")
    else:
        roots.append(si.home / ".mozilla" / "firefox")
    return roots


def _parse_ini(path: Path) -> list[dict[str, str]]:
    sections: list[dict[str, str]] = []
    current: dict[str, str] | None = None
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return sections
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("[") and line.endswith("]"):
            current = {"__section__": line[1:-1]}
            sections.append(current)
        elif "=" in line and current is not None:
            key, _, value = line.partition("=")
            current[key.strip()] = value.strip()
    return sections


def _find_profile(si: SystemInfo, source: str) -> Path | None:
    """Return the default Firefox profile directory, if there is one."""
    for root in _profile_roots(si, source):
        ini = root / "profiles.ini"
        if not ini.is_file():
            continue
        sections = [
            s for s in _parse_ini(ini) if s.get("__section__", "").startswith("Profile")
        ]
        for section in sections:
            if section.get("Default") == "1" and section.get("Path"):
                candidate = root / section["Path"]
                if candidate.is_dir():
                    return candidate
        for section in sections:
            if section.get("Path"):
                candidate = root / section["Path"]
                if candidate.is_dir():
                    return candidate
    return None


# ---------------------------------------------------------------------------
# Candidate collection
# ---------------------------------------------------------------------------
def _binary_names(si: SystemInfo) -> tuple[str, ...]:
    if si.is_windows:
        return tuple(f"{name}.exe" for name in C.FIREFOX_BINARIES)
    return C.FIREFOX_BINARIES


def _path_candidates(si: SystemInfo) -> list[Path]:
    paths: list[Path] = []
    for name in _binary_names(si):
        found = shutil.which(name)
        if found:
            candidate = Path(found)
            if _looks_like_firefox(candidate):
                paths.append(candidate)
    return paths


def _package_candidates(si: SystemInfo) -> list[Path]:
    if not si.is_linux:
        return []
    paths: list[Path] = []
    keywords = ("firefox", "librewolf", "waterfox", "icecat", "basilisk", "palemoon")
    names = set(C.FIREFOX_BINARIES)

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
                    if candidate.name in names and _looks_like_firefox(candidate):
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
                    if candidate.name in names and _looks_like_firefox(candidate):
                        paths.append(candidate)

    return paths


def _scan_candidates(si: SystemInfo) -> list[Path]:
    if si.is_windows:
        return _windows_candidates()

    paths: list[Path] = []
    names = set(C.FIREFOX_BINARIES)
    for directory in C.FIREFOX_SCAN_DIRS:
        base = Path(directory)
        if not base.is_dir():
            continue
        for name in names:
            candidate = base / name
            if _looks_like_firefox(candidate):
                paths.append(candidate)
    return paths


def _windows_candidates() -> list[Path]:
    bases: list[Path] = []
    for variable in ("PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"):
        value = os.environ.get(variable)
        if value:
            bases.append(Path(value))
    paths: list[Path] = []
    for base in bases:
        for relative in ("Mozilla Firefox", "Firefox", "LibreWolf", "Waterfox"):
            candidate = base / relative / "firefox.exe"
            if candidate.is_file():
                paths.append(candidate)
    return paths


def _flatpak_candidates(si: SystemInfo) -> list[tuple[Path, str, str]]:
    if not si.flatpak:
        return []
    listing = _run(
        ["flatpak", "list", "--app", "--columns=application,version,installation"]
    )
    if not listing:
        return []
    results: list[tuple[Path, str, str]] = []
    for line in listing.splitlines():
        parts = line.split("\t")
        if not parts:
            continue
        app_id = parts[0].strip()
        if app_id not in C.FIREFOX_FLATPAK_APPS:
            continue
        version = parts[1].strip() if len(parts) > 1 else ""
        installation = parts[2].strip() if len(parts) > 2 else "system"
        if installation == "user":
            app_root = (
                Path.home() / ".local/share/flatpak/app" / app_id
                / "current/active/files"
            )
        else:
            app_root = Path("/var/lib/flatpak/app") / app_id / "current/active/files"
        binary = _find_binary(app_root) or (app_root / "firefox")
        results.append((binary, version, app_id))
    return results


def _snap_candidates(si: SystemInfo) -> list[tuple[Path, str]]:
    if not si.snap:
        return []
    listing = _run(["snap", "list"])
    if not listing:
        return []
    results: list[tuple[Path, str]] = []
    for line in listing.splitlines()[1:]:
        parts = line.split()
        if not parts or parts[0] not in C.FIREFOX_SNAP_APPS:
            continue
        version = parts[1] if len(parts) > 1 else ""
        app_root = Path("/snap") / parts[0] / "current"
        binary = _find_binary(app_root) or (app_root / "firefox")
        results.append((binary, version))
    return results


# ---------------------------------------------------------------------------
# Browser construction
# ---------------------------------------------------------------------------
def _build(
    binary: Path,
    si: SystemInfo,
    *,
    source: str | None = None,
    version: str = "",
    notes: list[str] | None = None,
) -> Firefox | None:
    try:
        real = binary.resolve()
    except OSError:
        real = binary
    if not real.is_file():
        return None

    install_dir = _resolve_install_dir(real)
    # Point at the real binary inside the installation directory so that a
    # wrapper in /usr/bin and the binary itself deduplicate.
    real_binary = install_dir / real.name
    if real_binary.is_file():
        real = real_binary

    if source is None:
        source = _package_source(real)
    if not version:
        version = _probe_version(real)

    firefox = Firefox(
        name=real.name,
        binary=real,
        install_dir=install_dir,
        version=version,
        source=source,
        library=si.lib_name or "libwidevinecdm.so",
        notes=list(notes or []),
    )
    firefox.profile_dir = _find_profile(si, source)
    firefox.detect_widevine()
    return firefox


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------
def discover(
    si: SystemInfo, *, deep: bool = False, target: str | None = None
) -> list[Firefox]:
    """Find Firefox installations on this system."""
    browsers: list[Firefox] = []
    seen: set[str] = set()

    def push(browser: Firefox | None) -> None:
        if browser is None:
            return
        key = str(browser.binary)
        if key in seen:
            return
        seen.add(key)
        browsers.append(browser)

    if target:
        path = Path(target).expanduser()
        if path.is_file():
            if _is_firefox_name(path.name):
                push(_build(path, si))
        elif path.is_dir():
            found = _find_binary(path)
            if found is not None:
                push(_build(found, si))
    else:
        for path in _path_candidates(si):
            push(_build(path, si))
        for path in _package_candidates(si):
            push(_build(path, si))
        for path in _scan_candidates(si):
            push(_build(path, si))
        for binary, version, app_id in _flatpak_candidates(si):
            push(
                _build(
                    binary,
                    si,
                    source="flatpak",
                    version=version,
                    notes=[f"Flatpak application {app_id}"],
                )
            )
        for binary, version in _snap_candidates(si):
            push(
                _build(
                    binary,
                    si,
                    source="snap",
                    version=version,
                    notes=["Snap package firefox"],
                )
            )

    browsers.sort(key=lambda b: (b.name, str(b.binary)))
    return browsers


# ---------------------------------------------------------------------------
# Installation
# ---------------------------------------------------------------------------
def _pref_content() -> str:
    lines = ["// Widevine CDM installed by wideviner."]
    for name, value in C.FIREFOX_PREFS:
        lines.append(f'pref("{name}", {value});')
    return "\n".join(lines) + "\n"


def _shell_snippet(gmp_dir: Path, separator: str = ":") -> str:
    directory = os.path.abspath(str(gmp_dir))
    return (
        'if [[ ":$MOZ_GMP_PATH:" != *":' + directory + ':"* ]]; then\n'
        '    MOZ_GMP_PATH="${MOZ_GMP_PATH}${MOZ_GMP_PATH:+' + separator + '}'
        + directory + '"\n'
        "    export MOZ_GMP_PATH\n"
        "fi\n"
    )


def _fish_snippet(gmp_dir: Path) -> str:
    directory = os.path.abspath(str(gmp_dir))
    return (
        "set -x --path MOZ_GMP_PATH $MOZ_GMP_PATH\n"
        f"set dir {directory}\n"
        "if not contains $dir $MOZ_GMP_PATH\n"
        "    set -p MOZ_GMP_PATH $dir\n"
        "end\n"
        "set -e dir\n"
    )


def _set_windows_env(ui, name: str, value: str) -> None:
    """Add *value* to the user's environment variable *name* (Windows)."""
    try:
        import winreg  # type: ignore

        with winreg.OpenKey(
            winreg.HKEY_CURRENT_USER,
            "Environment",
            0,
            winreg.KEY_READ | winreg.KEY_WRITE,
        ) as key:
            try:
                current, _ = winreg.QueryValueEx(key, name)
            except FileNotFoundError:
                current = ""
            parts = [p for p in str(current).split(";") if p]
            if value not in parts:
                parts.append(value)
            winreg.SetValueEx(
                key, name, 0, winreg.REG_EXPAND_SZ, ";".join(parts)
            )
    except (ImportError, OSError) as exc:
        ui.warn(f"could not set {name}: {exc}")


def _write_user_prefs(profile_dir: Path) -> Path:
    """Add the Widevine prefs to the profile's user.js file."""
    user_js = profile_dir / "user.js"
    managed = {name for name, _ in C.FIREFOX_PREFS}
    kept: list[str] = []
    if user_js.is_file():
        try:
            for line in user_js.read_text(encoding="utf-8", errors="replace").splitlines():
                stripped = line.strip()
                if any(stripped.startswith(f'pref("{name}"') for name in managed):
                    continue
                kept.append(line)
        except OSError:
            kept = []
    block = _pref_content().rstrip("\n").splitlines()
    lines = list(kept)
    if lines:
        lines.append("")
    lines.append("// Added by wideviner.")
    lines.extend(block)
    user_js.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return user_js


def _firefox_files(payload, library: str) -> dict[str, bytes]:
    """Flatten the Chromium CDM layout into the layout Firefox expects.

    Chromium keeps the library below ``_platform_specific/<platform>/``, while
    Firefox wants ``manifest.json`` and the library directly inside the GMP
    directory (the same layout Fedora uses for OpenH264).
    """
    wanted = {C.CDM_MANIFEST_NAME, C.CDM_LICENSE_NAME, library}
    files: dict[str, bytes] = {}
    for relative, content in payload.files.items():
        name = Path(relative).name
        if name in wanted:
            files[name] = content
    return files


def _write_files(files: dict[str, bytes], target: Path) -> None:
    target.mkdir(parents=True, exist_ok=True)
    for relative, content in files.items():
        destination = target / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(content)


def _chown(path: Path, owner: tuple[int, int]) -> None:
    try:
        os.chown(path, owner[0], owner[1])
    except (OSError, AttributeError, NotImplementedError):
        pass


def _chown_tree(path: Path, owner: tuple[int, int]) -> None:
    _chown(path, owner)
    for dirpath, dirnames, filenames in os.walk(path):
        for name in dirnames + filenames:
            _chown(Path(dirpath) / name, owner)


def install(
    firefox: Firefox,
    payload,
    si: SystemInfo,
    ui,
    *,
    dry_run: bool = False,
    user_only: bool = False,
    owner: tuple[int, int] | None = None,
):
    """Install the CDM for *firefox*.

    On Linux a system wide installation puts the GMP below the Mozilla plugin
    directory and ships profile.d/environment.d snippets. Everywhere else, and
    whenever the browser directory is not writable, the GMP goes into the
    profile and the prefs into ``user.js``, which needs no administrator
    rights.
    """
    from .widevine import InstallResult

    version = payload.version or "unknown"
    library = si.lib_name or firefox.library

    system_gmp: Path | None = None
    if si.is_linux and not firefox.sandboxed:
        for base in C.MOZILLA_PLUGIN_DIRS:
            candidate = Path(base)
            if candidate.is_dir() and (si.is_admin or os.access(candidate, os.W_OK)):
                system_gmp = (
                    candidate / C.FIREFOX_GMP_ID / C.FIREFOX_GMP_VERSION
                )
                break
        if system_gmp is None and si.is_admin:
            system_gmp = (
                Path(C.MOZILLA_PLUGIN_DIRS[0])
                / C.FIREFOX_GMP_ID
                / C.FIREFOX_GMP_VERSION
            )

    profile_gmp = firefox.profile_gmp_dir()

    if not user_only and system_gmp is not None:
        target = system_gmp
        mode = "system"
    elif profile_gmp is not None:
        target = profile_gmp
        mode = "user"
    else:
        return InstallResult(
            ok=False,
            message=(
                "no writable installation location found; run the browser once "
                "so that it creates a profile, or run wideviner as administrator"
            ),
        )

    pref_file: Path | None = None
    env_files: list[Path] = []

    if mode == "system":
        pref_file = firefox.pref_dir() / C.FIREFOX_PREF_FILE
        env_files = [
            Path("/etc/profile.d/widevine.sh"),
            Path("/usr/share/fish/vendor_conf.d/widevine.fish"),
            Path("/usr/lib/environment.d/50-widevine.conf"),
        ]
    elif firefox.profile_dir is not None:
        pref_file = firefox.profile_dir / "user.js"

    if dry_run:
        return InstallResult(
            ok=True, version=version, path=target, mode=mode, message="dry run"
        )

    try:
        _write_files(_firefox_files(payload, library), target)
    except OSError as exc:
        return InstallResult(ok=False, message=f"cannot write to {target}: {exc}")

    hint_file: Path | None = None
    if pref_file is not None:
        try:
            if mode == "system":
                pref_file.parent.mkdir(parents=True, exist_ok=True)
                pref_file.write_text(_pref_content(), encoding="utf-8")
            else:
                pref_file = _write_user_prefs(pref_file.parent)
            hint_file = pref_file
        except OSError as exc:
            ui.warn(f"could not write the Firefox prefs: {exc}")

    for env_file in env_files:
        try:
            env_file.parent.mkdir(parents=True, exist_ok=True)
            if env_file.suffix == ".fish":
                env_file.write_text(_fish_snippet(target), encoding="utf-8")
            elif env_file.suffix == ".conf":
                env_file.write_text(
                    "MOZ_GMP_PATH=$MOZ_GMP_PATH${MOZ_GMP_PATH:+:}"
                    + os.path.abspath(str(target))
                    + "\n",
                    encoding="utf-8",
                )
            else:
                env_file.write_text(_shell_snippet(target), encoding="utf-8")
        except OSError as exc:
            ui.warn(f"could not write {env_file}: {exc}")

    if si.is_windows:
        _set_windows_env(ui, "MOZ_GMP_PATH", os.path.abspath(str(target)))
    if owner is not None and mode == "user" and firefox.profile_dir is not None:
        _chown_tree(firefox.profile_dir / C.FIREFOX_GMP_ID, owner)
        if pref_file is not None:
            _chown(pref_file, owner)

    return InstallResult(
        ok=True,
        version=version,
        path=target,
        mode=mode,
        hint_file=hint_file,
    )

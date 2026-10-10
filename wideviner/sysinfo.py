# SPDX-License-Identifier: MIT
"""Detection of the operating system, distribution and environment."""

from __future__ import annotations

import ctypes
import getpass
import os
import platform
import shutil
import sys
from dataclasses import dataclass, field
from pathlib import Path

from . import constants as C

# Distribution families and the IDs that belong to them (Linux only).
_FAMILIES = {
    "fedora": ("fedora", "rhel", "centos", "rocky", "almalinux", "ol"),
    "debian": ("debian", "ubuntu", "linuxmint", "pop", "raspbian", "kali", "neon"),
    "arch": ("arch", "manjaro", "endeavouros", "garuda", "cachyos", "artix"),
    "suse": ("opensuse", "suse", "sles", "sled"),
    "alpine": ("alpine",),
    "gentoo": ("gentoo", "funtoo"),
    "void": ("void",),
}

_PACKAGE_MANAGERS = (
    "dnf", "apt", "apt-get", "pacman", "zypper", "apk", "xbps-install", "emerge",
)


@dataclass
class SystemInfo:
    """Everything wideviner knows about the running system."""

    os_name: str = ""
    os_id: str = "unknown"
    os_like: tuple[str, ...] = ()
    os_name_pretty: str = "Unknown"
    os_version: str = ""
    pretty_name: str = "Unknown"
    family: str = "unknown"
    machine: str = ""
    arch: str = ""
    kernel: str = ""
    glibc: str = ""
    session_type: str = ""
    desktop: str = ""
    user: str = ""
    home: Path = field(default_factory=Path.home)
    is_admin: bool = False
    has_sudo: bool = False
    flatpak: bool = False
    snap: bool = False
    package_manager: str = ""

    # -- platform ----------------------------------------------------------
    @property
    def is_linux(self) -> bool:
        return self.os_name == "linux"

    @property
    def is_windows(self) -> bool:
        return self.os_name == "windows"

    @property
    def arch_supported(self) -> bool:
        return C.cdm_platform(self.os_name, self.arch) is not None

    @property
    def cdm(self) -> dict:
        """Layout of the Widevine CDM for this platform."""
        return C.cdm_platform(self.os_name, self.arch) or {}

    @property
    def lib_name(self) -> str:
        return self.cdm.get("lib", "")

    @property
    def platform_dir(self) -> str:
        return self.cdm.get("dir", "")

    @property
    def config_home(self) -> Path:
        """Directory that holds browser profiles."""
        if self.is_windows:
            local = os.environ.get("LOCALAPPDATA")
            return Path(local) if local else self.home / "AppData" / "Local"
        xdg = os.environ.get("XDG_CONFIG_HOME")
        return Path(xdg) if xdg else self.home / ".config"

    @property
    def arch_label(self) -> str:
        return C.ARCHES.get(self.arch, {}).get("label", self.machine or "unknown")

    @property
    def env_separator(self) -> str:
        """Separator used in environment variables such as MOZ_GMP_PATH."""
        return ";" if self.is_windows else ":"


def _read_os_release(path: Path = Path("/etc/os-release")) -> dict[str, str]:
    data: dict[str, str] = {}
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return data
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
            value = value[1:-1]
        data[key.strip()] = value
    return data


def _glibc_version() -> str:
    if not sys.platform.startswith("linux"):
        return ""
    try:
        libc = ctypes.CDLL("libc.so.6", use_errno=True)
        libc.gnu_get_libc_version.restype = ctypes.c_char_p
        version = libc.gnu_get_libc_version()
        return version.decode("ascii") if version else ""
    except (OSError, AttributeError):
        return ""


def _detect_family(os_id: str, os_like: tuple[str, ...]) -> str:
    candidates = (os_id,) + os_like
    for family, ids in _FAMILIES.items():
        for candidate in candidates:
            if candidate in ids:
                return family
    return "unknown"


def _detect_package_manager() -> str:
    for manager in _PACKAGE_MANAGERS:
        if shutil.which(manager):
            return manager
    return ""


def _is_admin() -> bool:
    if os.name == "nt":
        try:
            return bool(ctypes.windll.shell32.IsUserAnAdmin())  # type: ignore[attr-defined]
        except (AttributeError, OSError):
            return False
    return os.geteuid() == 0


def detect() -> SystemInfo:
    """Inspect the running system and return a :class:`SystemInfo`."""
    info = SystemInfo()

    if sys.platform.startswith("linux"):
        info.os_name = "linux"
    elif os.name == "nt":
        info.os_name = "windows"
    else:
        # macOS and anything else is not supported; the caller reports this.
        info.os_name = sys.platform

    if info.is_linux:
        release = _read_os_release()
        info.os_id = release.get("ID", "unknown")
        info.os_like = tuple(release.get("ID_LIKE", "").split())
        info.os_name_pretty = release.get("NAME", "Linux")
        info.os_version = release.get("VERSION_ID", "")
        info.pretty_name = release.get("PRETTY_NAME", info.os_name_pretty)
        info.glibc = _glibc_version()
        info.package_manager = _detect_package_manager()
        info.family = _detect_family(info.os_id, info.os_like)
        info.flatpak = shutil.which("flatpak") is not None
        info.snap = shutil.which("snap") is not None
    elif info.is_windows:
        release = platform.win32_ver()
        info.os_id = "windows"
        info.os_version = release[1] or ""
        info.os_name_pretty = "Windows"
        info.pretty_name = f"Windows {release[0]} {release[1]}".strip()
        info.family = "windows"

    info.machine = platform.machine()
    info.arch = C.normalize_arch(info.machine)
    info.kernel = platform.release()

    info.session_type = os.environ.get("XDG_SESSION_TYPE", "")
    info.desktop = os.environ.get("XDG_CURRENT_DESKTOP", "")

    info.is_admin = _is_admin()
    info.has_sudo = shutil.which("sudo") is not None

    # When running through sudo, report the invoking user rather than root.
    sudo_user = os.environ.get("SUDO_USER")
    if sudo_user and sudo_user != "root":
        info.user = sudo_user
        home = _home_for_user(sudo_user)
        if home is not None:
            info.home = home
    else:
        try:
            info.user = getpass.getuser()
        except (OSError, KeyError, AttributeError):
            info.user = os.environ.get("USER", "")
        info.home = Path.home()

    return info


def _home_for_user(name: str) -> Path | None:
    try:
        import pwd

        return Path(pwd.getpwnam(name).pw_dir)
    except (ImportError, KeyError):
        return None


def invoking_user() -> tuple[str, int, int, Path] | None:
    """Return (name, uid, gid, home) of the user that invoked sudo, if any."""
    name = os.environ.get("SUDO_USER")
    if not name or name == "root":
        return None
    try:
        import pwd

        entry = pwd.getpwnam(name)
        return name, entry.pw_uid, entry.pw_gid, Path(entry.pw_dir)
    except (ImportError, KeyError, AttributeError):
        return None


def report(info: SystemInfo) -> list[tuple[str, str]]:
    """Return the system information as a list of (label, value) rows."""
    if info.is_linux:
        system_row = ("Distribution", info.pretty_name)
        family = info.family + (
            f" [{info.package_manager}]" if info.package_manager else ""
        )
    else:
        system_row = ("Operating system", info.pretty_name)
        family = info.family

    rows: list[tuple[str, str]] = [
        system_row,
        ("Family", family),
        ("Architecture", f"{info.machine} -> {info.arch_label}"),
    ]
    if info.is_linux:
        rows.append(("Kernel", info.kernel))
        rows.append(("glibc", info.glibc or "unknown"))
    else:
        rows.append(("Kernel", info.kernel))
    if info.is_linux:
        rows.append(
            (
                "Session",
                " ".join(p for p in (info.session_type, info.desktop) if p) or "unknown",
            )
        )
    admin = "yes" if info.is_admin else "no"
    sudo = "yes" if info.has_sudo else "no"
    rows.append(("User", f"{info.user} (admin: {admin}, sudo: {sudo})"))
    if info.is_linux:
        rows.append(("Flatpak", "available" if info.flatpak else "not available"))
        rows.append(("Snap", "available" if info.snap else "not available"))
    if info.arch_supported:
        source = "Chrome package" if info.is_linux and info.arch == "amd64" else "Google CRX package"
        if info.is_linux and info.arch == "arm64":
            source = "Chrome package"
        rows.append(
            (
                "Widevine source",
                f"{info.platform_dir} / {info.lib_name} ({source})",
            )
        )
    return rows

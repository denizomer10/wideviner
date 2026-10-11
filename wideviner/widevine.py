# SPDX-License-Identifier: MIT
"""Obtaining the Widevine CDM and installing it into Chromium based browsers.

Two official sources are used, depending on the platform:

* **Chrome package (Linux)** - Google's stable Chrome ``.deb`` from
  dl.google.com always contains the current CDM. The CDM sits near the start
  of the archive, so a partial HTTP range download is usually enough.
* **Google CRX package (Windows, Linux fallback)** - Mozilla publishes a
  manifest that lists the official Google download URLs and SHA-512 hashes of
  the current Widevine CRX for every platform. Chrome and Firefox both consume
  exactly these packages.

Chromium looks for the CDM in ``<browser directory>/WidevineCdm``. On Linux
(and ChromeOS only) it additionally reads a hint file from the profile that
points at a component-updated CDM.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

from . import constants as C
from . import crx, deb
from .chromium import Browser
from .sysinfo import SystemInfo
from .ui import UI


class WidevineError(Exception):
    """Raised when the Widevine CDM cannot be obtained or installed."""


@dataclass
class CdmPayload:
    """The Widevine CDM files extracted from an official package."""

    version: str
    files: dict[str, bytes]
    source: str = "full"
    manifest: dict = field(default_factory=dict)


@dataclass
class InstallResult:
    ok: bool
    version: str = ""
    path: Path | None = None
    mode: str = ""  # "system" or "user"
    hint_file: Path | None = None
    message: str = ""


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
def cache_root() -> Path:
    """Return the platform's cache directory."""
    if os.name == "nt":
        base = os.environ.get("LOCALAPPDATA") or os.environ.get("TEMP")
        if base:
            return Path(base) / C.APP_NAME
        return Path.home() / f".{C.APP_NAME}"
    base = os.environ.get("XDG_CACHE_HOME") or str(Path.home() / ".cache")
    return Path(base) / C.APP_NAME


def default_cache_dir() -> Path:
    return cache_root()


def deb_url(arch: str) -> str:
    """Return the official Chrome .deb URL for *arch*."""
    return C.CHROME_DEB_URL.format(deb_arch=C.ARCHES[arch]["deb_arch"])


def version_key(version: str) -> tuple[int, ...]:
    """Turn a Widevine version string into a comparable tuple."""
    parts: list[int] = []
    for chunk in re.split(r"[.\-+]", version or ""):
        if chunk.isdigit():
            parts.append(int(chunk))
        else:
            break
    return tuple(parts)


def build_supports_widevine(binary: Path, chunk_size: int = 8 * 1024 * 1024) -> bool | None:
    """Check whether a browser binary was built with Widevine support.

    Returns True/False, or None when the binary could not be read.
    """
    needle = b"com.widevine.alpha"
    try:
        with open(binary, "rb") as handle:
            tail = b""
            while True:
                chunk = handle.read(chunk_size)
                if not chunk:
                    return False
                data = tail + chunk
                if needle in data:
                    return True
                tail = data[-len(needle) :]
    except OSError:
        return None


def _open(url: str, headers: dict | None = None, method: str = "GET", timeout: int = 60):
    request = urllib.request.Request(url, headers=headers or {}, method=method)
    request.add_header("User-Agent", f"{C.APP_NAME}/{C.APP_VERSION}")
    return urllib.request.urlopen(request, timeout=timeout)


def _read_response(response, ui: UI, prefix: str = "    ") -> bytes:
    total = int(response.headers.get("Content-Length") or 0)
    chunks: list[bytes] = []
    done = 0
    while True:
        chunk = response.read(256 * 1024)
        if not chunk:
            break
        chunks.append(chunk)
        done += len(chunk)
        ui.progress(done, total, prefix=prefix)
    ui.progress_done()
    return b"".join(chunks)


def _download(url: str, ui: UI) -> bytes:
    with _open(url) as response:
        return _read_response(response, ui)


def _fetch_range(url: str, length: int, ui: UI) -> tuple[bytes, bool]:
    """Fetch the first *length* bytes of *url*.

    Returns ``(data, partial)``; ``partial`` is True when the server honoured
    the range request.
    """
    headers = {"Range": f"bytes=0-{length - 1}"}
    with _open(url, headers=headers) as response:
        status = getattr(response, "status", 200)
        data = _read_response(response, ui)
    return data, status == 206


def _payload_from_files(files: dict[str, bytes], source: str) -> CdmPayload:
    manifest: dict = {}
    raw = files.get(C.CDM_MANIFEST_NAME)
    if raw:
        try:
            parsed = json.loads(raw.decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            parsed = {}
        if isinstance(parsed, dict):
            manifest = parsed
    version = manifest.get("version")
    return CdmPayload(
        version=version if isinstance(version, str) else "",
        files=files,
        source=source,
        manifest=manifest,
    )


# ---------------------------------------------------------------------------
# Source: Chrome .deb (Linux)
# ---------------------------------------------------------------------------
def _deb_cache_path(cache_dir: Path, arch: str) -> Path:
    return cache_dir / f"google-chrome-stable_current_{C.ARCHES[arch]['deb_arch']}.deb"


def _remote_info(url: str) -> dict:
    with _open(url, method="HEAD") as response:
        return {
            "size": int(response.headers.get("Content-Length") or 0),
            "etag": response.headers.get("ETag") or "",
            "last_modified": response.headers.get("Last-Modified") or "",
            "accept_ranges": (response.headers.get("Accept-Ranges") or "").lower()
            == "bytes",
        }


def _get_deb(url: str, cache_path: Path, ui: UI, *, refresh: bool = False) -> Path:
    """Return a local copy of the Chrome package, downloading it if needed."""
    meta_path = cache_path.with_name(cache_path.name + ".json")
    try:
        info = _remote_info(url)
    except (urllib.error.URLError, OSError) as exc:
        raise WidevineError(f"cannot reach {url}: {exc}") from exc

    if not refresh and cache_path.is_file() and meta_path.is_file():
        try:
            meta = json.loads(meta_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            meta = {}
        if (
            meta.get("etag")
            and meta.get("etag") == info.get("etag")
            and cache_path.stat().st_size == info.get("size")
        ):
            ui.info(f"Using the cached package: {cache_path}")
            return cache_path

    cache_path.parent.mkdir(parents=True, exist_ok=True)
    data = _download(url, ui)
    tmp = cache_path.with_name(cache_path.name + ".part")
    tmp.write_bytes(data)
    tmp.replace(cache_path)
    try:
        meta_path.write_text(json.dumps(info), encoding="utf-8")
    except OSError:
        pass
    return cache_path


def _fetch_from_deb(
    si: SystemInfo,
    platform_dir: str,
    library: str,
    cache_dir: Path,
    ui: UI,
    *,
    refresh: bool,
    allow_range: bool,
) -> CdmPayload:
    url = deb_url(si.arch)
    cache_path = _deb_cache_path(cache_dir, si.arch)

    # The CDM sits near the start of the package, so a partial download is
    # usually enough. This avoids transferring ~140 MB on every run.
    if allow_range and not refresh:
        ui.info("Downloading the Widevine CDM from Google (partial package) ...")
        try:
            data, partial = _fetch_range(url, C.RANGE_PROBE_BYTES, ui)
        except (urllib.error.URLError, OSError) as exc:
            raise WidevineError(f"download failed: {exc}") from exc

        if partial:
            files = deb.collect_cdm(data)
            if deb.cdm_is_complete(files, platform_dir, library):
                return _payload_from_files(files, "range")
            ui.warn(
                "The partial download did not contain the complete CDM; "
                "fetching the full package instead."
            )
        else:
            # The server ignored the range request and sent the whole file.
            files = deb.collect_cdm(data)
            if deb.cdm_is_complete(files, platform_dir, library):
                cache_dir.mkdir(parents=True, exist_ok=True)
                cache_path.write_bytes(data)
                return _payload_from_files(files, "full")

    ui.info("Downloading the official Chrome package from Google ...")
    deb_path = _get_deb(url, cache_path, ui, refresh=refresh)
    files = deb.collect_cdm(deb_path.read_bytes())
    if not deb.cdm_is_complete(files, platform_dir, library):
        raise WidevineError("Widevine CDM not found in the downloaded package")
    return _payload_from_files(files, "full")


# ---------------------------------------------------------------------------
# Source: Google CRX via Mozilla's GMP manifest (Windows, Linux fallback)
# ---------------------------------------------------------------------------
def _load_mozilla_manifest(cache_dir: Path, ui: UI, refresh: bool) -> dict:
    cache_path = cache_dir / "mozilla-widevinecdm.json"
    if not refresh and cache_path.is_file():
        try:
            return json.loads(cache_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            pass

    for url in (C.MOZILLA_GMP_MANIFEST, C.MOZILLA_GMP_MANIFEST_FALLBACK):
        try:
            data = _download(url, ui)
            manifest = json.loads(data.decode("utf-8"))
        except (urllib.error.URLError, OSError, ValueError):
            continue
        try:
            cache_dir.mkdir(parents=True, exist_ok=True)
            cache_path.write_text(json.dumps(manifest), encoding="utf-8")
        except OSError:
            pass
        return manifest

    # Offline: use the last known manifest if there is one.
    if cache_path.is_file():
        try:
            return json.loads(cache_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            pass
    raise WidevineError("cannot fetch Mozilla's Widevine manifest")


def _resolve_platform_entry(platforms: dict, key: str) -> dict | None:
    entry = platforms.get(key)
    hops = 0
    while isinstance(entry, dict) and "alias" in entry and hops < 5:
        entry = platforms.get(entry["alias"])
        hops += 1
    return entry if isinstance(entry, dict) else None


def _fetch_from_mozilla(
    mozilla_key: str | None,
    platform_dir: str,
    library: str,
    cache_dir: Path,
    ui: UI,
    *,
    refresh: bool,
) -> CdmPayload:
    if not mozilla_key:
        raise WidevineError("no CRX package is published for this platform")

    ui.info("Fetching Mozilla's Widevine manifest ...")
    manifest = _load_mozilla_manifest(cache_dir, ui, refresh)
    vendor = (manifest.get("vendors") or {}).get(C.MOZILLA_GMP_VENDOR)
    if not isinstance(vendor, dict):
        raise WidevineError("Widevine is not listed in Mozilla's GMP manifest")

    entry = _resolve_platform_entry(vendor.get("platforms") or {}, mozilla_key)
    if not entry:
        raise WidevineError(f"no Widevine package for {mozilla_key}")

    url = entry.get("fileUrl")
    if not url and entry.get("mirrorUrls"):
        url = entry["mirrorUrls"][0]
    if not url:
        raise WidevineError("the manifest entry has no download URL")

    version = str(vendor.get("version") or "")
    expected_hash = str(entry.get("hashValue") or "").lower()
    expected_size = int(entry.get("filesize") or 0)

    cache_path = cache_dir / (
        f"widevine-{version}-{platform_dir}.crx3" if version else
        f"widevine-{platform_dir}.crx3"
    )

    data: bytes | None = None
    if not refresh and cache_path.is_file():
        cached = cache_path.read_bytes()
        hash_ok = not expected_hash or hashlib.sha512(cached).hexdigest() == expected_hash
        size_ok = not expected_size or len(cached) == expected_size
        if hash_ok and size_ok:
            ui.info(f"Using the cached package: {cache_path}")
            data = cached

    if data is None:
        ui.info("Downloading the Widevine CDM from Google ...")
        try:
            data = _download(url, ui)
        except (urllib.error.URLError, OSError) as exc:
            raise WidevineError(f"download failed: {exc}") from exc

        if expected_size and len(data) != expected_size:
            raise WidevineError(
                f"size mismatch: expected {expected_size} bytes, got {len(data)}"
            )
        if expected_hash and hashlib.sha512(data).hexdigest() != expected_hash:
            raise WidevineError("SHA-512 mismatch, refusing the downloaded package")

        try:
            cache_dir.mkdir(parents=True, exist_ok=True)
            tmp = cache_path.with_name(cache_path.name + ".part")
            tmp.write_bytes(data)
            tmp.replace(cache_path)
        except OSError:
            pass

    try:
        files = crx.collect_cdm(data, platform_dir, library)
    except crx.CrxError as exc:
        raise WidevineError(str(exc)) from exc
    if not crx.cdm_is_complete(files, platform_dir, library):
        raise WidevineError("Widevine CDM not found in the downloaded package")
    return _payload_from_files(files, "crx")


# ---------------------------------------------------------------------------
# Fetching
# ---------------------------------------------------------------------------
def sources_for(si: SystemInfo) -> list[str]:
    """Return the sources to try, in order, for this platform."""
    sources: list[str] = []
    if si.is_linux and si.arch in ("amd64", "arm64"):
        sources.append("deb")
    if si.cdm.get("mozilla_key"):
        sources.append("mozilla")
    return sources


def fetch_cdm(
    si: SystemInfo,
    ui: UI,
    cache_dir: Path | None = None,
    *,
    refresh: bool = False,
    allow_range: bool = True,
    sources: list[str] | None = None,
) -> CdmPayload:
    """Obtain the Widevine CDM from an official source."""
    if not si.arch_supported:
        raise WidevineError(
            f"unsupported platform: {si.os_name or 'unknown'} / "
            f"{si.machine or 'unknown'}"
        )

    cdm = si.cdm
    platform_dir = cdm["dir"]
    library = cdm["lib"]
    cache_dir = cache_dir or default_cache_dir()

    available = sources_for(si)
    if sources is None:
        selected = available
    else:
        selected = [s for s in sources if s in available]
        if not selected:
            raise WidevineError(
                "the requested source is not available on this platform "
                f"(available: {', '.join(available) or 'none'})"
            )

    if not selected:
        raise WidevineError("no Widevine source is available for this platform")

    errors: list[str] = []
    for index, source in enumerate(selected):
        try:
            if source == "deb":
                return _fetch_from_deb(
                    si,
                    platform_dir,
                    library,
                    cache_dir,
                    ui,
                    refresh=refresh,
                    allow_range=allow_range,
                )
            return _fetch_from_mozilla(
                cdm.get("mozilla_key"),
                platform_dir,
                library,
                cache_dir,
                ui,
                refresh=refresh,
            )
        except WidevineError as exc:
            errors.append(f"{source}: {exc}")
            if index + 1 < len(selected):
                ui.warn(f"The {source} source failed, trying another one.")

    raise WidevineError("; ".join(errors))


# ---------------------------------------------------------------------------
# Installing
# ---------------------------------------------------------------------------
def _chown(path: Path, owner: tuple[int, int]) -> None:
    try:
        os.chown(path, owner[0], owner[1])
    except (OSError, AttributeError, NotImplementedError):
        pass


def _mkdir(path: Path, owner: tuple[int, int] | None = None) -> None:
    """Create *path* (and its parents) and hand new directories to *owner*.

    When wideviner runs through sudo, directories created for the invoking user
    would otherwise be owned by root, which breaks the browser profile - and,
    for Flatpak, the application sandbox.

    A dangling symlink or a plain file with the same name - distribution
    packages ship such placeholders - is replaced by a real directory, because
    ``mkdir(exist_ok=True)`` only tolerates an existing *directory*.
    """
    missing: list[Path] = []
    probe = path
    while not probe.exists():
        missing.append(probe)
        parent = probe.parent
        if parent == probe:
            break
        probe = parent

    if not path.is_dir():
        if path.is_symlink() or path.exists():
            path.unlink()
        path.mkdir(parents=True, exist_ok=True)
        if path not in missing:
            missing.append(path)

    if owner is not None:
        for directory in reversed(missing):
            _chown(directory, owner)


def _write_files(
    files: dict[str, bytes], target: Path, owner: tuple[int, int] | None = None
) -> None:
    _mkdir(target, owner)
    try:
        os.chmod(target, 0o755)
    except OSError:
        pass
    for relative, content in files.items():
        destination = target / relative
        _mkdir(destination.parent, owner)
        destination.write_bytes(content)
        try:
            os.chmod(destination, 0o644)
        except OSError:
            pass


def write_hint_file(
    user_data_dir: Path,
    cdm_dir: Path,
    version: str,
    owner: tuple[int, int] | None = None,
) -> Path:
    """Write the Chromium CDM hint file pointing at *cdm_dir* (Linux only)."""
    hint_dir = user_data_dir / C.CDM_DIR_NAME
    _mkdir(hint_dir, owner)
    hint_path = hint_dir / C.HINT_FILE_NAME
    payload = {
        "Path": os.path.abspath(str(cdm_dir)),
        "LastBundledVersion": version,
    }
    hint_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    if owner is not None:
        _chown(hint_path, owner)
    return hint_path


def _chown_tree(path: Path, owner: tuple[int, int]) -> None:
    _chown(path, owner)
    for dirpath, dirnames, filenames in os.walk(path):
        for name in dirnames + filenames:
            _chown(Path(dirpath) / name, owner)


def install(
    browser: Browser,
    payload: CdmPayload,
    si: SystemInfo,
    ui: UI,
    *,
    dry_run: bool = False,
    user_only: bool = False,
    owner: tuple[int, int] | None = None,
) -> InstallResult:
    """Install the CDM for *browser*.

    The CDM goes into ``<browser directory>/WidevineCdm`` when that directory
    can be written, and into the profile otherwise. Flatpak and Snap apps are
    always installed into the profile: their deployment is an immutable image
    and writing into it corrupts the application. On Linux the hint file in the
    profile is written as well, because Chromium builds with the component
    updater look for it there. That mechanism does not exist on Windows, so no
    hint file is written there.
    """
    version = payload.version or "unknown"
    can_write_system = (si.is_admin or browser.writable) and not browser.sandboxed

    if not user_only and can_write_system:
        target = browser.bundled_cdm_dir
        mode = "system"
    else:
        target = browser.user_cdm_dir(version)
        mode = "user"
        if target is None:
            return InstallResult(
                ok=False, message="no writable installation location found"
            )

    if dry_run:
        return InstallResult(
            ok=True, version=version, path=target, mode=mode, message="dry run"
        )

    # Only a profile installation belongs to the invoking user; a system wide
    # CDM must stay owned by root.
    try:
        _write_files(payload.files, target, owner if mode == "user" else None)
    except OSError as exc:
        return InstallResult(ok=False, message=f"cannot write to {target}: {exc}")

    hint_file: Path | None = None
    if si.is_linux and browser.user_data_dir is not None:
        try:
            hint_file = write_hint_file(
                browser.user_data_dir, target, version, owner
            )
        except OSError as exc:
            ui.warn(f"could not write the CDM hint file: {exc}")

    if owner is not None and browser.user_data_dir is not None:
        _chown_tree(browser.user_data_dir / C.CDM_DIR_NAME, owner)

    return InstallResult(
        ok=True,
        version=version,
        path=target,
        mode=mode,
        hint_file=hint_file,
    )

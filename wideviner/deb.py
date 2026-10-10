# SPDX-License-Identifier: MIT
"""Reading Debian packages (.deb) without external tools.

A .deb is an "ar" archive that contains:

    debian-binary     the format version ("2.0\\n")
    control.tar.*     package metadata
    data.tar.*        the files themselves

This module implements just enough of the ar and tar formats to pull the
Widevine CDM out of an official Google Chrome .deb. It works on any Linux
distribution (no dpkg, no ar, no third-party Python modules required) and it
tolerates truncated input, which allows extracting the CDM from a partial
HTTP download.
"""

from __future__ import annotations

import bz2
import io
import lzma
import shutil
import subprocess
import tarfile
import zlib
from dataclasses import dataclass
from pathlib import Path

from .constants import CDM_MANIFEST_NAME, DEB_CDM_PATH

AR_MAGIC = b"!<arch>\n"
AR_HEADER_LEN = 60


class DebError(Exception):
    """Raised when a .deb file cannot be parsed."""


@dataclass(frozen=True)
class ArEntry:
    name: str
    offset: int
    size: int


def read_ar_entries(buf: bytes) -> list[ArEntry]:
    """Parse the ar directory from *buf*, which may be truncated."""
    if not buf.startswith(AR_MAGIC):
        raise DebError("not an ar archive (bad magic)")
    entries: list[ArEntry] = []
    pos = len(AR_MAGIC)
    while pos + AR_HEADER_LEN <= len(buf):
        header = buf[pos : pos + AR_HEADER_LEN]
        if header[58:60] != b"`\n":
            break
        raw_name = header[0:16].decode("ascii", "replace").strip()
        name = raw_name.rstrip("/")
        try:
            size = int(header[48:58].decode("ascii").strip())
        except ValueError:
            break
        offset = pos + AR_HEADER_LEN
        entries.append(ArEntry(name=name, offset=offset, size=size))
        pos = offset + size + (size % 2)
    return entries


def find_data_tar(entries: list[ArEntry]) -> ArEntry | None:
    """Return the data.tar.* entry of an ar directory."""
    for entry in entries:
        if entry.name.startswith("data.tar"):
            return entry
    return None


def _decompress_zstd(blob: bytes) -> bytes:
    try:
        import zstandard  # type: ignore
    except ImportError:
        pass
    else:
        try:
            return zstandard.ZstdDecompressor().decompressobj().decompress(blob)
        except (zstandard.ZstdError, OSError, ValueError):
            return b""

    zstd = shutil.which("zstd")
    if zstd:
        proc = subprocess.run(
            [zstd, "-d", "-c", "-q"], input=blob, capture_output=True, check=False
        )
        if proc.returncode == 0:
            return proc.stdout
    raise DebError(
        "the package uses zstd compression but neither the 'zstandard' Python "
        "module nor the 'zstd' command is available"
    )


def _decompress(blob: bytes, name: str) -> bytes:
    """Decompress *blob* according to the suffix of *name*.

    Truncated input is tolerated: as much data as can be recovered is
    returned.
    """
    if name.endswith(".xz"):
        try:
            return lzma.LZMADecompressor().decompress(blob)
        except lzma.LZMAError:
            return b""
    if name.endswith(".gz"):
        decompressor = zlib.decompressobj(zlib.MAX_WBITS | 16)
        try:
            return decompressor.decompress(blob)
        except zlib.error:
            return b""
    if name.endswith(".bz2"):
        try:
            return bz2.BZ2Decompressor().decompress(blob)
        except (OSError, EOFError):
            return b""
    if name.endswith(".zst"):
        return _decompress_zstd(blob)
    return blob


def _collect_from_tar(tar_bytes: bytes, cdm_path: str) -> dict[str, bytes]:
    prefix = cdm_path + "/"
    files: dict[str, bytes] = {}
    try:
        with tarfile.open(fileobj=io.BytesIO(tar_bytes), mode="r:") as archive:
            for member in archive:
                name = member.name.removeprefix("./")
                if not name.startswith(prefix) or not member.isfile():
                    continue
                handle = archive.extractfile(member)
                if handle is None:
                    continue
                content = handle.read()
                if len(content) != member.size:
                    # The archive was truncated in the middle of this file.
                    return files
                files[name[len(prefix) :]] = content
    except (tarfile.TarError, EOFError, OSError):
        pass
    return files


def collect_cdm(buf: bytes, cdm_path: str = DEB_CDM_PATH) -> dict[str, bytes]:
    """Extract the Widevine CDM files from *buf*.

    *buf* may be a truncated .deb; whatever could be recovered is returned.
    The result maps paths relative to the CDM directory to file contents.
    """
    entries = read_ar_entries(buf)
    data_tar = find_data_tar(entries)
    if data_tar is None:
        return {}
    blob = buf[data_tar.offset : data_tar.offset + data_tar.size]
    tar_bytes = _decompress(blob, data_tar.name)
    if not tar_bytes:
        return {}
    return _collect_from_tar(tar_bytes, cdm_path)


def cdm_is_complete(files: dict[str, bytes], platform_dir: str, library: str) -> bool:
    """Check whether *files* contains a usable CDM for the platform."""
    return (
        CDM_MANIFEST_NAME in files
        and f"_platform_specific/{platform_dir}/{library}" in files
    )


def write_cdm_files(files: dict[str, bytes], dest: Path) -> Path:
    """Write the collected CDM files below *dest*."""
    for relative, content in files.items():
        target = dest / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)
        target.chmod(0o644)
    return dest


def extract_cdm_from_deb(
    deb_path: Path, dest: Path, platform_dir: str, library: str
) -> Path:
    """Extract the Widevine CDM from a complete .deb file."""
    files = collect_cdm(deb_path.read_bytes())
    if not cdm_is_complete(files, platform_dir, library):
        raise DebError("Widevine CDM not found in the package")
    return write_cdm_files(files, dest)

# SPDX-License-Identifier: MIT
"""Reading Chrome extension packages (CRX3).

Google ships the Widevine CDM as a CRX3 package. The format is simple:

    bytes 0-3     "Cr24"
    bytes 4-7     format version (3, little endian)
    bytes 8-11    length of the following header (little endian)
    header        protobuf with the signatures
    payload       a plain ZIP archive

The ZIP contains the same layout as the Chrome .deb:

    manifest.json
    LICENSE
    _platform_specific/<platform>/<library>
    _platform_specific/<platform>/<library>.sig   (Windows)
"""

from __future__ import annotations

import io
import struct
import zipfile

from .constants import CDM_LICENSE_NAME, CDM_MANIFEST_NAME

CRX_MAGIC = b"Cr24"


class CrxError(Exception):
    """Raised when a CRX package cannot be parsed."""


def is_crx(buf: bytes) -> bool:
    """Check whether *buf* starts with the CRX magic."""
    return buf.startswith(CRX_MAGIC)


def payload_offset(buf: bytes) -> int:
    """Return the offset of the ZIP payload inside a CRX package."""
    if not is_crx(buf):
        raise CrxError("not a CRX package (bad magic)")
    if len(buf) < 12:
        raise CrxError("truncated CRX header")
    version = struct.unpack("<I", buf[4:8])[0]
    if version != 3:
        raise CrxError(f"unsupported CRX version {version}")
    header_length = struct.unpack("<I", buf[8:12])[0]
    offset = 12 + header_length
    if offset >= len(buf):
        raise CrxError("truncated CRX header")
    return offset


def open_payload(buf: bytes) -> zipfile.ZipFile:
    """Open the ZIP payload of a CRX package."""
    offset = payload_offset(buf)
    try:
        return zipfile.ZipFile(io.BytesIO(buf[offset:]))
    except zipfile.BadZipFile as exc:
        raise CrxError(f"CRX payload is not a valid ZIP archive: {exc}") from exc


def collect_cdm(buf: bytes, platform_dir: str, library: str) -> dict[str, bytes]:
    """Extract the CDM files for *platform_dir* from a CRX package.

    The result uses the same relative layout as the Chrome .deb, so both
    sources can feed the same installation code.
    """
    prefix = f"_platform_specific/{platform_dir}/"
    wanted = {CDM_MANIFEST_NAME, CDM_LICENSE_NAME}
    files: dict[str, bytes] = {}

    with open_payload(buf) as archive:
        for name in archive.namelist():
            if name.endswith("/"):
                continue
            base = name.rsplit("/", 1)[-1]
            if name.startswith(prefix):
                if base == library or base == library + ".sig":
                    files[name] = archive.read(name)
            elif base in wanted:
                files[base] = archive.read(name)
    return files


def cdm_is_complete(files: dict[str, bytes], platform_dir: str, library: str) -> bool:
    """Check whether *files* contains a usable CDM for the platform."""
    return (
        CDM_MANIFEST_NAME in files
        and f"_platform_specific/{platform_dir}/{library}" in files
    )

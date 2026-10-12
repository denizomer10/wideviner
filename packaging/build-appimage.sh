#!/bin/sh
# SPDX-License-Identifier: MIT
#
# Build a wideviner AppImage.
#
#   packaging/build-appimage.sh <arch> <binary> <libdir> [output]
#
#     arch   : x86_64 or aarch64 (the AppImage architecture name)
#     binary : the compiled wideviner binary
#     libdir : directory that holds the shared libraries to bundle
#     output : optional output file name
#
# The script assembles an AppDir and feeds it to appimagetool. Set
# APPIMAGETOOL if the tool is not on PATH.

set -eu

ARCH="${1:?usage: build-appimage.sh <arch> <binary> <libdir> [output]}"
BINARY="${2:?usage: build-appimage.sh <arch> <binary> <libdir> [output]}"
LIBDIR="${3:?usage: build-appimage.sh <arch> <binary> <libdir> [output]}"

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n1)"
OUT="${4:-$ROOT/wideviner-$VERSION-$ARCH.AppImage}"
APPIMAGETOOL="${APPIMAGETOOL:-appimagetool}"

WORK="$(mktemp -d)"
APPDIR="$WORK/AppDir"
trap 'rm -rf "$WORK"' EXIT

# The libraries that are bundled. The GPU stack (Mesa, libdrm, libGL, ...) is
# deliberately left to the system, because it has to match the installed driver.
LIBS="
libbrotlicommon.so.1
libbrotlidec.so.1
libbz2.so.1.0
libexpat.so.1
libffi.so.8
libfontconfig.so.1
libfreetype.so.6
libpng16.so.16
libwayland-client.so.0
libwayland-cursor.so.0
libwayland-egl.so.1
libX11.so.6
libX11-xcb.so.1
libXau.so.6
libXdmcp.so.6
libxcb.so.1
libxcb-dri3.so.0
libxcb-present.so.0
libxcb-randr.so.0
libxcb-shm.so.0
libxcb-sync.so.1
libxcb-xfixes.so.0
libxkbcommon.so.0
libxshmfence.so.1
libz.so.1
"

echo "==> Assembling the AppDir for $ARCH"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps"

cp "$BINARY" "$APPDIR/usr/bin/wideviner"
chmod 755 "$APPDIR/usr/bin/wideviner"

for lib in $LIBS; do
    if [ -e "$LIBDIR/$lib" ]; then
        cp "$LIBDIR/$lib" "$APPDIR/usr/lib/$lib"
    else
        echo "    ! missing: $lib" >&2
    fi
done

cp "$ROOT/packaging/AppRun" "$APPDIR/AppRun"
chmod 755 "$APPDIR/AppRun"
cp "$ROOT/packaging/wideviner.desktop" "$APPDIR/wideviner.desktop"
cp "$ROOT/packaging/wideviner.desktop" "$APPDIR/usr/share/applications/"
cp "$ROOT/packaging/wideviner.png" "$APPDIR/wideviner.png"
cp "$ROOT/packaging/wideviner.png" \
   "$APPDIR/usr/share/icons/hicolor/256x256/apps/wideviner.png"

echo "==> Running appimagetool"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" "$APPIMAGETOOL" "$APPDIR" "$OUT"

echo "==> $OUT"
ls -l "$OUT"

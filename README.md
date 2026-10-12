# wideviner

**Widevine CDM installer for Chromium and Firefox on Linux — a native desktop app in Rust, with a Slint UI.**

`wideviner` finds the browsers installed on your system, obtains the Widevine
CDM from Google's official packages and installs it where each browser expects
it. This makes DRM protected content — Netflix, Spotify, Amazon Prime Video and
friends — playable.

The application has a small, modern interface with three actions:

```
┌──────────────────────────────────────────────────────────────┐
│  wideviner                                                    │
│                                                               │
│  SYSTEM                                                       │
│  Distribution   Debian GNU/Linux 13 (trixie)                  │
│  Architecture   x86_64 -> x86_64 (amd64)                      │
│  User           pc (admin: no, sudo: yes)                     │
│                                                               │
│  BROWSERS                                                     │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │ chromium-browser-unstable 155.0.8054.0     ● Widevine   │  │
│  │ Firefox 128.0                              ○ No Widev… │  │
│  └─────────────────────────────────────────────────────────┘  │
│                                                               │
│  [ Install ] [ Update ] [ Remove ]             [ Refresh ]    │
└──────────────────────────────────────────────────────────────┘
```

Pressing **Install**, **Update** or **Remove** opens a second window showing a
progress bar and a live log:

```
┌──────────────────────────────────────────────┐
│  Installing Widevine                         │
│  chromium-browser-unstable 155.0.8054.0      │
│                                              │
│  62%                                         │
│  ███████████████░░░░░░░░░░░                  │
│                                              │
│  LOG                                         │
│  > Installing Widevine                       │
│  > Downloading the Widevine CDM …            │
│  > 14.9 MiB / 24.0 MiB                       │
└──────────────────────────────────────────────┘
```

## Download

Ready-to-run AppImages are attached to every
[release](https://github.com/denizomer10/wideviner/releases):

| Architecture | File |
|---|---|
| x86_64 | `wideviner-<version>-x86_64.AppImage` |
| ARM64 | `wideviner-<version>-aarch64.AppImage` |

```bash
chmod +x wideviner-*.AppImage
./wideviner-*.AppImage
```

The bundle contains everything the application needs apart from the GPU stack
(Mesa, libEGL, libGL) and glibc, which always have to come from the system so
that they match the installed graphics driver.

## Requirements

Running the AppImage:

- **Linux** on x86_64 or ARM64, with a normal desktop (X11 or Wayland)
- **An internet connection** — the CDM (~24 MB) is downloaded from Google
- **Administrator rights** for a system wide installation; a user level
  installation needs none

Building from source additionally needs:

- **Rust 1.92 or newer** and Cargo (the Slint UI toolkit requires it)
- A C toolchain and the usual desktop development packages (fontconfig,
  xkbcommon, OpenGL). They are already present on most desktop systems; if the
  build complains, install them:

  ```bash
  # Debian / Ubuntu
  sudo apt install build-essential pkg-config libfontconfig-dev libfreetype-dev \
       libxkbcommon-dev libgl1-mesa-dev

  # Fedora
  sudo dnf install gcc-c++ pkgconf-pkg-config fontconfig-devel freetype-devel \
       libxkbcommon-devel mesa-libGL-devel
  ```

## Building from source

```bash
git clone https://github.com/denizomer10/wideviner.git
cd wideviner
cargo run --release
```

The binary ends up in `target/release/wideviner`.

### Building an AppImage

`packaging/build-appimage.sh` assembles an AppDir and feeds it to
[appimagetool](https://github.com/AppImage/appimagetool):

```bash
# get appimagetool once
curl -Lo appimagetool https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
chmod +x appimagetool

APPIMAGETOOL=./appimagetool \
  packaging/build-appimage.sh x86_64 target/release/wideviner /lib/x86_64-linux-gnu
```

The icon is generated with `python3 packaging/make-icon.py packaging/wideviner.png`.

### Cross-compiling for ARM64

The ARM64 binary can be built on an x86_64 host with the Debian cross toolchain:

```bash
sudo apt install gcc-aarch64-linux-gnu
sudo dpkg --add-architecture arm64 && sudo apt update
rustup target add aarch64-unknown-linux-gnu
```

Then download the ARM64 development libraries (fontconfig, freetype, expat,
zlib, libpng, brotli, bzip2) into a sysroot and build:

```bash
mkdir -p /tmp/arm64/debs && cd /tmp/arm64/debs
apt-get download libfontconfig1:arm64 libfontconfig-dev:arm64 \
    libfreetype6:arm64 libfreetype-dev:arm64 libexpat1:arm64 libexpat1-dev:arm64 \
    zlib1g:arm64 zlib1g-dev:arm64 libpng16-16t64:arm64 libpng-dev:arm64 \
    libbrotli1:arm64 libbrotli-dev:arm64 libbz2-1.0:arm64 libbz2-dev:arm64
mkdir -p /tmp/arm64/root
for d in *.deb; do dpkg-deb -x "$d" /tmp/arm64/root; done

cd -
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH=/tmp/arm64/root/usr/lib/aarch64-linux-gnu/pkgconfig:/tmp/arm64/root/usr/share/pkgconfig
export PKG_CONFIG_LIBDIR=$PKG_CONFIG_PATH
export PKG_CONFIG_SYSROOT_DIR=/tmp/arm64/root
export RUSTFLAGS="-L /tmp/arm64/root/usr/lib/aarch64-linux-gnu \
  -C link-arg=-Wl,-rpath-link,/tmp/arm64/root/usr/lib/aarch64-linux-gnu"

cargo build --release --target aarch64-unknown-linux-gnu
```

Finally bundle it with the ARM64 runtime libraries:

```bash
APPIMAGETOOL=./appimagetool packaging/build-appimage.sh aarch64 \
  target/aarch64-unknown-linux-gnu/release/wideviner \
  /tmp/arm64/root/usr/lib/aarch64-linux-gnu
```

## Usage

Run the application and press **Install**. The tool finds the browsers itself;
select the one you want in the list and use the buttons:

| Button | What it does |
|---|---|
| **Install** | Downloads the current Widevine CDM and installs it into the selected browser |
| **Update** | Same, but first checks whether the installed CDM is already current and skips the download if so |
| **Remove** | Deletes the CDM and the associated hint file / prefs from the selected browser |
| **Refresh** | Scans the system again and rebuilds the browser list |

**Restart the browser after installing** — the CDM is only picked up at startup.

### Where the CDM is installed

- **System wide** (when the browser directory is writable, or when running as
  root): right next to the browser binary, in `WidevineCdm/`. A hint file in the
  profile points Chromium at the CDM as well.
- **User level** (otherwise): inside the browser's profile, which needs no
  administrator rights. This is the usual case for distribution packages that
  live in a root-owned directory such as `/opt`.

## Supported browsers

**Only open source browsers are handled.**

| Engine | Browsers | How the CDM is loaded |
|---|---|---|
| **Chromium** | Chromium (`chromium`, `chromium-browser*`, `chromium-freeworld`, `ungoogled-chromium`) | `WidevineCdm/` next to the browser binary, plus a hint file in the profile |
| **Gecko** | Firefox, Firefox ESR / Nightly / Developer Edition, LibreWolf, Waterfox, IceCat, and other Firefox derivatives | Gecko Media Plugin in `gmp-widevinecdm/system-installed` |

Google Chrome, Microsoft Edge, Opera, Vivaldi and Brave are deliberately **not**
touched: they already ship Widevine, and copying a second, unsigned copy of a
proprietary DRM component into a third-party product is legally questionable.

Firefox downloads Widevine by itself as well, so the tool is mainly useful for
**Chromium** builds (which ship without it), for updating an outdated CDM, and
for systems where the browser's own download is blocked.

## Supported platforms

| Architecture | CDM source | Library |
|---|---|---|
| x86_64 (amd64) | Chrome `.deb` (Google CRX as fallback) | `libwidevinecdm.so` |
| ARM64 (aarch64) | Chrome `.deb` | `libwidevinecdm.so` |

The CDM always comes from Google's own packages — that is where the component is
published — but it is only ever installed into open source browsers.

## How it works

1. **System detection** — distribution, architecture, session, package manager
   and privileges are inspected; the architecture decides which package is
   downloaded.
2. **Browser discovery** — browsers are looked up on `$PATH`, through the
   package manager (`rpm` / `dpkg`), in the well-known directories, and in
   Flatpak and Snap applications. Candidates are verified to be real browser
   executables.
3. **CDM download** — the CDM is taken from Google's official packages: the
   Chrome `.deb` (only the first 24 MiB are fetched with an HTTP range request,
   which is enough to extract the CDM) or, as a fallback, the Google CRX package
   listed in Mozilla's GMP manifest, verified against its published SHA-512
   hash.
4. **Installation** — the CDM is written to `WidevineCdm/` next to the browser
   binary, or to the profile when a system wide installation is not possible. A
   hint file in the profile points Chromium at the CDM; for Firefox the CDM is
   installed as a Gecko Media Plugin together with the required prefs.

Only these files are ever taken from a package:

```
manifest.json
LICENSE
_platform_specific/<platform>/<library>
```

## Verifying the installation

**Chromium** — its own log states definitively whether the CDM was loaded:

```bash
chromium --headless=new --enable-logging=stderr --v=1 about:blank 2>&1 | grep -i widevine
```

Expected: `Registering hinted Widevine 4.10.3112.0` (Linux) or
`Registering bundled Widevine ...`. Without a CDM you get
`Widevine enabled but no library found`.

Headless mode uses its own profile directory (`<profile>-headless`) unless
`--user-data-dir` is given explicitly, so a **user level** installation is only
seen when the real profile is passed (`~/.config/chromium-clang` for the
official Chromium Linux builds):

```bash
chromium --headless=new --user-data-dir="$HOME/.config/chromium-clang" \
    --enable-logging=stderr --v=1 about:blank 2>&1 | grep -i widevine
```

**Firefox** — logs its GMP handling with:

```bash
MOZ_LOG=GMP:5 firefox --headless about:blank 2>&1 | grep -i widevine
```

Expected: `gmp-widevinecdm version=4.10.3112.0 tags=[...:com.widevine.alpha:...]`.

**Either browser** — a real DRM query is the strongest check. Load this page:

```html
<!doctype html><meta charset="utf-8"><body>pending
<script>
navigator.requestMediaKeySystemAccess('com.widevine.alpha', [{
  initDataTypes: ['cenc'],
  videoCapabilities: [{contentType: 'video/mp4; codecs="avc1.42E01E"'}]
}])
  .then(function (a) { document.body.textContent = 'WIDEVINE_OK ' + a.keySystem; })
  .catch(function (e) { document.body.textContent = 'WIDEVINE_FAIL ' + e.name; });
</script></body>
```

It prints `WIDEVINE_OK com.widevine.alpha` on success and
`WIDEVINE_FAIL NotSupportedError` when no CDM is available.

At least one audio or video capability is required: a configuration with only
`initDataTypes` is rejected with `NotSupportedError` even when the CDM is
installed correctly.

## Troubleshooting

- **`chrome://components` looks empty** — Close the browser completely and
  start it again; the CDM is only picked up at startup.
- **"This browser build does not appear to include Widevine support"** — The
  browser was not built with `enable_widevine=true`. The CDM is still
  installed, but the browser cannot load it; you need a Widevine enabled
  package.
- **Firefox does not see the CDM** — Check that `MOZ_GMP_PATH` contains the
  plugin directory (`echo $MOZ_GMP_PATH`). A new login is needed after the
  change.
- **Permission errors** — A system wide install needs administrator rights.
  Without them the CDM goes into the profile instead, which works just as well.
- **AppImage does not start** — It needs FUSE. On systems without it, run the
  AppImage with `--appimage-extract-and-run`.

## Status

Tested end to end on Linux x86_64 — Chromium logs
`Registering hinted Widevine ...` and a real `requestMediaKeySystemAccess()`
query returns `WIDEVINE_OK`. The ARM64 build is cross-compiled from the same
source.

## Legal notice

Widevine is a **proprietary** DRM component developed and licensed by Google.
This tool only extracts it from the official packages that Google distributes
and copies it to the location the browser expects; it does not modify the
component in any way. Use of Widevine is subject to Google's licence terms.
This tool is not affiliated with Google, Mozilla, the Chromium project or the
GNOME project.

## License

MIT — see [LICENSE](LICENSE).

The Widevine CDM itself is proprietary and is **not** covered by this licence;
see the legal notice above.

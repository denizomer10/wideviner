# wideviner

**Widevine CDM installer for Chromium and Firefox — a native desktop app in Rust, with a Slint UI.**

`wideviner` finds the browsers installed on your system, obtains the Widevine
CDM from Google's official packages and installs it where each browser expects
it. This makes DRM protected content — Netflix, Spotify, Amazon Prime Video and
friends — playable.

The application has a small, modern interface with three actions:

```
┌──────────────────────────────────────────────────────────────┐
│  wideviner                                   Widevine CDM …  │
│                                                              │
│  SYSTEM                                                      │
│  Distribution   Debian GNU/Linux 13 (trixie)                 │
│  Architecture   x86_64 -> x86_64 (amd64)                     │
│  User           pc (admin: no, sudo: yes)                    │
│                                                              │
│  BROWSERS                                                    │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ chromium-browser-unstable 155.0.8054.0      ● Widevine │  │
│  │ Firefox 128.0                               ○ No Wide… │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                              │
│  [ Install ] [ Update ] [ Remove ]            [ Refresh ]    │
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

## Requirements

- **Rust 1.92 or newer** and Cargo (the Slint UI toolkit requires it)
- **Linux** (x86_64, ARM64) or **Windows** (x86_64, ARM64, x86)
- **An internet connection** — the CDM (~24 MB) is downloaded from Google
- **Administrator rights** for a system wide installation; a user level
  installation needs none

On Linux the Slint windowing backend needs the usual desktop development
packages (fontconfig, xkbcommon and OpenGL). They are already present on most
desktop systems; if the build complains, install them:

```bash
# Debian / Ubuntu
sudo apt install build-essential pkg-config libfontconfig1-dev libxkbcommon-dev libgl1-mesa-dev

# Fedora
sudo dnf install gcc-c++ pkgconf-pkg-config fontconfig-devel libxkbcommon-devel mesa-libGL-devel
```

## Building and running

```bash
git clone https://github.com/denizomer10/wideviner.git
cd wideviner
cargo run --release
```

The binary ends up in `target/release/wideviner`.

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
  root): right next to the browser binary, in `WidevineCdm/`. On Linux a hint
  file in the profile points Chromium at the CDM as well.
- **User level** (otherwise): inside the browser's profile, which needs no
  administrator rights. This is the usual case for distribution packages that
  live in a root-owned directory such as `/opt`.

## Supported browsers

**Only open source browsers are handled.**

| Engine | Browsers | How the CDM is loaded |
|---|---|---|
| **Chromium** | Chromium (`chromium`, `chromium-browser*`, `chromium-freeworld`, `ungoogled-chromium`) | `WidevineCdm/` next to the browser binary, plus a hint file in the profile on Linux |
| **Gecko** | Firefox, Firefox ESR / Nightly / Developer Edition, LibreWolf, Waterfox, IceCat, and other Firefox derivatives | Gecko Media Plugin in `gmp-widevinecdm/system-installed` |

Google Chrome, Microsoft Edge, Opera, Vivaldi and Brave are deliberately **not**
touched: they already ship Widevine, and copying a second, unsigned copy of a
proprietary DRM component into a third-party product is legally questionable.

Firefox downloads Widevine by itself as well, so the tool is mainly useful for
**Chromium** builds (which ship without it), for updating an outdated CDM, and
for systems where the browser's own download is blocked.

## Supported platforms

| Platform | Architecture | CDM source | Library |
|---|---|---|---|
| Linux | x86_64, ARM64 | Chrome `.deb` (CRX as fallback) | `libwidevinecdm.so` |
| Windows | x86_64, ARM64, x86 | Google CRX | `widevinecdm.dll` |

The CDM always comes from Google's own packages — that is where the component is
published — but it is only ever installed into open source browsers.

## How it works

1. **System detection** — platform, architecture, session, package manager and
   privileges are inspected; the architecture decides which package is
   downloaded.
2. **Browser discovery** — browsers are looked up on `$PATH`, through the
   package manager (`rpm` / `dpkg`), in the well-known directories, and in
   Flatpak and Snap applications. Candidates are verified to be real browser
   executables.
3. **CDM download** — the CDM is taken from Google's official packages: the
   Chrome `.deb` on Linux (only the first 24 MiB are fetched with an HTTP range
   request, which is enough to extract the CDM) or the Google CRX package
   listed in Mozilla's GMP manifest, verified against its published SHA-512
   hash.
4. **Installation** — the CDM is written to `WidevineCdm/` next to the browser
   binary, or to the profile when a system wide installation is not possible.
   On Linux a hint file in the profile points Chromium at the CDM; for Firefox
   the CDM is installed as a Gecko Media Plugin together with the required
   prefs.

Only these files are ever taken from a package:

```
manifest.json
LICENSE
_platform_specific/<platform>/<library>
_platform_specific/<platform>/<library>.sig   (Windows)
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
  plugin directory (`echo $MOZ_GMP_PATH`, or `set MOZ_GMP_PATH` on Windows).
  A new login (Linux) or a new process (Windows) is needed after the change.
- **Permission errors** — A system wide install needs administrator rights.
  Without them the CDM goes into the profile instead, which works just as well.
- **ARM64** — the official Chrome Linux arm64 package is used on Linux (not a
  ChromeOS image); on Windows the Google CRX build for that architecture is
  used.

## Status

| Platform | State |
|---|---|
| Linux | Tested end to end — Chromium logs `Registering hinted Widevine ...` and a real `requestMediaKeySystemAccess()` query returns `WIDEVINE_OK` |
| Windows | Implemented, not tested |

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

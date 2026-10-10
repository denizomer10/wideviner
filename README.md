# wideviner

**Widevine CDM installer for Chromium and Firefox on Linux and Windows.**

`wideviner` finds the browsers installed on your system, obtains the Widevine
CDM from Google's official packages and installs it where each browser expects
it. This makes DRM protected content — Netflix, Spotify, Amazon Prime Video and
friends — playable.

```
  wideviner 0.2.0
  Widevine CDM installer for Chromium and Firefox
  -----------------
==> System information
    Distribution:                Debian GNU/Linux 13 (trixie)
    Architecture:                x86_64 -> x86_64 (amd64)
    ...
Main menu
  [1] Install Widevine
  [2] Update Widevine
  [3] System information
  [4] Exit
```

## Requirements

- **Python 3.9 or newer** — standard library only, no dependencies
- **Linux** (x86_64, ARM64) or **Windows** (x86_64, ARM64, x86)
- **An internet connection** — the CDM (~24 MB) is downloaded from Google
- **Administrator rights** for a system wide installation; a user level
  installation needs none

## Installation

There is nothing to build or install — clone the repository and run the script:

```bash
git clone https://github.com/denizomer10/wideviner.git
cd wideviner
python3 wideviner.py
```

## Usage

### Interactive

```bash
python3 wideviner.py
```

The menu offers to install or update Widevine, or to print system information.
The tool finds the browsers itself, asks which one to use when there are
several, and downloads the CDM from Google.

### Command line

```bash
python3 wideviner.py --install     # install without the menu
python3 wideviner.py --update      # update an existing installation
python3 wideviner.py --info        # only print system information
```

| Option | Description |
|---|---|
| `--install` / `--update` | Skip the menu and install / update directly |
| `--info` | Only print system information |
| `--target PATH` | Use an explicit browser binary or directory |
| `--cache-dir PATH` | Where to keep the downloaded package |
| `--source auto\|deb\|mozilla` | Which CDM source to use |
| `--deep` | Walk the usual roots for browsers |
| `--refresh` | Ignore the cached package and download again |
| `--user` | Install for the current user only |
| `--dry-run` | Show what would be done, write nothing |
| `--yes` | Answer yes to all questions |
| `--no-color`, `--quiet` | Disable colours / only print errors |

**Restart the browser after installing** — the CDM is only picked up at startup.

## Supported browsers

**Only open source browsers are handled.**

| Engine | Browsers | How the CDM is loaded |
|---|---|---|
| **Chromium** | Chromium (`chromium`, `chromium-browser*`, `chromium-freeworld`, `ungoogled-chromium`) | `WidevineCdm/` next to the browser binary, plus a hint file in the profile on Linux |
| **Gecko** | Firefox, Firefox ESR / Nightly / Developer Edition, LibreWolf, Waterfox, IceCat, and other Firefox derivatives | Gecko Media Plugin in `gmp-widevinecdm/system-installed` |

Google Chrome, Microsoft Edge, Opera, Vivaldi and Brave are deliberately **not**
touched: they already ship Widevine, and copying a second, unsigned copy of a
proprietary DRM component into a third-party product is legally questionable.
Pointing `--target` at one of them is rejected:

```
$ wideviner --install --target /opt/google/chrome
[!] no supported browser found.
```

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
seen when the real profile is passed — the one wideviner reports after
installing (`~/.config/chromium-clang` for the official Chromium Linux builds):

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
  Use `--user` for the user level installation instead.
- **ARM64** — the official Chrome Linux arm64 package is used on Linux (not a
  ChromeOS image); on Windows the Google CRX build for that architecture is
  used.

## Status

| Platform | State |
|---|---|
| Linux | Tested end to end — Chromium logs `Registering hinted Widevine ...` and a real `requestMediaKeySystemAccess()` query returns `WIDEVINE_OK` |
| Windows | Implemented, not tested |

If something does not work on Windows, `--dry-run` plus the `System
information` output is the most useful thing to report.

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

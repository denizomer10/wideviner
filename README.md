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
    Distribution:                Fedora Linux 45 (Budgie Prerelease)
    Architecture:                x86_64 -> x86_64 (amd64)
    ...
Main menu
  [1] Install Widevine
  [2] Update Widevine
  [3] System information
  [4] Exit
```

## Supported browsers

**Only open source browsers are handled.**

| Engine | Browsers | How the CDM is loaded |
|---|---|---|
| **Chromium** | Chromium (`chromium`, `chromium-browser*`, `chromium-freeworld`, `ungoogled-chromium`) | `WidevineCdm/` next to the browser binary, plus a hint file in the profile on Linux |
| **Gecko** | Firefox, Firefox ESR / Nightly / Developer Edition, LibreWolf, Waterfox, IceCat, and other Firefox derivatives | Gecko Media Plugin in `gmp-widevinecdm/system-installed` |

### Why commercial browsers are left alone

Google Chrome, Microsoft Edge, Opera, Vivaldi and Brave are deliberately **not**
touched:

- they already ship Widevine, usually through their own agreement with Google,
  so there is nothing to install;
- copying a second, unsigned copy of a proprietary DRM component into a
  third-party product is legally questionable and could break their support;
- most of them are distributed under a proprietary licence.

The tool refuses these explicitly — pointing `--target` at Google Chrome, for
example, is rejected instead of silently patching it:

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

Note that the CDM always comes from Google's own packages — that is where the
component is published — but it is only ever installed into open source
browsers.

## How it works

### 1. System detection

On startup the tool inspects the platform in detail: operating system and
version, kernel, architecture, session, package manager (Linux), Flatpak/Snap
availability and privileges. The architecture decides which package is
downloaded.

### 2. Browser discovery

Browsers are looked up through several channels:

- known browser names on `$PATH`
- Linux: package manager queries (`rpm -qa` / `dpkg-query`), well-known
  directories (`/usr/lib64/chromium-browser`, `/opt/chromium.org`,
  `/usr/lib64/firefox`, ...), Flatpak and Snap applications
- Windows: the usual install locations under `%PROGRAMFILES%`,
  `%PROGRAMFILES(X86)%` and `%LOCALAPPDATA%`, including Chrome's
  versioned `Application\<version>` directories
- `--deep` walks the usual installation roots

Candidates are verified to be real browser executables: files that
distribution packages drop in `/etc/cron.daily/` or `apparmor.d/` under the
same name are filtered out.

### 3. Widevine sources

Two official sources are used:

**Chrome package (Linux)**

```
https://dl.google.com/linux/direct/google-chrome-stable_current_<arch>.deb
```

Google's own download server, always serving the current stable release. The
CDM sits near the start of the archive, so the tool first requests only the
first 24 MiB with an HTTP range request and extracts the CDM from that —
usually ~24 MB instead of ~140 MB. If the probe is not enough it falls back to
the full package.

**Google CRX package (Windows, Linux fallback)**

Mozilla publishes, for every Firefox release, a manifest listing the official
Google download URLs and SHA-512 hashes of the current Widevine CRX for every
platform:

```
https://raw.githubusercontent.com/mozilla-firefox/firefox/refs/heads/main/toolkit/content/gmp-sources/widevinecdm.json
```

Chrome and Firefox both consume exactly these CRX packages, so this is the
official source outside Linux. The download is ~20–23 MB, verified against the
published SHA-512 hash, and the CRX header is parsed with pure Python (the
payload is a plain ZIP archive).

Only these files are ever taken from a package:

```
manifest.json
LICENSE
_platform_specific/<platform>/<library>
_platform_specific/<platform>/<library>.sig   (Windows)
```

### 4. Installation

#### Chromium based browsers

The CDM goes into `WidevineCdm` next to the browser binary:

```
Linux    /opt/chromium.org/chromium-unstable/WidevineCdm/
         /usr/lib64/chromium-browser/WidevineCdm/
Windows  C:\Program Files\Chromium\Application\<version>\WidevineCdm\
```

On **Linux only**, Chromium has a second mechanism: a hint file in the profile
points at a component-updated CDM, which is what distribution Chromium
packages rely on. `wideviner` writes it as well, so every build finds the CDM:

```
~/.config/chromium/WidevineCdm/latest-component-updated-widevine-cdm
```

```json
{
  "Path": "/opt/chromium.org/chromium-unstable/WidevineCdm",
  "LastBundledVersion": "4.10.3112.0"
}
```

This mechanism does not exist on Windows, so no hint file is written there.

#### Firefox (Gecko)

Firefox loads the CDM as a **Gecko Media Plugin (GMP)**. The layout is fixed by
the browser (`dom/media/gmp/GMPParent.cpp`):

```
<base>/gmp-widevinecdm/system-installed/
    manifest.json
    <library>
```

- the GMP **name** is the parent directory name without the `gmp-` prefix,
- the GMP **version** is the leaf directory name (`system-installed`),

which is why the prefs select exactly that version:

```js
pref("media.gmp-widevinecdm.version", "system-installed");
pref("media.gmp-widevinecdm.visible", true);
pref("media.gmp-widevinecdm.enabled", true);
pref("media.gmp-widevinecdm.autoupdate", false);
pref("media.eme.enabled", true);
```

Firefox discovers extra plugin directories through the `MOZ_GMP_PATH`
environment variable (separated by `:` on Unix and `;` on Windows):

| Platform | GMP location | Prefs | `MOZ_GMP_PATH` |
|---|---|---|---|
| Linux (system) | `/usr/lib64/mozilla/plugins/gmp-widevinecdm/system-installed/` | `<firefox>/defaults/pref/gmpwidevine.js` | `/etc/profile.d/widevine.sh`, environment.d, fish |
| Linux (user) | `<profile>/gmp-widevinecdm/system-installed/` | `<profile>/user.js` | `~/.config/environment.d/50-widevine.conf` |
| Windows (user) | `%APPDATA%\Mozilla\Firefox\Profiles\<p>\gmp-widevinecdm\system-installed\` | `<profile>\user.js` | `HKCU\Environment` (registry) |

#### Administrator rights

A system wide Chromium installation needs write access to the browser
directory. Without it the tool offers to re-run itself through `sudo`; if you
decline — or on Windows — it falls back to a user level installation. Firefox
always has a user level option (the profile), which needs no administrator
rights on any platform.

### How the Linux profile directory is found

Chromium does **not** have to use `~/.config/chromium` as its profile
directory. The directory name is `"chromium"` plus a build specific channel
suffix (`chrome_paths_linux.cc`):

```cpp
std::string data_dir_basename = "chromium";
*result = config_dir.Append(data_dir_basename + GetChannelSuffixForDataDir());
```

The official Chromium Linux packages set that suffix to `-clang`, so the real
directory is `~/.config/chromium-clang`. `wideviner` therefore locates the
profile directory like this:

1. It scans the configuration directory for existing profiles (looking for
   markers such as `Local State`, `Default` or `First Run`) and picks the most
   recently used one.
2. If there is no profile yet (the browser was never started), it starts the
   browser **once with a temporary home directory** and observes which
   directory it creates. Nothing is written to the real home directory.
3. Otherwise it falls back to the default name.

For Firefox the default profile is read from `profiles.ini`.

## Usage

```bash
# Interactive menu
python3 wideviner.py

# Without the menu
python3 wideviner.py --install
python3 wideviner.py --update
python3 wideviner.py --info

# Specific browser, cache directory and CDM source
python3 wideviner.py --install --target /usr/lib64/firefox
python3 wideviner.py --install --cache-dir ~/.cache/wideviner
python3 wideviner.py --install --source mozilla

# Dry run (writes nothing)
python3 wideviner.py --install --dry-run
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

There are no external dependencies; only the Python 3.9+ standard library is
used.

## Testing status

| Platform | State | How it was verified |
|---|---|---|
| **Linux** | **Tested end to end** | Chromium log shows `Registering hinted Widevine 4.10.3112.0`, and a real `requestMediaKeySystemAccess()` query returns `WIDEVINE_OK`. Firefox logs `gmp-widevinecdm ... tags=[...com.widevine.alpha...]` and the same query returns `WIDEVINE_OK`. |
| **Windows** | Implemented, **not tested** | Paths and the CRX source come from the browser sources, Google's packaging and Mozilla's manifest. |

If something does not work on Windows, `--dry-run` plus the `System
information` output is the most useful thing to report.

## Verifying an installation

**Chromium** — its own log states definitively whether the CDM was loaded:

```bash
chromium --headless=new --enable-logging=stderr --v=1 about:blank 2>&1 | grep -i widevine
```

Expected: `Registering hinted Widevine 4.10.3112.0` (Linux) or
`Registering bundled Widevine ...`. Without a CDM you get
`Widevine enabled but no library found`.

**Firefox** — logs its GMP handling with:

```bash
MOZ_LOG=GMP:5 firefox --headless about:blank 2>&1 | grep -i widevine
```

Expected: `gmp-widevinecdm version=4.10.3112.0 tags=[...:com.widevine.alpha:...]`.

**Either browser** — a real DRM query is the strongest check. Load this page:

```html
<!doctype html><meta charset="utf-8"><body>pending
<script>
navigator.requestMediaKeySystemAccess('com.widevine.alpha', [{initDataTypes: ['cenc']}])
  .then(function (a) { document.body.textContent = 'WIDEVINE_OK ' + a.keySystem; })
  .catch(function (e) { document.body.textContent = 'WIDEVINE_FAIL ' + e.name; });
</script></body>
```

It prints `WIDEVINE_OK com.widevine.alpha` on success and
`WIDEVINE_FAIL NotSupportedError` when no CDM is available.

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

## Technical references

The installation paths were verified against the browser sources:

**Chromium**

- `chrome/common/chrome_paths.cc` — `DIR_BUNDLED_WIDEVINE_CDM`,
  `DIR_COMPONENT_UPDATED_WIDEVINE_CDM`, `FILE_COMPONENT_WIDEVINE_CDM_HINT`
- `chrome/common/chrome_paths_linux.cc` — profile directory naming; the hint
  file mechanism is Linux/ChromeOS only
- `chrome/common/media/cdm_registration.cc` — bundled/hinted selection logic
- `chrome/common/media/component_widevine_cdm_hint_file_linux.cc` — hint file
- `media/cdm/library_cdm/cdm_paths.gni` — `_platform_specific/<os>_<arch>`
- `third_party/widevine/cdm/widevine.gni` — `enable_widevine`,
  `bundle_widevine_cdm`, `enable_widevine_cdm_component`

**Firefox**

- `dom/media/gmp/GMPParent.cpp` — GMP name/version from the directory layout,
  `.info` vs `manifest.json` handling, plugin type detection
- `dom/media/gmp/GMPServiceParent.cpp` — `MOZ_GMP_PATH` handling
- `toolkit/modules/GMPUtils.sys.mjs` — `media.gmp-<name>.version` prefs

**Packaging**

- Chrome `.deb` layout: `opt/google/chrome/WidevineCdm/`
- CRX3 format: `Cr24` magic, version, header length, then a ZIP payload
- Mozilla's GMP manifest: official Google URLs plus SHA-512 hashes per platform

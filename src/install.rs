//! Installing, updating and removing the Widevine CDM.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::chromium::Chromium;
use crate::constants as C;
use crate::firefox::Firefox;
use crate::sysinfo::SystemInfo;
use crate::widevine::{CdmPayload, Event, Reporter};

/// The outcome of an installation.
#[derive(Debug, Clone, Default)]
pub struct InstallResult {
    pub version: String,
    pub path: PathBuf,
    /// `"system"` or `"user"`.
    pub mode: String,
    pub hint_file: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------
fn write_files(files: &HashMap<String, Vec<u8>>, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target)
        .map_err(|err| format!("cannot create {}: {}", target.display(), err))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(target, fs::Permissions::from_mode(0o755));
    }
    for (relative, content) in files {
        let destination = target.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("cannot create {}: {}", parent.display(), err))?;
        }
        fs::write(&destination, content)
            .map_err(|err| format!("cannot write {}: {}", destination.display(), err))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&destination, fs::Permissions::from_mode(0o644));
        }
    }
    Ok(())
}

/// Give `path` back to the user that invoked sudo, if we are running as root.
fn restore_owner(path: &Path, reporter: Reporter) {
    let Some((_, uid, gid, _)) = crate::sysinfo::invoking_user() else {
        return;
    };
    let status = Command::new("chown")
        .args(["-R", &format!("{}:{}", uid, gid)])
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if let Err(err) = status {
        reporter(Event::Warn(format!(
            "could not change the owner of {}: {}",
            path.display(),
            err
        )));
    }
}

fn effective_version(payload: &CdmPayload) -> String {
    if payload.version.is_empty() {
        "unknown".to_string()
    } else {
        payload.version.clone()
    }
}

// ---------------------------------------------------------------------------
// Chromium
// ---------------------------------------------------------------------------
fn write_hint_file(user_data_dir: &Path, cdm_dir: &Path, version: &str) -> Result<PathBuf, String> {
    let hint_dir = user_data_dir.join(C::CDM_DIR_NAME);
    fs::create_dir_all(&hint_dir).map_err(|err| format!("cannot create {}: {}", hint_dir.display(), err))?;
    let hint_path = hint_dir.join(C::HINT_FILE_NAME);
    let payload = serde_json::json!({
        "Path": cdm_dir.to_string_lossy(),
        "LastBundledVersion": version,
    });
    let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
    fs::write(&hint_path, format!("{}\n", text))
        .map_err(|err| format!("cannot write {}: {}", hint_path.display(), err))?;
    Ok(hint_path)
}

/// Install the CDM into a Chromium based browser.
pub fn install_chromium(
    browser: &Chromium,
    payload: &CdmPayload,
    si: &SystemInfo,
    reporter: Reporter,
) -> Result<InstallResult, String> {
    let version = effective_version(payload);
    let can_write_system = si.is_admin || browser.writable;

    let (target, mode) = if can_write_system && !browser.sandboxed() {
        (browser.bundled_cdm_dir(), "system")
    } else {
        match browser.user_cdm_dir(&version) {
            Some(dir) => (dir, "user"),
            None => return Err("no writable installation location found".into()),
        }
    };

    reporter(Event::Info(format!("Installing to {}", target.display())));

    write_files(&payload.files, &target)?;

    let mut hint_file = None;
    if si.is_linux() {
        if let Some(user_data_dir) = &browser.user_data_dir {
            match write_hint_file(user_data_dir, &target, &version) {
                Ok(path) => hint_file = Some(path),
                Err(err) => reporter(Event::Warn(format!(
                    "could not write the CDM hint file: {}",
                    err
                ))),
            }
        }
    }

    if let Some(user_data_dir) = &browser.user_data_dir {
        let hint_dir = user_data_dir.join(C::CDM_DIR_NAME);
        if hint_dir.is_dir() {
            restore_owner(&hint_dir, reporter);
        }
    }

    Ok(InstallResult {
        version,
        path: target,
        mode: mode.to_string(),
        hint_file,
    })
}

/// Remove the Widevine CDM from a Chromium based browser.
pub fn remove_chromium(
    browser: &Chromium,
    si: &SystemInfo,
    reporter: Reporter,
) -> Result<Vec<PathBuf>, String> {
    let mut removed = Vec::new();

    // The CDM directory Chromium actually uses (bundled or hinted).
    if let Some(path) = &browser.widevine_path {
        if path.is_dir() {
            remove_dir(path, reporter)?;
            removed.push(path.clone());
        }
    }

    // The whole WidevineCdm directory in the profile.
    if let Some(user_data_dir) = &browser.user_data_dir {
        let cdm_root = user_data_dir.join(C::CDM_DIR_NAME);
        if cdm_root.is_dir() {
            remove_dir(&cdm_root, reporter)?;
            removed.push(cdm_root);
        }
    }

    // A system wide installation next to the binary.
    let bundled = browser.bundled_cdm_dir();
    if bundled.is_dir() && (si.is_admin || browser.writable) {
        remove_dir(&bundled, reporter)?;
        removed.push(bundled);
    }

    if removed.is_empty() {
        return Err("no Widevine CDM was found to remove".into());
    }
    Ok(removed)
}

fn remove_dir(path: &Path, reporter: Reporter) -> Result<(), String> {
    reporter(Event::Info(format!("Removing {}", path.display())));
    fs::remove_dir_all(path).map_err(|err| format!("cannot remove {}: {}", path.display(), err))
}

// ---------------------------------------------------------------------------
// Firefox
// ---------------------------------------------------------------------------
fn pref_content() -> String {
    let mut lines = vec!["// Widevine CDM installed by wideviner.".to_string()];
    for (name, value) in C::FIREFOX_PREFS {
        lines.push(format!("pref(\"{}\", {});", name, value));
    }
    lines.join("\n") + "\n"
}

fn shell_snippet(gmp_dir: &Path) -> String {
    let directory = gmp_dir.to_string_lossy();
    format!(
        "if [[ \":$MOZ_GMP_PATH:\" != *\":{dir}:\"* ]]; then\n    \
         MOZ_GMP_PATH=\"${{MOZ_GMP_PATH}}${{MOZ_GMP_PATH:+:}}{dir}\"\n    \
         export MOZ_GMP_PATH\nfi\n",
        dir = directory
    )
}

fn fish_snippet(gmp_dir: &Path) -> String {
    format!(
        "set -x --path MOZ_GMP_PATH $MOZ_GMP_PATH\nset dir {dir}\n\
         if not contains $dir $MOZ_GMP_PATH\n    set -p MOZ_GMP_PATH $dir\nend\nset -e dir\n",
        dir = gmp_dir.to_string_lossy()
    )
}

fn write_user_prefs(profile_dir: &Path) -> Result<PathBuf, String> {
    let user_js = profile_dir.join("user.js");
    let managed: Vec<&str> = C::FIREFOX_PREFS.iter().map(|(name, _)| *name).collect();

    let mut kept: Vec<String> = Vec::new();
    if let Ok(text) = fs::read_to_string(&user_js) {
        for line in text.lines() {
            let stripped = line.trim();
            if managed
                .iter()
                .any(|name| stripped.starts_with(&format!("pref(\"{}\"", name)))
            {
                continue;
            }
            kept.push(line.to_string());
        }
    }

    let mut lines = kept;
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.push("// Added by wideviner.".into());
    for line in pref_content().trim_end().lines() {
        lines.push(line.to_string());
    }
    fs::write(&user_js, lines.join("\n") + "\n")
        .map_err(|err| format!("cannot write {}: {}", user_js.display(), err))?;
    Ok(user_js)
}

/// Flatten the Chromium CDM layout into the layout Firefox expects.
fn firefox_files(payload: &CdmPayload, library: &str) -> HashMap<String, Vec<u8>> {
    let mut files = HashMap::new();
    for (relative, content) in &payload.files {
        let name = Path::new(relative)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name == C::CDM_MANIFEST_NAME || name == C::CDM_LICENSE_NAME || name == library {
            files.insert(name, content.clone());
        }
    }
    files
}

/// Install the CDM into a Firefox browser.
pub fn install_firefox(
    firefox: &Firefox,
    payload: &CdmPayload,
    si: &SystemInfo,
    reporter: Reporter,
) -> Result<InstallResult, String> {
    let version = effective_version(payload);
    let library = if si.lib_name().is_empty() {
        firefox.library.clone()
    } else {
        si.lib_name().to_string()
    };

    // System wide GMP directory, when we may write there.
    let mut system_gmp = None;
    if si.is_linux() && !firefox.sandboxed() {
        for base in C::MOZILLA_PLUGIN_DIRS {
            let candidate = Path::new(base);
            if candidate.is_dir() && (si.is_admin || is_writable(candidate)) {
                system_gmp = Some(
                    candidate
                        .join(C::FIREFOX_GMP_ID)
                        .join(C::FIREFOX_GMP_VERSION),
                );
                break;
            }
        }
        if system_gmp.is_none() && si.is_admin {
            system_gmp = Some(
                PathBuf::from(C::MOZILLA_PLUGIN_DIRS[0])
                    .join(C::FIREFOX_GMP_ID)
                    .join(C::FIREFOX_GMP_VERSION),
            );
        }
    }

    let profile_gmp = firefox.profile_gmp_dir();

    let (target, mode) = if let Some(system) = system_gmp {
        (system, "system")
    } else if let Some(profile) = profile_gmp {
        (profile, "user")
    } else {
        return Err(
            "no writable installation location found; run the browser once so that it \
             creates a profile, or run wideviner as administrator"
                .into(),
        );
    };

    reporter(Event::Info(format!("Installing to {}", target.display())));

    write_files(&firefox_files(payload, &library), &target)?;

    // Prefs.
    let mut hint_file = None;
    if mode == "system" {
        let pref_file = firefox.pref_dir().join(C::FIREFOX_PREF_FILE);
        if let Some(parent) = pref_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::write(&pref_file, pref_content()).is_ok() {
            hint_file = Some(pref_file);
        }
        // Environment snippets so that Firefox finds the plugin directory.
        let snippets: [(&str, String); 3] = [
            ("/etc/profile.d/widevine.sh", shell_snippet(&target)),
            (
                "/usr/share/fish/vendor_conf.d/widevine.fish",
                fish_snippet(&target),
            ),
            (
                "/usr/lib/environment.d/50-widevine.conf",
                format!(
                    "MOZ_GMP_PATH=$MOZ_GMP_PATH${{MOZ_GMP_PATH:+:}}{}\n",
                    target.display()
                ),
            ),
        ];
        for (path, content) in snippets {
            let path = Path::new(path);
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(err) = fs::write(path, content) {
                reporter(Event::Warn(format!("could not write {}: {}", path.display(), err)));
            }
        }
    } else if let Some(profile_dir) = &firefox.profile_dir {
        match write_user_prefs(profile_dir) {
            Ok(path) => hint_file = Some(path),
            Err(err) => reporter(Event::Warn(format!(
                "could not write the Firefox prefs: {}",
                err
            ))),
        }
    }

    if mode == "user" {
        if let Some(profile_dir) = &firefox.profile_dir {
            restore_owner(&profile_dir.join(C::FIREFOX_GMP_ID), reporter);
        }
    }

    Ok(InstallResult {
        version,
        path: target,
        mode: mode.to_string(),
        hint_file,
    })
}

/// Remove the Widevine CDM from a Firefox browser.
pub fn remove_firefox(
    firefox: &Firefox,
    reporter: Reporter,
) -> Result<Vec<PathBuf>, String> {
    let mut removed = Vec::new();

    for candidate in firefox.gmp_dirs() {
        if candidate.is_dir() {
            remove_dir(&candidate, reporter)?;
            removed.push(candidate);
        }
    }

    // System wide prefs and environment snippets.
    let pref_file = firefox.pref_dir().join(C::FIREFOX_PREF_FILE);
    if pref_file.is_file() {
        let _ = fs::remove_file(&pref_file);
        removed.push(pref_file);
    }
    for path in [
        "/etc/profile.d/widevine.sh",
        "/usr/share/fish/vendor_conf.d/widevine.fish",
        "/usr/lib/environment.d/50-widevine.conf",
    ] {
        let path = Path::new(path);
        if path.is_file() {
            let _ = fs::remove_file(path);
            removed.push(path.to_path_buf());
        }
    }

    if removed.is_empty() {
        return Err("no Widevine CDM was found to remove".into());
    }
    Ok(removed)
}

fn is_writable(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let probe = path.join(".wideviner-write-probe");
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

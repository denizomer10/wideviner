//! Obtaining the Widevine CDM from Google's official packages.
//!
//! Two official sources are used, depending on the platform:
//!
//! * **Chrome package (Linux)** — Google's stable Chrome `.deb` from
//!   `dl.google.com` always contains the current CDM. The CDM sits near the
//!   start of the archive, so a partial HTTP range download is usually enough.
//! * **Google CRX package (Windows, Linux fallback)** — Mozilla publishes a
//!   manifest that lists the official Google download URLs and SHA-512 hashes
//!   of the current Widevine CRX for every platform.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha512};

use crate::constants as C;
use crate::crx;
use crate::deb;
use crate::sysinfo::SystemInfo;

/// Progress events reported while downloading.
pub enum Event {
    Info(String),
    Warn(String),
    Progress { done: u64, total: u64 },
}

/// Callback used to report progress.
pub type Reporter<'a> = &'a dyn Fn(Event);

/// The Widevine CDM files extracted from an official package.
#[derive(Debug, Clone, Default)]
pub struct CdmPayload {
    pub version: String,
    pub files: HashMap<String, Vec<u8>>,
    pub source: String,
}

/// Return the platform's cache directory.
pub fn cache_root() -> PathBuf {
    if cfg!(windows) {
        if let Some(base) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(base).join(C::APP_NAME);
        }
        return dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(format!(".{}", C::APP_NAME));
    }
    if let Some(base) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(base).join(C::APP_NAME);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cache")
        .join(C::APP_NAME)
}

/// The official Chrome `.deb` URL for `arch`.
pub fn deb_url(arch: &str) -> String {
    let deb_arch = C::arch_info(arch).map(|i| i.deb_arch).unwrap_or("amd64");
    C::CHROME_DEB_URL.replace("{deb_arch}", deb_arch)
}

/// Turn a Widevine version string into a comparable vector.
pub fn version_key(version: &str) -> Vec<u64> {
    let mut parts = Vec::new();
    for chunk in version.split(['.', '-', '+']) {
        match chunk.parse::<u64>() {
            Ok(number) => parts.push(number),
            Err(_) => break,
        }
    }
    parts
}

/// Check whether a browser binary was built with Widevine support.
pub fn build_supports_widevine(binary: &Path) -> Option<bool> {
    const NEEDLE: &[u8] = b"com.widevine.alpha";
    let file = fs::File::open(binary).ok()?;
    let mut reader = std::io::BufReader::with_capacity(8 * 1024 * 1024, file);
    let mut buffer = vec![0u8; 8 * 1024 * 1024];
    let mut tail: Vec<u8> = Vec::new();
    loop {
        let Ok(read) = reader.read(&mut buffer) else {
            return Some(false);
        };
        if read == 0 {
            return Some(false);
        }
        let mut data = tail.clone();
        data.extend_from_slice(&buffer[..read]);
        if find(&data, NEEDLE) {
            return Some(true);
        }
        tail = data[data.len().saturating_sub(NEEDLE.len())..].to_vec();
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|window| window == needle)
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------
fn user_agent() -> String {
    format!("{}/{}", C::APP_NAME, C::APP_VERSION)
}

fn read_response(response: ureq::Response, reporter: Reporter) -> Result<Vec<u8>, String> {
    let total: u64 = response
        .header("Content-Length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut reader = response.into_reader();
    let mut data = Vec::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut done: u64 = 0;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| format!("download failed: {}", err))?;
        if read == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..read]);
        done += read as u64;
        reporter(Event::Progress { done, total });
    }
    Ok(data)
}

fn download(url: &str, reporter: Reporter) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .set("User-Agent", &user_agent())
        .call()
        .map_err(|err| format!("cannot reach {}: {}", url, err))?;
    read_response(response, reporter)
}

/// Fetch the first `length` bytes of `url`. Returns `(data, partial)`.
fn fetch_range(url: &str, length: u64, reporter: Reporter) -> Result<(Vec<u8>, bool), String> {
    let response = ureq::get(url)
        .set("User-Agent", &user_agent())
        .set("Range", &format!("bytes=0-{}", length - 1))
        .call()
        .map_err(|err| format!("download failed: {}", err))?;
    let partial = response.status() == 206;
    Ok((read_response(response, reporter)?, partial))
}

fn payload_from_files(files: HashMap<String, Vec<u8>>, source: &str) -> CdmPayload {
    let version = files
        .get(C::CDM_MANIFEST_NAME)
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(raw).ok())
        .and_then(|value| value.get("version").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_default();
    CdmPayload {
        version,
        files,
        source: source.to_string(),
    }
}

fn sha512_hex(data: &[u8]) -> String {
    let mut hasher = Sha512::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

// ---------------------------------------------------------------------------
// Source: Chrome .deb (Linux)
// ---------------------------------------------------------------------------
fn deb_cache_path(cache_dir: &Path, arch: &str) -> PathBuf {
    let deb_arch = C::arch_info(arch).map(|i| i.deb_arch).unwrap_or("amd64");
    cache_dir.join(format!("google-chrome-stable_current_{}.deb", deb_arch))
}

fn remote_size(url: &str) -> Result<u64, String> {
    let response = ureq::head(url)
        .set("User-Agent", &user_agent())
        .call()
        .map_err(|err| format!("cannot reach {}: {}", url, err))?;
    Ok(response
        .header("Content-Length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0))
}

fn get_deb(url: &str, cache_path: &Path, reporter: Reporter, refresh: bool) -> Result<PathBuf, String> {
    let remote = remote_size(url)?;

    if !refresh && cache_path.is_file() {
        let cached_size = fs::metadata(cache_path).map(|m| m.len()).unwrap_or(0);
        if remote == 0 || cached_size == remote {
            reporter(Event::Info(format!(
                "Using the cached package: {}",
                cache_path.display()
            )));
            return Ok(cache_path.to_path_buf());
        }
    }

    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("cannot create {}: {}", parent.display(), err))?;
    }
    let data = download(url, reporter)?;
    let tmp = cache_path.with_extension("part");
    fs::write(&tmp, &data).map_err(|err| format!("cannot write {}: {}", tmp.display(), err))?;
    fs::rename(&tmp, cache_path).map_err(|err| format!("cannot write {}: {}", cache_path.display(), err))?;
    Ok(cache_path.to_path_buf())
}

fn fetch_from_deb(
    si: &SystemInfo,
    platform_dir: &str,
    library: &str,
    cache_dir: &Path,
    reporter: Reporter,
    refresh: bool,
    allow_range: bool,
) -> Result<CdmPayload, String> {
    let url = deb_url(si.arch);
    let cache_path = deb_cache_path(cache_dir, si.arch);

    if allow_range && !refresh {
        reporter(Event::Info(
            "Downloading the Widevine CDM from Google (partial package) ...".into(),
        ));
        match fetch_range(&url, C::RANGE_PROBE_BYTES, reporter) {
            Ok((data, partial)) => {
                if partial {
                    let files = deb::collect_cdm(&data, C::DEB_CDM_PATH);
                    if deb::cdm_is_complete(&files, platform_dir, library) {
                        return Ok(payload_from_files(files, "range"));
                    }
                    reporter(Event::Warn(
                        "The partial download did not contain the complete CDM; \
                         fetching the full package instead."
                            .into(),
                    ));
                } else {
                    // The server ignored the range request and sent the whole file.
                    let files = deb::collect_cdm(&data, C::DEB_CDM_PATH);
                    if deb::cdm_is_complete(&files, platform_dir, library) {
                        if let Some(parent) = cache_path.parent() {
                            let _ = fs::create_dir_all(parent);
                        }
                        let _ = fs::write(&cache_path, &data);
                        return Ok(payload_from_files(files, "full"));
                    }
                }
            }
            Err(err) => return Err(err),
        }
    }

    reporter(Event::Info(
        "Downloading the official Chrome package from Google ...".into(),
    ));
    let deb_path = get_deb(&url, &cache_path, reporter, refresh)?;
    let data = fs::read(&deb_path).map_err(|err| format!("cannot read {}: {}", deb_path.display(), err))?;
    let files = deb::collect_cdm(&data, C::DEB_CDM_PATH);
    if !deb::cdm_is_complete(&files, platform_dir, library) {
        return Err("Widevine CDM not found in the downloaded package".into());
    }
    Ok(payload_from_files(files, "full"))
}

// ---------------------------------------------------------------------------
// Source: Google CRX via Mozilla's GMP manifest (Windows, Linux fallback)
// ---------------------------------------------------------------------------
fn load_mozilla_manifest(cache_dir: &Path, reporter: Reporter, refresh: bool) -> Result<serde_json::Value, String> {
    let cache_path = cache_dir.join("mozilla-widevinecdm.json");
    if !refresh && cache_path.is_file() {
        if let Ok(text) = fs::read_to_string(&cache_path) {
            if let Ok(value) = serde_json::from_str(&text) {
                return Ok(value);
            }
        }
    }

    for url in [C::MOZILLA_GMP_MANIFEST, C::MOZILLA_GMP_MANIFEST_FALLBACK] {
        if let Ok(data) = download(url, reporter) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) {
                let _ = fs::create_dir_all(cache_dir);
                let _ = fs::write(&cache_path, &data);
                return Ok(value);
            }
        }
    }

    if let Ok(text) = fs::read_to_string(&cache_path) {
        if let Ok(value) = serde_json::from_str(&text) {
            return Ok(value);
        }
    }
    Err("cannot fetch Mozilla's Widevine manifest".into())
}

fn resolve_platform_entry<'a>(platforms: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    let mut entry = platforms.get(key)?;
    let mut hops = 0;
    while let Some(alias) = entry.get("alias").and_then(|a| a.as_str()) {
        if hops >= 5 {
            break;
        }
        entry = platforms.get(alias)?;
        hops += 1;
    }
    Some(entry)
}

fn fetch_from_mozilla(
    mozilla_key: Option<&str>,
    platform_dir: &str,
    library: &str,
    cache_dir: &Path,
    reporter: Reporter,
    refresh: bool,
) -> Result<CdmPayload, String> {
    let Some(mozilla_key) = mozilla_key else {
        return Err("no CRX package is published for this platform".into());
    };

    reporter(Event::Info("Fetching Mozilla's Widevine manifest ...".into()));
    let manifest = load_mozilla_manifest(cache_dir, reporter, refresh)?;

    let vendor = manifest
        .get("vendors")
        .and_then(|v| v.get(C::MOZILLA_GMP_VENDOR))
        .ok_or("Widevine is not listed in Mozilla's GMP manifest")?;

    let entry = vendor
        .get("platforms")
        .and_then(|platforms| resolve_platform_entry(platforms, mozilla_key))
        .ok_or_else(|| format!("no Widevine package for {}", mozilla_key))?;

    let url = entry
        .get("fileUrl")
        .and_then(|u| u.as_str())
        .or_else(|| {
            entry
                .get("mirrorUrls")
                .and_then(|m| m.as_array())
                .and_then(|a| a.first())
                .and_then(|u| u.as_str())
        })
        .ok_or("the manifest entry has no download URL")?;

    let version = vendor
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let expected_hash = entry
        .get("hashValue")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let expected_size = entry.get("filesize").and_then(|v| v.as_u64()).unwrap_or(0);

    let cache_name = if version.is_empty() {
        format!("widevine-{}.crx3", platform_dir)
    } else {
        format!("widevine-{}-{}.crx3", version, platform_dir)
    };
    let cache_path = cache_dir.join(&cache_name);

    let mut data: Option<Vec<u8>> = None;
    if !refresh && cache_path.is_file() {
        if let Ok(cached) = fs::read(&cache_path) {
            let hash_ok = expected_hash.is_empty() || sha512_hex(&cached) == expected_hash;
            let size_ok = expected_size == 0 || cached.len() as u64 == expected_size;
            if hash_ok && size_ok {
                reporter(Event::Info(format!(
                    "Using the cached package: {}",
                    cache_path.display()
                )));
                data = Some(cached);
            }
        }
    }

    let data = match data {
        Some(data) => data,
        None => {
            reporter(Event::Info("Downloading the Widevine CDM from Google ...".into()));
            let data = download(url, reporter)?;
            if expected_size != 0 && data.len() as u64 != expected_size {
                return Err(format!(
                    "size mismatch: expected {} bytes, got {}",
                    expected_size,
                    data.len()
                ));
            }
            if !expected_hash.is_empty() && sha512_hex(&data) != expected_hash {
                return Err("SHA-512 mismatch, refusing the downloaded package".into());
            }
            let _ = fs::create_dir_all(cache_dir);
            let tmp = cache_path.with_extension("part");
            if fs::write(&tmp, &data).is_ok() {
                let _ = fs::rename(&tmp, &cache_path);
            }
            data
        }
    };

    let files = crx::collect_cdm(&data, platform_dir, library)?;
    if !crx::cdm_is_complete(&files, platform_dir, library) {
        return Err("Widevine CDM not found in the downloaded package".into());
    }
    Ok(payload_from_files(files, "crx"))
}

// ---------------------------------------------------------------------------
// Fetching
// ---------------------------------------------------------------------------
/// Return the sources to try, in order, for this platform.
pub fn sources_for(si: &SystemInfo) -> Vec<&'static str> {
    let mut sources = Vec::new();
    if si.is_linux() && (si.arch == "amd64" || si.arch == "arm64") {
        sources.push("deb");
    }
    if si.cdm().and_then(|c| c.mozilla_key).is_some() {
        sources.push("mozilla");
    }
    sources
}

/// Obtain the Widevine CDM from an official source.
pub fn fetch_cdm(
    si: &SystemInfo,
    cache_dir: Option<&Path>,
    reporter: Reporter,
    refresh: bool,
    source: Option<&str>,
) -> Result<CdmPayload, String> {
    if !si.arch_supported() {
        return Err(format!(
            "unsupported platform: {} / {}",
            si.os_name, si.machine
        ));
    }
    let cdm = si.cdm().ok_or("no CDM layout for this platform")?;
    let platform_dir = cdm.dir;
    let library = cdm.lib;
    let cache_dir = cache_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(cache_root);

    let available = sources_for(si);
    let selected: Vec<&str> = match source {
        None => available.clone(),
        Some(requested) => {
            let filtered: Vec<&str> = available
                .iter()
                .copied()
                .filter(|s| *s == requested)
                .collect();
            if filtered.is_empty() {
                return Err(format!(
                    "the requested source is not available on this platform (available: {})",
                    available.join(", ")
                ));
            }
            filtered
        }
    };

    if selected.is_empty() {
        return Err("no Widevine source is available for this platform".into());
    }

    let mut errors = Vec::new();
    for (index, source) in selected.iter().enumerate() {
        let result = match *source {
            "deb" => fetch_from_deb(si, platform_dir, library, &cache_dir, reporter, refresh, true),
            _ => fetch_from_mozilla(
                cdm.mozilla_key,
                platform_dir,
                library,
                &cache_dir,
                reporter,
                refresh,
            ),
        };
        match result {
            Ok(payload) => return Ok(payload),
            Err(err) => {
                errors.push(format!("{}: {}", source, err));
                if index + 1 < selected.len() {
                    reporter(Event::Warn(format!(
                        "The {} source failed, trying another one.",
                        source
                    )));
                }
            }
        }
    }

    Err(errors.join("; "))
}

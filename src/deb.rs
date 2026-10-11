//! Reading Debian packages (`.deb`) without external tools.
//!
//! A `.deb` is an "ar" archive that contains:
//!
//! ```text
//! debian-binary     the format version ("2.0\n")
//! control.tar.*     package metadata
//! data.tar.*        the files themselves
//! ```
//!
//! This module implements just enough of the ar and tar formats to pull the
//! Widevine CDM out of an official Google Chrome `.deb`. It tolerates truncated
//! input, which allows extracting the CDM from a partial HTTP download.

use std::collections::HashMap;
use std::io::Read;

use crate::constants as C;

const AR_MAGIC: &[u8] = b"!<arch>\n";
const AR_HEADER_LEN: usize = 60;

#[derive(Debug, Clone)]
pub struct ArEntry {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

/// Parse the ar directory from `buf`, which may be truncated.
pub fn read_ar_entries(buf: &[u8]) -> Result<Vec<ArEntry>, String> {
    if !buf.starts_with(AR_MAGIC) {
        return Err("not an ar archive (bad magic)".into());
    }
    let mut entries = Vec::new();
    let mut pos = AR_MAGIC.len();
    while pos + AR_HEADER_LEN <= buf.len() {
        let header = &buf[pos..pos + AR_HEADER_LEN];
        if &header[58..60] != b"`\n" {
            break;
        }
        let raw_name = String::from_utf8_lossy(&header[0..16]).trim().to_string();
        let name = raw_name.trim_end_matches('/').to_string();
        let Ok(size) = String::from_utf8_lossy(&header[48..58])
            .trim()
            .parse::<usize>()
        else {
            break;
        };
        let offset = pos + AR_HEADER_LEN;
        entries.push(ArEntry {
            name,
            offset,
            size,
        });
        pos = offset + size + (size % 2);
    }
    Ok(entries)
}

/// Return the `data.tar.*` entry of an ar directory.
pub fn find_data_tar(entries: &[ArEntry]) -> Option<&ArEntry> {
    entries.iter().find(|entry| entry.name.starts_with("data.tar"))
}

/// Decompress `blob` according to the suffix of `name`.
///
/// Truncated input is tolerated: as much data as can be recovered is returned.
fn decompress(blob: &[u8], name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    if name.ends_with(".xz") {
        let mut input = blob;
        // A truncated stream makes this fail, but everything decoded so far is
        // already in `out`, which is exactly what we want.
        let _ = lzma_rs::xz_decompress(&mut input, &mut out);
    } else if name.ends_with(".gz") {
        let mut decoder = flate2::read::GzDecoder::new(blob);
        let _ = decoder.read_to_end(&mut out);
    } else {
        out.extend_from_slice(blob);
    }
    out
}

fn collect_from_tar(tar_bytes: &[u8], cdm_path: &str) -> HashMap<String, Vec<u8>> {
    let prefix = format!("{}/", cdm_path);
    let mut files = HashMap::new();
    let mut archive = tar::Archive::new(tar_bytes);

    let Ok(entries) = archive.entries() else {
        return files;
    };
    for entry in entries {
        let Ok(mut entry) = entry else {
            // Truncated archive; keep what we have.
            break;
        };
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let Ok(path) = entry.path() else {
            continue;
        };
        let name = path
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        if !name.starts_with(&prefix) {
            continue;
        }
        let mut content = Vec::new();
        if entry.read_to_end(&mut content).is_err() {
            break;
        }
        files.insert(name[prefix.len()..].to_string(), content);
    }
    files
}

/// Extract the Widevine CDM files from `buf`.
///
/// `buf` may be a truncated `.deb`; whatever could be recovered is returned.
/// The result maps paths relative to the CDM directory to file contents.
pub fn collect_cdm(buf: &[u8], cdm_path: &str) -> HashMap<String, Vec<u8>> {
    let Ok(entries) = read_ar_entries(buf) else {
        return HashMap::new();
    };
    let Some(data_tar) = find_data_tar(&entries) else {
        return HashMap::new();
    };
    let end = (data_tar.offset + data_tar.size).min(buf.len());
    if data_tar.offset >= end {
        return HashMap::new();
    }
    let blob = &buf[data_tar.offset..end];
    let tar_bytes = decompress(blob, &data_tar.name);
    if tar_bytes.is_empty() {
        return HashMap::new();
    }
    collect_from_tar(&tar_bytes, cdm_path)
}

/// Check whether `files` contains a usable CDM for the platform.
pub fn cdm_is_complete(files: &HashMap<String, Vec<u8>>, platform_dir: &str, library: &str) -> bool {
    files.contains_key(C::CDM_MANIFEST_NAME)
        && files.contains_key(&format!(
            "{}/{}/{}",
            C::CDM_PLATFORM_DIR,
            platform_dir,
            library
        ))
}

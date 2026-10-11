//! Reading Chrome extension packages (CRX3).
//!
//! Google ships the Widevine CDM as a CRX3 package:
//!
//! ```text
//! bytes 0-3     "Cr24"
//! bytes 4-7     format version (3, little endian)
//! bytes 8-11    length of the following header (little endian)
//! header        protobuf with the signatures
//! payload       a plain ZIP archive
//! ```
//!
//! The ZIP contains the same layout as the Chrome `.deb`.

use std::collections::HashMap;
use std::io::Read;

use crate::constants as C;

const CRX_MAGIC: &[u8] = b"Cr24";

/// Return the offset of the ZIP payload inside a CRX package.
pub fn payload_offset(buf: &[u8]) -> Result<usize, String> {
    if !buf.starts_with(CRX_MAGIC) {
        return Err("not a CRX package (bad magic)".into());
    }
    if buf.len() < 12 {
        return Err("truncated CRX header".into());
    }
    let version = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if version != 3 {
        return Err(format!("unsupported CRX version {}", version));
    }
    let header_length = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]) as usize;
    let offset = 12 + header_length;
    if offset >= buf.len() {
        return Err("truncated CRX header".into());
    }
    Ok(offset)
}

/// Extract the CDM files for `platform_dir` from a CRX package.
///
/// The result uses the same relative layout as the Chrome `.deb`, so both
/// sources can feed the same installation code.
pub fn collect_cdm(
    buf: &[u8],
    platform_dir: &str,
    library: &str,
) -> Result<HashMap<String, Vec<u8>>, String> {
    let offset = payload_offset(buf)?;
    let cursor = std::io::Cursor::new(&buf[offset..]);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|err| format!("CRX payload is not a valid ZIP archive: {}", err))?;

    let prefix = format!("{}/{}/", C::CDM_PLATFORM_DIR, platform_dir);
    let signature = format!("{}.sig", library);
    let mut files = HashMap::new();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("cannot read the CRX payload: {}", err))?;
        let name = entry.name().to_string();
        if name.ends_with('/') {
            continue;
        }
        let base = name.rsplit('/').next().unwrap_or("").to_string();

        let key = if name.starts_with(&prefix) {
            if base != library && base != signature {
                continue;
            }
            name.clone()
        } else if base == C::CDM_MANIFEST_NAME || base == C::CDM_LICENSE_NAME {
            base.clone()
        } else {
            continue;
        };

        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|err| format!("cannot read {} from the CRX package: {}", name, err))?;
        files.insert(key, content);
    }

    Ok(files)
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

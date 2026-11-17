//! Reads the legacy Unity gzip/tar container without extracting asset paths.
use super::*;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone)]
pub(crate) struct PackageAsset {
    pub path: String,
    pub guid: String,
    pub hash: Option<String>,
    pub meta_hash: String,
    pub size: u64,
}

#[derive(Default)]
struct Record {
    pathname: Option<String>,
    asset: Option<(String, u64)>,
    meta: Option<Vec<u8>>,
}

pub(crate) fn valid_guid(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn meta_guid(bytes: &[u8]) -> CoreResult<String> {
    let text = std::str::from_utf8(bytes).map_err(|_| CoreError::Invalid("INVALID_ASSET_META"))?;
    let guids: Vec<_> = text
        .lines()
        .filter_map(|line| line.strip_prefix("guid:").map(str::trim))
        .collect();
    if guids.len() != 1 || !valid_guid(guids[0]) {
        return Err(CoreError::Invalid("INVALID_ASSET_META"));
    }
    Ok(guids[0].to_ascii_lowercase())
}

pub(crate) fn asset_path(value: &str) -> CoreResult<()> {
    if !value.starts_with("Assets/")
        || value.len() > 4096
        || value.to_ascii_lowercase().ends_with(".meta")
    {
        return Err(CoreError::Invalid("UNITYPACKAGE_UNSAFE_PATH"));
    }
    for part in value.split('/') {
        let reserved = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || matches!(part, "." | "..")
            || part.ends_with([' ', '.'])
            || part
                .chars()
                .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
            || matches!(reserved.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (reserved.len() == 4
                && (reserved.starts_with("COM") || reserved.starts_with("LPT"))
                && matches!(reserved.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(CoreError::Invalid("UNITYPACKAGE_UNSAFE_PATH"));
        }
    }
    Ok(())
}

pub(crate) fn inspect(path: &Path) -> CoreResult<Vec<PackageAsset>> {
    let decoder = flate2::read::MultiGzDecoder::new(File::open(path)?);
    let mut archive = tar::Archive::new(decoder.take(MAX_EXPANDED_BYTES + 1));
    let mut records = BTreeMap::<String, Record>::new();
    let mut seen = HashSet::new();
    let mut count = 0;
    for entry in archive
        .entries()
        .map_err(|_| CoreError::Invalid("UNITYPACKAGE_INVALID"))?
        .raw(true)
    {
        count += 1;
        if count > MAX_ARCHIVE_ENTRIES {
            return Err(CoreError::Invalid("UNITYPACKAGE_TOO_LARGE"));
        }
        let mut entry = entry.map_err(|_| CoreError::Invalid("UNITYPACKAGE_INVALID"))?;
        // Unity's macOS exporter includes PAX timestamps/xattrs and AppleDouble
        // metadata. Ignore only metadata that cannot redefine tar paths or sizes.
        if entry.header().entry_type().is_pax_local_extensions()
            || entry.header().entry_type().is_pax_global_extensions()
        {
            if entry.size() > 1024 * 1024 {
                return Err(CoreError::Invalid("UNITYPACKAGE_INVALID"));
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            validate_pax_metadata(&bytes)?;
            continue;
        }
        let raw_name = std::str::from_utf8(&entry.path_bytes())
            .map_err(|_| CoreError::Invalid("UNITYPACKAGE_INVALID"))?
            .trim_end_matches('/')
            .to_owned();
        let mut name = raw_name.as_str();
        while let Some(rest) = name.strip_prefix("./") {
            name = rest;
        }
        if name == "." && entry.header().entry_type().is_dir() && entry.size() == 0 {
            continue;
        }
        let name = name.to_owned();
        if !seen.insert(name.clone()) {
            return Err(CoreError::Invalid("UNITYPACKAGE_DUPLICATE_ENTRY"));
        }
        let parts: Vec<_> = name.split('/').collect();
        let apple_double = (parts.len() == 1
            && (parts[0] == "._." || parts[0].strip_prefix("._").is_some_and(valid_guid)))
            || (parts.len() == 2
                && valid_guid(parts[0])
                && matches!(
                    parts[1],
                    "._asset" | "._asset.meta" | "._pathname" | "._preview.png"
                ));
        if apple_double {
            if !entry.header().entry_type().is_file()
                || !(26..=16 * 1024 * 1024).contains(&entry.size())
            {
                return Err(CoreError::Invalid("UNITYPACKAGE_INVALID"));
            }
            let mut header = [0; 8];
            entry.read_exact(&mut header)?;
            if header != [0, 5, 22, 7, 0, 2, 0, 0] {
                return Err(CoreError::Invalid("UNITYPACKAGE_INVALID"));
            }
            std::io::copy(&mut entry, &mut std::io::sink())?;
            continue;
        }
        if !valid_guid(parts[0]) {
            return Err(CoreError::Invalid("UNITYPACKAGE_INVALID"));
        }
        if entry.header().entry_type().is_dir() && parts.len() == 1 && entry.size() == 0 {
            continue;
        }
        if !entry.header().entry_type().is_file() || parts.len() != 2 {
            return Err(CoreError::Invalid("UNITYPACKAGE_INVALID"));
        }
        let record = records.entry(parts[0].to_ascii_lowercase()).or_default();
        match parts[1] {
            "pathname" => {
                if entry.size() > 4096 || record.pathname.is_some() {
                    return Err(CoreError::Invalid("UNITYPACKAGE_INVALID"));
                }
                let mut text = String::new();
                entry
                    .read_to_string(&mut text)
                    .map_err(|_| CoreError::Invalid("UNITYPACKAGE_INVALID"))?;
                let text = text.trim_end_matches(['\r', '\n']).to_owned();
                asset_path(&text)?;
                record.pathname = Some(text);
            }
            "asset.meta" => {
                if entry.size() > 16 * 1024 * 1024 || record.meta.is_some() {
                    return Err(CoreError::Invalid("UNITYPACKAGE_INVALID"));
                }
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes)?;
                record.meta = Some(bytes);
            }
            "asset" => {
                if record.asset.is_some() {
                    return Err(CoreError::Invalid("UNITYPACKAGE_INVALID"));
                }
                let mut hash = Sha256::new();
                let size = std::io::copy(&mut entry, &mut hash)?;
                record.asset = Some((format!("{:x}", hash.finalize()), size));
            }
            "preview.png" => {
                std::io::copy(&mut entry, &mut std::io::sink())?;
            }
            _ => return Err(CoreError::Invalid("UNITYPACKAGE_INVALID")),
        }
    }
    // Consume padding and the gzip trailer, including its CRC. Never accept a second tar.
    let mut remainder = archive.into_inner();
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = remainder
            .read(&mut buffer)
            .map_err(|_| CoreError::Invalid("UNITYPACKAGE_INVALID"))?;
        if n == 0 {
            break;
        }
        if buffer[..n].iter().any(|b| *b != 0) {
            return Err(CoreError::Invalid("UNITYPACKAGE_INVALID"));
        }
    }
    if remainder.limit() == 0 || records.is_empty() {
        return Err(CoreError::Invalid("UNITYPACKAGE_INVALID"));
    }
    let mut paths = HashSet::new();
    let mut assets = Vec::new();
    for (guid, record) in records {
        let path = record
            .pathname
            .ok_or(CoreError::Invalid("UNITYPACKAGE_INVALID"))?;
        if !paths.insert(path.to_lowercase()) {
            return Err(CoreError::Invalid("UNITYPACKAGE_PATH_COLLISION"));
        }
        let meta = record
            .meta
            .ok_or(CoreError::Invalid("UNITYPACKAGE_INVALID"))?;
        if meta_guid(&meta)? != guid {
            return Err(CoreError::Invalid("UNITYPACKAGE_GUID_MISMATCH"));
        }
        let folder = std::str::from_utf8(&meta)
            .unwrap()
            .lines()
            .any(|line| line.trim() == "folderAsset: yes");
        let (hash, size) = if folder {
            if record.asset.as_ref().is_some_and(|(_, size)| *size != 0) {
                return Err(CoreError::Invalid("UNITYPACKAGE_INVALID"));
            }
            (None, 0)
        } else {
            let (hash, size) = record
                .asset
                .ok_or(CoreError::Invalid("UNITYPACKAGE_INVALID"))?;
            (Some(hash), size)
        };
        assets.push(PackageAsset {
            path,
            guid,
            hash,
            meta_hash: format!("{:x}", Sha256::digest(&meta)),
            size: size + meta.len() as u64,
        });
    }
    Ok(assets)
}

fn validate_pax_metadata(bytes: &[u8]) -> CoreResult<()> {
    let invalid = || CoreError::Invalid("UNITYPACKAGE_INVALID");
    let mut offset = 0;
    while offset < bytes.len() {
        let data = &bytes[offset..];
        let space = data
            .iter()
            .position(|b| *b == b' ')
            .filter(|n| *n <= 12)
            .ok_or_else(invalid)?;
        let size = std::str::from_utf8(&data[..space])
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .ok_or_else(invalid)?;
        if size <= space + 2 || size > data.len() || data[size - 1] != b'\n' {
            return Err(invalid());
        }
        let pair = &data[space + 1..size - 1];
        let equals = pair.iter().position(|b| *b == b'=').ok_or_else(invalid)?;
        let key = std::str::from_utf8(&pair[..equals]).map_err(|_| invalid())?;
        if !matches!(
            key,
            "mtime"
                | "atime"
                | "ctime"
                | "uid"
                | "gid"
                | "uname"
                | "gname"
                | "comment"
                | "charset"
                | "hdrcharset"
        ) && !key.starts_with("LIBARCHIVE.xattr.")
            && !key.starts_with("SCHILY.xattr.")
        {
            return Err(invalid());
        }
        offset += size;
    }
    Ok(())
}

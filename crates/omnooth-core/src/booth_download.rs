//! BOOTH download helpers. This module has no database/service lock dependency so callers can
//! perform network I/O on a worker while the UI continues using the application service.

use crate::{
    deep_link::{expiry_time, validate_download_url},
    import_format, CoreError, CoreResult, ImportFormat, ParsedItemImport, MAX_BOOTH_DOWNLOAD_BYTES,
};
use chrono::{DateTime, Utc};
use reqwest::{
    blocking::{Client, Response},
    header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION},
    redirect::Policy,
    StatusCode,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use url::Url;
use uuid::Uuid;

const MAX_REDIRECTS: usize = 5;
const MAX_METADATA_BYTES: usize = 1024 * 1024;

/// Safe subset of public product metadata used by the library. No delivery URL or token is
/// included. `source_updated_at` is present only for one exact variation/filename match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoothItemMetadata {
    pub title: Option<String>,
    pub source_updated_at: Option<String>,
}

/// Completed download staged beneath app data. Its path and digest are private to prevent
/// accidental logging/serialization; dropping the value removes the temporary file.
pub struct DownloadedBoothArchive {
    pub(crate) archive_path: PathBuf,
    pub(crate) sha256: String,
    pub(crate) size_bytes: u64,
}

impl DownloadedBoothArchive {
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

impl Drop for DownloadedBoothArchive {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.archive_path);
    }
}

/// Downloads a validated signed BOOTH URL without touching SQLite. Redirects are followed
/// manually and every Location is checked against the same HTTPS BOOTH delivery-host policy.
pub fn download_booth_archive(
    request: &ParsedItemImport,
    staging_dir: &Path,
) -> CoreResult<DownloadedBoothArchive> {
    download_booth_archive_with_progress(request, staging_dir, |_, _| {})
}

/// Downloads a validated archive and reports cumulative byte progress after each streamed
/// chunk. Callbacks receive only byte counts and the optional Content-Length.
pub fn download_booth_archive_with_progress<F>(
    request: &ParsedItemImport,
    staging_dir: &Path,
    mut progress: F,
) -> CoreResult<DownloadedBoothArchive>
where
    F: FnMut(u64, Option<u64>),
{
    if !staging_dir.is_absolute() {
        return Err(CoreError::Download("DOWNLOAD_IO_ERROR"));
    }
    ensure_not_expired(request)?;
    fs::create_dir_all(staging_dir).map_err(|_| CoreError::Download("DOWNLOAD_IO_ERROR"))?;
    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(60 * 60))
        .user_agent("omnooth/0.1 (BOOTH library import)")
        .build()
        .map_err(|_| CoreError::Download("DOWNLOAD_HTTP_ERROR"))?;

    let mut url = request.download_url().to_owned();
    let mut response = None;
    for redirect_count in 0..=MAX_REDIRECTS {
        let (parsed, _, _) =
            validate_download_url(&url).map_err(|_| CoreError::Download("DOWNLOAD_REJECTED"))?;
        ensure_url_not_expired(&parsed)?;
        let current = client
            .get(parsed)
            .send()
            .map_err(|error| classify_reqwest_error(&error))?;
        if is_redirect(current.status()) {
            if redirect_count == MAX_REDIRECTS {
                return Err(CoreError::Download("DOWNLOAD_REJECTED"));
            }
            let location = current
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(CoreError::Download("DOWNLOAD_REJECTED"))?;
            url = resolve_redirect(&url, location)?.to_string();
            continue;
        }
        response = Some(current);
        break;
    }
    let response = response.ok_or(CoreError::Download("DOWNLOAD_REJECTED"))?;
    let response = check_status(response)?;
    if response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/html"))
    {
        return Err(CoreError::Download("DOWNLOAD_REJECTED"));
    }
    let expected_len = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if expected_len.is_some_and(|size| size > MAX_BOOTH_DOWNLOAD_BYTES) {
        return Err(CoreError::Download("DOWNLOAD_TOO_LARGE"));
    }

    let part_path = staging_dir.join(format!("{}.part", Uuid::new_v4()));
    let final_path = staging_dir.join(format!("{}.download", Uuid::new_v4()));
    let result = stream_response(response, &part_path, expected_len, &mut progress);
    let (sha256, size_bytes) = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = fs::remove_file(&part_path);
            return Err(error);
        }
    };
    if let Err(_) = fs::rename(&part_path, &final_path) {
        let _ = fs::remove_file(&part_path);
        return Err(CoreError::Download("DOWNLOAD_IO_ERROR"));
    }
    if !valid_download_body(&final_path, &request.receipt().filename) {
        let _ = fs::remove_file(&final_path);
        return Err(CoreError::Download("DOWNLOAD_REJECTED"));
    }
    Ok(DownloadedBoothArchive {
        archive_path: final_path,
        sha256,
        size_bytes,
    })
}

/// Fetches a product title from the public BOOTH API. Failure is safe to treat as `None` by the
/// caller, which should keep the original filename as the display fallback.
pub fn fetch_booth_item_title(booth_item_id: &str) -> CoreResult<Option<String>> {
    Ok(fetch_booth_metadata_json(booth_item_id)?
        .and_then(|data| adapt_metadata(&data, None, None))
        .and_then(|metadata| metadata.title))
}

/// Fetches safe product metadata and matches the BOOTH file timestamp using the reviewed
/// variation ID and exact original filename rule.
pub fn fetch_booth_item_metadata(
    request: &ParsedItemImport,
) -> CoreResult<Option<BoothItemMetadata>> {
    let receipt = request.receipt();
    Ok(
        fetch_booth_metadata_json(&receipt.booth_item_id)?.and_then(|data| {
            adapt_metadata(
                &data,
                receipt.variation_id.as_deref(),
                Some(&receipt.filename),
            )
        }),
    )
}

fn fetch_booth_metadata_json(booth_item_id: &str) -> CoreResult<Option<serde_json::Value>> {
    if booth_item_id.is_empty()
        || !booth_item_id.bytes().all(|byte| byte.is_ascii_digit())
        || booth_item_id.bytes().all(|byte| byte == b'0')
    {
        return Err(CoreError::Invalid("INVALID_DEEP_LINK"));
    }
    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .user_agent("omnooth/0.1 (BOOTH library import)")
        .build()
        .map_err(|_| CoreError::Download("DOWNLOAD_HTTP_ERROR"))?;
    let response = client
        .get(format!("https://api.booth.pm/vroid/items/{booth_item_id}"))
        .send()
        .map_err(|error| classify_reqwest_error(&error))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_METADATA_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(classify_body_error)?;
    if bytes.len() > MAX_METADATA_BYTES {
        return Ok(None);
    }
    let data: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let id_matches = data
        .get("id")
        .and_then(|id| {
            id.as_str()
                .map(str::to_owned)
                .or_else(|| id.as_u64().map(|value| value.to_string()))
        })
        .as_deref()
        == Some(booth_item_id);
    if !id_matches {
        return Ok(None);
    }
    Ok(Some(data))
}

fn adapt_metadata(
    data: &serde_json::Value,
    variation_id: Option<&str>,
    filename: Option<&str>,
) -> Option<BoothItemMetadata> {
    let title = data
        .get("name")
        .and_then(serde_json::Value::as_str)
        .and_then(sanitize_booth_title);
    let source_updated_at = match (variation_id, filename) {
        (Some(variation_id), Some(filename)) => {
            let matches: Vec<_> = data
                .get("variations")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flat_map(|variations| variations.iter())
                .filter(|variation| {
                    variation.get("id").and_then(json_id).as_deref() == Some(variation_id)
                })
                .flat_map(|variation| {
                    variation
                        .get("downloadables")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .filter(|file| {
                    file.get("name").and_then(serde_json::Value::as_str) == Some(filename)
                })
                .collect();
            if matches.len() != 1 {
                None
            } else {
                matches[0]
                    .get("updated_at")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
                    .map(|date| {
                        date.with_timezone(&Utc)
                            .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
                    })
            }
        }
        _ => None,
    };
    if title.is_none() && source_updated_at.is_none() {
        return None;
    }
    Some(BoothItemMetadata {
        title,
        source_updated_at,
    })
}

fn json_id(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

pub(crate) fn sanitize_booth_title(title: &str) -> Option<String> {
    let title = title.trim();
    if title.is_empty() || title.len() > 512 || title.chars().any(char::is_control) {
        return None;
    }
    Some(title.to_owned())
}

fn ensure_not_expired(request: &ParsedItemImport) -> CoreResult<()> {
    if let Some(expiry) = request.url_expires_at() {
        let expiry = DateTime::parse_from_rfc3339(expiry)
            .map_err(|_| CoreError::Download("DOWNLOAD_REJECTED"))?;
        if expiry.with_timezone(&Utc) <= Utc::now() {
            return Err(CoreError::Download("DOWNLOAD_EXPIRED"));
        }
    }
    Ok(())
}

fn ensure_url_not_expired(url: &Url) -> CoreResult<()> {
    let Some(expiry) = expiry_time(url).map_err(|_| CoreError::Download("DOWNLOAD_REJECTED"))?
    else {
        return Ok(());
    };
    let expiry = DateTime::parse_from_rfc3339(&expiry)
        .map_err(|_| CoreError::Download("DOWNLOAD_REJECTED"))?;
    if expiry.with_timezone(&Utc) <= Utc::now() {
        return Err(CoreError::Download("DOWNLOAD_EXPIRED"));
    }
    Ok(())
}

fn resolve_redirect(base: &str, location: &str) -> CoreResult<Url> {
    let current = Url::parse(base).map_err(|_| CoreError::Download("DOWNLOAD_REJECTED"))?;
    let redirected = current
        .join(location)
        .map_err(|_| CoreError::Download("DOWNLOAD_REJECTED"))?;
    let (validated, _, _) = validate_download_url(redirected.as_str())
        .map_err(|_| CoreError::Download("DOWNLOAD_REJECTED"))?;
    ensure_url_not_expired(&validated)?;
    Ok(validated)
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

fn valid_download_body(path: &Path, filename: &str) -> bool {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut head = [0u8; 512];
    let count = match file.read(&mut head) {
        Ok(count) => count,
        Err(_) => return false,
    };
    let bytes = &head[..count];
    let text_start = bytes
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .collect::<Vec<_>>();
    if text_start.starts_with(b"<!doctype html")
        || text_start.starts_with(b"<html")
        || text_start.starts_with(b"<?xml")
    {
        return false;
    }
    match import_format(filename) {
        Ok(ImportFormat::Zip) => {
            bytes.starts_with(b"PK\x03\x04")
                || bytes.starts_with(b"PK\x05\x06")
                || bytes.starts_with(b"PK\x07\x08")
        }
        Ok(ImportFormat::Unitypackage) => bytes.starts_with(&[0x1f, 0x8b]),
        Ok(ImportFormat::SevenZip) => bytes.starts_with(b"7z\xbc\xaf\x27\x1c"),
        Ok(ImportFormat::Rar) => bytes.starts_with(b"Rar!\x1a\x07"),
        Ok(ImportFormat::TarGzip) => bytes.starts_with(&[0x1f, 0x8b]),
        Ok(ImportFormat::TarBzip2) => bytes.starts_with(b"BZh"),
        Ok(ImportFormat::TarXz) => bytes.starts_with(b"\xfd7zXZ\0"),
        Ok(ImportFormat::Tar) => bytes.len() > 262 && &bytes[257..262] == b"ustar",
        Err(_) => false,
    }
}

fn check_status(response: Response) -> CoreResult<Response> {
    validate_status(response.status())?;
    Ok(response)
}

fn validate_status(status: StatusCode) -> CoreResult<()> {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(CoreError::Download("DOWNLOAD_REJECTED"));
    }
    if status == StatusCode::OK {
        return Ok(());
    }
    Err(CoreError::DownloadHttpStatus(status.as_u16()))
}

fn classify_reqwest_error(error: &reqwest::Error) -> CoreError {
    if error.is_timeout() {
        CoreError::Download("DOWNLOAD_TIMEOUT")
    } else if error.is_connect() {
        CoreError::Download("DOWNLOAD_CONNECT_ERROR")
    } else {
        CoreError::Download("DOWNLOAD_HTTP_ERROR")
    }
}

fn classify_body_error(error: std::io::Error) -> CoreError {
    if let Some(network_error) = error
        .get_ref()
        .and_then(|error| error.downcast_ref::<reqwest::Error>())
    {
        classify_reqwest_error(network_error)
    } else {
        CoreError::Download("DOWNLOAD_BODY_ERROR")
    }
}

fn stream_response<F>(
    mut response: Response,
    path: &Path,
    expected_len: Option<u64>,
    progress: &mut F,
) -> CoreResult<(String, u64)>
where
    F: FnMut(u64, Option<u64>),
{
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| CoreError::Download("DOWNLOAD_IO_ERROR"))?;
    stream_to_file_with_progress(&mut response, file, expected_len, progress)
}

#[cfg(test)]
fn stream_to_file(
    reader: &mut impl Read,
    file: File,
    expected_len: Option<u64>,
) -> CoreResult<(String, u64)> {
    stream_to_file_limited(reader, file, expected_len, MAX_BOOTH_DOWNLOAD_BYTES)
}

#[cfg(test)]
fn stream_to_file_limited(
    reader: &mut impl Read,
    file: File,
    expected_len: Option<u64>,
    limit: u64,
) -> CoreResult<(String, u64)> {
    stream_to_file_limited_with_progress(reader, file, expected_len, limit, &mut |_, _| {})
}

fn stream_to_file_with_progress<F>(
    reader: &mut impl Read,
    file: File,
    expected_len: Option<u64>,
    progress: &mut F,
) -> CoreResult<(String, u64)>
where
    F: FnMut(u64, Option<u64>),
{
    stream_to_file_limited_with_progress(
        reader,
        file,
        expected_len,
        MAX_BOOTH_DOWNLOAD_BYTES,
        progress,
    )
}

fn stream_to_file_limited_with_progress<F>(
    reader: &mut impl Read,
    mut file: File,
    expected_len: Option<u64>,
    limit: u64,
    progress: &mut F,
) -> CoreResult<(String, u64)>
where
    F: FnMut(u64, Option<u64>),
{
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer).map_err(classify_body_error)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or(CoreError::Download("DOWNLOAD_TOO_LARGE"))?;
        if total > limit {
            return Err(CoreError::Download("DOWNLOAD_TOO_LARGE"));
        }
        file.write_all(&buffer[..count])
            .map_err(|_| CoreError::Download("DOWNLOAD_IO_ERROR"))?;
        hasher.update(&buffer[..count]);
        progress(total, expected_len);
    }
    if expected_len.is_some_and(|expected| expected != total) || total == 0 {
        return Err(CoreError::Download("DOWNLOAD_HTTP_ERROR"));
    }
    file.sync_all()
        .map_err(|_| CoreError::Download("DOWNLOAD_IO_ERROR"))?;
    Ok((format!("{:x}", hasher.finalize()), total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn stream_hashes_and_checks_length_without_buffering_whole_body() {
        let temp = std::env::temp_dir().join(format!("omnooth-download-test-{}", Uuid::new_v4()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .unwrap();
        let (hash, size) =
            stream_to_file(&mut Cursor::new(b"archive bytes"), file, Some(13)).unwrap();
        assert_eq!(hash, format!("{:x}", Sha256::digest(b"archive bytes")));
        assert_eq!(size, 13);
        fs::remove_file(temp).unwrap();
    }

    #[test]
    fn progress_callback_reports_cumulative_bytes_and_known_total() {
        let temp = std::env::temp_dir().join(format!("omnooth-download-test-{}", Uuid::new_v4()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .unwrap();
        let mut observed = Vec::new();
        let (_, size) = stream_to_file_limited_with_progress(
            &mut Cursor::new(b"archive bytes"),
            file,
            Some(13),
            100,
            &mut |bytes, total| observed.push((bytes, total)),
        )
        .unwrap();
        assert_eq!(size, 13);
        assert_eq!(observed, vec![(13, Some(13))]);
        fs::remove_file(temp).unwrap();
    }

    #[test]
    fn stream_rejects_mismatched_content_length() {
        let temp = std::env::temp_dir().join(format!("omnooth-download-test-{}", Uuid::new_v4()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .unwrap();
        let error = stream_to_file(&mut Cursor::new(b"x"), file, Some(2)).unwrap_err();
        assert_eq!(error.code(), "DOWNLOAD_HTTP_ERROR");
        let _ = fs::remove_file(temp);
    }

    #[test]
    fn stream_enforces_actual_byte_limit_even_without_content_length() {
        let temp = std::env::temp_dir().join(format!("omnooth-download-test-{}", Uuid::new_v4()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .unwrap();
        let error = stream_to_file_limited(&mut Cursor::new(b"12345"), file, None, 4).unwrap_err();
        assert_eq!(error.code(), "DOWNLOAD_TOO_LARGE");
        let _ = fs::remove_file(temp);
    }

    #[test]
    fn rejects_html_body_disguised_as_archive() {
        let temp = std::env::temp_dir().join(format!("omnooth-download-test-{}", Uuid::new_v4()));
        fs::write(&temp, b"<!doctype html><html>sign-in</html>").unwrap();
        assert!(!valid_download_body(&temp, "thing.zip"));
        fs::remove_file(temp).unwrap();
    }

    #[test]
    fn redirect_target_must_remain_on_validated_booth_https_host() {
        let base = "https://s6.booth.pm/files/a.zip?token=one";
        assert_eq!(
            resolve_redirect(base, "/files/b.zip?token=two")
                .unwrap()
                .host_str(),
            Some("s6.booth.pm")
        );
        for location in [
            "https://example.com/file.zip",
            "http://s6.booth.pm/file.zip",
            "//s6.booth.pm:444/file.zip",
        ] {
            assert_eq!(
                resolve_redirect(base, location).unwrap_err().code(),
                "DOWNLOAD_REJECTED"
            );
        }
    }

    #[test]
    fn expired_signed_url_is_rejected_before_network_or_staging() {
        let request = crate::parse_item_import_deep_link("booth-library-manager://item-import?dlurl=https%3A%2F%2Fs6.booth.pm%2Ffiles%2Fa.zip%3FX-Amz-Date%3D20200101T000000Z%26X-Amz-Expires%3D1&downloadable_filename=a.zip&item_id=1").unwrap();
        let temp = std::env::temp_dir().join(format!("omnooth-expired-{}", Uuid::new_v4()));
        assert_eq!(
            download_booth_archive(&request, &temp)
                .err()
                .unwrap()
                .code(),
            "DOWNLOAD_EXPIRED"
        );
        assert!(!temp.exists());
    }

    #[test]
    fn only_complete_http_200_is_accepted_as_a_download() {
        assert!(validate_status(StatusCode::OK).is_ok());
        assert_eq!(
            validate_status(StatusCode::PARTIAL_CONTENT)
                .unwrap_err()
                .code(),
            "DOWNLOAD_HTTP_ERROR"
        );
        assert_eq!(
            validate_status(StatusCode::NO_CONTENT).unwrap_err().code(),
            "DOWNLOAD_HTTP_ERROR"
        );
        assert_eq!(
            validate_status(StatusCode::FORBIDDEN).unwrap_err().code(),
            "DOWNLOAD_REJECTED"
        );
        let error = validate_status(StatusCode::SERVICE_UNAVAILABLE).unwrap_err();
        assert_eq!(error.code(), "DOWNLOAD_HTTP_ERROR");
        assert_eq!(error.safe_detail().as_deref(), Some("503"));
        assert_eq!(error.to_string(), "DOWNLOAD_HTTP_ERROR (503)");
    }

    #[test]
    fn metadata_timestamp_requires_one_exact_variation_and_filename_match() {
        let data = serde_json::json!({
            "id": 8596249,
            "name": "Kirisame",
            "variations": [{"id":14346065,"downloadables":[
                {"name":"Kirisame.zip","updated_at":"2026-08-18T00:30:00+09:00"},
                {"name":"other.zip","updated_at":"2026-08-20T00:00:00Z"}
            ]}]
        });
        let matched = adapt_metadata(&data, Some("14346065"), Some("Kirisame.zip")).unwrap();
        assert_eq!(matched.title.as_deref(), Some("Kirisame"));
        assert_eq!(
            matched.source_updated_at.as_deref(),
            Some("2026-08-17T15:30:00Z")
        );
        let mismatch =
            adapt_metadata(&data, Some("14346065"), Some("Kirisame_ver1.01.zip")).unwrap();
        assert_eq!(mismatch.source_updated_at, None);
        let ambiguous = serde_json::json!({"id":8596249,"name":"Kirisame","variations":[
            {"id":14346065,"downloadables":[{"name":"Kirisame.zip","updated_at":"2026-08-18T00:30:00+09:00"}]},
            {"id":14346065,"downloadables":[{"name":"Kirisame.zip","updated_at":"2026-08-18T00:30:00+09:00"}]}
        ]});
        let ambiguous = adapt_metadata(&ambiguous, Some("14346065"), Some("Kirisame.zip")).unwrap();
        assert_eq!(ambiguous.source_updated_at, None);
    }
}

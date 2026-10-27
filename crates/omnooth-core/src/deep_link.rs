use crate::{import_format, CoreError, CoreResult};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::{Host, Url};

/// Metadata safe to retain or send to the UI. It never contains the signed URL or order ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemImportReceipt {
    pub booth_item_id: String,
    pub variation_id: Option<String>,
    pub filename: String,
    pub source_identity: String,
}

/// Parsed request. The signed URL and order ID are deliberately private and this type has no
/// `Serialize` implementation. Its custom `Debug` output includes only sanitized metadata.
pub struct ParsedItemImport {
    receipt: ItemImportReceipt,
    download_url: String,
    _order_id: Option<String>,
    url_expires_at: Option<String>,
}

impl std::fmt::Debug for ParsedItemImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParsedItemImport")
            .field("receipt", &self.receipt)
            .field("url_expires_at", &self.url_expires_at)
            .finish_non_exhaustive()
    }
}

impl ParsedItemImport {
    pub fn receipt(&self) -> &ItemImportReceipt {
        &self.receipt
    }

    /// Returns the exact once-decoded `dlurl` value for the HTTP client. Do not log it.
    pub fn download_url(&self) -> &str {
        &self.download_url
    }

    pub fn url_expires_at(&self) -> Option<&str> {
        self.url_expires_at.as_deref()
    }
}

/// Parses `booth-library-manager://item-import` without performing I/O or exposing secrets in
/// error strings. The outer query is decoded exactly once, preserving inner signed URL escapes.
pub fn parse_item_import_deep_link(url: &str) -> CoreResult<ParsedItemImport> {
    const PREFIX: &str = "booth-library-manager://";
    if url.len() > 64 * 1024 {
        return Err(CoreError::Invalid("INVALID_DEEP_LINK"));
    }
    let rest = url
        .strip_prefix(PREFIX)
        .ok_or(CoreError::Invalid("INVALID_DEEP_LINK"))?;
    let (authority, query) = rest
        .split_once('?')
        .ok_or(CoreError::Invalid("INVALID_DEEP_LINK"))?;
    if authority != "item-import" || query.is_empty() || query.contains('#') {
        return Err(CoreError::Invalid("INVALID_DEEP_LINK"));
    }

    let mut values = std::collections::HashMap::<String, String>::new();
    let known = [
        "dlurl",
        "downloadable_filename",
        "item_id",
        "variation_id",
        "order_id",
    ];
    for pair in query.split('&') {
        if pair.is_empty() {
            return Err(CoreError::Invalid("INVALID_DEEP_LINK"));
        }
        let (key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = strict_form_decode(key).ok_or(CoreError::Invalid("INVALID_DEEP_LINK"))?;
        let value = strict_form_decode(raw_value).ok_or(CoreError::Invalid("INVALID_DEEP_LINK"))?;
        if known.contains(&key.as_str()) && values.insert(key.clone(), value).is_some() {
            return Err(CoreError::Invalid("INVALID_DEEP_LINK"));
        }
        if !known.contains(&key.as_str()) {
            continue;
        }
    }

    let download_url = values
        .remove("dlurl")
        .filter(|v| !v.is_empty())
        .ok_or(CoreError::Invalid("INVALID_DOWNLOAD_URL"))?;
    let filename = values
        .remove("downloadable_filename")
        .filter(|v| !v.is_empty())
        .ok_or(CoreError::Invalid("INVALID_FILENAME"))?;
    if filename.len() > 1024 {
        return Err(CoreError::Invalid("INVALID_FILENAME"));
    }
    let booth_item_id = values
        .remove("item_id")
        .filter(|v| is_positive_decimal(&v))
        .ok_or(CoreError::Invalid("INVALID_DEEP_LINK"))?;
    if crate::validate_filename(&filename).is_err() || import_format(&filename).is_err() {
        return Err(CoreError::Invalid("INVALID_FILENAME"));
    }
    let variation_id = optional_id(values.remove("variation_id"))?;
    let order_id = optional_order_id(values.remove("order_id"))?;

    let (parsed_url, host, path) = validate_download_url(&download_url)?;
    let url_expires_at = expiry_time(&parsed_url)?;
    let source_identity = source_identity(&booth_item_id, variation_id.as_deref(), &host, &path);
    Ok(ParsedItemImport {
        receipt: ItemImportReceipt {
            booth_item_id,
            variation_id,
            filename,
            source_identity,
        },
        download_url,
        _order_id: order_id,
        url_expires_at,
    })
}

fn strict_form_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' => {
                let hi = *bytes.get(index + 1)?;
                let lo = *bytes.get(index + 2)?;
                decoded.push((hex(hi)? << 4) | hex(lo)?);
                index += 3;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn is_positive_decimal(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.bytes().any(|b| b != b'0')
}

fn optional_id(value: Option<String>) -> CoreResult<Option<String>> {
    match value {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) if is_positive_decimal(&value) => Ok(Some(value)),
        _ => Err(CoreError::Invalid("INVALID_DEEP_LINK")),
    }
}

fn optional_order_id(value: Option<String>) -> CoreResult<Option<String>> {
    match value {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) if value.len() <= 2048 && !value.chars().any(char::is_control) => {
            Ok(Some(value))
        }
        _ => Err(CoreError::Invalid("INVALID_DEEP_LINK")),
    }
}

fn validate_download_url(raw: &str) -> CoreResult<(Url, String, String)> {
    if raw.contains('#') || !valid_percent_escapes(raw) {
        return Err(CoreError::Invalid("INVALID_DOWNLOAD_URL"));
    }
    let parsed = Url::parse(raw).map_err(|_| CoreError::Invalid("INVALID_DOWNLOAD_URL"))?;
    if parsed.scheme() != "https"
        || parsed.fragment().is_some()
        || parsed.port().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(CoreError::Invalid("INVALID_DOWNLOAD_URL"));
    }
    let authority = raw
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    if authority.contains('@') {
        return Err(CoreError::Invalid("INVALID_DOWNLOAD_URL"));
    }
    let host = match parsed.host() {
        Some(Host::Domain(host)) if approved_booth_host(host) => host.to_ascii_lowercase(),
        _ => return Err(CoreError::Invalid("INVALID_DOWNLOAD_URL")),
    };
    let path = parsed.path().to_string();
    if path == "/" {
        return Err(CoreError::Invalid("INVALID_DOWNLOAD_URL"));
    }
    Ok((parsed, host, path))
}

fn approved_booth_host(host: &str) -> bool {
    let Some(prefix) = host.strip_suffix(".booth.pm") else {
        return false;
    };
    let Some(number) = prefix.strip_prefix('s') else {
        return false;
    };
    !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
}

fn valid_percent_escapes(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() || hex(bytes[i + 1]).is_none() || hex(bytes[i + 2]).is_none() {
                return false;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    true
}

fn expiry_time(url: &Url) -> CoreResult<Option<String>> {
    let mut date = None;
    let mut expires = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "X-Amz-Date" => {
                if date.replace(value.into_owned()).is_some() {
                    return Err(CoreError::Invalid("INVALID_DOWNLOAD_URL"));
                }
            }
            "X-Amz-Expires" => {
                if expires.replace(value.into_owned()).is_some() {
                    return Err(CoreError::Invalid("INVALID_DOWNLOAD_URL"));
                }
            }
            _ => {}
        }
    }
    match (date, expires) {
        (None, None) => Ok(None),
        (Some(date), Some(expires)) => {
            let start = NaiveDateTime::parse_from_str(&date, "%Y%m%dT%H%M%SZ")
                .map_err(|_| CoreError::Invalid("INVALID_DOWNLOAD_URL"))?
                .and_utc();
            let seconds = expires
                .parse::<i64>()
                .map_err(|_| CoreError::Invalid("INVALID_DOWNLOAD_URL"))?;
            let deadline: DateTime<Utc> = start
                .checked_add_signed(Duration::seconds(seconds))
                .ok_or(CoreError::Invalid("INVALID_DOWNLOAD_URL"))?;
            Ok(Some(
                deadline.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            ))
        }
        _ => Err(CoreError::Invalid("INVALID_DOWNLOAD_URL")),
    }
}

fn source_identity(item_id: &str, variation_id: Option<&str>, host: &str, path: &str) -> String {
    let mut digest = Sha256::new();
    for part in [item_id, variation_id.unwrap_or(""), host, path] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    format!("sha256:{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(dlurl: &str, extra: &str) -> String {
        format!("booth-library-manager://item-import?dlurl={}&downloadable_filename=Kirisame.zip&item_id=8596249{}",dlurl,extra)
    }

    fn outer_encode(value: &str) -> String {
        value
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }

    #[test]
    fn decodes_signed_url_once_and_exposes_only_sanitized_receipt() {
        let signed="https://s6.booth.pm/path/to/file.zip?X-Amz-Credential=a%2Fb&X-Amz-Date=20261027T010203Z&X-Amz-Expires=600";
        let parsed = parse_item_import_deep_link(&link(
            &outer_encode(signed),
            "&variation_id=14346065&order_id=",
        ))
        .unwrap();
        assert_eq!(parsed.download_url(), signed);
        assert_eq!(parsed.receipt().booth_item_id, "8596249");
        assert_eq!(parsed.receipt().variation_id.as_deref(), Some("14346065"));
        assert_eq!(parsed.receipt().filename, "Kirisame.zip");
        assert!(parsed
            .url_expires_at()
            .unwrap()
            .starts_with("2026-10-27T01:12:03"));
        let receipt = serde_json::to_string(parsed.receipt()).unwrap();
        assert!(!receipt.contains("X-Amz"));
        assert!(!receipt.contains("order"));
        assert!(!receipt.contains("a%2Fb"));
        let debug = format!("{parsed:?}");
        assert!(!debug.contains("X-Amz"));
        assert!(!debug.contains("a%2Fb"));
    }

    #[test]
    fn source_identity_omits_signed_query_but_includes_file_path() {
        let a = parse_item_import_deep_link(&link(
            &outer_encode("https://s6.booth.pm/a/file.zip?signature=one"),
            "",
        ))
        .unwrap();
        let b = parse_item_import_deep_link(&link(
            &outer_encode("https://s6.booth.pm/a/file.zip?signature=two"),
            "",
        ))
        .unwrap();
        let c = parse_item_import_deep_link(&link(
            &outer_encode("https://s6.booth.pm/b/file.zip?signature=two"),
            "",
        ))
        .unwrap();
        assert_eq!(a.receipt().source_identity, b.receipt().source_identity);
        assert_ne!(a.receipt().source_identity, c.receipt().source_identity);
    }

    #[test]
    fn accepts_opaque_bounded_order_ids_without_including_them_in_receipt_or_debug() {
        let parsed = parse_item_import_deep_link(&link(
            &outer_encode("https://s6.booth.pm/file.zip?signature=secret"),
            "&order_id=opaque-test-abc",
        ))
        .unwrap();
        assert_eq!(parsed._order_id.as_deref(), Some("opaque-test-abc"));
        assert!(!serde_json::to_string(parsed.receipt())
            .unwrap()
            .contains("opaque-test-abc"));
        assert!(!format!("{parsed:?}").contains("opaque-test-abc"));
    }

    #[test]
    fn enforces_link_and_filename_size_limits() {
        let oversized = "x".repeat(64 * 1024 + 1);
        assert_eq!(
            parse_item_import_deep_link(&oversized)
                .err()
                .unwrap()
                .code(),
            "INVALID_DEEP_LINK"
        );
        let name = format!("{}.zip", "a".repeat(1025));
        let url = format!(
            "booth-library-manager://item-import?dlurl={}&downloadable_filename={}&item_id=1",
            outer_encode("https://s6.booth.pm/a.zip"),
            outer_encode(&name)
        );
        assert_eq!(
            parse_item_import_deep_link(&url).err().unwrap().code(),
            "INVALID_FILENAME"
        );
    }

    #[test]
    fn rejects_bad_actions_duplicate_parameters_ids_names_and_percent_encoding() {
        assert_eq!(
            parse_item_import_deep_link("booth-library-manager://other?x=y")
                .unwrap_err()
                .code(),
            "INVALID_DEEP_LINK"
        );
        assert_eq!(
            parse_item_import_deep_link(&link("https%3A%2F%2Fs6.booth.pm%2Fa.zip", "&item_id=2"))
                .unwrap_err()
                .code(),
            "INVALID_DEEP_LINK"
        );
        assert_eq!(parse_item_import_deep_link("booth-library-manager://item-import?dlurl=%Q1&downloadable_filename=a.zip&item_id=1").unwrap_err().code(),"INVALID_DEEP_LINK");
        assert_eq!(parse_item_import_deep_link("booth-library-manager://item-import?dlurl=https%3A%2F%2Fs6.booth.pm%2Fa.zip&downloadable_filename=..%2Fbad.png&item_id=1").unwrap_err().code(),"INVALID_FILENAME");
        assert_eq!(parse_item_import_deep_link("booth-library-manager://item-import?dlurl=https%3A%2F%2Fs6.booth.pm%2Fa.zip&downloadable_filename=a.zip&item_id=0").unwrap_err().code(),"INVALID_DEEP_LINK");
    }

    #[test]
    fn rejects_untrusted_download_hosts_credentials_ports_fragments_and_ip_hosts() {
        for invalid in [
            "https://evil.example/a.zip",
            "https://s6.booth.pm.evil.example/a.zip",
            "https://user@s6.booth.pm/a.zip",
            "https://s6.booth.pm:8443/a.zip",
            "https://s6.booth.pm/a.zip#fragment",
            "https://127.0.0.1/a.zip",
        ] {
            let url = link(&outer_encode(invalid), "");
            assert_eq!(
                parse_item_import_deep_link(&url).unwrap_err().code(),
                "INVALID_DOWNLOAD_URL"
            );
        }
    }
}

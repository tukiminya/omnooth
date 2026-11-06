use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_MEMORY_ENTRIES: usize = 2_000;
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_SESSION_SEGMENTS: usize = 4;
const MAX_RETAINED_SESSIONS: usize = 8;
const MAX_OPERATION_BYTES: usize = 128;
const MAX_MESSAGE_BYTES: usize = 2_048;
const MAX_CORRELATION_ID_BYTES: usize = 128;

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: String,
    pub level: LogLevel,
    pub operation: String,
    pub message: String,
    pub correlation_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSnapshot {
    pub session_id: String,
    pub entries: Vec<LogEntry>,
}

#[derive(Debug)]
pub enum ActivityLogError {
    Io(io::Error),
    Serialization(serde_json::Error),
}

impl fmt::Display for ActivityLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "activity log I/O failed: {error}"),
            Self::Serialization(error) => write!(f, "activity log serialization failed: {error}"),
        }
    }
}

impl Error for ActivityLogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Serialization(error) => Some(error),
        }
    }
}

impl From<io::Error> for ActivityLogError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ActivityLogError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

/// A bounded, append-only JSONL activity log for the current application session.
///
/// Callers should serialize access externally (for example with a `Mutex`).
pub struct ActivityLog {
    session_id: String,
    log_directory: PathBuf,
    segment: usize,
    segment_path: PathBuf,
    segment_bytes: u64,
    next_id: u64,
    entries: VecDeque<LogEntry>,
}

impl ActivityLog {
    pub fn open(app_data: impl AsRef<Path>) -> Result<Self, ActivityLogError> {
        let log_directory = app_data.as_ref().join("logs");
        fs::create_dir_all(&log_directory)?;
        prune_old_sessions(&log_directory, MAX_RETAINED_SESSIONS - 1)?;

        let now = UtcTimestamp::now();
        let session_id = format!(
            "{}-{}-{}",
            now.compact,
            std::process::id(),
            SESSION_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let segment_path = session_segment_path(&log_directory, &session_id, 0);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&segment_path)?;
        file.sync_data()?;

        Ok(Self {
            session_id,
            log_directory,
            segment: 0,
            segment_path,
            segment_bytes: 0,
            next_id: 1,
            entries: VecDeque::with_capacity(MAX_MEMORY_ENTRIES),
        })
    }

    pub fn record(
        &mut self,
        level: LogLevel,
        operation: &str,
        message: &str,
        correlation_id: Option<&str>,
    ) -> Result<(), ActivityLogError> {
        let entry = LogEntry {
            id: self.next_id,
            timestamp: UtcTimestamp::now().rfc3339,
            level,
            operation: sanitize(operation, MAX_OPERATION_BYTES),
            message: sanitize(message, MAX_MESSAGE_BYTES),
            correlation_id: correlation_id.map(|value| sanitize(value, MAX_CORRELATION_ID_BYTES)),
        };
        let mut line = serde_json::to_vec(&entry)?;
        line.push(b'\n');

        if self.segment_bytes + line.len() as u64 > MAX_FILE_BYTES {
            self.rotate_segment()?;
        }

        let mut file = OpenOptions::new().append(true).open(&self.segment_path)?;
        file.write_all(&line)?;
        file.flush()?;
        self.segment_bytes += line.len() as u64;

        self.entries.push_back(entry);
        while self.entries.len() > MAX_MEMORY_ENTRIES {
            self.entries.pop_front();
        }
        self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    pub fn snapshot(&self) -> LogSnapshot {
        LogSnapshot {
            session_id: self.session_id.clone(),
            entries: self.entries.iter().cloned().collect(),
        }
    }

    fn rotate_segment(&mut self) -> Result<(), ActivityLogError> {
        let next_segment = (self.segment + 1) % MAX_SESSION_SEGMENTS;
        let next_path = session_segment_path(&self.log_directory, &self.session_id, next_segment);
        match fs::remove_file(&next_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&next_path)?;
        file.sync_data()?;
        self.segment = next_segment;
        self.segment_path = next_path;
        self.segment_bytes = 0;
        Ok(())
    }
}

fn session_segment_path(directory: &Path, session_id: &str, segment: usize) -> PathBuf {
    directory.join(format!("session-{session_id}-{segment:04}.jsonl"))
}

fn prune_old_sessions(directory: &Path, keep: usize) -> Result<(), ActivityLogError> {
    let mut sessions: Vec<(String, SystemTime)> = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(base) = name
            .strip_prefix("session-")
            .and_then(|name| name.strip_suffix(".jsonl"))
        else {
            continue;
        };
        let Some((session_id, segment)) = base.rsplit_once('-') else {
            continue;
        };
        if session_id.is_empty()
            || segment.len() != 4
            || !segment.bytes().all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let modified = entry.metadata()?.modified().unwrap_or(UNIX_EPOCH);
        if let Some((_, newest)) = sessions.iter_mut().find(|(id, _)| id == session_id) {
            if modified > *newest {
                *newest = modified;
            }
        } else {
            sessions.push((session_id.to_owned(), modified));
        }
    }
    sessions.sort_by(|left, right| right.1.cmp(&left.1));
    let retained: Vec<String> = sessions
        .iter()
        .take(keep)
        .map(|(session_id, _)| session_id.clone())
        .collect();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(base) = name
            .strip_prefix("session-")
            .and_then(|name| name.strip_suffix(".jsonl"))
        else {
            continue;
        };
        let Some((session_id, segment)) = base.rsplit_once('-') else {
            continue;
        };
        if segment.len() == 4
            && segment.bytes().all(|byte| byte.is_ascii_digit())
            && !retained.iter().any(|retained_id| retained_id == session_id)
        {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn sanitize(input: &str, max_bytes: usize) -> String {
    let without_secrets = redact_sensitive_values(input);
    let without_urls = redact_urls(&without_secrets);
    let mut output = String::with_capacity(without_urls.len().min(max_bytes));
    let mut bytes = 0;
    for ch in without_urls.chars() {
        let ch = if ch.is_control() { ' ' } else { ch };
        let width = ch.len_utf8();
        if bytes + width > max_bytes {
            break;
        }
        output.push(ch);
        bytes += width;
    }
    output
}

fn redact_sensitive_values(input: &str) -> String {
    const KEYS: &[&str] = &[
        "access_token",
        "refresh_token",
        "session_token",
        "authorization",
        "download_url",
        "downloadurl",
        "downloadable_url",
        "signed_url",
        "signedurl",
        "order_id",
        "orderid",
        "password",
        "signature",
        "deep_link",
        "deeplink",
        "api_key",
        "apikey",
        "cookie",
        "secret",
        "dlurl",
        "token",
        "url",
        "sig",
    ];

    let mut output = input.to_owned();
    for key in KEYS {
        let mut search_from = 0;
        loop {
            let lower = output.to_ascii_lowercase();
            let Some(relative) = lower[search_from..].find(key) else {
                break;
            };
            let start = search_from + relative;
            let end_key = start + key.len();
            let before_is_word = start > 0 && is_ascii_word(lower.as_bytes()[start - 1]);
            let after_is_word = end_key < lower.len() && is_ascii_word(lower.as_bytes()[end_key]);
            if before_is_word || after_is_word {
                search_from = end_key;
                continue;
            }

            let bytes = output.as_bytes();
            let mut value_start = end_key;
            if value_start < bytes.len()
                && (bytes[value_start] == b'\'' || bytes[value_start] == b'"')
            {
                value_start += 1;
            }
            while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
                value_start += 1;
            }
            if value_start >= bytes.len() || !matches!(bytes[value_start], b'=' | b':') {
                search_from = end_key;
                continue;
            }
            value_start += 1;
            while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
                value_start += 1;
            }
            if value_start < bytes.len()
                && (bytes[value_start] == b'\'' || bytes[value_start] == b'"')
            {
                value_start += 1;
            }
            let mut value_end = value_start;
            if *key == "authorization" || *key == "cookie" {
                while value_end < bytes.len() && !matches!(bytes[value_end], b'\r' | b'\n') {
                    value_end += 1;
                }
            } else {
                while value_end < bytes.len()
                    && !matches!(
                        bytes[value_end],
                        b'&' | b','
                            | b';'
                            | b' '
                            | b'\t'
                            | b'\r'
                            | b'\n'
                            | b'\''
                            | b'"'
                            | b'}'
                            | b'>'
                    )
                {
                    value_end += 1;
                }
            }
            if value_end == value_start {
                search_from = end_key;
                continue;
            }
            output.replace_range(start..value_end, "[REDACTED]");
            search_from = start + "[REDACTED]".len();
        }
    }
    output
}

fn redact_urls(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let Some(relative) = input[cursor..].find("://") else {
            output.push_str(&input[cursor..]);
            break;
        };
        let marker = cursor + relative;
        let mut start = marker;
        while start > 0 && is_scheme_byte(bytes[start - 1]) {
            start -= 1;
        }
        let has_scheme = marker > start
            && bytes[start].is_ascii_alphabetic()
            && (start == 0 || !is_ascii_word(bytes[start - 1]));
        if !has_scheme {
            output.push_str(&input[cursor..marker + 3]);
            cursor = marker + 3;
            continue;
        }
        output.push_str(&input[cursor..start]);
        output.push_str("[REDACTED_URL]");
        let mut end = marker + 3;
        while end < bytes.len()
            && !bytes[end].is_ascii_whitespace()
            && !matches!(
                bytes[end],
                b'\'' | b'"' | b'<' | b'>' | b'(' | b')' | b'[' | b']' | b'{' | b'}'
            )
        {
            end += 1;
        }
        cursor = end;
    }
    output
}

fn is_ascii_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_scheme_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')
}

struct UtcTimestamp {
    rfc3339: String,
    compact: String,
}

impl UtcTimestamp {
    fn now() -> Self {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let seconds = duration.as_secs();
        let days = (seconds / 86_400) as i64;
        let day_seconds = seconds % 86_400;
        let (year, month, day) = civil_from_days(days);
        let hour = day_seconds / 3_600;
        let minute = (day_seconds % 3_600) / 60;
        let second = day_seconds % 60;
        let millis = duration.subsec_millis();
        Self {
            rfc3339: format!(
                "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z"
            ),
            compact: format!(
                "{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}{millis:03}Z"
            ),
        }
    }
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::ops::Deref;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TemporaryDirectory(PathBuf);

    impl AsRef<Path> for TemporaryDirectory {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Deref for TemporaryDirectory {
        type Target = Path;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temporary_directory() -> TemporaryDirectory {
        let path = std::env::temp_dir().join(format!(
            "omnooth-activity-log-test-{}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TemporaryDirectory(path)
    }

    #[test]
    fn redacts_urls_and_sensitive_values_from_every_user_controlled_field() {
        let directory = temporary_directory();
        let mut log = ActivityLog::open(&directory).unwrap();
        log.record(
            LogLevel::Error,
            "import?order_id=ord-123",
            "request failed https://download.example/file?token=secret and order_id: 987 password=abc booth-library-manager://item-import?dlurl=https://signed.example/x Authorization: Bearer header-secret",
            Some("token=correlation-secret"),
        )
        .unwrap();

        let snapshot = log.snapshot();
        let entry = &snapshot.entries[0];
        let combined = format!(
            "{} {} {:?}",
            entry.operation, entry.message, entry.correlation_id
        );
        for secret in [
            "ord-123",
            "987",
            "abc",
            "download.example",
            "signed.example",
            "correlation-secret",
            "header-secret",
        ] {
            assert!(!combined.contains(secret), "secret remained in: {combined}");
        }
        assert!(entry.message.contains("[REDACTED_URL]"));
        assert!(entry.message.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_repeated_secrets_after_replacement_changes_byte_offsets() {
        for message in [
            "token=x token=very-long-secret token=日本語の秘密",
            "password=first-long-value password=b password=最後の秘密",
        ] {
            let redacted = sanitize(message, 2048);
            assert_eq!(redacted, "[REDACTED] [REDACTED] [REDACTED]");
        }
    }

    #[test]
    fn appends_jsonl_and_exposes_iso_utc_snapshot_dto() {
        let directory = temporary_directory();
        let mut log = ActivityLog::open(&directory).unwrap();
        log.record(LogLevel::Info, "app.start", "started", None)
            .unwrap();
        let snapshot = log.snapshot();
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(snapshot.entries[0].id, 1);
        assert!(snapshot.entries[0].timestamp.ends_with('Z'));
        assert!(snapshot.entries[0]
            .timestamp
            .as_bytes()
            .get(10)
            .is_some_and(|byte| *byte == b'T'));

        let file = fs::read_dir(directory.join("logs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let content = fs::read_to_string(file).unwrap();
        let persisted: LogEntry = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(persisted, snapshot.entries[0]);

        let wire = serde_json::to_value(snapshot).unwrap();
        assert!(wire.get("sessionId").is_some());
        assert_eq!(wire["entries"][0]["level"], "info");
        assert!(wire["entries"][0].get("correlationId").is_some());
    }

    #[test]
    fn keeps_memory_and_session_files_bounded() {
        let directory = temporary_directory();
        let mut log = ActivityLog::open(&directory).unwrap();
        for index in 0..(MAX_MEMORY_ENTRIES + 50) {
            log.record(LogLevel::Warn, "test", &format!("entry-{index}"), None)
                .unwrap();
        }
        assert_eq!(log.entries.len(), MAX_MEMORY_ENTRIES);
        assert_eq!(log.snapshot().entries.first().unwrap().id, 51);
        assert!(log.segment_bytes <= MAX_FILE_BYTES);
        let segment_count = fs::read_dir(directory.join("logs"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("session-"))
            .count();
        assert!(segment_count <= MAX_SESSION_SEGMENTS);
    }

    #[test]
    fn rotates_session_files_at_the_configured_size_limit() {
        let directory = temporary_directory();
        let mut log = ActivityLog::open(&directory).unwrap();
        let message = "x".repeat(MAX_MESSAGE_BYTES);
        for _ in 0..140 {
            log.record(LogLevel::Info, "large-entry", &message, None)
                .unwrap();
        }
        assert!(log.segment > 0);
        assert!(log.segment_bytes <= MAX_FILE_BYTES);
        let files: Vec<_> = fs::read_dir(directory.join("logs"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("session-"))
            .collect();
        assert!(files.len() <= MAX_SESSION_SEGMENTS);
        let total_size: u64 = files
            .iter()
            .map(|entry| entry.metadata().unwrap().len())
            .sum();
        assert!(total_size <= MAX_FILE_BYTES * MAX_SESSION_SEGMENTS as u64);
    }

    #[test]
    fn retains_only_the_newest_session_files() {
        let directory = temporary_directory();
        for index in 0..(MAX_RETAINED_SESSIONS + 2) {
            let mut log = ActivityLog::open(&directory).unwrap();
            log.record(LogLevel::Info, "session", &index.to_string(), None)
                .unwrap();
        }
        let sessions: Vec<_> = fs::read_dir(directory.join("logs"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("session-"))
            .collect();
        assert_eq!(sessions.len(), MAX_RETAINED_SESSIONS);
    }
}

use omnooth_core::{AppSettings, ItemImportReceipt, ParsedItemImport};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_ACTIVE_JOBS: usize = 100;
const MAX_PERSISTED_RECORDS: usize = 1_000;
const MAX_QUEUE_FILE_BYTES: usize = 12 * 1024 * 1024;
const MAX_LIBRARY_PATH_BYTES: usize = 4_096;
const MAX_ERROR_CODE_BYTES: usize = 64;
const MAX_LOCAL_ITEM_ID_BYTES: usize = 128;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Queued,
    Downloading,
    AwaitingUrl,
    Failed,
    Succeeded,
}

impl JobStatus {
    fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Downloading | Self::AwaitingUrl)
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Failed | Self::Succeeded)
    }
}

/// Safe persisted job metadata. This type intentionally has no field for a signed URL,
/// parsed request, order id, or other download credential.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueRecord {
    pub id: u64,
    pub receipt: ItemImportReceipt,
    pub extract_archives: bool,
    pub library_path: String,
    pub status: JobStatus,
    pub error_code: Option<String>,
    pub local_item_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedQueueRecord {
    id: u64,
    receipt: PersistedReceipt,
    extract_archives: bool,
    library_path: String,
    status: JobStatus,
    error_code: Option<String>,
    local_item_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedReceipt {
    booth_item_id: String,
    variation_id: Option<String>,
    filename: String,
    source_identity: String,
}

impl<'de> Deserialize<'de> for QueueRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = PersistedQueueRecord::deserialize(deserializer)?;
        Ok(Self {
            id: value.id,
            receipt: ItemImportReceipt {
                booth_item_id: value.receipt.booth_item_id,
                variation_id: value.receipt.variation_id,
                filename: value.receipt.filename,
                source_identity: value.receipt.source_identity,
            },
            extract_archives: value.extract_archives,
            library_path: value.library_path,
            status: value.status,
            error_code: value.error_code,
            local_item_id: value.local_item_id,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EnqueueDisposition {
    Accepted,
    Reused,
    AlreadySucceeded,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueOutcome {
    pub record: QueueRecord,
    pub disposition: EnqueueDisposition,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    pub jobs: Vec<QueueRecord>,
    pub active_count: usize,
}

/// A one-use in-memory download request paired with the settings captured at receipt time.
/// `request` contains the signed URL and must never be serialized or logged.
pub struct ImportJob {
    pub id: u64,
    pub receipt: ItemImportReceipt,
    pub request: ParsedItemImport,
    pub extract_archives: bool,
    pub library_path: String,
}

#[derive(Debug)]
pub enum QueueError {
    Io(io::Error),
    Serialization(serde_json::Error),
    Corrupt,
    InvalidInput,
    QueueFull,
    HistoryFull,
    JobNotFound,
    InvalidTransition,
}

impl QueueError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "QUEUE_IO_ERROR",
            Self::Serialization(_) | Self::Corrupt => "QUEUE_STORAGE_ERROR",
            Self::InvalidInput => "INVALID_QUEUE_INPUT",
            Self::QueueFull => "IMPORT_QUEUE_FULL",
            Self::HistoryFull => "IMPORT_HISTORY_FULL",
            Self::JobNotFound => "IMPORT_JOB_NOT_FOUND",
            Self::InvalidTransition => "INVALID_IMPORT_TRANSITION",
        }
    }
}

impl fmt::Display for QueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl Error for QueueError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Serialization(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for QueueError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for QueueError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

/// Persistent sanitized import queue. Signed requests are held only in this process's memory.
/// Callers should serialize access externally (for example with a `Mutex`).
pub struct ImportQueue {
    storage_path: PathBuf,
    records: VecDeque<QueueRecord>,
    requests: HashMap<u64, ParsedItemImport>,
}

impl ImportQueue {
    pub fn open(app_data: impl AsRef<Path>) -> Result<Self, QueueError> {
        let storage_path = app_data.as_ref().join("import-queue.json");
        if let Some(parent) = storage_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let (records, exists) = match fs::read(&storage_path) {
            Ok(bytes) => {
                if bytes.len() > MAX_QUEUE_FILE_BYTES {
                    return Err(QueueError::Corrupt);
                }
                let records: Vec<QueueRecord> =
                    serde_json::from_slice(&bytes).map_err(|_| QueueError::Corrupt)?;
                (records, true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (Vec::new(), false),
            Err(error) => return Err(error.into()),
        };

        validate_records(&records)?;
        let mut queue = Self {
            storage_path,
            records: records.into(),
            requests: HashMap::new(),
        };
        let mut recovered = false;
        for record in &mut queue.records {
            if matches!(record.status, JobStatus::Queued | JobStatus::Downloading) {
                record.status = JobStatus::AwaitingUrl;
                record.error_code = Some("DOWNLOAD_URL_NOT_RESTORED".to_owned());
                recovered = true;
            }
        }
        if !exists || recovered {
            queue.persist()?;
        }
        Ok(queue)
    }

    pub fn enqueue(
        &mut self,
        parsed: ParsedItemImport,
        settings: &AppSettings,
    ) -> Result<EnqueueOutcome, QueueError> {
        let receipt = parsed.receipt().clone();
        validate_receipt(&receipt)?;
        validate_library_path(&settings.library_path)?;

        if let Some(index) = self.records.iter().rposition(|record| {
            record.receipt.source_identity == receipt.source_identity
                && record.status == JobStatus::Succeeded
        }) {
            return Ok(EnqueueOutcome {
                record: self.records[index].clone(),
                disposition: EnqueueDisposition::AlreadySucceeded,
            });
        }

        if let Some(index) = self.records.iter().position(|record| {
            record.receipt.source_identity == receipt.source_identity
                && matches!(record.status, JobStatus::Queued | JobStatus::Downloading)
        }) {
            if self.records[index].status == JobStatus::Queued {
                let previous = self.records[index].clone();
                self.records[index].receipt = receipt;
                if let Err(error) = self.persist() {
                    self.records[index] = previous;
                    return Err(error);
                }
                let record = self.records[index].clone();
                self.requests.insert(record.id, parsed);
                return Ok(EnqueueOutcome {
                    record,
                    disposition: EnqueueDisposition::Reused,
                });
            }
            let record = self.records[index].clone();
            // Keep the newest URL and receipt as a one-time retry if the active download fails.
            self.requests.insert(record.id, parsed);
            return Ok(EnqueueOutcome {
                record,
                disposition: EnqueueDisposition::Reused,
            });
        }

        if let Some(index) = self.records.iter().position(|record| {
            record.receipt.source_identity == receipt.source_identity
                && matches!(record.status, JobStatus::AwaitingUrl | JobStatus::Failed)
        }) {
            let previous = self.records[index].clone();
            self.records[index].receipt = receipt;
            self.records[index].status = JobStatus::Queued;
            self.records[index].error_code = None;
            self.records[index].local_item_id = None;
            if let Err(error) = self.persist() {
                self.records[index] = previous;
                return Err(error);
            }
            let record = self.records[index].clone();
            self.requests.insert(record.id, parsed);
            return Ok(EnqueueOutcome {
                record,
                disposition: EnqueueDisposition::Reused,
            });
        }

        if self.active_count() >= MAX_ACTIVE_JOBS {
            return Err(QueueError::QueueFull);
        }
        self.trim_history()?;
        if self.records.len() >= MAX_PERSISTED_RECORDS {
            return Err(QueueError::HistoryFull);
        }
        let id = self
            .records
            .back()
            .map(|record| record.id.checked_add(1).ok_or(QueueError::HistoryFull))
            .transpose()?
            .unwrap_or(1);
        let record = QueueRecord {
            id,
            receipt,
            extract_archives: settings.extract_archives,
            library_path: settings.library_path.clone(),
            status: JobStatus::Queued,
            error_code: None,
            local_item_id: None,
        };
        self.records.push_back(record.clone());
        if let Err(error) = self.persist() {
            self.records.pop_back();
            return Err(error);
        }
        self.requests.insert(id, parsed);
        Ok(EnqueueOutcome {
            record,
            disposition: EnqueueDisposition::Accepted,
        })
    }

    /// Returns the oldest queued request and marks it active durably before handing it off.
    /// A persisted `Downloading` status prevents a second worker from starting concurrently.
    pub fn take_next(&mut self) -> Result<Option<ImportJob>, QueueError> {
        if self
            .records
            .iter()
            .any(|record| record.status == JobStatus::Downloading)
        {
            return Ok(None);
        }

        loop {
            let Some(index) = self
                .records
                .iter()
                .position(|record| record.status == JobStatus::Queued)
            else {
                return Ok(None);
            };
            let id = self.records[index].id;
            if !self.requests.contains_key(&id) {
                let previous = self.records[index].clone();
                self.records[index].status = JobStatus::AwaitingUrl;
                self.records[index].error_code = Some("DOWNLOAD_URL_NOT_RESTORED".to_owned());
                if let Err(error) = self.persist() {
                    self.records[index] = previous;
                    return Err(error);
                }
                continue;
            }

            let previous = self.records[index].clone();
            self.records[index].status = JobStatus::Downloading;
            if let Err(error) = self.persist() {
                self.records[index] = previous;
                return Err(error);
            }
            let record = self.records[index].clone();
            let request = self
                .requests
                .remove(&id)
                .ok_or(QueueError::InvalidTransition)?;
            return Ok(Some(ImportJob {
                id,
                receipt: record.receipt,
                request,
                extract_archives: record.extract_archives,
                library_path: record.library_path,
            }));
        }
    }

    pub fn finish(
        &mut self,
        job_id: u64,
        status: JobStatus,
        error_code: Option<&str>,
        local_item_id: Option<&str>,
    ) -> Result<QueueRecord, QueueError> {
        if !matches!(
            status,
            JobStatus::AwaitingUrl | JobStatus::Failed | JobStatus::Succeeded
        ) {
            return Err(QueueError::InvalidTransition);
        }
        let error_code = error_code.map(validate_error_code).transpose()?;
        let local_item_id = local_item_id.map(validate_local_item_id).transpose()?;
        if (status == JobStatus::Succeeded) != local_item_id.is_some()
            || (status == JobStatus::Succeeded && error_code.is_some())
        {
            return Err(QueueError::InvalidInput);
        }
        let index = self
            .records
            .iter()
            .position(|record| record.id == job_id)
            .ok_or(QueueError::JobNotFound)?;
        if self.records[index].status != JobStatus::Downloading {
            return Err(QueueError::InvalidTransition);
        }

        let previous = self.records[index].clone();
        self.records[index].status = status;
        self.records[index].error_code = error_code;
        self.records[index].local_item_id = local_item_id;
        let retry_receipt = self
            .requests
            .get(&job_id)
            .map(|request| request.receipt().clone());
        if let Some(receipt) = retry_receipt.filter(|_| status != JobStatus::Succeeded) {
            self.records[index].receipt = receipt;
            self.records[index].status = JobStatus::Queued;
            self.records[index].error_code = None;
        }
        if let Err(error) = self.persist() {
            self.records[index] = previous;
            return Err(error);
        }
        if status == JobStatus::Succeeded {
            self.requests.remove(&job_id);
        }
        let record = self.records[index].clone();
        self.trim_history()?;
        Ok(record)
    }

    /// Releases an active slot after a worker failure even if its normal finish write failed.
    /// The returned failed record is authoritative in memory; disk persistence is best effort.
    pub fn release_after_failure(&mut self, job_id: u64, error_code: &str) -> Option<QueueRecord> {
        let index = self
            .records
            .iter()
            .position(|record| record.id == job_id && record.status == JobStatus::Downloading)?;
        let safe_code =
            validate_error_code(error_code).unwrap_or_else(|_| "IMPORT_WORKER_FAILED".to_owned());
        self.requests.remove(&job_id);
        let record = &mut self.records[index];
        record.status = JobStatus::Failed;
        record.error_code = Some(safe_code);
        record.local_item_id = None;
        let failed = record.clone();
        let _ = self.persist();
        Some(failed)
    }

    pub fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            jobs: self.records.iter().cloned().collect(),
            active_count: self.active_count(),
        }
    }

    pub fn active_count(&self) -> usize {
        self.records
            .iter()
            .filter(|record| record.status.is_active())
            .count()
    }

    fn trim_history(&mut self) -> Result<(), QueueError> {
        let previous_records = self.records.clone();
        let mut removed_ids = Vec::new();
        let mut removed = false;
        while self.records.len() >= MAX_PERSISTED_RECORDS {
            let Some(index) = self
                .records
                .iter()
                .position(|record| record.status.is_terminal())
            else {
                break;
            };
            let removed_record = self.records.remove(index).expect("position was found");
            removed_ids.push(removed_record.id);
            removed = true;
        }
        if removed {
            if let Err(error) = self.persist() {
                self.records = previous_records;
                return Err(error);
            }
            for id in removed_ids {
                self.requests.remove(&id);
            }
        }
        Ok(())
    }

    fn persist(&self) -> Result<(), QueueError> {
        let bytes = serde_json::to_vec(&self.records)?;
        if bytes.len() > MAX_QUEUE_FILE_BYTES {
            return Err(QueueError::HistoryFull);
        }
        let parent = self.storage_path.parent().ok_or(QueueError::InvalidInput)?;
        let temp_path = parent.join(format!(
            ".import-queue-{}-{}.tmp",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> Result<(), QueueError> {
            let mut temp = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)?;
            temp.write_all(&bytes)?;
            temp.sync_all()?;
            drop(temp);
            replace_file(&temp_path, &self.storage_path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }
}

fn validate_records(records: &[QueueRecord]) -> Result<(), QueueError> {
    if records.len() > MAX_PERSISTED_RECORDS {
        return Err(QueueError::Corrupt);
    }
    let mut prior_id = 0;
    let mut active_sources = std::collections::HashSet::new();
    let mut active_count = 0;
    let mut downloading_count = 0;
    for record in records {
        if record.id == 0 || record.id <= prior_id {
            return Err(QueueError::Corrupt);
        }
        prior_id = record.id;
        validate_receipt(&record.receipt).map_err(|_| QueueError::Corrupt)?;
        validate_library_path(&record.library_path).map_err(|_| QueueError::Corrupt)?;
        if let Some(code) = record.error_code.as_deref() {
            validate_error_code(code).map_err(|_| QueueError::Corrupt)?;
        }
        if let Some(item_id) = record.local_item_id.as_deref() {
            validate_local_item_id(item_id).map_err(|_| QueueError::Corrupt)?;
        }
        if (record.status == JobStatus::Succeeded) != record.local_item_id.is_some()
            || (record.status == JobStatus::Succeeded && record.error_code.is_some())
        {
            return Err(QueueError::Corrupt);
        }
        if record.status.is_active() {
            active_count += 1;
            if !active_sources.insert(record.receipt.source_identity.as_str()) {
                return Err(QueueError::Corrupt);
            }
        }
        if record.status == JobStatus::Downloading {
            downloading_count += 1;
        }
    }
    if active_count > MAX_ACTIVE_JOBS || downloading_count > 1 {
        return Err(QueueError::Corrupt);
    }
    Ok(())
}

fn validate_receipt(receipt: &ItemImportReceipt) -> Result<(), QueueError> {
    if receipt.booth_item_id.is_empty()
        || receipt.booth_item_id.len() > 32
        || !receipt
            .booth_item_id
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        || receipt.filename.is_empty()
        || receipt.filename.len() > 1_024
        || receipt.filename.contains('/')
        || receipt.filename.contains('\\')
        || receipt.source_identity.len() != 71
        || !receipt.source_identity.starts_with("sha256:")
        || !receipt.source_identity[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(QueueError::InvalidInput);
    }
    if receipt.variation_id.as_ref().is_some_and(|value| {
        value.is_empty() || value.len() > 64 || !value.bytes().all(|b| b.is_ascii_digit())
    }) {
        return Err(QueueError::InvalidInput);
    }
    Ok(())
}

fn validate_library_path(value: &str) -> Result<(), QueueError> {
    if value.is_empty() || value.len() > MAX_LIBRARY_PATH_BYTES || value.contains('\0') {
        return Err(QueueError::InvalidInput);
    }
    Ok(())
}

fn validate_error_code(value: &str) -> Result<String, QueueError> {
    if value.is_empty()
        || value.len() > MAX_ERROR_CODE_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(QueueError::InvalidInput);
    }
    Ok(value.to_owned())
}

fn validate_local_item_id(value: &str) -> Result<String, QueueError> {
    if value.is_empty()
        || value.len() > MAX_LOCAL_ITEM_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(QueueError::InvalidInput);
    }
    Ok(value.to_owned())
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Result<(), QueueError> {
    use std::os::windows::ffi::OsStrExt;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnooth_core::parse_item_import_deep_link;
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

    fn temp_dir() -> TemporaryDirectory {
        let path = std::env::temp_dir().join(format!(
            "omnooth-import-queue-test-{}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TemporaryDirectory(path)
    }

    fn parsed(filename: &str, token: &str, order_id: &str) -> ParsedItemImport {
        parsed_named(filename, filename, token, order_id)
    }

    fn parsed_named(
        path_name: &str,
        filename: &str,
        token: &str,
        order_id: &str,
    ) -> ParsedItemImport {
        let url = format!(
            "booth-library-manager://item-import?dlurl=https%3A%2F%2Fs6.booth.pm%2Ffiles%2F{path_name}%3Ftoken%3D{token}&downloadable_filename={filename}&item_id=8596249&variation_id=14346065&order_id={order_id}"
        );
        parse_item_import_deep_link(&url).unwrap()
    }

    fn settings(library_path: &str, extract_archives: bool) -> AppSettings {
        AppSettings {
            language: "ja".to_owned(),
            library_path: library_path.to_owned(),
            extract_archives,
            keep_old_versions: true,
        }
    }

    #[test]
    fn persists_only_safe_receipts_and_restarts_unfinished_jobs_awaiting_fresh_urls() {
        let directory = temp_dir();
        let original_settings = settings("/library/original", false);
        {
            let mut queue = ImportQueue::open(&directory).unwrap();
            queue
                .enqueue(
                    parsed("first.zip", "signed-secret-one", "order-secret-one"),
                    &original_settings,
                )
                .unwrap();
            queue
                .enqueue(
                    parsed("second.zip", "signed-secret-two", "order-secret-two"),
                    &original_settings,
                )
                .unwrap();
            assert_eq!(queue.take_next().unwrap().unwrap().id, 1);
        }

        let raw = fs::read_to_string(directory.join("import-queue.json")).unwrap();
        for secret in ["signed-secret", "order-secret", "dlurl", "token="] {
            assert!(
                !raw.contains(secret),
                "secret remained in queue file: {secret}"
            );
        }
        let stored: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let expected_keys = [
            "errorCode",
            "extractArchives",
            "id",
            "libraryPath",
            "localItemId",
            "receipt",
            "status",
        ];
        for record in stored.as_array().unwrap() {
            let mut keys: Vec<_> = record
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(keys, expected_keys);
        }

        let mut queue = ImportQueue::open(&directory).unwrap();
        let snapshot = queue.snapshot();
        assert_eq!(snapshot.active_count, 2);
        assert_eq!(snapshot.jobs[0].status, JobStatus::AwaitingUrl);
        assert_eq!(snapshot.jobs[1].status, JobStatus::AwaitingUrl);
        assert_eq!(
            snapshot.jobs[0].error_code.as_deref(),
            Some("DOWNLOAD_URL_NOT_RESTORED")
        );

        let refreshed = queue
            .enqueue(
                parsed("first.zip", "fresh-secret", "new-order-secret"),
                &settings("/library/changed", true),
            )
            .unwrap();
        assert_eq!(refreshed.disposition, EnqueueDisposition::Reused);
        assert_eq!(refreshed.record.id, 1);
        assert_eq!(refreshed.record.library_path, "/library/original");
        assert!(!refreshed.record.extract_archives);
        let job = queue.take_next().unwrap().unwrap();
        assert_eq!(job.id, 1);
        assert!(job.request.download_url().contains("fresh-secret"));
        assert_eq!(job.library_path, "/library/original");
        assert!(!job.extract_archives);
    }

    #[test]
    fn deduplicates_sources_uses_fifo_and_continues_after_failure() {
        let directory = temp_dir();
        let initial_settings = settings("/library/first", true);
        let mut queue = ImportQueue::open(&directory).unwrap();
        let first = queue
            .enqueue(
                parsed("first.zip", "token-one", "order-one"),
                &initial_settings,
            )
            .unwrap();
        let duplicate = queue
            .enqueue(
                parsed("first.zip", "refreshed-one", "order-refreshed"),
                &settings("/library/ignored", false),
            )
            .unwrap();
        assert_eq!(first.disposition, EnqueueDisposition::Accepted);
        assert_eq!(duplicate.disposition, EnqueueDisposition::Reused);
        assert_eq!(duplicate.record.id, first.record.id);
        assert_eq!(queue.snapshot().jobs.len(), 1);

        queue
            .enqueue(
                parsed("second.zip", "token-two", "order-two"),
                &initial_settings,
            )
            .unwrap();
        let active = queue.take_next().unwrap().unwrap();
        assert_eq!(active.id, 1);
        assert!(active.request.download_url().contains("refreshed-one"));
        assert!(queue.take_next().unwrap().is_none());
        queue
            .finish(1, JobStatus::Failed, Some("DOWNLOAD_REJECTED"), None)
            .unwrap();

        let next = queue.take_next().unwrap().unwrap();
        assert_eq!(next.id, 2);
        assert!(next.request.download_url().contains("token-two"));
        queue
            .finish(2, JobStatus::Succeeded, None, Some("local-item-2"))
            .unwrap();

        let duplicate_success = queue
            .enqueue(
                parsed("second.zip", "new-token", "new-order"),
                &initial_settings,
            )
            .unwrap();
        assert_eq!(
            duplicate_success.disposition,
            EnqueueDisposition::AlreadySucceeded
        );
        assert_eq!(
            duplicate_success.record.local_item_id.as_deref(),
            Some("local-item-2")
        );
        assert_eq!(queue.snapshot().active_count, 0);
    }

    #[test]
    fn fresh_url_received_during_active_job_becomes_a_single_retry_if_it_fails() {
        let directory = temp_dir();
        let mut queue = ImportQueue::open(&directory).unwrap();
        queue
            .enqueue(
                parsed("retry.zip", "old-token", "order-one"),
                &settings("/first", false),
            )
            .unwrap();
        let first_try = queue.take_next().unwrap().unwrap();
        queue
            .enqueue(
                parsed_named("retry.zip", "renamed-retry.zip", "new-token", "order-two"),
                &settings("/second", true),
            )
            .unwrap();
        assert!(queue.take_next().unwrap().is_none());
        assert_eq!(first_try.receipt.filename, "retry.zip");
        assert_eq!(queue.snapshot().jobs[0].receipt.filename, "retry.zip");

        let retry_record = queue
            .finish(
                1,
                JobStatus::AwaitingUrl,
                Some("DOWNLOAD_URL_EXPIRED"),
                None,
            )
            .unwrap();
        assert_eq!(retry_record.status, JobStatus::Queued);
        assert_eq!(retry_record.library_path, "/first");
        drop(first_try);
        let retry = queue.take_next().unwrap().unwrap();
        assert_eq!(retry.id, 1);
        assert!(retry.request.download_url().contains("new-token"));
        assert_eq!(retry.receipt.filename, "renamed-retry.zip");
        assert_eq!(retry.library_path, "/first");
        assert!(!retry.extract_archives);
    }

    #[test]
    fn releases_active_job_after_finish_persistence_failure() {
        let directory = temp_dir();
        let mut queue = ImportQueue::open(&directory).unwrap();
        let settings = settings("/library", false);
        queue
            .enqueue(parsed("active.zip", "token-a", "order-a"), &settings)
            .unwrap();
        queue
            .enqueue(parsed("next.zip", "token-b", "order-b"), &settings)
            .unwrap();
        let active = queue.take_next().unwrap().unwrap();
        let storage_path = queue.storage_path.clone();
        queue.storage_path = directory.join("missing-parent").join("import-queue.json");

        assert!(queue
            .finish(1, JobStatus::Failed, Some("DOWNLOAD_REJECTED"), None)
            .is_err());
        let failed = queue
            .release_after_failure(1, "IMPORT_WORKER_FAILED")
            .expect("active job should be released");
        assert_eq!(failed.status, JobStatus::Failed);
        assert_eq!(queue.records[0].status, JobStatus::Failed);

        queue.storage_path = storage_path;
        let next = queue.take_next().unwrap().unwrap();
        assert_eq!(next.id, 2);
        drop(active);
    }

    #[test]
    fn queued_duplicate_updates_filename_without_changing_captured_settings() {
        let directory = temp_dir();
        let mut queue = ImportQueue::open(&directory).unwrap();
        queue
            .enqueue(
                parsed_named("same.zip", "original.zip", "old-token", "order-a"),
                &settings("/original-library", false),
            )
            .unwrap();
        let duplicate = queue
            .enqueue(
                parsed_named("same.zip", "fresh-name.zip", "fresh-token", "order-b"),
                &settings("/changed-library", true),
            )
            .unwrap();
        assert_eq!(duplicate.record.receipt.filename, "fresh-name.zip");
        assert_eq!(duplicate.record.library_path, "/original-library");
        assert!(!duplicate.record.extract_archives);
        let job = queue.take_next().unwrap().unwrap();
        assert_eq!(job.receipt.filename, "fresh-name.zip");
        assert!(job.request.download_url().contains("fresh-token"));
        assert_eq!(job.library_path, "/original-library");
    }

    #[test]
    fn enforces_the_active_queue_limit() {
        let directory = temp_dir();
        let mut queue = ImportQueue::open(&directory).unwrap();
        let settings = settings("/library", false);
        for index in 0..MAX_ACTIVE_JOBS {
            queue
                .enqueue(
                    parsed(&format!("item-{index}.zip"), "token", "order"),
                    &settings,
                )
                .unwrap();
        }
        let error = queue
            .enqueue(parsed("overflow.zip", "token", "order"), &settings)
            .unwrap_err();
        assert_eq!(error.code(), "IMPORT_QUEUE_FULL");
        assert_eq!(queue.snapshot().active_count, MAX_ACTIVE_JOBS);
    }

    #[test]
    fn refuses_corrupt_or_unknown_persisted_fields_without_resetting_the_file() {
        let directory = temp_dir();
        let storage = directory.join("import-queue.json");
        let corrupt = br#"[{"id":1,"receipt":{"boothItemId":"8596249","variationId":null,"filename":"x.zip","sourceIdentity":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","downloadUrl":"secret"},"extractArchives":false,"libraryPath":"/library","status":"queued","errorCode":null,"localItemId":null}]"#;
        fs::write(&storage, corrupt).unwrap();
        let error = match ImportQueue::open(&directory) {
            Ok(_) => panic!("corrupt queue unexpectedly opened"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "QUEUE_STORAGE_ERROR");
        assert_eq!(fs::read(&storage).unwrap(), corrupt);
    }
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> Result<(), QueueError> {
    fs::rename(from, to)?;
    if let Some(parent) = to.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

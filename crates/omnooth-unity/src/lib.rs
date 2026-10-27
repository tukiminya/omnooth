//! Local manifest transport for the Unity Editor adapter.
//!
//! Requests are written under `<project>/Library/Omnooth`; Unity owns applying
//! them and writing the matching result. This crate never edits project assets.

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const PROTOCOL_VERSION: u32 = 1;
pub const ADAPTER_DIRECTORY: &str = "Omnooth";
const REQUEST_FILE: &str = "request.json";
const RESULT_FILE: &str = "result.json";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnityImportRequest {
    pub protocol_version: u32,
    pub job_id: String,
    /// Explicit application order. Unity imports one entry at a time.
    pub packages: Vec<UnityPackageImport>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnityPackageImport {
    pub operation_id: String,
    /// Canonical absolute path to one existing `.unitypackage` file.
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnityImportStatus {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnityImportResult {
    pub protocol_version: u32,
    pub job_id: String,
    pub status: UnityImportStatus,
    /// Completed operations are always an ordered prefix of the request.
    pub completed_operation_ids: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "empty_string_is_none"
    )]
    pub error: Option<String>,
}

fn empty_string_is_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|value| !value.is_empty()))
}

#[derive(Debug)]
pub enum AdapterError {
    Io(io::Error),
    Json(serde_json::Error),
    Invalid(String),
    JobInProgress,
    ResultNotFound,
    ResultMismatch(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Unity adapter I/O error: {e}"),
            Self::Json(e) => write!(f, "Unity adapter manifest error: {e}"),
            Self::Invalid(reason) => write!(f, "invalid Unity adapter input: {reason}"),
            Self::JobInProgress => write!(f, "a Unity import request is still awaiting its result"),
            Self::ResultNotFound => write!(f, "Unity import result is not available yet"),
            Self::ResultMismatch(reason) => {
                write!(f, "Unity result does not match request: {reason}")
            }
        }
    }
}

impl std::error::Error for AdapterError {}
impl From<io::Error> for AdapterError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<serde_json::Error> for AdapterError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

/// Validates and atomically publishes a request for an already-open Unity project.
/// A live request cannot be replaced. The Editor extension consumes the fixed
/// request file and writes `result.json` only after a terminal callback.
pub fn write_import_request(
    project_path: impl AsRef<Path>,
    request: &UnityImportRequest,
) -> Result<PathBuf, AdapterError> {
    let project = validate_project(project_path.as_ref())?;
    validate_request(request)?;
    let adapter_dir = project.join("Library").join(ADAPTER_DIRECTORY);
    fs::create_dir_all(&adapter_dir)?;
    let request_path = adapter_dir.join(REQUEST_FILE);
    if request_path.exists() {
        let previous_request = fs::read(&request_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<UnityImportRequest>(&bytes).ok());
        let prior_job_is_terminal = previous_request.as_ref().is_some_and(|old_request| {
            read_import_result_for_request(&project, old_request)
                .is_ok_and(|old_result| old_result.job_id == old_request.job_id)
        });
        if !prior_job_is_terminal {
            return Err(AdapterError::JobInProgress);
        }
        let old_request = previous_request.expect("checked above");
        if old_request.job_id == request.job_id {
            if old_request == *request {
                return Ok(request_path);
            }
            return Err(AdapterError::Invalid(
                "job_id cannot be reused for a different request".into(),
            ));
        }
        // Leave the completed request in place until both its result and identity
        // have been checked. Remove it before publishing the next file so rename
        // remains atomic on platforms that do not replace existing files.
        fs::remove_file(&request_path)?;
    }
    atomic_write(&request_path, &serde_json::to_vec_pretty(request)?)?;
    Ok(request_path)
}

/// Reads only a terminal result for the expected job, validating its shape and identity.
pub fn read_import_result(
    project_path: impl AsRef<Path>,
    expected_job_id: &str,
) -> Result<UnityImportResult, AdapterError> {
    validate_id(expected_job_id, "job_id")?;
    let project = validate_project(project_path.as_ref())?;
    let result_path = project
        .join("Library")
        .join(ADAPTER_DIRECTORY)
        .join(RESULT_FILE);
    let bytes = match fs::read(result_path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(AdapterError::ResultNotFound),
        Err(e) => return Err(e.into()),
    };
    let result: UnityImportResult = serde_json::from_slice(&bytes)?;
    validate_result(&result)?;
    if result.job_id != expected_job_id {
        return Err(AdapterError::ResultMismatch("job_id differs".into()));
    }
    Ok(result)
}

/// Reads a result and verifies that completed operations are an ordered prefix
/// of this exact request (and completed requests report every operation).
pub fn read_import_result_for_request(
    project_path: impl AsRef<Path>,
    request: &UnityImportRequest,
) -> Result<UnityImportResult, AdapterError> {
    validate_request(request)?;
    let result = read_import_result(project_path, &request.job_id)?;
    let expected: Vec<&str> = request
        .packages
        .iter()
        .map(|entry| entry.operation_id.as_str())
        .collect();
    let completed: Vec<&str> = result
        .completed_operation_ids
        .iter()
        .map(String::as_str)
        .collect();
    if completed.len() > expected.len() || completed != expected[..completed.len()] {
        return Err(AdapterError::ResultMismatch(
            "completed operations are not an ordered prefix of the request".into(),
        ));
    }
    if matches!(result.status, UnityImportStatus::Completed) && completed.len() != expected.len() {
        return Err(AdapterError::ResultMismatch(
            "completed result omits requested operations".into(),
        ));
    }
    Ok(result)
}

/// Resolves the adapter directory without creating or modifying any Unity project.
pub fn adapter_directory(project_path: impl AsRef<Path>) -> Result<PathBuf, AdapterError> {
    Ok(validate_project(project_path.as_ref())?
        .join("Library")
        .join(ADAPTER_DIRECTORY))
}

fn validate_project(path: &Path) -> Result<PathBuf, AdapterError> {
    let canonical = path.canonicalize().map_err(|e| {
        AdapterError::Invalid(format!(
            "Unity project does not exist or cannot be resolved: {e}"
        ))
    })?;
    if !canonical.is_dir() || !canonical.join("Assets").is_dir() {
        return Err(AdapterError::Invalid(
            "Unity project must be an existing directory with an Assets folder".into(),
        ));
    }
    Ok(canonical)
}

fn validate_request(request: &UnityImportRequest) -> Result<(), AdapterError> {
    if request.protocol_version != PROTOCOL_VERSION {
        return Err(AdapterError::Invalid(format!(
            "unsupported protocol_version {}",
            request.protocol_version
        )));
    }
    validate_id(&request.job_id, "job_id")?;
    if request.packages.is_empty() {
        return Err(AdapterError::Invalid(
            "packages must contain at least one unitypackage".into(),
        ));
    }
    let mut operation_ids = std::collections::HashSet::new();
    let mut package_names = std::collections::HashSet::new();
    for package in &request.packages {
        validate_id(&package.operation_id, "operation_id")?;
        if !operation_ids.insert(&package.operation_id) {
            return Err(AdapterError::Invalid(
                "operation_id values must be unique".into(),
            ));
        }
        let path = Path::new(&package.path);
        if !path.is_absolute() {
            return Err(AdapterError::Invalid(
                "unitypackage paths must be absolute".into(),
            ));
        }
        let canonical = path
            .canonicalize()
            .map_err(|e| AdapterError::Invalid(format!("unitypackage path is unavailable: {e}")))?;
        if !canonical.is_file()
            || !canonical
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("unitypackage"))
        {
            return Err(AdapterError::Invalid(
                "each package must be an existing .unitypackage file".into(),
            ));
        }
        if canonical.to_string_lossy() != package.path {
            return Err(AdapterError::Invalid(
                "unitypackage paths must be canonical".into(),
            ));
        }
        let basename = canonical
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| {
                AdapterError::Invalid("unitypackage filename is not valid UTF-8".into())
            })?;
        if !package_names.insert(basename.to_lowercase()) {
            return Err(AdapterError::Invalid(
                "unitypackage filenames must be unique within one request".into(),
            ));
        }
    }
    Ok(())
}

fn validate_result(result: &UnityImportResult) -> Result<(), AdapterError> {
    if result.protocol_version != PROTOCOL_VERSION {
        return Err(AdapterError::ResultMismatch(
            "unsupported protocol_version".into(),
        ));
    }
    validate_id(&result.job_id, "job_id")
        .map_err(|e| AdapterError::ResultMismatch(e.to_string()))?;
    let mut seen = std::collections::HashSet::new();
    for operation_id in &result.completed_operation_ids {
        validate_id(operation_id, "operation_id")
            .map_err(|e| AdapterError::ResultMismatch(e.to_string()))?;
        if !seen.insert(operation_id) {
            return Err(AdapterError::ResultMismatch(
                "completed operation IDs must be unique".into(),
            ));
        }
    }
    if matches!(result.status, UnityImportStatus::Completed) && result.error.is_some() {
        return Err(AdapterError::ResultMismatch(
            "completed result cannot contain an error".into(),
        ));
    }
    if !matches!(result.status, UnityImportStatus::Completed)
        && result.error.as_deref().unwrap_or("").trim().is_empty()
    {
        return Err(AdapterError::ResultMismatch(
            "failed or cancelled result requires an error".into(),
        ));
    }
    Ok(())
}

fn validate_id(value: &str, field: &str) -> Result<(), AdapterError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !valid {
        return Err(AdapterError::Invalid(format!(
            "{field} must be 1–128 ASCII letters, digits, '.', '_' or '-'"
        )));
    }
    Ok(())
}

fn atomic_write(destination: &Path, bytes: &[u8]) -> Result<(), AdapterError> {
    let parent = destination
        .parent()
        .ok_or_else(|| AdapterError::Invalid("manifest has no parent directory".into()))?;
    let name = destination
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("manifest");
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), sequence));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, destination)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(AdapterError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    static TEMP_PROJECT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TempProject(PathBuf);
    impl TempProject {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let sequence = TEMP_PROJECT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "omnooth-unity-{}-{nonce}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("Assets")).unwrap();
            fs::create_dir_all(root.join("Library")).unwrap();
            Self(root)
        }
        fn package(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, b"test").unwrap();
            path.canonicalize().unwrap()
        }
    }
    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn request(project: &TempProject) -> UnityImportRequest {
        UnityImportRequest {
            protocol_version: PROTOCOL_VERSION,
            job_id: "job-123".into(),
            packages: vec![
                UnityPackageImport {
                    operation_id: "op-a".into(),
                    path: project
                        .package("one.unitypackage")
                        .to_string_lossy()
                        .into_owned(),
                },
                UnityPackageImport {
                    operation_id: "op-b".into(),
                    path: project
                        .package("two.unitypackage")
                        .to_string_lossy()
                        .into_owned(),
                },
            ],
        }
    }

    #[test]
    fn request_round_trips_and_existing_pending_job_is_not_overwritten() {
        let project = TempProject::new();
        let request = request(&project);
        let path = write_import_request(&project.0, &request).unwrap();
        assert_eq!(
            serde_json::from_slice::<UnityImportRequest>(&fs::read(&path).unwrap()).unwrap(),
            request
        );
        assert!(matches!(
            write_import_request(&project.0, &request),
            Err(AdapterError::JobInProgress)
        ));
        let terminal = UnityImportResult {
            protocol_version: PROTOCOL_VERSION,
            job_id: request.job_id.clone(),
            status: UnityImportStatus::Completed,
            completed_operation_ids: request
                .packages
                .iter()
                .map(|entry| entry.operation_id.clone())
                .collect(),
            error: None,
        };
        fs::write(
            adapter_directory(&project.0).unwrap().join(RESULT_FILE),
            serde_json::to_vec(&terminal).unwrap(),
        )
        .unwrap();
        assert_eq!(write_import_request(&project.0, &request).unwrap(), path);
        let mut reused_id = request.clone();
        reused_id.packages.reverse();
        assert!(matches!(
            write_import_request(&project.0, &reused_id),
            Err(AdapterError::Invalid(_))
        ));
        let next = UnityImportRequest {
            job_id: "job-456".into(),
            ..request.clone()
        };
        let path = write_import_request(&project.0, &next).unwrap();
        assert_eq!(
            serde_json::from_slice::<UnityImportRequest>(&fs::read(path).unwrap()).unwrap(),
            next
        );
    }

    #[test]
    fn rejects_project_without_assets_and_non_package_paths() {
        let project = TempProject::new();
        let request = request(&project);
        let no_assets = std::env::temp_dir().join("omnooth-not-a-project");
        fs::create_dir_all(&no_assets).unwrap();
        assert!(write_import_request(&no_assets, &request).is_err());
        fs::remove_dir_all(no_assets).unwrap();
        let mut invalid = request.clone();
        invalid.packages[0].path = project
            .package("not-a-package.zip")
            .to_string_lossy()
            .into_owned();
        assert!(write_import_request(&project.0, &invalid).is_err());
        fs::create_dir_all(project.0.join("nested")).unwrap();
        let mut ambiguous = request.clone();
        ambiguous.packages[1].path = project
            .package("nested/ONE.unitypackage")
            .to_string_lossy()
            .into_owned();
        assert!(write_import_request(&project.0, &ambiguous).is_err());
    }

    #[test]
    fn result_is_checked_for_expected_job_and_status_shape() {
        let project = TempProject::new();
        let request = request(&project);
        let dir = adapter_directory(&project.0).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let result = UnityImportResult {
            protocol_version: PROTOCOL_VERSION,
            job_id: "job-123".into(),
            status: UnityImportStatus::Completed,
            completed_operation_ids: vec!["op-a".into(), "op-b".into()],
            error: None,
        };
        fs::write(dir.join(RESULT_FILE), serde_json::to_vec(&result).unwrap()).unwrap();
        assert_eq!(read_import_result(&project.0, "job-123").unwrap(), result);
        assert_eq!(
            read_import_result_for_request(&project.0, &request).unwrap(),
            result
        );
        let out_of_order = UnityImportResult {
            completed_operation_ids: vec!["op-b".into()],
            status: UnityImportStatus::Failed,
            error: Some("failed".into()),
            ..result.clone()
        };
        fs::write(
            dir.join(RESULT_FILE),
            serde_json::to_vec(&out_of_order).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            read_import_result_for_request(&project.0, &request),
            Err(AdapterError::ResultMismatch(_))
        ));
        assert!(matches!(
            read_import_result(&project.0, "another-job"),
            Err(AdapterError::ResultMismatch(_))
        ));
    }
}

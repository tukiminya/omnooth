//! ZIP staging lifecycle for the library import coordinator. This transport does
//! not replace the installation planner or commit a product installation record.

use super::*;
use omnooth_core::{CoreError, PreparedUnityImport, Service};

#[derive(Debug)]
pub enum ZipImportError {
    Core(CoreError),
    Adapter(AdapterError),
    Publication {
        job_id: String,
        source: AdapterError,
    },
}

impl std::fmt::Display for ZipImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core(error) => error.fmt(f),
            Self::Adapter(error) => error.fmt(f),
            Self::Publication { job_id, source } => {
                write!(f, "ZIP import {job_id} retained for recovery: {source}")
            }
        }
    }
}
impl std::error::Error for ZipImportError {}
impl From<CoreError> for ZipImportError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}
impl From<AdapterError> for ZipImportError {
    fn from(error: AdapterError) -> Self {
        Self::Adapter(error)
    }
}

fn request_for(prepared: &PreparedUnityImport) -> UnityImportRequest {
    UnityImportRequest {
        protocol_version: PROTOCOL_VERSION,
        job_id: prepared.job_id().into(),
        packages: prepared
            .packages()
            .iter()
            .map(|package| UnityPackageImport {
                operation_id: package.operation_id.clone(),
                path: package.path.to_string_lossy().into_owned(),
            })
            .collect(),
    }
}

/// Materializes only the explicit selection and publishes its ordered request.
/// The returned job ID can be used after restart to resume checking completion.
/// Publication errors retain staging because Unity may already have seen the request.
pub fn start_zip_import(
    service: &Service,
    project_path: impl AsRef<Path>,
    version_id: &str,
    selected_ids: &[String],
) -> Result<String, ZipImportError> {
    validate_project(project_path.as_ref())?;
    let prepared = service.prepare_zip_unity_import(version_id, selected_ids)?;
    let request = request_for(&prepared);
    if let Err(source) = write_import_request(project_path, &request) {
        return Err(ZipImportError::Publication {
            job_id: prepared.job_id().into(),
            source,
        });
    }
    Ok(prepared.job_id().into())
}

/// Pending, failed, cancelled, malformed and mismatched results retain staging.
/// Only a completed result for every operation in the exact published request
/// permits deletion. The content-addressed ZIP and library extraction are untouched.
pub fn poll_zip_import(
    service: &Service,
    project_path: impl AsRef<Path>,
    job_id: &str,
) -> Result<Option<UnityImportResult>, ZipImportError> {
    let prepared = service.load_prepared_unity_import(job_id)?;
    let request = request_for(&prepared);
    let project = validate_project(project_path.as_ref())?;
    let published: UnityImportRequest = serde_json::from_slice(
        &fs::read(
            project
                .join("Library")
                .join(ADAPTER_DIRECTORY)
                .join(REQUEST_FILE),
        )
        .map_err(AdapterError::Io)?,
    )
    .map_err(AdapterError::Json)?;
    if published != request {
        return Err(
            AdapterError::ResultMismatch("published request differs from staging".into()).into(),
        );
    }
    let result = match read_import_result_for_request(&project, &request) {
        Ok(result) => result,
        Err(AdapterError::ResultNotFound) => return Ok(None),
        // A previous job's validated result can remain until Unity finishes this one.
        Err(AdapterError::ResultMismatch(reason)) if reason == "job_id differs" => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if result.status == UnityImportStatus::Completed {
        service.cleanup_completed_unity_import(job_id)?;
    }
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn staging_survives_pending_failure_cancellation_and_restart_and_is_deleted_only_on_matching_success(
    ) {
        let project = crate::tests::TempProject::new();
        let app = project.0.join("app");
        let service = Service::open(&app).unwrap();
        let source = project.0.join("Bundle.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&source).unwrap());
        for (name, bytes) in [
            ("PC.unitypackage", b"pc".as_slice()),
            ("Quest.unitypackage", b"quest"),
            ("image.png", b"png"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let imported = service
            .import_archive(omnooth_core::ImportArchiveInput {
                archive_path: source,
                original_filename: "Bundle.zip".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap();
        let version = &imported.item.versions[0];
        let entries = service.inspect_archive(&version.id).unwrap().entries;
        let ids: Vec<_> = ["Quest.unitypackage", "PC.unitypackage"]
            .iter()
            .map(|name| {
                entries
                    .iter()
                    .find(|e| &e.relative_path == name)
                    .unwrap()
                    .id
                    .clone()
            })
            .collect();
        let job_id = start_zip_import(&service, &project.0, &version.id, &ids).unwrap();
        let stage = app.join("unity-staging").join(&job_id);
        let manifest_path = project.0.join("Library/Omnooth/request.json");
        let request: UnityImportRequest =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        assert_eq!(request.packages[0].operation_id, ids[0]);
        assert_eq!(request.packages[1].operation_id, ids[1]);
        assert!(request
            .packages
            .iter()
            .all(|p| Path::new(&p.path).is_file()));
        assert_eq!(
            poll_zip_import(&service, &project.0, &job_id).unwrap(),
            None
        );
        assert!(stage.is_dir());
        let service = Service::open(&app).unwrap(); // persisted job, no in-memory handle needed
        let result_path = project.0.join("Library/Omnooth/result.json");
        let mut result = UnityImportResult {
            protocol_version: PROTOCOL_VERSION,
            job_id: job_id.clone(),
            status: UnityImportStatus::Failed,
            completed_operation_ids: vec![ids[0].clone()],
            error: Some("failed".into()),
        };
        for status in [UnityImportStatus::Failed, UnityImportStatus::Cancelled] {
            result.status = status;
            fs::write(&result_path, serde_json::to_vec(&result).unwrap()).unwrap();
            assert_eq!(
                poll_zip_import(&service, &project.0, &job_id).unwrap(),
                Some(result.clone())
            );
            assert!(stage.is_dir());
        }
        result.status = UnityImportStatus::Completed;
        result.error = None;
        fs::write(&result_path, serde_json::to_vec(&result).unwrap()).unwrap();
        assert!(poll_zip_import(&service, &project.0, &job_id).is_err()); // incomplete success
        assert!(stage.is_dir());
        result.completed_operation_ids = ids.clone();
        result.job_id = "another-job".into();
        fs::write(&result_path, serde_json::to_vec(&result).unwrap()).unwrap();
        assert_eq!(
            poll_zip_import(&service, &project.0, &job_id).unwrap(),
            None
        );
        assert!(stage.is_dir());
        result.job_id = job_id.clone();
        fs::write(&result_path, serde_json::to_vec(&result).unwrap()).unwrap();
        let mut different_request = request.clone();
        different_request.packages.reverse();
        fs::write(
            &manifest_path,
            serde_json::to_vec(&different_request).unwrap(),
        )
        .unwrap();
        assert!(poll_zip_import(&service, &project.0, &job_id).is_err());
        assert!(stage.is_dir());
        fs::write(&manifest_path, serde_json::to_vec(&request).unwrap()).unwrap();
        assert_eq!(
            poll_zip_import(&service, &project.0, &job_id).unwrap(),
            Some(result)
        );
        assert!(!stage.exists());
        assert!(app
            .join("Library/objects")
            .join(&version.content_hash)
            .is_file());
        assert_eq!(
            service.snapshot().unwrap().items[0].versions[0].state,
            "stored"
        );
        // Cleaning old inputs must not make the completed request look pending forever.
        let next_job = start_zip_import(&service, &project.0, &version.id, &ids).unwrap();
        assert_ne!(next_job, job_id);
        assert_eq!(
            poll_zip_import(&service, &project.0, &next_job).unwrap(),
            None
        );
    }
}

use super::*;
use omnooth_core::{CoreError, CoreResult, UnityGateway, UnityImportFile, UnityOutcome};
use std::time::{SystemTime, UNIX_EPOCH};

/// Application adapter; all planning, journalling and commits remain in omnooth-core.
pub struct EditorGateway;

#[derive(Deserialize)]
struct Status {
    protocol_version: u32,
    updated_at: u64,
    is_busy: bool,
}

fn error(error: AdapterError) -> CoreError {
    CoreError::Invalid(match error {
        AdapterError::JobInProgress => "UNITY_BUSY",
        AdapterError::ResultMismatch(_) | AdapterError::Json(_) => "UNITY_RESULT_MISMATCH",
        _ => "UNITY_ADAPTER_ERROR",
    })
}
fn request(job_id: &str, packages: &[UnityImportFile]) -> UnityImportRequest {
    UnityImportRequest {
        protocol_version: PROTOCOL_VERSION,
        job_id: job_id.into(),
        packages: packages
            .iter()
            .map(|p| UnityPackageImport {
                operation_id: p.operation_id.clone(),
                path: p.path.to_string_lossy().into_owned(),
            })
            .collect(),
    }
}
impl UnityGateway for EditorGateway {
    fn setup(&self, project: &Path) -> CoreResult<()> {
        let project = validate_project(project).map_err(error)?;
        let packages = project.join("Packages");
        let metadata = fs::symlink_metadata(&packages)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CoreError::Invalid("PROJECT_UNSAFE_PATH"));
        }
        let destination = packages.join("com.omnooth.editor");
        let contents = [
            (
                "package.json",
                include_str!("../../../unity/com.omnooth.editor/package.json"),
            ),
            (
                "Editor/Omnooth.Editor.asmdef",
                include_str!("../../../unity/com.omnooth.editor/Editor/Omnooth.Editor.asmdef"),
            ),
            (
                "Editor/OmnoothImportBridge.cs",
                include_str!("../../../unity/com.omnooth.editor/Editor/OmnoothImportBridge.cs"),
            ),
        ];
        if destination.exists() {
            for (path, bytes) in &contents {
                let target = destination.join(path);
                if fs::symlink_metadata(&destination)?.file_type().is_symlink()
                    || fs::canonicalize(&target)
                        .ok()
                        .is_none_or(|p| !p.starts_with(&destination))
                    || fs::read(&target).ok().as_deref() != Some(bytes.as_bytes())
                {
                    return Err(CoreError::Invalid("UNITY_ADAPTER_ALREADY_EXISTS"));
                }
            }
            return Ok(());
        }
        let temp = packages.join(format!(
            ".omnooth-{}",
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&temp)?;
        let result = (|| {
            fs::create_dir(temp.join("Editor"))?;
            for (path, bytes) in contents {
                let mut file = fs::File::create(temp.join(path))?;
                file.write_all(bytes.as_bytes())?;
                file.sync_all()?;
            }
            fs::rename(&temp, &destination)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(temp);
        }
        result
    }
    fn ensure_ready(&self, project: &Path) -> CoreResult<()> {
        let root = adapter_directory(project).map_err(error)?;
        let status: Status = fs::read(root.join("status.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .ok_or(CoreError::Invalid("UNITY_UNAVAILABLE"))?;
        let current = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CoreError::Invalid("UNITY_UNAVAILABLE"))?
            .as_secs();
        if status.protocol_version != PROTOCOL_VERSION
            || status.updated_at > current + 2
            || current.saturating_sub(status.updated_at) > 5
        {
            return Err(CoreError::Invalid("UNITY_UNAVAILABLE"));
        }
        if status.is_busy {
            return Err(CoreError::Invalid("UNITY_BUSY"));
        }
        if root.join(REQUEST_FILE).exists() {
            let request: UnityImportRequest =
                serde_json::from_slice(&fs::read(root.join(REQUEST_FILE))?)
                    .map_err(|_| CoreError::Invalid("UNITY_RESULT_MISMATCH"))?;
            read_import_result_for_request(project, &request)
                .map_err(|_| CoreError::Invalid("UNITY_BUSY"))?;
        }
        Ok(())
    }
    fn publish(
        &self,
        project: &Path,
        job_id: &str,
        packages: &[UnityImportFile],
    ) -> CoreResult<()> {
        write_import_request(project, &request(job_id, packages)).map_err(error)?;
        Ok(())
    }
    fn poll(
        &self,
        project: &Path,
        job_id: &str,
        packages: &[UnityImportFile],
    ) -> CoreResult<UnityOutcome> {
        let root = adapter_directory(project).map_err(error)?;
        let expected = request(job_id, packages);
        let published: UnityImportRequest = match fs::read(root.join(REQUEST_FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| CoreError::Invalid("UNITY_RESULT_MISMATCH"))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.ensure_ready(project)?;
                return Ok(UnityOutcome::Failed);
            }
            Err(e) => return Err(e.into()),
        };
        if published != expected {
            // No matching request was published (e.g. preparation failed). A
            // terminal previous job and fresh idle heartbeat prove recovery safe.
            if published.job_id != job_id {
                self.ensure_ready(project)?;
                return Ok(UnityOutcome::Failed);
            }
            return Err(CoreError::Invalid("UNITY_RESULT_MISMATCH"));
        }
        match read_import_result_for_request(project, &expected) {
            Ok(result) => Ok(match result.status {
                UnityImportStatus::Completed => UnityOutcome::Completed,
                UnityImportStatus::Failed => UnityOutcome::Failed,
                UnityImportStatus::Cancelled => UnityOutcome::Cancelled,
            }),
            Err(AdapterError::ResultNotFound) => Ok(UnityOutcome::Pending),
            Err(AdapterError::ResultMismatch(_))
                if fs::read(root.join(RESULT_FILE))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<UnityImportResult>(&b).ok())
                    .is_some_and(|r| r.job_id != job_id) =>
            {
                Ok(UnityOutcome::Pending)
            }
            Err(e) => Err(error(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::TempProject;
    fn heartbeat(project: &TempProject, age: u64, busy: bool) {
        let root = adapter_directory(&project.0).unwrap();
        fs::create_dir_all(&root).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        fs::write(
            root.join("status.json"),
            serde_json::to_vec(
                &serde_json::json!({"protocol_version":1,"updated_at":now-age,"is_busy":busy}),
            )
            .unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn availability_requires_fresh_idle_editor_and_no_unfinished_request() {
        let project = TempProject::new();
        let gateway = EditorGateway;
        assert_eq!(
            gateway.ensure_ready(&project.0).unwrap_err().code(),
            "UNITY_UNAVAILABLE"
        );
        heartbeat(&project, 30, false);
        assert_eq!(
            gateway.ensure_ready(&project.0).unwrap_err().code(),
            "UNITY_UNAVAILABLE"
        );
        heartbeat(&project, 0, true);
        assert_eq!(
            gateway.ensure_ready(&project.0).unwrap_err().code(),
            "UNITY_BUSY"
        );
        heartbeat(&project, 0, false);
        gateway.ensure_ready(&project.0).unwrap();
        gateway
            .publish(&project.0, "empty-refresh-job", &[])
            .unwrap();
        assert_eq!(
            gateway.ensure_ready(&project.0).unwrap_err().code(),
            "UNITY_BUSY"
        );
        assert_eq!(
            gateway.poll(&project.0, "empty-refresh-job", &[]).unwrap(),
            UnityOutcome::Pending
        );
        let result = UnityImportResult {
            protocol_version: 1,
            job_id: "empty-refresh-job".into(),
            status: UnityImportStatus::Completed,
            completed_operation_ids: vec![],
            error: None,
        };
        fs::write(
            adapter_directory(&project.0).unwrap().join(RESULT_FILE),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
        gateway.ensure_ready(&project.0).unwrap();
        assert_eq!(
            gateway.poll(&project.0, "empty-refresh-job", &[]).unwrap(),
            UnityOutcome::Completed
        );
    }
    #[test]
    fn stale_result_is_pending_but_mismatched_operation_identity_is_rejected() {
        let project = TempProject::new();
        let gateway = EditorGateway;
        heartbeat(&project, 0, false);
        let path = project.0.join("Input.unitypackage");
        fs::write(&path, b"fixture").unwrap();
        let files = vec![UnityImportFile {
            operation_id: "operation".into(),
            path: path.canonicalize().unwrap(),
        }];
        gateway.publish(&project.0, "new-job", &files).unwrap();
        let mut result = UnityImportResult {
            protocol_version: 1,
            job_id: "old-job".into(),
            status: UnityImportStatus::Completed,
            completed_operation_ids: vec![],
            error: None,
        };
        let root = adapter_directory(&project.0).unwrap();
        fs::write(root.join(RESULT_FILE), serde_json::to_vec(&result).unwrap()).unwrap();
        assert_eq!(
            gateway.poll(&project.0, "new-job", &files).unwrap(),
            UnityOutcome::Pending
        );
        result.job_id = "new-job".into();
        result.completed_operation_ids = vec!["other-operation".into()];
        fs::write(root.join(RESULT_FILE), serde_json::to_vec(&result).unwrap()).unwrap();
        assert_eq!(
            gateway
                .poll(&project.0, "new-job", &files)
                .unwrap_err()
                .code(),
            "UNITY_RESULT_MISMATCH"
        );
    }
    #[test]
    fn setup_embeds_adapter_and_preserves_existing_different_code() {
        let project = TempProject::new();
        fs::create_dir(project.0.join("Packages")).unwrap();
        let gateway = EditorGateway;
        gateway.setup(&project.0).unwrap();
        gateway.setup(&project.0).unwrap();
        let path = project
            .0
            .join("Packages/com.omnooth.editor/Editor/OmnoothImportBridge.cs");
        fs::write(&path, b"custom user adapter").unwrap();
        assert_eq!(
            gateway.setup(&project.0).unwrap_err().code(),
            "UNITY_ADAPTER_ALREADY_EXISTS"
        );
        assert_eq!(fs::read(&path).unwrap(), b"custom user adapter");
    }
    #[cfg(unix)]
    #[test]
    fn linked_transport_directories_are_rejected_before_writing() {
        let project = TempProject::new();
        let other = TempProject::new();
        std::os::unix::fs::symlink(&other.0, project.0.join("Library/Omnooth")).unwrap();
        assert!(EditorGateway.publish(&project.0, "job", &[]).is_err());
        assert!(!other.0.join(REQUEST_FILE).exists());
    }
}

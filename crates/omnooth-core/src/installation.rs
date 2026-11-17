//! Confirmed plans, durable file journals and ownership for Unity installations.
use super::*;
use crate::unitypackage::{asset_path, meta_guid};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub trait UnityGateway {
    fn setup(&self, _project: &Path) -> CoreResult<()> {
        Err(CoreError::Invalid("UNITY_UNAVAILABLE"))
    }
    fn ensure_ready(&self, project: &Path) -> CoreResult<()>;
    fn publish(&self, project: &Path, job_id: &str, packages: &[UnityImportFile])
        -> CoreResult<()>;
    fn poll(
        &self,
        project: &Path,
        job_id: &str,
        packages: &[UnityImportFile],
    ) -> CoreResult<UnityOutcome>;
}

pub struct DisconnectedUnity;
impl UnityGateway for DisconnectedUnity {
    fn ensure_ready(&self, _: &Path) -> CoreResult<()> {
        Err(CoreError::Invalid("UNITY_UNAVAILABLE"))
    }
    fn publish(&self, _: &Path, _: &str, _: &[UnityImportFile]) -> CoreResult<()> {
        Err(CoreError::Invalid("UNITY_UNAVAILABLE"))
    }
    fn poll(&self, _: &Path, _: &str, _: &[UnityImportFile]) -> CoreResult<UnityOutcome> {
        Err(CoreError::Invalid("UNITY_UNAVAILABLE"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnityImportFile {
    pub operation_id: String,
    pub path: PathBuf,
}
#[derive(Debug, PartialEq, Eq)]
pub enum UnityOutcome {
    Pending,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallSelection {
    pub version_id: String,
    pub file_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInstallPlanInput {
    pub project_id: String,
    pub selections: Vec<InstallSelection>,
    #[serde(default)]
    pub remove_package_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallItemInput {
    pub project_id: String,
    pub plan_id: String,
    pub plan_hash: String,
    pub idempotency_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOperation {
    pub package_id: String,
    pub title: String,
    pub kind: String,
    pub from_version_id: Option<String>,
    pub to_version_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallAsset {
    pub source_file_id: String,
    pub package_id: String,
    pub version_id: String,
    pub relative_path: String,
    pub asset_guid: String,
    /// None identifies a directory; only files and .meta files enter the ownership ledger.
    pub content_hash: Option<String>,
    pub owned: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub id: String,
    pub project_id: String,
    pub base_state_hash: String,
    pub plan_hash: String,
    pub selections: Vec<InstallSelection>,
    pub operations: Vec<InstallOperation>,
    pub asset_manifest: Vec<InstallAsset>,
    pub removed_paths: Vec<String>,
    pub import_order: Vec<String>,
    pub required_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationJob {
    pub id: String,
    pub project_id: String,
    pub status: String,
    pub error_code: Option<String>,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPackage {
    pub package_id: String,
    pub title: String,
    pub version_id: String,
    pub version_label: String,
    pub file_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationState {
    pub packages: Vec<InstalledPackage>,
    pub jobs: Vec<InstallationJob>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryMode {
    Inspect,
    Restore,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    pub restored_paths: Vec<String>,
    pub conflicts: Vec<String>,
    pub requires_manual_action: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StagedPackage {
    file: UnityImportFile,
    hash: String,
    source_file_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPlan {
    plan: InstallPlan,
    project_path: PathBuf,
    packages: Vec<StagedPackage>,
    affected: Vec<String>,
    desired: BTreeMap<String, InstallSelection>,
    /// Original object hashes are checked again just before mutation.
    originals: BTreeMap<PathBuf, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupEntry {
    path: String,
    before: Option<String>,
    after: Option<String>,
    backup: Option<PathBuf>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Journal {
    plan: StoredPlan,
    entries: Vec<BackupEntry>,
}
#[derive(Debug, Clone, Serialize)]
struct DiskState {
    files: BTreeMap<String, String>,
    directories: BTreeSet<String>,
    guids: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize)]
struct OwnedFile {
    path: String,
    hash: String,
    package_id: String,
    version_id: String,
    source_file_id: String,
    guid: String,
}

pub(crate) fn migrate(conn: &mut Connection) -> CoreResult<()> {
    let tx = conn.transaction()?;
    tx.execute_batch("
      CREATE UNIQUE INDEX version_package_identity ON package_versions(package_id,id);
      CREATE UNIQUE INDEX file_version_identity ON package_files(version_id,id);
      CREATE TABLE package_dependencies(version_id TEXT NOT NULL REFERENCES package_versions(id),required_version_id TEXT NOT NULL REFERENCES package_versions(id),PRIMARY KEY(version_id,required_version_id),CHECK(version_id<>required_version_id));
      CREATE TABLE install_plans(id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id),plan_hash TEXT NOT NULL,payload TEXT NOT NULL,created_at TEXT NOT NULL);
      CREATE TABLE install_jobs(id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id),plan_id TEXT NOT NULL REFERENCES install_plans(id),idempotency_key TEXT NOT NULL UNIQUE,status TEXT NOT NULL CHECK(status IN ('preparing','waitingForUnity','recovering','succeeded','restored')),error_code TEXT,journal TEXT NOT NULL,created_at TEXT NOT NULL);
      CREATE UNIQUE INDEX one_install_per_project ON install_jobs(project_id) WHERE status IN ('preparing','waitingForUnity','recovering');
      CREATE TABLE project_packages(project_id TEXT NOT NULL REFERENCES projects(id),package_id TEXT NOT NULL,version_id TEXT NOT NULL,job_id TEXT NOT NULL REFERENCES install_jobs(id),PRIMARY KEY(project_id,package_id),UNIQUE(project_id,package_id,version_id),FOREIGN KEY(package_id,version_id) REFERENCES package_versions(package_id,id) DEFERRABLE INITIALLY DEFERRED);
      CREATE TABLE project_selections(project_id TEXT NOT NULL,package_id TEXT NOT NULL,version_id TEXT NOT NULL,source_file_id TEXT NOT NULL,apply_order INTEGER NOT NULL CHECK(apply_order>=0),PRIMARY KEY(project_id,source_file_id),UNIQUE(project_id,package_id,apply_order),UNIQUE(project_id,package_id,version_id,source_file_id),FOREIGN KEY(project_id,package_id,version_id) REFERENCES project_packages(project_id,package_id,version_id) DEFERRABLE INITIALLY DEFERRED,FOREIGN KEY(version_id,source_file_id) REFERENCES package_files(version_id,id) DEFERRABLE INITIALLY DEFERRED);
      CREATE TABLE project_files(project_id TEXT NOT NULL,relative_path TEXT NOT NULL,source_file_id TEXT NOT NULL,package_id TEXT NOT NULL,version_id TEXT NOT NULL,installed_hash TEXT NOT NULL CHECK(length(installed_hash)=64),asset_guid TEXT NOT NULL,job_id TEXT NOT NULL REFERENCES install_jobs(id),PRIMARY KEY(project_id,relative_path),FOREIGN KEY(project_id,package_id,version_id,source_file_id) REFERENCES project_selections(project_id,package_id,version_id,source_file_id) DEFERRABLE INITIALLY DEFERRED,FOREIGN KEY(project_id,package_id,version_id) REFERENCES project_packages(project_id,package_id,version_id) DEFERRABLE INITIALLY DEFERRED,FOREIGN KEY(version_id,source_file_id) REFERENCES package_files(version_id,id) DEFERRABLE INITIALLY DEFERRED);
    ")?;
    tx.pragma_update(None, "user_version", 5)?;
    tx.commit()?;
    Ok(())
}

fn digest<T: Serialize>(value: &T) -> CoreResult<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(value).map_err(|_| CoreError::Invalid("SERIALIZATION_ERROR"))?
        )
    ))
}
fn json<T: Serialize>(value: &T) -> CoreResult<String> {
    serde_json::to_string(value).map_err(|_| CoreError::Invalid("SERIALIZATION_ERROR"))
}
fn decode<T: for<'a> Deserialize<'a>>(value: &str) -> CoreResult<T> {
    serde_json::from_str(value).map_err(|_| CoreError::Invalid("INSTALLATION_RECORD_INVALID"))
}
fn no_busy(conn: &Connection, project_id: &str) -> CoreResult<()> {
    if conn.query_row("SELECT EXISTS(SELECT 1 FROM install_jobs WHERE project_id=?1 AND status IN ('preparing','waitingForUnity','recovering'))", [project_id], |r| r.get::<_,bool>(0))? { return Err(CoreError::Invalid("PROJECT_BUSY")); }
    Ok(())
}
fn desired_state(
    conn: &Connection,
    project_id: &str,
) -> CoreResult<BTreeMap<String, InstallSelection>> {
    let mut stmt = conn.prepare("SELECT package_id,version_id FROM project_packages WHERE project_id=?1 ORDER BY package_id")?;
    let rows = stmt
        .query_map([project_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut result = BTreeMap::new();
    for (package, version_id) in rows {
        let mut files = conn.prepare("SELECT source_file_id FROM project_selections WHERE project_id=?1 AND package_id=?2 ORDER BY apply_order")?;
        let file_ids = files
            .query_map(params![project_id, package], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        result.insert(
            package,
            InstallSelection {
                version_id,
                file_ids,
            },
        );
    }
    Ok(result)
}
fn owned_files(conn: &Connection, project_id: &str) -> CoreResult<Vec<OwnedFile>> {
    let mut stmt = conn.prepare("SELECT relative_path,installed_hash,package_id,version_id,source_file_id,asset_guid FROM project_files WHERE project_id=?1 ORDER BY relative_path")?;
    let result = stmt
        .query_map([project_id], |r| {
            Ok(OwnedFile {
                path: r.get(0)?,
                hash: r.get(1)?,
                package_id: r.get(2)?,
                version_id: r.get(3)?,
                source_file_id: r.get(4)?,
                guid: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(result)
}
fn scan_project(project: &Path) -> CoreResult<DiskState> {
    fn walk(
        project: &Path,
        directory: &Path,
        state: &mut DiskState,
        aliases: &mut HashSet<String>,
    ) -> CoreResult<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let ty = entry.file_type()?;
            if ty.is_symlink() || (!ty.is_file() && !ty.is_dir()) {
                return Err(CoreError::Invalid("PROJECT_UNSAFE_PATH"));
            }
            let relative = path
                .strip_prefix(project)
                .unwrap()
                .to_str()
                .ok_or(CoreError::Invalid("PROJECT_UNSAFE_PATH"))?
                .replace('\\', "/");
            // Validate .meta paths through their asset name.
            asset_path(relative.strip_suffix(".meta").unwrap_or(&relative))?;
            if !aliases.insert(relative.to_lowercase()) {
                return Err(CoreError::Invalid("PROJECT_PATH_COLLISION"));
            }
            if state.files.len() + state.directories.len() >= MAX_ARCHIVE_ENTRIES {
                return Err(CoreError::Invalid("PROJECT_TOO_LARGE"));
            }
            if ty.is_dir() {
                state.directories.insert(relative);
                walk(project, &path, state, aliases)?;
            } else {
                state.files.insert(relative.clone(), hash_file(&path)?);
                if relative.ends_with(".meta") {
                    if entry.metadata()?.len() > 16 * 1024 * 1024 {
                        return Err(CoreError::Invalid("INVALID_ASSET_META"));
                    }
                    let guid = meta_guid(&fs::read(&path)?)?;
                    if state
                        .guids
                        .insert(guid, relative.trim_end_matches(".meta").into())
                        .is_some()
                    {
                        return Err(CoreError::Invalid("PROJECT_GUID_COLLISION"));
                    }
                }
            }
        }
        Ok(())
    }
    validate_unity_project(project)?;
    if fs::symlink_metadata(project.join("Assets"))?
        .file_type()
        .is_symlink()
    {
        return Err(CoreError::Invalid("PROJECT_UNSAFE_PATH"));
    }
    let mut state = DiskState {
        files: BTreeMap::new(),
        directories: BTreeSet::from(["Assets".into()]),
        guids: BTreeMap::new(),
    };
    walk(
        project,
        &project.join("Assets"),
        &mut state,
        &mut HashSet::new(),
    )?;
    Ok(state)
}
fn base_hash(
    conn: &Connection,
    project_id: &str,
    project: &Path,
    disk: &DiskState,
) -> CoreResult<String> {
    let (_, version) = validate_unity_project(project)?;
    let mut deps = conn.prepare("SELECT version_id,required_version_id FROM package_dependencies ORDER BY version_id,required_version_id")?;
    let dependencies = deps
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    digest(&(
        version,
        disk,
        desired_state(conn, project_id)?,
        owned_files(conn, project_id)?,
        dependencies,
    ))
}
fn dependency_order(
    conn: &Connection,
    desired: &BTreeMap<String, InstallSelection>,
) -> CoreResult<Vec<String>> {
    fn visit(
        conn: &Connection,
        package: &str,
        desired: &BTreeMap<String, InstallSelection>,
        active: &mut HashSet<String>,
        done: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) -> CoreResult<()> {
        if done.contains(package) {
            return Ok(());
        }
        if !active.insert(package.into()) {
            return Err(CoreError::Invalid("DEPENDENCY_CYCLE"));
        }
        let mut stmt = conn.prepare("SELECT d.required_version_id,v.package_id FROM package_dependencies d JOIN package_versions v ON v.id=d.required_version_id WHERE d.version_id=?1 ORDER BY v.package_id")?;
        let deps = stmt
            .query_map([&desired[package].version_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (version, dependency) in deps {
            if desired
                .get(&dependency)
                .is_none_or(|s| s.version_id != version || s.file_ids.is_empty())
            {
                return Err(CoreError::Invalid("DEPENDENCY_SELECTION_REQUIRED"));
            }
            visit(conn, &dependency, desired, active, done, order)?;
        }
        active.remove(package);
        done.insert(package.into());
        order.push(package.into());
        Ok(())
    }
    let mut order = Vec::new();
    let mut active = HashSet::new();
    let mut done = HashSet::new();
    for package in desired.keys() {
        visit(conn, package, desired, &mut active, &mut done, &mut order)?;
    }
    Ok(order)
}

impl Service {
    pub fn pending_installation_jobs(&self) -> CoreResult<Vec<InstallationJob>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT id,project_id,status,error_code,created_at FROM install_jobs WHERE status='waitingForUnity' ORDER BY created_at")?;
        let jobs = stmt
            .query_map([], job_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }
    pub fn setup_unity_adapter(
        &self,
        project_id: &str,
        unity: &dyn UnityGateway,
    ) -> CoreResult<()> {
        let conn = self.connect()?;
        let _lock = self.installation_lock(project_id)?;
        no_busy(&conn, project_id)?;
        let project = self.installation_project(&conn, project_id)?;
        unity.setup(&project)
    }
    fn installation_lock(&self, project_id: &str) -> CoreResult<File> {
        let root = self.app_data_dir.join("installation-locks");
        fs::create_dir_all(&root)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(digest(&project_id)?))?;
        file.try_lock()
            .map_err(|_| CoreError::Invalid("PROJECT_BUSY"))?;
        Ok(file)
    }
    fn installation_project(&self, conn: &Connection, project_id: &str) -> CoreResult<PathBuf> {
        let path: String = conn
            .query_row(
                "SELECT canonical_path FROM projects WHERE id=?1",
                [project_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(CoreError::Invalid("PROJECT_NOT_FOUND"))?;
        let project = PathBuf::from(path);
        if fs::canonicalize(&project)? != project {
            return Err(CoreError::Invalid("PROJECT_INVALID"));
        }
        validate_unity_project(&project)?;
        Ok(project)
    }
    fn stage_selection(
        &self,
        conn: &Connection,
        selection: &InstallSelection,
        root: &Path,
        originals: &mut BTreeMap<PathBuf, String>,
    ) -> CoreResult<Vec<StagedPackage>> {
        let (filename, object, expected): (String,String,String) = conn.query_row("SELECT original_filename,object_path,content_hash FROM package_versions WHERE id=?1", [&selection.version_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(CoreError::Invalid("VERSION_NOT_FOUND"))?;
        if hash_file(Path::new(&object))? != expected {
            return Err(CoreError::Invalid("OBJECT_STORE_CORRUPT"));
        }
        originals.insert(PathBuf::from(&object), expected.clone());
        let mut result = Vec::new();
        if filename.to_ascii_lowercase().ends_with(".zip") {
            let mut archive = ZipArchive::new(File::open(&object)?)
                .map_err(|e| CoreError::Archive(e.to_string()))?;
            let entries =
                super::archive_contents::zip_entries(&mut archive, &selection.version_id)?;
            let mut total = 0;
            for id in &selection.file_ids {
                let stored_path: Option<String> = conn
                    .query_row(
                        "SELECT relative_path FROM package_files WHERE version_id=?1 AND id=?2",
                        params![selection.version_id, id],
                        |r| r.get(0),
                    )
                    .optional()?;
                let entry = entries
                    .iter()
                    .find(|e| {
                        e.entry.id == *id || stored_path.as_ref() == Some(&e.entry.relative_path)
                    })
                    .ok_or(CoreError::Invalid("INVALID_CONTENT_SELECTION"))?;
                if entry.entry.kind != "unitypackage" || entry.entry.encrypted {
                    return Err(CoreError::Invalid("INVALID_CONTENT_SELECTION"));
                }
                let target = root.join(format!("{}.unitypackage", Uuid::new_v4()));
                let mut output = File::create(&target)?;
                let size = copy_bounded(
                    &mut archive
                        .by_index(entry.index)
                        .map_err(|e| CoreError::Archive(e.to_string()))?,
                    &mut output,
                    MAX_EXPANDED_BYTES - total,
                )?;
                total += size;
                output.sync_all()?;
                if size != entry.entry.size_bytes {
                    return Err(CoreError::Invalid("ARCHIVE_SIZE_MISMATCH"));
                }
                let hash = hash_file(&target)?;
                let stored_hash: Option<String> = conn
                    .query_row(
                        "SELECT content_hash FROM package_files WHERE version_id=?1 AND id=?2",
                        params![selection.version_id, id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if stored_hash.is_some_and(|h| h != hash) {
                    return Err(CoreError::Invalid("OBJECT_STORE_CORRUPT"));
                }
                conn.execute("INSERT OR IGNORE INTO package_files(id,version_id,relative_path,kind,size_bytes,content_hash) VALUES(?1,?2,?3,'unitypackage',?4,?5)", params![id,selection.version_id,entry.entry.relative_path,size as i64,hash])?;
                result.push(StagedPackage {
                    file: UnityImportFile {
                        operation_id: Uuid::new_v4().to_string(),
                        path: fs::canonicalize(target)?,
                    },
                    hash,
                    source_file_id: id.clone(),
                });
            }
        } else if filename.to_ascii_lowercase().ends_with(".unitypackage") {
            if selection.file_ids.len() != 1 {
                return Err(CoreError::Invalid("INVALID_CONTENT_SELECTION"));
            }
            let valid: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM package_files WHERE version_id=?1 AND id=?2 AND kind='unitypackage' AND content_hash=?3)", params![selection.version_id,selection.file_ids[0],expected], |r| r.get(0))?;
            if !valid {
                return Err(CoreError::Invalid("INVALID_CONTENT_SELECTION"));
            }
            let target = root.join(format!("{}.unitypackage", Uuid::new_v4()));
            atomic_copy(Path::new(&object), &target, &expected)?;
            result.push(StagedPackage {
                file: UnityImportFile {
                    operation_id: Uuid::new_v4().to_string(),
                    path: fs::canonicalize(target)?,
                },
                hash: expected,
                source_file_id: selection.file_ids[0].clone(),
            });
        } else if filename == "custom.manifest.json" {
            let extracted: String = conn.query_row(
                "SELECT extracted_path FROM package_versions WHERE id=?1",
                [&selection.version_id],
                |r| r.get(0),
            )?;
            let extracted = PathBuf::from(extracted);
            if fs::symlink_metadata(&extracted)?.file_type().is_symlink() {
                return Err(CoreError::Invalid("OBJECT_STORE_CORRUPT"));
            }
            for id in &selection.file_ids {
                let (relative,hash): (String,String) = conn.query_row("SELECT relative_path,content_hash FROM package_files WHERE version_id=?1 AND id=?2 AND kind='unitypackage'",params![selection.version_id,id],|r|Ok((r.get(0)?,r.get(1)?)))?;
                let source = extracted.join(safe_member_path(&relative)?);
                if fs::symlink_metadata(&source)?.file_type().is_symlink() {
                    return Err(CoreError::Invalid("OBJECT_STORE_CORRUPT"));
                }
                let target = root.join(format!("{}.unitypackage", Uuid::new_v4()));
                atomic_copy(&source, &target, &hash)?;
                originals.insert(source, hash.clone());
                result.push(StagedPackage {
                    file: UnityImportFile {
                        operation_id: Uuid::new_v4().to_string(),
                        path: fs::canonicalize(target)?,
                    },
                    hash,
                    source_file_id: id.clone(),
                });
            }
        } else {
            return Err(CoreError::Invalid("EXTRACTION_UNSUPPORTED"));
        }
        Ok(result)
    }

    pub fn build_install_plan(&self, input: BuildInstallPlanInput) -> CoreResult<InstallPlan> {
        for selection in &input.selections {
            self.require_version_visible(&selection.version_id)?;
        }
        for id in &input.remove_package_ids {
            self.require_item_visible(id)?;
        }
        let conn = self.connect()?;
        let _lock = self.installation_lock(&input.project_id)?;
        no_busy(&conn, &input.project_id)?;
        let project = self.installation_project(&conn, &input.project_id)?;
        let disk = scan_project(&project)?;
        let current = desired_state(&conn, &input.project_id)?;
        let owned = owned_files(&conn, &input.project_id)?;
        let mut desired = current.clone();
        let mut affected = BTreeSet::new();
        let mut operations = Vec::new();
        for selection in &input.selections {
            if selection.file_ids.is_empty()
                || selection.file_ids.iter().collect::<HashSet<_>>().len()
                    != selection.file_ids.len()
            {
                return Err(CoreError::Invalid("INVALID_CONTENT_SELECTION"));
            }
            let (package_id,title): (String,String) = conn.query_row("SELECT p.id,p.title FROM packages p JOIN package_versions v ON v.package_id=p.id WHERE v.id=?1", [&selection.version_id], |r| Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(CoreError::Invalid("VERSION_NOT_FOUND"))?;
            if !affected.insert(package_id.clone()) {
                return Err(CoreError::Invalid("DUPLICATE_PACKAGE_SELECTION"));
            }
            if current.get(&package_id) != Some(selection) {
                operations.push(InstallOperation {
                    package_id: package_id.clone(),
                    title,
                    kind: match current.get(&package_id) {
                        None => "add",
                        Some(old) if old.version_id == selection.version_id => "reselect",
                        _ => "update",
                    }
                    .into(),
                    from_version_id: current.get(&package_id).map(|s| s.version_id.clone()),
                    to_version_id: Some(selection.version_id.clone()),
                });
            }
            desired.insert(package_id, selection.clone());
        }
        for package in &input.remove_package_ids {
            if !affected.insert(package.clone()) {
                return Err(CoreError::Invalid("DUPLICATE_PACKAGE_SELECTION"));
            }
            let old = desired
                .remove(package)
                .ok_or(CoreError::Invalid("PACKAGE_NOT_INSTALLED"))?;
            let title =
                conn.query_row("SELECT title FROM packages WHERE id=?1", [package], |r| {
                    r.get(0)
                })?;
            operations.push(InstallOperation {
                package_id: package.clone(),
                title,
                kind: "remove".into(),
                from_version_id: Some(old.version_id),
                to_version_id: None,
            });
        }
        if operations.is_empty() {
            return Err(CoreError::Invalid("NO_INSTALL_CHANGES"));
        }
        // Same selection for an unaffected package must not silently re-import it.
        affected = operations.iter().map(|o| o.package_id.clone()).collect();
        for id in desired.keys() {
            self.require_item_visible(id)?;
        }
        let order = dependency_order(&conn, &desired)?;
        let id = Uuid::new_v4().to_string();
        let root = self.app_data_dir.join("install-staging").join(&id);
        fs::create_dir_all(&root)?;
        let result = (|| {
            let mut packages = Vec::new();
            let mut assets = BTreeMap::<String, InstallAsset>::new();
            let mut originals = BTreeMap::new();
            let mut bytes = 0u64;
            let mut import_order = Vec::new();
            for package in order.iter().filter(|p| affected.contains(*p)) {
                let selection = &desired[package];
                let staged = self.stage_selection(&conn, selection, &root, &mut originals)?;
                for staged_package in staged {
                    for asset in crate::unitypackage::inspect(&staged_package.file.path)? {
                        bytes = bytes
                            .checked_add(asset.size)
                            .ok_or(CoreError::Invalid("UNITYPACKAGE_TOO_LARGE"))?;
                        if bytes > MAX_EXPANDED_BYTES || assets.len() >= MAX_ARCHIVE_ENTRIES {
                            return Err(CoreError::Invalid("UNITYPACKAGE_TOO_LARGE"));
                        }
                        for (path, hash) in [
                            (asset.path.clone(), asset.hash.clone()),
                            (format!("{}.meta", asset.path), Some(asset.meta_hash)),
                        ] {
                            let incoming = InstallAsset {
                                source_file_id: staged_package.source_file_id.clone(),
                                package_id: package.clone(),
                                version_id: selection.version_id.clone(),
                                relative_path: path.clone(),
                                asset_guid: asset.guid.clone(),
                                content_hash: hash,
                                owned: true,
                            };
                            if let Some(existing) = assets.get(&path.to_lowercase()) {
                                // A shared folder is harmless only when its definition is identical.
                                if existing.content_hash != incoming.content_hash
                                    || existing.asset_guid != incoming.asset_guid
                                    || (!path.ends_with(".meta") && incoming.content_hash.is_some())
                                {
                                    return Err(conflict("ASSET_PATH_CONFLICT", path));
                                }
                            } else {
                                assets.insert(path.to_lowercase(), incoming);
                            }
                        }
                    }
                    let needs_import = current.get(package).is_none_or(|old| {
                        old.version_id != selection.version_id
                            || !old.file_ids.contains(&staged_package.source_file_id)
                    });
                    if needs_import {
                        let path: String = conn.query_row(
                            "SELECT relative_path FROM package_files WHERE id=?1",
                            [&staged_package.source_file_id],
                            |r| r.get(0),
                        )?;
                        import_order.push(path);
                        packages.push(staged_package);
                    }
                }
            }
            let old_by_path: BTreeMap<_, _> = owned.iter().map(|f| (f.path.as_str(), f)).collect();
            // Validate every affected ownership row, including files slated for removal.
            for file in owned.iter().filter(|f| affected.contains(&f.package_id)) {
                if disk.files.get(&file.path) != Some(&file.hash) {
                    return Err(conflict("USER_FILE_MODIFIED", &file.path));
                }
            }
            let mut incoming_guids = BTreeMap::new();
            for asset in assets.values_mut() {
                let path = &asset.relative_path;
                let is_folder = asset.content_hash.is_none();
                if let Some(existing_path) = disk
                    .files
                    .keys()
                    .chain(disk.directories.iter())
                    .find(|p| p.to_lowercase() == path.to_lowercase())
                {
                    if existing_path != path {
                        return Err(conflict("ASSET_PATH_CONFLICT", path));
                    }
                }
                if is_folder {
                    asset.owned = false;
                    if disk.files.contains_key(path) {
                        return Err(conflict("ASSET_PATH_CONFLICT", path));
                    }
                } else {
                    if disk.directories.contains(path) {
                        return Err(conflict("ASSET_PATH_CONFLICT", path));
                    }
                    if let Some(existing_hash) = disk.files.get(path) {
                        if path.ends_with(".meta")
                            && disk.directories.contains(path.trim_end_matches(".meta"))
                            && Some(existing_hash) != asset.content_hash.as_ref()
                        {
                            // Existing folder metadata can be shared by unowned
                            // descendants. Never rewrite its GUID or importer data.
                            return Err(conflict("ASSET_PATH_CONFLICT", path));
                        }
                        if old_by_path
                            .get(path.as_str())
                            .is_some_and(|old| old.package_id == asset.package_id)
                        {
                            // The hash was checked above.
                        } else if path.ends_with(".meta")
                            && disk.directories.contains(path.trim_end_matches(".meta"))
                            && Some(existing_hash) == asset.content_hash.as_ref()
                        {
                            asset.owned = false;
                        } else {
                            return Err(conflict("ASSET_PATH_CONFLICT", path));
                        }
                    }
                }
                if !path.ends_with(".meta") {
                    if incoming_guids
                        .insert(asset.asset_guid.clone(), path.clone())
                        .is_some()
                    {
                        return Err(conflict("ASSET_GUID_CONFLICT", path));
                    }
                    if disk
                        .guids
                        .get(&asset.asset_guid)
                        .is_some_and(|existing| existing != path)
                    {
                        return Err(conflict("ASSET_GUID_CONFLICT", path));
                    }
                }
            }
            for asset in assets.values() {
                for path in Path::new(&asset.relative_path).ancestors().skip(1) {
                    let path = path.to_str().unwrap().replace('\\', "/");
                    if path == "Assets" {
                        break;
                    }
                    if disk.files.contains_key(&path) {
                        return Err(conflict("ASSET_PATH_CONFLICT", path));
                    }
                    let incoming = assets.get(&path.to_lowercase());
                    if !disk.directories.contains(&path)
                        && incoming.is_none_or(|a| a.content_hash.is_some())
                    {
                        return Err(conflict("PARENT_FOLDER_REQUIRED", &path));
                    }
                    if disk.directories.contains(&path)
                        && !disk.files.contains_key(&format!("{path}.meta"))
                        && incoming.is_none()
                    {
                        return Err(conflict("PARENT_FOLDER_REQUIRED", &path));
                    }
                }
            }
            let mut removed_paths = Vec::new();
            for file in owned.iter().filter(|f| affected.contains(&f.package_id)) {
                if !assets.values().any(|a| a.relative_path == file.path) {
                    // Never delete metadata of a shared folder; ownership is released.
                    if file.path.ends_with(".meta")
                        && disk
                            .directories
                            .contains(file.path.trim_end_matches(".meta"))
                    {
                        continue;
                    }
                    removed_paths.push(file.path.clone());
                }
            }
            let base_state_hash = base_hash(&conn, &input.project_id, &project, &disk)?;
            bytes += disk
                .files
                .iter()
                .filter(|(path, _)| {
                    assets.values().any(|a| &a.relative_path == *path)
                        || removed_paths.contains(path)
                })
                .map(|(p, _)| fs::metadata(project.join(p)).map(|m| m.len()))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .sum::<u64>();
            bytes += packages
                .iter()
                .map(|p| fs::metadata(&p.file.path).map(|m| m.len()))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .sum::<u64>();
            let plan = InstallPlan {
                id: id.clone(),
                project_id: input.project_id.clone(),
                base_state_hash,
                plan_hash: String::new(),
                selections: input.selections,
                operations,
                asset_manifest: assets.into_values().collect(),
                removed_paths,
                import_order,
                required_bytes: bytes,
            };
            let mut stored = StoredPlan {
                plan,
                project_path: project,
                packages,
                affected: affected.into_iter().collect(),
                desired,
                originals,
            };
            stored.plan.plan_hash = digest(&stored)?;
            conn.execute("INSERT INTO install_plans(id,project_id,plan_hash,payload,created_at) VALUES(?1,?2,?3,?4,?5)", params![id,input.project_id,stored.plan.plan_hash,json(&stored)?,now()])?;
            Ok(stored.plan)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(root);
        }
        result
    }

    pub fn get_installations(&self, project_id: &str) -> CoreResult<InstallationState> {
        let conn = self.connect()?;
        let desired = desired_state(&conn, project_id)?;
        let mut packages = Vec::new();
        for (package_id, selection) in desired {
            if self.require_item_visible(&package_id).is_err() {
                continue;
            }
            let (title,date,hash): (String,String,String) = conn.query_row("SELECT p.title,v.version_date,v.content_hash FROM packages p JOIN package_versions v ON v.package_id=p.id WHERE v.id=?1", [&selection.version_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            packages.push(InstalledPackage {
                package_id,
                title,
                version_id: selection.version_id,
                version_label: format!("{date}@{}", &hash[..8]),
                file_ids: selection.file_ids,
            });
        }
        let mut stmt = conn.prepare("SELECT id,project_id,status,error_code,created_at FROM install_jobs WHERE project_id=?1 ORDER BY created_at DESC,id DESC LIMIT 100")?;
        let jobs = stmt
            .query_map([project_id], job_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(InstallationState { packages, jobs })
    }

    pub fn install_item(
        &self,
        input: InstallItemInput,
        unity: &dyn UnityGateway,
    ) -> CoreResult<InstallationJob> {
        if input.idempotency_key.is_empty() || input.idempotency_key.len() > 128 {
            return Err(CoreError::Invalid("INVALID_IDEMPOTENCY_KEY"));
        }
        let conn = self.connect()?;
        let _lock = self.installation_lock(&input.project_id)?;
        let existing: Option<(String, String, String)> = conn
            .query_row(
                "SELECT id,project_id,plan_id FROM install_jobs WHERE idempotency_key=?1",
                [&input.idempotency_key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((id, project, plan)) = existing {
            let expected: String = conn.query_row(
                "SELECT plan_hash FROM install_plans WHERE id=?1",
                [&plan],
                |r| r.get(0),
            )?;
            if project != input.project_id || plan != input.plan_id || expected != input.plan_hash {
                return Err(CoreError::Invalid("IDEMPOTENCY_CONFLICT"));
            }
            return load_job(&conn, &id);
        }
        no_busy(&conn, &input.project_id)?;
        let payload: String = conn
            .query_row(
                "SELECT payload FROM install_plans WHERE id=?1 AND project_id=?2 AND plan_hash=?3",
                params![input.plan_id, input.project_id, input.plan_hash],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(CoreError::Invalid("PLAN_STALE"))?;
        let stored: StoredPlan = decode(&payload)?;
        for id in stored.affected.iter().chain(stored.desired.keys()) {
            self.require_item_visible(id)?;
        }
        let project = self.installation_project(&conn, &input.project_id)?;
        if project != stored.project_path
            || base_hash(&conn, &input.project_id, &project, &scan_project(&project)?)?
                != stored.plan.base_state_hash
        {
            return Err(CoreError::Invalid("PLAN_STALE"));
        }
        for (path, hash) in &stored.originals {
            if hash_file(path)? != *hash {
                return Err(CoreError::Invalid("OBJECT_STORE_CORRUPT"));
            }
        }
        for package in &stored.packages {
            if hash_file(&package.file.path)? != package.hash {
                return Err(CoreError::Invalid("PLAN_STALE"));
            }
        }
        unity.ensure_ready(&project)?;
        let id = Uuid::new_v4().to_string();
        let backup_root = self.app_data_dir.join("install-backups").join(&id);
        fs::create_dir_all(&backup_root)?;
        // Check both filesystems before any project mutation (staging has already been materialized).
        if fs2_available_space(&project)? < stored.plan.required_bytes
            || fs2_available_space(&backup_root)? < stored.plan.required_bytes
        {
            return Err(CoreError::Invalid("INSUFFICIENT_SPACE"));
        }
        let mut entries = Vec::new();
        let paths: BTreeSet<_> = stored
            .plan
            .asset_manifest
            .iter()
            .filter(|a| a.content_hash.is_some() && a.owned)
            .map(|a| a.relative_path.clone())
            .chain(stored.plan.removed_paths.iter().cloned())
            .collect();
        for (index, path) in paths.into_iter().enumerate() {
            let target = safe_project_file(&project, &path)?;
            let before = if target.is_file() {
                Some(hash_file(&target)?)
            } else {
                None
            };
            let backup = if let Some(hash) = &before {
                let backup = backup_root.join(index.to_string());
                atomic_copy(&target, &backup, hash)?;
                Some(backup)
            } else {
                None
            };
            let after = stored
                .plan
                .asset_manifest
                .iter()
                .find(|a| a.relative_path == path)
                .and_then(|a| a.content_hash.clone());
            entries.push(BackupEntry {
                path,
                before,
                after,
                backup,
            });
        }
        #[cfg(unix)]
        {
            File::open(&backup_root)?.sync_all()?;
            File::open(backup_root.parent().unwrap())?.sync_all()?;
        }
        let journal = Journal {
            plan: stored,
            entries,
        };
        conn.execute("INSERT INTO install_jobs(id,project_id,plan_id,idempotency_key,status,journal,created_at) VALUES(?1,?2,?3,?4,'preparing',?5,?6)", params![id,input.project_id,input.plan_id,input.idempotency_key,json(&journal)?,now()])?;
        // Re-check after backing up, so a file edited during preparation invalidates the plan.
        let result = (|| {
            if base_hash(&conn, &input.project_id, &project, &scan_project(&project)?)?
                != journal.plan.plan.base_state_hash
            {
                return Err(CoreError::Invalid("PLAN_STALE"));
            }
            unity.ensure_ready(&project)?;
            for path in &journal.plan.plan.removed_paths {
                fs::remove_file(safe_project_file(&project, path)?)?;
            }
            conn.execute(
                "UPDATE install_jobs SET status='waitingForUnity' WHERE id=?1",
                [&id],
            )?;
            unity.publish(
                &project,
                &id,
                &journal
                    .plan
                    .packages
                    .iter()
                    .map(|p| p.file.clone())
                    .collect::<Vec<_>>(),
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            set_recovering(&conn, &id, error.code())?;
        }
        load_job(&conn, &id)
    }

    pub fn poll_installation(
        &self,
        job_id: &str,
        unity: &dyn UnityGateway,
    ) -> CoreResult<InstallationJob> {
        let conn = self.connect()?;
        let job = load_job(&conn, job_id)?;
        let _lock = self.installation_lock(&job.project_id)?;
        let job = load_job(&conn, job_id)?;
        if job.status != "waitingForUnity" {
            return Ok(job);
        }
        let journal = load_journal(&conn, job_id)?;
        let files = journal
            .plan
            .packages
            .iter()
            .map(|p| p.file.clone())
            .collect::<Vec<_>>();
        match unity.poll(&journal.plan.project_path, job_id, &files) {
            Ok(UnityOutcome::Pending) => return Ok(job),
            Ok(UnityOutcome::Completed) => {
                // Compilation/reload can outlive the import callback. Wait for
                // a fresh idle Editor before examining and committing its files.
                if let Err(error) = unity.ensure_ready(&journal.plan.project_path) {
                    if matches!(error.code(), "UNITY_BUSY" | "UNITY_UNAVAILABLE") {
                        return Ok(job);
                    }
                    set_recovering(&conn, job_id, error.code())?;
                    return load_job(&conn, job_id);
                }
                if let Err(error) = self.commit_installation(&conn, job_id, &journal) {
                    set_recovering(&conn, job_id, error.code())?;
                } else {
                    let _ = fs::remove_dir_all(
                        self.app_data_dir
                            .join("install-staging")
                            .join(&journal.plan.plan.id),
                    );
                }
            }
            Ok(UnityOutcome::Failed) => set_recovering(&conn, job_id, "IMPORT_FAILED")?,
            Ok(UnityOutcome::Cancelled) => set_recovering(&conn, job_id, "IMPORT_CANCELLED")?,
            Err(error) => {
                set_recovering(&conn, job_id, error.code())?;
            }
        }
        load_job(&conn, job_id)
    }

    fn commit_installation(
        &self,
        conn: &Connection,
        job_id: &str,
        journal: &Journal,
    ) -> CoreResult<()> {
        let plan = &journal.plan;
        let project_id = &plan.plan.project_id;
        let disk = scan_project(&plan.project_path)?;
        for asset in &plan.plan.asset_manifest {
            if let Some(hash) = &asset.content_hash {
                if disk.files.get(&asset.relative_path) != Some(hash) {
                    return Err(CoreError::Invalid("INSTALLED_FILE_MISMATCH"));
                }
            } else if !disk.directories.contains(&asset.relative_path) {
                return Err(CoreError::Invalid("INSTALLED_FILE_MISMATCH"));
            }
            let asset_path = asset
                .relative_path
                .strip_suffix(".meta")
                .unwrap_or(&asset.relative_path);
            if disk.guids.get(&asset.asset_guid).map(String::as_str) != Some(asset_path) {
                return Err(CoreError::Invalid("INSTALLED_GUID_MISMATCH"));
            }
        }
        if plan
            .plan
            .removed_paths
            .iter()
            .any(|p| disk.files.contains_key(p))
        {
            return Err(CoreError::Invalid("REMOVED_FILE_PRESENT"));
        }
        // Other installed packages must still match; callbacks alone are never a commit.
        for file in owned_files(conn, project_id)?
            .iter()
            .filter(|f| !plan.affected.contains(&f.package_id))
        {
            if disk.files.get(&file.path) != Some(&file.hash) {
                return Err(conflict("USER_FILE_MODIFIED", &file.path));
            }
        }
        let tx = conn.unchecked_transaction()?;
        for package in &plan.affected {
            tx.execute(
                "DELETE FROM project_files WHERE project_id=?1 AND package_id=?2",
                params![project_id, package],
            )?;
            tx.execute(
                "DELETE FROM project_selections WHERE project_id=?1 AND package_id=?2",
                params![project_id, package],
            )?;
            tx.execute(
                "DELETE FROM project_packages WHERE project_id=?1 AND package_id=?2",
                params![project_id, package],
            )?;
            if let Some(selection) = plan.desired.get(package) {
                tx.execute("INSERT INTO project_packages(project_id,package_id,version_id,job_id) VALUES(?1,?2,?3,?4)", params![project_id,package,selection.version_id,job_id])?;
                for (order, id) in selection.file_ids.iter().enumerate() {
                    tx.execute("INSERT INTO project_selections(project_id,package_id,version_id,source_file_id,apply_order) VALUES(?1,?2,?3,?4,?5)", params![project_id,package,selection.version_id,id,order as i64])?;
                }
            }
        }
        for asset in plan
            .plan
            .asset_manifest
            .iter()
            .filter(|a| a.owned && a.content_hash.is_some())
        {
            tx.execute("INSERT INTO project_files(project_id,relative_path,source_file_id,package_id,version_id,installed_hash,asset_guid,job_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![project_id,asset.relative_path,asset.source_file_id,asset.package_id,asset.version_id,asset.content_hash,asset.asset_guid,job_id])?;
        }
        tx.execute(
            "UPDATE install_jobs SET status='succeeded',error_code=NULL WHERE id=?1",
            [job_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn recover_installation(
        &self,
        job_id: &str,
        mode: RecoveryMode,
        unity: &dyn UnityGateway,
    ) -> CoreResult<RecoveryReport> {
        let conn = self.connect()?;
        let job = load_job(&conn, job_id)?;
        let _lock = self.installation_lock(&job.project_id)?;
        let job = load_job(&conn, job_id)?;
        if job.status == "succeeded" || job.status == "restored" {
            return Err(CoreError::Invalid("RECOVERY_NOT_REQUIRED"));
        }
        let journal = load_journal(&conn, job_id)?;
        for id in &journal.plan.affected {
            self.require_item_visible(id)?;
        }
        let project = &journal.plan.project_path;
        self.installation_project(&conn, &job.project_id)?;
        let mut conflicts = Vec::new();
        for entry in &journal.entries {
            let target = safe_project_file(project, &entry.path)?;
            let current = if target.is_file() {
                Some(hash_file(&target)?)
            } else {
                None
            };
            if current != entry.before && current != entry.after {
                conflicts.push(entry.path.clone());
            }
            if let (Some(backup), Some(hash)) = (&entry.backup, &entry.before) {
                if hash_file(backup)? != *hash {
                    conflicts.push(entry.path.clone());
                }
            }
        }
        // A new folder may now contain user work. Do not remove its metadata
        // while leaving unrelated children with a newly generated Unity GUID.
        let disk = scan_project(project)?;
        for entry in journal
            .entries
            .iter()
            .filter(|e| e.before.is_none() && e.path.ends_with(".meta"))
        {
            let folder = entry.path.strip_suffix(".meta").unwrap();
            if disk.directories.contains(folder) {
                let prefix = format!("{folder}/");
                if disk.files.keys().any(|p| {
                    p.starts_with(&prefix)
                        && !journal
                            .entries
                            .iter()
                            .any(|e| &e.path == p && e.before.is_none())
                }) {
                    conflicts.push(entry.path.clone());
                }
            }
        }
        let mut restored_paths = Vec::new();
        if matches!(mode, RecoveryMode::Restore) && conflicts.is_empty() {
            unity.ensure_ready(project)?;
            let files = journal
                .plan
                .packages
                .iter()
                .map(|p| p.file.clone())
                .collect::<Vec<_>>();
            // A published request must be terminal before touching assets. A missing
            // publication is safe only if the adapter confirms no current request.
            if unity.poll(project, job_id, &files)? == UnityOutcome::Pending {
                return Err(CoreError::Invalid("UNITY_BUSY"));
            }
            for entry in &journal.entries {
                let target = safe_project_file(project, &entry.path)?;
                let current = if target.is_file() {
                    Some(hash_file(&target)?)
                } else {
                    None
                };
                if current == entry.before {
                    continue;
                }
                if current != entry.after {
                    return Err(CoreError::Invalid("RECOVERY_CONFLICT"));
                }
                if let (Some(backup), Some(hash)) = (&entry.backup, &entry.before) {
                    atomic_copy(backup, &target, hash)?;
                } else if target.exists() {
                    fs::remove_file(target)?;
                }
                restored_paths.push(entry.path.clone());
            }
            conn.execute(
                "UPDATE install_jobs SET status='restored',error_code=NULL WHERE id=?1",
                [job_id],
            )?;
            // Retain backups and journals for diagnosis. Unity refreshes on its next update.
        }
        Ok(RecoveryReport {
            requires_manual_action: !conflicts.is_empty(),
            restored_paths,
            conflicts,
        })
    }
}

fn job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InstallationJob> {
    Ok(InstallationJob {
        id: row.get(0)?,
        project_id: row.get(1)?,
        status: row.get(2)?,
        error_code: row.get(3)?,
        created_at: row.get(4)?,
    })
}
fn load_job(conn: &Connection, id: &str) -> CoreResult<InstallationJob> {
    conn.query_row(
        "SELECT id,project_id,status,error_code,created_at FROM install_jobs WHERE id=?1",
        [id],
        job_row,
    )
    .optional()?
    .ok_or(CoreError::Invalid("INSTALLATION_NOT_FOUND"))
}
fn load_journal(conn: &Connection, id: &str) -> CoreResult<Journal> {
    decode(&conn.query_row::<String, _, _>(
        "SELECT journal FROM install_jobs WHERE id=?1",
        [id],
        |r| r.get(0),
    )?)
}
fn set_recovering(conn: &Connection, id: &str, code: &str) -> CoreResult<()> {
    conn.execute(
        "UPDATE install_jobs SET status='recovering',error_code=?2 WHERE id=?1",
        params![id, code],
    )?;
    Ok(())
}
fn safe_project_file(project: &Path, relative: &str) -> CoreResult<PathBuf> {
    asset_path(relative.strip_suffix(".meta").unwrap_or(relative))?;
    let mut path = project.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(CoreError::Invalid("PROJECT_UNSAFE_PATH"))
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(path)
}
fn fs2_available_space(path: &Path) -> CoreResult<u64> {
    Ok(fs2::available_space(path)?)
}

fn conflict(code: &'static str, path: impl AsRef<str>) -> CoreError {
    CoreError::InstallationConflict {
        code,
        path: path.as_ref().to_owned(),
    }
}

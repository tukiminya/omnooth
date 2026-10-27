//! Local application services shared by the desktop and future stdio adapters.
//! This crate deliberately owns a database separate from the design-review database.

use chrono::{SecondsFormat, Utc};
use rusqlite::{backup::Backup, params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;
use zip::ZipArchive;

mod deep_link;
pub use deep_link::{parse_item_import_deep_link, ItemImportReceipt, ParsedItemImport};

const SCHEMA_VERSION: i64 = 1;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_EXPANDED_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Database(#[from] rusqlite::Error),
    #[error("{0}")]
    Archive(String),
}

impl CoreError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(code) => code,
            Self::Io(_) => "IO_ERROR",
            Self::Database(_) => "DATABASE_ERROR",
            Self::Archive(message) => match message.split(':').next().unwrap_or("") {
                "ARCHIVE_UNSAFE" => "ARCHIVE_UNSAFE",
                "ARCHIVE_TOO_MANY_ENTRIES" => "ARCHIVE_TOO_MANY_ENTRIES",
                "ARCHIVE_TOO_LARGE" => "ARCHIVE_TOO_LARGE",
                "ARCHIVE_TRUNCATED" => "ARCHIVE_TRUNCATED",
                "ARCHIVE_SYMLINK" => "ARCHIVE_SYMLINK",
                "ARCHIVE_SPECIAL_FILE" => "ARCHIVE_SPECIAL_FILE",
                "ARCHIVE_DUPLICATE_PATH" => "ARCHIVE_DUPLICATE_PATH",
                "ARCHIVE_PATH_COLLISION" => "ARCHIVE_PATH_COLLISION",
                "ARCHIVE_INVALID_DIRECTORY" => "ARCHIVE_INVALID_DIRECTORY",
                "ARCHIVE_SIZE_MISMATCH" => "ARCHIVE_SIZE_MISMATCH",
                _ => "ARCHIVE_INVALID",
            },
        }
    }
}

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub unity_version: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub language: String,
    pub library_path: String,
    pub extract_archives: bool,
    pub keep_old_versions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: String,
    pub title: String,
    pub versions: Vec<LibraryVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryVersion {
    pub id: String,
    pub label: String,
    pub version_date: String,
    pub content_hash: String,
    pub original_filename: String,
    pub state: String,
    pub files: Vec<ImportableFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportableFile {
    pub id: String,
    pub relative_path: String,
    pub kind: String,
    pub size_bytes: u64,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub projects: Vec<Project>,
    pub items: Vec<LibraryItem>,
    pub settings: AppSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportArchiveInput {
    pub archive_path: PathBuf,
    pub original_filename: String,
    pub extract_archive: bool,
    /// Existing logical library item to append this version to, when supplied.
    pub item_id: Option<String>,
}

/// Shared command protocol for Tauri and the future local stdio adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "input",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ApplicationCommand {
    GetSnapshot,
    LinkProject { directory_path: PathBuf },
    ImportArchive(ImportArchiveInput),
    ExtractArchive { version_id: String },
    UpdateSettings { settings: AppSettings },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub item: LibraryItem,
    pub reused: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Service {
    db_path: PathBuf,
    app_data_dir: PathBuf,
}

impl Service {
    /// Opens the product database at `<app_data_dir>/omnooth.sqlite` and initializes
    /// the default library at `<app_data_dir>/Library`.
    pub fn open(app_data_dir: impl AsRef<Path>) -> CoreResult<Self> {
        let app_data_dir = app_data_dir.as_ref().to_path_buf();
        fs::create_dir_all(&app_data_dir)?;
        let db_path = app_data_dir.join("omnooth.sqlite");
        let service = Self {
            db_path,
            app_data_dir,
        };
        let mut conn = service.connect()?;
        migrate(&mut conn)?;
        let default_library = service.app_data_dir.join("Library");
        fs::create_dir_all(default_library.join("objects"))?;
        fs::create_dir_all(default_library.join("extracted"))?;
        conn.execute(
            "INSERT OR IGNORE INTO app_settings(id, language, library_path, extract_archives, keep_old_versions, updated_at) VALUES (1, 'ja', ?1, 0, 1, ?2)",
            params![default_library.to_string_lossy(), now()],
        )?;
        Ok(service)
    }

    /// Imports a prior app-ID database only when the new app-data directory has no database.
    /// Existing object paths/settings remain unchanged in the backup.
    pub fn open_migrating_legacy(
        new_app_data_dir: impl AsRef<Path>,
        old_app_data_dir: impl AsRef<Path>,
    ) -> CoreResult<Self> {
        migrate_legacy_database(&new_app_data_dir, &old_app_data_dir)?;
        Self::open(new_app_data_dir)
    }

    fn connect(&self) -> CoreResult<Connection> {
        let conn = Connection::open(&self.db_path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(conn)
    }

    pub fn snapshot(&self) -> CoreResult<AppSnapshot> {
        let conn = self.connect()?;
        let settings = read_settings(&conn)?;
        let mut stmt = conn.prepare("SELECT id,name,canonical_path,unity_version,NULL FROM projects ORDER BY name COLLATE NOCASE,id")?;
        let projects = stmt
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    unity_version: row.get(3)?,
                    last_used_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut stmt =
            conn.prepare("SELECT id,title FROM packages ORDER BY title COLLATE NOCASE,id")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut items = Vec::with_capacity(rows.len());
        for (id, title) in rows {
            items.push(load_item(&conn, id, title)?);
        }
        Ok(AppSnapshot {
            projects,
            items,
            settings,
        })
    }

    pub fn dispatch(&self, command: ApplicationCommand) -> CoreResult<serde_json::Value> {
        match command {
            ApplicationCommand::GetSnapshot => serde_json::to_value(self.snapshot()?),
            ApplicationCommand::LinkProject { directory_path } => {
                serde_json::to_value(self.link_project(directory_path)?)
            }
            ApplicationCommand::ImportArchive(input) => {
                serde_json::to_value(self.import_archive(input)?)
            }
            ApplicationCommand::ExtractArchive { version_id } => {
                serde_json::to_value(self.extract_archive(&version_id)?)
            }
            ApplicationCommand::UpdateSettings { settings } => {
                serde_json::to_value(self.update_settings(settings)?)
            }
        }
        .map_err(|_| CoreError::Invalid("SERIALIZATION_ERROR"))
    }

    pub fn link_project(&self, path: impl AsRef<Path>) -> CoreResult<Project> {
        let input = path.as_ref();
        let canonical =
            fs::canonicalize(input).map_err(|_| CoreError::Invalid("PROJECT_INVALID"))?;
        let (name, unity_version) = validate_unity_project(&canonical)?;
        let canonical_path = canonical.to_string_lossy().into_owned();
        let dedupe_path = project_dedupe_key(&canonical, &canonical_path)?;
        let conn = self.connect()?;
        let existing = conn.query_row(
            "SELECT id,name,canonical_path,unity_version,linked_at FROM projects WHERE dedupe_path=?1",
            [&dedupe_path],
            |row| Ok(Project { id: row.get(0)?, name: row.get(1)?, path: row.get(2)?, unity_version: row.get(3)?, last_used_at: None }),
        ).optional()?;
        if let Some(project) = existing {
            return Ok(project);
        }
        let project = Project {
            id: Uuid::new_v4().to_string(),
            name,
            path: canonical_path,
            unity_version,
            last_used_at: None,
        };
        conn.execute("INSERT INTO projects(id,name,canonical_path,dedupe_path,unity_version,linked_at) VALUES (?1,?2,?3,?4,?5,?6)", params![project.id, project.name, project.path, dedupe_path, project.unity_version, now()])?;
        Ok(project)
    }

    pub fn update_settings(&self, settings: AppSettings) -> CoreResult<AppSettings> {
        if !matches!(settings.language.as_str(), "ja" | "en" | "ko") {
            return Err(CoreError::Invalid("INVALID_LANGUAGE"));
        }
        if settings.library_path.trim().is_empty() {
            return Err(CoreError::Invalid("INVALID_LIBRARY_PATH"));
        }
        let library = PathBuf::from(&settings.library_path);
        if !library.is_absolute() {
            return Err(CoreError::Invalid("INVALID_LIBRARY_PATH"));
        }
        fs::create_dir_all(library.join("objects"))?;
        fs::create_dir_all(library.join("extracted"))?;
        let canonical = fs::canonicalize(&library)?.to_string_lossy().into_owned();
        let conn = self.connect()?;
        conn.execute("UPDATE app_settings SET language=?1,library_path=?2,extract_archives=?3,keep_old_versions=?4,updated_at=?5 WHERE id=1", params![settings.language, canonical, settings.extract_archives, settings.keep_old_versions, now()])?;
        read_settings(&conn)
    }

    /// Saves an immutable source object and creates/reuses its date+SHA-256 version.
    /// BOOTH metadata is not connected yet, so the version date is the UTC import date.
    /// A `.unitypackage` is accepted by extension and preserved byte-for-byte; its internal
    /// gzip/tar structure is not parsed in this MVP. ZIP signatures are checked against names.
    pub fn import_archive(&self, input: ImportArchiveInput) -> CoreResult<ImportResult> {
        validate_filename(&input.original_filename)?;
        let format = import_format(&input.original_filename)?;
        let metadata = fs::metadata(&input.archive_path)?;
        if !metadata.is_file() {
            return Err(CoreError::Invalid("INVALID_ARCHIVE_PATH"));
        }
        if format == ImportFormat::Zip && !has_zip_signature(&input.archive_path)? {
            return Err(CoreError::Invalid("ARCHIVE_FORMAT_MISMATCH"));
        }
        let hash = hash_file(&input.archive_path)?;
        let lower = input.original_filename.to_ascii_lowercase();
        let direct_unitypackage = lower.ends_with(".unitypackage");
        let supported_zip = format == ImportFormat::Zip;
        let mut warnings = Vec::new();
        let mut state = "stored".to_string();
        let mut files = Vec::new();
        let library = self.current_library_path()?;
        let conn = self.connect()?;
        let existing = conn
            .query_row(
                "SELECT id,package_id,state FROM package_versions WHERE content_hash=?1",
                [&hash],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((version_id, package_id, old_state)) = existing {
            let registered_path = conn.query_row(
                "SELECT object_path FROM package_versions WHERE id=?1",
                [&version_id],
                |r| r.get::<_, String>(0),
            )?;
            reconcile_object(&input.archive_path, Path::new(&registered_path), &hash)?;
            if input.extract_archive && supported_zip && old_state != "extracted" {
                self.extract_archive(&version_id)?;
            }
            let title = conn.query_row(
                "SELECT title FROM packages WHERE id=?1",
                [&package_id],
                |r| r.get::<_, String>(0),
            )?;
            let item = load_item(&conn, package_id, title)?;
            return Ok(ImportResult {
                item,
                reused: true,
                warnings,
            });
        }
        let package_id = if let Some(id) = input.item_id.as_deref() {
            conn.query_row("SELECT id FROM packages WHERE id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .ok_or(CoreError::Invalid("ITEM_NOT_FOUND"))?
        } else if let Some(id) = conn
            .query_row(
                "SELECT id FROM packages WHERE identity_filename=?1",
                [&input.original_filename],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            id
        } else {
            Uuid::new_v4().to_string()
        };
        let new_package = conn
            .query_row("SELECT 1 FROM packages WHERE id=?1", [&package_id], |r| {
                r.get::<_, i32>(0)
            })
            .optional()?
            .is_none();
        let title = if new_package {
            filename_stem(&input.original_filename)
        } else {
            conn.query_row(
                "SELECT title FROM packages WHERE id=?1",
                [&package_id],
                |r| r.get::<_, String>(0),
            )?
        };
        let object_path = library.join("objects").join(&hash);
        atomic_copy(&input.archive_path, &object_path, &hash)?;
        if direct_unitypackage {
            state = "ready".to_string();
            files.push(ImportableFile {
                id: Uuid::new_v4().to_string(),
                relative_path: input.original_filename.clone(),
                kind: "unitypackage".into(),
                size_bytes: metadata.len(),
                content_hash: hash.clone(),
            });
        } else if input.extract_archive && supported_zip {
            let extracted_root = object_path
                .parent()
                .and_then(Path::parent)
                .ok_or(CoreError::Invalid("INVALID_LIBRARY_PATH"))?
                .join("extracted");
            let final_dir = extracted_root.join(&hash);
            fs::create_dir_all(&extracted_root)?;
            if !final_dir.exists() {
                let staging = extracted_root.join(format!(".staging-{}", Uuid::new_v4()));
                fs::create_dir_all(&staging)?;
                if let Err(err) = extract_zip_safely(&object_path, &staging) {
                    let _ = fs::remove_dir_all(&staging);
                    return Err(err);
                }
                if let Err(err) = fs::rename(&staging, &final_dir) {
                    let _ = fs::remove_dir_all(&staging);
                    if !final_dir.exists() {
                        return Err(err.into());
                    }
                }
            }
            files = collect_unitypackages(&final_dir)?;
            state = "extracted".into();
            if files.is_empty() {
                warnings.push("NO_UNITYPACKAGE_FOUND".into());
            }
        } else if input.extract_archive {
            warnings.push("EXTRACTION_UNSUPPORTED_STORED_ONLY".into());
        }
        let version_id = Uuid::new_v4().to_string();
        let imported_at = now();
        let version_date = imported_at[..10].to_string();
        let extracted_path = if state == "extracted" {
            object_path.parent().and_then(Path::parent).map(|p| {
                p.join("extracted")
                    .join(&hash)
                    .to_string_lossy()
                    .into_owned()
            })
        } else {
            None
        };
        let item: LibraryItem;
        let tx = conn.unchecked_transaction()?;
        if new_package {
            tx.execute(
                "INSERT INTO packages(id,title,identity_filename,created_at) VALUES (?1,?2,?3,?4)",
                params![package_id, title, input.original_filename, imported_at],
            )?;
        }
        tx.execute("INSERT INTO package_versions(id,package_id,original_filename,content_hash,version_date,state,object_path,extracted_path,imported_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![version_id,package_id,input.original_filename,hash,version_date,state,object_path.to_string_lossy(),extracted_path,imported_at])?;
        for file in files {
            tx.execute("INSERT INTO package_files(id,version_id,relative_path,kind,size_bytes,content_hash) VALUES (?1,?2,?3,?4,?5,?6)",params![file.id,version_id,file.relative_path,file.kind,file.size_bytes as i64,file.content_hash])?;
        }
        tx.commit()?;
        item = load_item(&conn, package_id, title)?;
        Ok(ImportResult {
            item,
            reused: false,
            warnings,
        })
    }

    fn current_library_path(&self) -> CoreResult<PathBuf> {
        Ok(PathBuf::from(read_settings(&self.connect()?)?.library_path))
    }

    /// Explicitly extracts a previously stored ZIP, preserving its content-addressed original.
    pub fn extract_archive(&self, version_id: &str) -> CoreResult<LibraryVersion> {
        let conn = self.connect()?;
        let (original_filename, hash, current_state, object_path, package_id) = conn.query_row(
            "SELECT original_filename,content_hash,state,object_path,package_id FROM package_versions WHERE id=?1", [version_id],
            |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?)),
        )?;
        if !original_filename.to_ascii_lowercase().ends_with(".zip") {
            return Err(CoreError::Invalid("EXTRACTION_UNSUPPORTED"));
        }
        let original_path = Path::new(&object_path);
        if !original_path.is_file() {
            return Err(CoreError::Invalid("OBJECT_STORE_MISSING"));
        }
        if hash_file(original_path)? != hash {
            return Err(CoreError::Invalid("OBJECT_STORE_CORRUPT"));
        }
        if current_state == "extracted" {
            let date = conn.query_row(
                "SELECT version_date FROM package_versions WHERE id=?1",
                [version_id],
                |r| r.get::<_, String>(0),
            )?;
            return load_version(
                &conn,
                version_id.to_string(),
                original_filename,
                hash,
                date,
                current_state,
            );
        }
        let extracted_root = Path::new(&object_path)
            .parent()
            .and_then(Path::parent)
            .ok_or(CoreError::Invalid("INVALID_LIBRARY_PATH"))?
            .join("extracted");
        let final_dir = extracted_root.join(&hash);
        if !final_dir.exists() {
            fs::create_dir_all(&extracted_root)?;
            let staging = extracted_root.join(format!(".staging-{}", Uuid::new_v4()));
            fs::create_dir_all(&staging)?;
            if let Err(err) = extract_zip_safely(Path::new(&object_path), &staging) {
                let _ = fs::remove_dir_all(&staging);
                return Err(err);
            }
            if let Err(err) = fs::rename(&staging, &final_dir) {
                let _ = fs::remove_dir_all(&staging);
                if !final_dir.exists() {
                    return Err(err.into());
                }
            }
        }
        let files = collect_unitypackages(&final_dir)?;
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE package_versions SET state='extracted',extracted_path=?2 WHERE id=?1",
            params![version_id, final_dir.to_string_lossy()],
        )?;
        tx.execute(
            "DELETE FROM package_files WHERE version_id=?1",
            [version_id],
        )?;
        for file in files {
            tx.execute("INSERT INTO package_files(id,version_id,relative_path,kind,size_bytes,content_hash) VALUES (?1,?2,?3,?4,?5,?6)", params![file.id,version_id,file.relative_path,file.kind,file.size_bytes as i64,file.content_hash])?;
        }
        tx.commit()?;
        let date = conn.query_row(
            "SELECT version_date FROM package_versions WHERE id=?1",
            [version_id],
            |r| r.get::<_, String>(0),
        )?;
        let _ = package_id;
        load_version(
            &conn,
            version_id.to_string(),
            original_filename,
            hash,
            date,
            "extracted".into(),
        )
    }
}

/// Online SQLite backup of a legacy app database into a new app-data directory.
/// Returns false when the new database already exists or the legacy database is absent.
/// The source is opened read-only; SQLite's backup API includes committed WAL content.
pub fn migrate_legacy_database(
    new_app_data_dir: impl AsRef<Path>,
    old_app_data_dir: impl AsRef<Path>,
) -> CoreResult<bool> {
    let new_dir = new_app_data_dir.as_ref();
    let old_db = old_app_data_dir.as_ref().join("omnooth.sqlite");
    let new_db = new_dir.join("omnooth.sqlite");
    fs::create_dir_all(new_dir)?;
    if new_db.exists() || !old_db.is_file() {
        return Ok(false);
    }
    let staging = new_dir.join(format!(".omnooth-migration-{}.sqlite", Uuid::new_v4()));
    let result = (|| -> CoreResult<()> {
        let source = Connection::open_with_flags(&old_db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut destination = Connection::open(&staging)?;
        let backup = Backup::new(&source, &mut destination)?;
        backup.run_to_completion(64, std::time::Duration::from_millis(10), None)?;
        drop(backup);
        drop(destination);
        File::open(&staging)?.sync_all()?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    match fs::hard_link(&staging, &new_db) {
        Ok(()) => {
            fs::remove_file(&staging)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&staging);
            Ok(false)
        }
        Err(error) => {
            let _ = fs::remove_file(&staging);
            Err(error.into())
        }
    }
}

fn migrate(conn: &mut Connection) -> CoreResult<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(CoreError::Invalid("DATABASE_VERSION_NEWER"));
    }
    if version == 0 {
        let tx = conn.transaction()?;
        tx.execute_batch("\
          CREATE TABLE projects(id TEXT PRIMARY KEY,name TEXT NOT NULL,canonical_path TEXT NOT NULL,dedupe_path TEXT NOT NULL UNIQUE,unity_version TEXT NOT NULL,linked_at TEXT NOT NULL);\
          CREATE TABLE packages(id TEXT PRIMARY KEY,title TEXT NOT NULL,identity_filename TEXT NOT NULL UNIQUE,created_at TEXT NOT NULL);\
          CREATE TABLE package_versions(id TEXT PRIMARY KEY,package_id TEXT NOT NULL REFERENCES packages(id) ON DELETE CASCADE,original_filename TEXT NOT NULL,content_hash TEXT NOT NULL UNIQUE CHECK(length(content_hash)=64),version_date TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN ('stored','extracted','ready')),object_path TEXT NOT NULL,extracted_path TEXT,imported_at TEXT NOT NULL,UNIQUE(package_id,content_hash));\
          CREATE TABLE package_files(id TEXT PRIMARY KEY,version_id TEXT NOT NULL REFERENCES package_versions(id) ON DELETE CASCADE,relative_path TEXT NOT NULL,kind TEXT NOT NULL,size_bytes INTEGER NOT NULL CHECK(size_bytes>=0),content_hash TEXT NOT NULL CHECK(length(content_hash)=64),UNIQUE(version_id,relative_path));\
          CREATE TABLE app_settings(id INTEGER PRIMARY KEY CHECK(id=1),language TEXT NOT NULL CHECK(language IN ('ja','en','ko')),library_path TEXT NOT NULL,extract_archives INTEGER NOT NULL DEFAULT 0 CHECK(extract_archives IN (0,1)),privacy_notice_acknowledged_at TEXT,keep_old_versions INTEGER NOT NULL DEFAULT 1 CHECK(keep_old_versions IN (0,1)),updated_at TEXT NOT NULL);\
        ")?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        tx.commit()?;
    }
    Ok(())
}

fn read_settings(conn: &Connection) -> CoreResult<AppSettings> {
    Ok(conn.query_row("SELECT language,library_path,extract_archives,keep_old_versions FROM app_settings WHERE id=1", [], |r| Ok(AppSettings { language: r.get(0)?, library_path: r.get(1)?, extract_archives: r.get(2)?, keep_old_versions: r.get(3)? }))?)
}

fn validate_unity_project(path: &Path) -> CoreResult<(String, String)> {
    let assets = path.join("Assets");
    let settings = path.join("ProjectSettings").join("ProjectVersion.txt");
    if !assets.is_dir() {
        return Err(CoreError::Invalid("ASSETS_MISSING"));
    }
    if !settings.is_file() {
        return Err(CoreError::Invalid("PROJECT_INVALID"));
    }
    let text = fs::read_to_string(settings).map_err(|_| CoreError::Invalid("PROJECT_INVALID"))?;
    let version = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("m_EditorVersion:").map(str::trim))
        .filter(|v| !v.is_empty())
        .ok_or(CoreError::Invalid("PROJECT_INVALID"))?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Unity Project")
        .to_string();
    Ok((name, version.to_string()))
}

fn validate_filename(name: &str) -> CoreResult<()> {
    if name.trim().is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || name.chars().any(char::is_control)
    {
        return Err(CoreError::Invalid("INVALID_FILENAME"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportFormat {
    Unitypackage,
    Zip,
    SevenZip,
    Rar,
    Tar,
    TarGzip,
    TarBzip2,
    TarXz,
}

fn import_format(name: &str) -> CoreResult<ImportFormat> {
    let lower = name.to_ascii_lowercase();
    let format = if lower.ends_with(".unitypackage") {
        ImportFormat::Unitypackage
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        ImportFormat::TarGzip
    } else if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz2") {
        ImportFormat::TarBzip2
    } else if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
        ImportFormat::TarXz
    } else if lower.ends_with(".zip") {
        ImportFormat::Zip
    } else if lower.ends_with(".7z") {
        ImportFormat::SevenZip
    } else if lower.ends_with(".rar") {
        ImportFormat::Rar
    } else if lower.ends_with(".tar") {
        ImportFormat::Tar
    } else {
        return Err(CoreError::Invalid("UNSUPPORTED_FORMAT"));
    };
    Ok(format)
}

fn has_zip_signature(path: &Path) -> CoreResult<bool> {
    let mut file = File::open(path)?;
    let mut bytes = [0u8; 4];
    let count = file.read(&mut bytes)?;
    Ok(count >= 4
        && matches!(
            bytes,
            [b'P', b'K', 0x03, 0x04] | [b'P', b'K', 0x05, 0x06] | [b'P', b'K', 0x07, 0x08]
        ))
}

fn project_dedupe_key(canonical: &Path, _canonical_path: &str) -> CoreResult<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(canonical)?;
        return Ok(format!("unix:{}:{}", metadata.dev(), metadata.ino()));
    }
    #[cfg(windows)]
    {
        return Ok(_canonical_path.to_lowercase());
    }
    #[allow(unreachable_code)]
    Ok(_canonical_path.to_string())
}

fn filename_stem(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    for suffix in [
        ".tar.gz",
        ".tar.bz2",
        ".tar.xz",
        ".unitypackage",
        ".zip",
        ".7z",
        ".rar",
        ".tar",
        ".tgz",
        ".tbz2",
        ".txz",
    ] {
        if lower.ends_with(suffix) {
            return name[..name.len() - suffix.len()].to_string();
        }
    }
    name.to_string()
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn hash_file(path: &Path) -> CoreResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn atomic_copy(source: &Path, destination: &Path, expected_hash: &str) -> CoreResult<()> {
    let parent = destination
        .parent()
        .ok_or(CoreError::Invalid("INVALID_LIBRARY_PATH"))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".tmp-{}", Uuid::new_v4()));
    fs::copy(source, &tmp)?;
    let copied_hash = match hash_file(&tmp) {
        Ok(hash) => hash,
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            return Err(err);
        }
    };
    if copied_hash != expected_hash {
        let _ = fs::remove_file(&tmp);
        return Err(CoreError::Invalid("SOURCE_CHANGED_DURING_IMPORT"));
    }
    if destination.exists() {
        if hash_file(destination)? == expected_hash {
            let _ = fs::remove_file(&tmp);
            return Ok(());
        }
        let backup = parent.join(format!(".corrupt-{}", Uuid::new_v4()));
        fs::rename(destination, &backup)?;
        match fs::rename(&tmp, destination) {
            Ok(()) => {
                let _ = fs::remove_file(backup);
                Ok(())
            }
            Err(err) => {
                let _ = fs::rename(&backup, destination);
                let _ = fs::remove_file(&tmp);
                Err(err.into())
            }
        }
    } else {
        match fs::rename(&tmp, destination) {
            Ok(()) => Ok(()),
            Err(_err) if destination.exists() && hash_file(destination)? == expected_hash => {
                let _ = fs::remove_file(tmp);
                Ok(())
            }
            Err(err) => {
                let _ = fs::remove_file(tmp);
                Err(err.into())
            }
        }
    }
}

fn reconcile_object(source: &Path, destination: &Path, expected_hash: &str) -> CoreResult<()> {
    if destination.is_file() && hash_file(destination)? == expected_hash {
        return Ok(());
    }
    atomic_copy(source, destination, expected_hash)
}

fn safe_member_path(name: &str) -> CoreResult<PathBuf> {
    let path = Path::new(name);
    if path.as_os_str().is_empty() || path.is_absolute() || name.contains('\\') {
        return Err(CoreError::Archive("ARCHIVE_UNSAFE".into()));
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => return Err(CoreError::Archive("ARCHIVE_UNSAFE".into())),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(CoreError::Archive("ARCHIVE_UNSAFE".into()));
    }
    Ok(out)
}

fn extract_zip_safely(path: &Path, destination: &Path) -> CoreResult<()> {
    extract_zip_safely_with_limits(path, destination, MAX_ARCHIVE_ENTRIES, MAX_EXPANDED_BYTES)
}

fn extract_zip_safely_with_limits(
    path: &Path,
    destination: &Path,
    max_entries: usize,
    max_bytes: u64,
) -> CoreResult<()> {
    let file = File::open(path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| CoreError::Archive(format!("INVALID_ARCHIVE: {e}")))?;
    if archive.len() > max_entries {
        return Err(CoreError::Archive("ARCHIVE_TOO_MANY_ENTRIES".into()));
    }
    let mut total = 0u64;
    let mut seen: HashMap<PathBuf, bool> = HashMap::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| CoreError::Archive(e.to_string()))?;
        let relative = safe_member_path(entry.name())?;
        let is_dir = entry.is_dir();
        if let Some(mode) = entry.unix_mode() {
            match mode & 0o170000 {
                0 => {}
                0o100000 if !is_dir => {}
                0o040000 if is_dir => {}
                0o120000 => return Err(CoreError::Archive("ARCHIVE_SYMLINK".into())),
                _ => return Err(CoreError::Archive("ARCHIVE_SPECIAL_FILE".into())),
            }
        }
        if seen.contains_key(&relative) {
            return Err(CoreError::Archive("ARCHIVE_DUPLICATE_PATH".into()));
        }
        let mut prefix = PathBuf::new();
        for component in relative.components() {
            prefix.push(component.as_os_str());
            if prefix != relative && seen.get(&prefix) == Some(&false) {
                return Err(CoreError::Archive("ARCHIVE_PATH_COLLISION".into()));
            }
        }
        if !is_dir && seen.keys().any(|path| path.starts_with(&relative)) {
            return Err(CoreError::Archive("ARCHIVE_PATH_COLLISION".into()));
        }
        seen.insert(relative.clone(), is_dir);
        if total.saturating_add(entry.size()) > max_bytes {
            return Err(CoreError::Archive("ARCHIVE_TOO_LARGE".into()));
        }
        let target = destination.join(relative);
        if is_dir {
            if entry.size() != 0 {
                return Err(CoreError::Archive("ARCHIVE_INVALID_DIRECTORY".into()));
            }
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&target)?;
        let copied = copy_bounded(&mut entry, &mut output, max_bytes.saturating_sub(total))?;
        total = total.saturating_add(copied);
        if copied != entry.size() {
            return Err(CoreError::Archive(
                if copied > entry.size() {
                    "ARCHIVE_SIZE_MISMATCH"
                } else {
                    "ARCHIVE_TRUNCATED"
                }
                .into(),
            ));
        }
        output.flush()?;
    }
    Ok(())
}

fn copy_bounded<R: Read, W: Write>(reader: &mut R, writer: &mut W, budget: u64) -> CoreResult<u64> {
    let mut copied = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let remaining = budget.saturating_sub(copied);
        if remaining == 0 {
            let mut extra = [0u8; 1];
            if reader.read(&mut extra)? != 0 {
                return Err(CoreError::Archive("ARCHIVE_TOO_LARGE".into()));
            }
            break;
        }
        let amount = buffer
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));
        let count = reader.read(&mut buffer[..amount])?;
        if count == 0 {
            break;
        }
        writer.write_all(&buffer[..count])?;
        copied += count as u64;
    }
    Ok(copied)
}

fn collect_unitypackages(root: &Path) -> CoreResult<Vec<ImportableFile>> {
    fn walk(root: &Path, current: &Path, files: &mut Vec<ImportableFile>) -> CoreResult<()> {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_symlink() {
                return Err(CoreError::Archive("ARCHIVE_SYMLINK".into()));
            }
            let path = entry.path();
            if ty.is_dir() {
                walk(root, &path, files)?;
            } else if ty.is_file()
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("unitypackage"))
            {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| CoreError::Invalid("INTERNAL_PATH_ERROR"))?
                    .to_string_lossy()
                    .replace('\\', "/");
                let metadata = fs::metadata(&path)?;
                files.push(ImportableFile {
                    id: Uuid::new_v4().to_string(),
                    relative_path: relative,
                    kind: "unitypackage".into(),
                    size_bytes: metadata.len(),
                    content_hash: hash_file(&path)?,
                });
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(files)
}

fn load_item(conn: &Connection, id: String, title: String) -> CoreResult<LibraryItem> {
    let mut stmt=conn.prepare("SELECT id,original_filename,content_hash,version_date,state FROM package_versions WHERE package_id=?1 ORDER BY imported_at DESC,id")?;
    let versions = stmt
        .query_map([&id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut output = Vec::with_capacity(versions.len());
    for (version_id, original_filename, content_hash, version_date, state) in versions {
        output.push(load_version(
            conn,
            version_id,
            original_filename,
            content_hash,
            version_date,
            state,
        )?);
    }
    Ok(LibraryItem {
        id,
        title,
        versions: output,
    })
}

fn load_version(
    conn: &Connection,
    id: String,
    original_filename: String,
    content_hash: String,
    version_date: String,
    state: String,
) -> CoreResult<LibraryVersion> {
    let mut stmt=conn.prepare("SELECT id,relative_path,kind,size_bytes,content_hash FROM package_files WHERE version_id=?1 ORDER BY relative_path")?;
    let files = stmt
        .query_map([&id], |r| {
            Ok(ImportableFile {
                id: r.get(0)?,
                relative_path: r.get(1)?,
                kind: r.get(2)?,
                size_bytes: r.get::<_, i64>(3)? as u64,
                content_hash: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let label = format!("{}@{}", version_date, &content_hash[..8]);
    Ok(LibraryVersion {
        id,
        label,
        version_date,
        content_hash,
        original_filename,
        state,
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        let path = std::env::temp_dir().join(format!("omnooth-core-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn hex_hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn set_zip_unix_member_type(path: &Path, mode: u32) {
        let mut bytes = fs::read(path).unwrap();
        let central = bytes
            .windows(4)
            .position(|window| window == b"PK\x01\x02")
            .unwrap();
        bytes[central + 5] = 3;
        bytes[central + 38..central + 42].copy_from_slice(&(mode << 16).to_le_bytes());
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn initializes_settings_and_rejects_invalid_language() {
        let dir = temp();
        let service = Service::open(&dir).unwrap();
        let settings = service.snapshot().unwrap().settings;
        assert_eq!(settings.language, "ja");
        assert!(!settings.extract_archives);
        let mut invalid = settings;
        invalid.language = "fr".into();
        assert_eq!(
            service.update_settings(invalid).unwrap_err().code(),
            "INVALID_LANGUAGE"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_the_shared_tagged_command_envelope() {
        let parsed: ApplicationCommand=serde_json::from_value(serde_json::json!({"type":"importArchive","input":{"archivePath":"/tmp/a.zip","originalFilename":"A.zip","extractArchive":false,"itemId":null}})).unwrap();
        match parsed {
            ApplicationCommand::ImportArchive(input) => {
                assert_eq!(input.original_filename, "A.zip");
                assert_eq!(input.item_id, None);
            }
            _ => panic!("wrong command variant"),
        }
    }

    #[test]
    fn rejects_unsupported_extensions_and_zip_signature_mismatches_with_codes() {
        let dir = temp();
        let service = Service::open(&dir).unwrap();
        let source = dir.join("source.bin");
        fs::write(&source, b"not a package").unwrap();
        let png = service
            .import_archive(ImportArchiveInput {
                archive_path: source.clone(),
                original_filename: "image.png".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap_err();
        assert_eq!(png.code(), "UNSUPPORTED_FORMAT");
        let mislabeled = service
            .import_archive(ImportArchiveInput {
                archive_path: source,
                original_filename: "claimed.zip".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap_err();
        assert_eq!(mislabeled.code(), "ARCHIVE_FORMAT_MISMATCH");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_relative_library_path_without_creating_it() {
        let dir = temp();
        let service = Service::open(&dir).unwrap();
        let mut settings = service.snapshot().unwrap().settings;
        let relative = format!("relative-library-{}", Uuid::new_v4());
        settings.library_path = relative.clone();
        let error = service.update_settings(settings).unwrap_err();
        assert_eq!(error.code(), "INVALID_LIBRARY_PATH");
        assert!(!Path::new(&relative).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_import_repairs_missing_or_corrupt_registered_original() {
        let dir = temp();
        let service = Service::open(&dir).unwrap();
        let source = dir.join("Repair.unitypackage");
        fs::write(&source, b"original bytes").unwrap();
        let input = ImportArchiveInput {
            archive_path: source.clone(),
            original_filename: "Repair.unitypackage".into(),
            extract_archive: false,
            item_id: None,
        };
        let first = service.import_archive(input.clone()).unwrap();
        let object = dir
            .join("Library/objects")
            .join(&first.item.versions[0].content_hash);
        fs::remove_file(&object).unwrap();
        let second = service.import_archive(input.clone()).unwrap();
        assert!(second.reused);
        assert_eq!(
            hash_file(&object).unwrap(),
            first.item.versions[0].content_hash
        );
        fs::write(&object, b"corrupt").unwrap();
        let third = service.import_archive(input).unwrap();
        assert!(third.reused);
        assert_eq!(
            hash_file(&object).unwrap(),
            first.item.versions[0].content_hash
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn atomic_copy_rejects_a_source_that_changed_after_hashing() {
        let dir = temp();
        let source = dir.join("source");
        let destination = dir.join("objects/hash");
        fs::write(&source, b"changed").unwrap();
        let error = atomic_copy(&source, &destination, &hex_hash(b"original")).unwrap_err();
        assert_eq!(error.code(), "SOURCE_CHANGED_DURING_IMPORT");
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn explicit_extraction_rejects_a_corrupt_original_object() {
        let dir = temp();
        let service = Service::open(&dir).unwrap();
        let source = dir.join("Stored.zip");
        let file = File::create(&source).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(
            "item.unitypackage",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"bytes").unwrap();
        zip.finish().unwrap();
        let stored = service
            .import_archive(ImportArchiveInput {
                archive_path: source,
                original_filename: "Stored.zip".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap();
        let object = dir
            .join("Library/objects")
            .join(&stored.item.versions[0].content_hash);
        fs::write(&object, b"tampered").unwrap();
        let error = service
            .extract_archive(&stored.item.versions[0].id)
            .unwrap_err();
        assert_eq!(error.code(), "OBJECT_STORE_CORRUPT");
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn distinct_case_sensitive_project_directories_are_not_deduplicated() {
        let dir = temp();
        let upper = dir.join("Project");
        let lower = dir.join("project");
        for root in [&upper, &lower] {
            fs::create_dir_all(root.join("Assets")).unwrap();
            fs::create_dir_all(root.join("ProjectSettings")).unwrap();
            fs::write(
                root.join("ProjectSettings/ProjectVersion.txt"),
                "m_EditorVersion: 2022.3.1f1\n",
            )
            .unwrap();
        }
        if fs::canonicalize(&upper).unwrap() == fs::canonicalize(&lower).unwrap() {
            let _ = fs::remove_dir_all(dir);
            return;
        }
        let service = Service::open(dir.join("app")).unwrap();
        let first = service.link_project(upper).unwrap();
        let second = service.link_project(lower).unwrap();
        assert_ne!(first.id, second.id);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn migrates_legacy_database_with_live_wal_and_preserves_existing_library_paths() {
        let dir = temp();
        let old_dir = dir.join("old-app-id");
        let new_dir = dir.join("new-app-id");
        let old = Service::open(&old_dir).unwrap();
        let source = dir.join("Legacy.unitypackage");
        fs::write(&source, b"legacy bytes").unwrap();
        old.import_archive(ImportArchiveInput {
            archive_path: source,
            original_filename: "Legacy.unitypackage".into(),
            extract_archive: false,
            item_id: None,
        })
        .unwrap();
        let old_library = old.snapshot().unwrap().settings.library_path;
        let writer = Connection::open(old_dir.join("omnooth.sqlite")).unwrap();
        writer.pragma_update(None, "journal_mode", "WAL").unwrap();
        writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        writer.execute_batch("CREATE TABLE wal_probe(value TEXT); INSERT INTO wal_probe VALUES('committed in WAL');").unwrap();
        assert!(migrate_legacy_database(&new_dir, &old_dir).unwrap());
        let copied = Connection::open(new_dir.join("omnooth.sqlite")).unwrap();
        let wal_value: String = copied
            .query_row("SELECT value FROM wal_probe", [], |r| r.get(0))
            .unwrap();
        assert_eq!(wal_value, "committed in WAL");
        drop(copied);
        drop(writer);
        let migrated = Service::open(&new_dir).unwrap();
        assert_eq!(
            migrated.snapshot().unwrap().settings.library_path,
            old_library
        );
        assert_eq!(migrated.snapshot().unwrap().items.len(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_migration_never_replaces_an_existing_new_database() {
        let dir = temp();
        let old_dir = dir.join("old");
        let new_dir = dir.join("new");
        fs::create_dir_all(&old_dir).unwrap();
        fs::create_dir_all(&new_dir).unwrap();
        let old = Connection::open(old_dir.join("omnooth.sqlite")).unwrap();
        old.execute_batch("CREATE TABLE marker(value TEXT); INSERT INTO marker VALUES('old');")
            .unwrap();
        drop(old);
        let new = Connection::open(new_dir.join("omnooth.sqlite")).unwrap();
        new.execute_batch("CREATE TABLE marker(value TEXT); INSERT INTO marker VALUES('new');")
            .unwrap();
        drop(new);
        assert!(!migrate_legacy_database(&new_dir, &old_dir).unwrap());
        let current = Connection::open(new_dir.join("omnooth.sqlite")).unwrap();
        let marker: String = current
            .query_row("SELECT value FROM marker", [], |r| r.get(0))
            .unwrap();
        assert_eq!(marker, "new");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn links_only_valid_projects_and_deduplicates_canonical_path() {
        let dir = temp();
        let project = dir.join("Existing");
        fs::create_dir_all(project.join("Assets")).unwrap();
        fs::create_dir_all(project.join("ProjectSettings")).unwrap();
        fs::write(
            project.join("ProjectSettings/ProjectVersion.txt"),
            "m_EditorVersion: 2022.3.20f1\n",
        )
        .unwrap();
        let service = Service::open(dir.join("app")).unwrap();
        let first = service.link_project(&project).unwrap();
        let second = service.link_project(project.join(".")).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.unity_version, "2022.3.20f1");
        let invalid = dir.join("NoAssets");
        fs::create_dir_all(invalid.join("ProjectSettings")).unwrap();
        fs::write(
            invalid.join("ProjectSettings/ProjectVersion.txt"),
            "m_EditorVersion: 2022\n",
        )
        .unwrap();
        assert_eq!(
            service.link_project(invalid).unwrap_err().code(),
            "ASSETS_MISSING"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn imports_direct_unitypackage_by_content_hash_and_reuses_first_version() {
        let dir = temp();
        let service = Service::open(&dir).unwrap();
        let source = dir.join("Custom_v1.unitypackage");
        fs::write(&source, b"fake package bytes").unwrap();
        let input = ImportArchiveInput {
            archive_path: source,
            original_filename: "Custom_v1.unitypackage".into(),
            extract_archive: false,
            item_id: None,
        };
        let a = service.import_archive(input.clone()).unwrap();
        let b = service.import_archive(input).unwrap();
        assert_eq!(
            a.item.versions[0].content_hash,
            b.item.versions[0].content_hash
        );
        assert_eq!(a.item.id, b.item.id);
        assert!(!a.reused);
        assert!(b.reused);
        assert!(a.item.versions[0]
            .label
            .starts_with(&a.item.versions[0].version_date));
        assert_eq!(a.item.versions[0].files.len(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn same_exact_filename_groups_distinct_hashes_as_versions_and_reuses_each_hash() {
        let dir = temp();
        let service = Service::open(&dir).unwrap();
        let first_path = dir.join("original-a");
        let second_path = dir.join("original-b");
        fs::write(&first_path, b"first content").unwrap();
        fs::write(&second_path, b"second content").unwrap();
        let first = service
            .import_archive(ImportArchiveInput {
                archive_path: first_path,
                original_filename: "Asset.unitypackage".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap();
        let second = service
            .import_archive(ImportArchiveInput {
                archive_path: second_path,
                original_filename: "Asset.unitypackage".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap();
        assert_eq!(first.item.id, second.item.id);
        assert_eq!(second.item.versions.len(), 2);
        let duplicate_path = dir.join("duplicate");
        fs::write(&duplicate_path, b"second content").unwrap();
        let duplicate = service
            .import_archive(ImportArchiveInput {
                archive_path: duplicate_path,
                original_filename: "Asset.unitypackage".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap();
        assert!(duplicate.reused);
        assert_eq!(duplicate.item.versions.len(), 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn changing_library_path_keeps_existing_content_addressed_objects_accessible() {
        let dir = temp();
        let service = Service::open(&dir).unwrap();
        let source = dir.join("One.unitypackage");
        fs::write(&source, b"original package").unwrap();
        let imported = service
            .import_archive(ImportArchiveInput {
                archive_path: source,
                original_filename: "One.unitypackage".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap();
        let hash = imported.item.versions[0].content_hash.clone();
        let old_object = dir.join("Library/objects").join(&hash);
        let mut settings = service.snapshot().unwrap().settings;
        settings.library_path = dir.join("new-library").to_string_lossy().into_owned();
        service.update_settings(settings).unwrap();
        assert!(old_object.is_file());
        assert_eq!(
            service.snapshot().unwrap().items[0].versions[0].content_hash,
            hash
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn extracts_zip_and_only_lists_unitypackages() {
        let dir = temp();
        let service = Service::open(&dir).unwrap();
        let source = dir.join("Bundle.zip");
        let file = File::create(&source).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("folder/Chosen.unitypackage", options)
            .unwrap();
        zip.write_all(b"package").unwrap();
        zip.start_file("image.png", options).unwrap();
        zip.write_all(b"image").unwrap();
        zip.finish().unwrap();
        let result = service
            .import_archive(ImportArchiveInput {
                archive_path: source,
                original_filename: "Bundle.zip".into(),
                extract_archive: true,
                item_id: None,
            })
            .unwrap();
        assert_eq!(result.item.versions[0].state, "extracted");
        assert_eq!(result.item.versions[0].files.len(), 1);
        assert_eq!(
            result.item.versions[0].files[0].relative_path,
            "folder/Chosen.unitypackage"
        );
        assert!(dir
            .join("Library/objects")
            .join(&result.item.versions[0].content_hash)
            .exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_zip_slip_entries() {
        let dir = temp();
        let source = dir.join("bad.zip");
        let file = File::create(&source).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("../escape.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
        let service = Service::open(dir.join("app")).unwrap();
        let result = service.import_archive(ImportArchiveInput {
            archive_path: source,
            original_filename: "bad.zip".into(),
            extract_archive: true,
            item_id: None,
        });
        assert_eq!(result.unwrap_err().code(), "ARCHIVE_UNSAFE");
        assert!(!dir.join("escape.txt").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_duplicate_normalized_paths_and_file_directory_collisions() {
        let dir = temp();
        let duplicate = dir.join("duplicate.zip");
        let file = File::create(&duplicate).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("folder//entry", opts).unwrap();
        zip.write_all(b"a").unwrap();
        zip.start_file("folder/entry", opts).unwrap();
        zip.write_all(b"b").unwrap();
        zip.finish().unwrap();
        let err = extract_zip_safely(&duplicate, &dir.join("duplicate-out")).unwrap_err();
        assert_eq!(err.code(), "ARCHIVE_DUPLICATE_PATH");

        let collision = dir.join("collision.zip");
        let file = File::create(&collision).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("node", opts).unwrap();
        zip.write_all(b"file").unwrap();
        zip.start_file("node/child", opts).unwrap();
        zip.write_all(b"child").unwrap();
        zip.finish().unwrap();
        let err = extract_zip_safely(&collision, &dir.join("collision-out")).unwrap_err();
        assert_eq!(err.code(), "ARCHIVE_PATH_COLLISION");
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_zip_symlinks_and_special_unix_members() {
        let dir = temp();
        let symlink = dir.join("symlink.zip");
        let file = File::create(&symlink).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("link", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"target").unwrap();
        zip.finish().unwrap();
        set_zip_unix_member_type(&symlink, 0o120777);
        assert_eq!(
            extract_zip_safely(&symlink, &dir.join("symlink-out"))
                .unwrap_err()
                .code(),
            "ARCHIVE_SYMLINK"
        );
        let fifo = dir.join("fifo.zip");
        let file = File::create(&fifo).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("pipe", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
        set_zip_unix_member_type(&fifo, 0o010644);
        assert_eq!(
            extract_zip_safely(&fifo, &dir.join("fifo-out"))
                .unwrap_err()
                .code(),
            "ARCHIVE_SPECIAL_FILE"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn bounded_streaming_copy_never_writes_past_small_budget() {
        let mut source = std::io::Cursor::new(b"0123456789".to_vec());
        let mut output = Vec::new();
        let error = copy_bounded(&mut source, &mut output, 4).unwrap_err();
        assert_eq!(error.code(), "ARCHIVE_TOO_LARGE");
        assert_eq!(&output, b"0123");
    }

    #[test]
    fn allows_explicit_later_extraction_without_removing_original() {
        let dir = temp();
        let service = Service::open(&dir).unwrap();
        let source = dir.join("Stored.zip");
        let file = File::create(&source).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(
            "asset.unitypackage",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"package").unwrap();
        zip.finish().unwrap();
        let imported = service
            .import_archive(ImportArchiveInput {
                archive_path: source,
                original_filename: "Stored.zip".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap();
        assert_eq!(imported.item.versions[0].state, "stored");
        assert!(imported.item.versions[0].files.is_empty());
        let extracted = service
            .extract_archive(&imported.item.versions[0].id)
            .unwrap();
        assert_eq!(extracted.state, "extracted");
        assert_eq!(extracted.files.len(), 1);
        assert!(dir
            .join("Library/objects")
            .join(&extracted.content_hash)
            .exists());
        let _ = fs::remove_dir_all(dir);
    }
}

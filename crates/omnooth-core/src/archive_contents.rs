//! ZIP directory inspection never inflates entries. Import staging inflates only the
//! explicitly selected unitypackages and keeps them until the adapter confirms success.

use super::*;
use std::{
    collections::HashSet,
    io::{Seek, SeekFrom},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    pub id: String,
    pub relative_path: String,
    pub kind: String,
    pub size_bytes: u64,
    pub encrypted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveContents {
    pub version_id: String,
    pub entries: Vec<ArchiveEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedUnityPackage {
    pub operation_id: String,
    pub path: PathBuf,
    pub content_hash: String,
}

/// Persisted under the app's private staging directory; not a library extraction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedUnityImport {
    job_id: String,
    packages: Vec<PreparedUnityPackage>,
}

impl PreparedUnityImport {
    pub fn job_id(&self) -> &str {
        &self.job_id
    }
    pub fn packages(&self) -> &[PreparedUnityPackage] {
        &self.packages
    }
}

struct IndexedEntry {
    index: usize,
    entry: ArchiveEntry,
}

fn zip_entries(archive: &mut ZipArchive<File>, version_id: &str) -> CoreResult<Vec<IndexedEntry>> {
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(CoreError::Archive("ARCHIVE_TOO_MANY_ENTRIES".into()));
    }
    let mut entries = Vec::with_capacity(archive.len());
    let mut seen = HashMap::new();
    let mut parents = HashSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        // Raw access reads metadata, including for encrypted/unsupported members.
        let file = archive
            .by_index_raw(index)
            .map_err(|e| CoreError::Archive(e.to_string()))?;
        let relative = safe_member_path(file.name())?;
        let is_dir = file.is_dir();
        if let Some(mode) = file.unix_mode() {
            match mode & 0o170000 {
                0 => {}
                0o100000 if !is_dir => {}
                0o040000 if is_dir => {}
                0o120000 => return Err(CoreError::Archive("ARCHIVE_SYMLINK".into())),
                _ => return Err(CoreError::Archive("ARCHIVE_SPECIAL_FILE".into())),
            }
        }
        // Reject aliases on case-insensitive filesystems as well as exact duplicates.
        let relative_path = relative.to_string_lossy().replace('\\', "/");
        let key = relative_path.to_lowercase();
        if seen.contains_key(&key) {
            return Err(CoreError::Archive("ARCHIVE_DUPLICATE_PATH".into()));
        }
        if !is_dir && parents.contains(&key) {
            return Err(CoreError::Archive("ARCHIVE_PATH_COLLISION".into()));
        }
        for (offset, _) in key.match_indices('/') {
            let parent = &key[..offset];
            if seen.get(parent) == Some(&false) {
                return Err(CoreError::Archive("ARCHIVE_PATH_COLLISION".into()));
            }
            parents.insert(parent.to_owned());
        }
        seen.insert(key, is_dir);
        if is_dir && file.size() != 0 {
            return Err(CoreError::Archive("ARCHIVE_INVALID_DIRECTORY".into()));
        }
        total = total
            .checked_add(file.size())
            .ok_or_else(|| CoreError::Archive("ARCHIVE_TOO_LARGE".into()))?;
        if total > MAX_EXPANDED_BYTES {
            return Err(CoreError::Archive("ARCHIVE_TOO_LARGE".into()));
        }
        let kind = if is_dir {
            "directory"
        } else if relative_path
            .to_ascii_lowercase()
            .ends_with(".unitypackage")
        {
            "unitypackage"
        } else {
            "file"
        };
        entries.push(IndexedEntry {
            index,
            entry: ArchiveEntry {
                id: format!(
                    "{:x}",
                    Sha256::digest(format!("{version_id}\0{relative_path}"))
                ),
                relative_path,
                kind: kind.into(),
                size_bytes: file.size(),
                encrypted: file.encrypted(),
            },
        });
    }
    Ok(entries)
}

impl Service {
    fn zip_source(&self, version_id: &str) -> CoreResult<(PathBuf, String)> {
        let conn = self.connect()?;
        let (filename, path, hash): (String, String, String) = conn.query_row(
            "SELECT original_filename,object_path,content_hash FROM package_versions WHERE id=?1",
            [version_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?.ok_or(CoreError::Invalid("VERSION_NOT_FOUND"))?;
        if !filename.to_ascii_lowercase().ends_with(".zip") {
            return Err(CoreError::Invalid("INSPECTION_UNSUPPORTED"));
        }
        if !Path::new(&path).is_file() {
            return Err(CoreError::Invalid("OBJECT_STORE_MISSING"));
        }
        Ok((PathBuf::from(path), hash))
    }

    /// Reads only the ZIP directory, leaving original, extraction state and DB untouched.
    /// Entry sizes are declarations; SHA-256/CRC verification happens when materialized.
    pub fn inspect_archive(&self, version_id: &str) -> CoreResult<ArchiveContents> {
        let (path, _) = self.zip_source(version_id)?;
        let mut archive =
            ZipArchive::new(File::open(path)?).map_err(|e| CoreError::Archive(e.to_string()))?;
        let mut entries: Vec<_> = zip_entries(&mut archive, version_id)?
            .into_iter()
            .map(|indexed| indexed.entry)
            .collect();
        entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        Ok(ArchiveContents {
            version_id: version_id.into(),
            entries,
        })
    }

    /// Verifies the original, then inflates only selected unitypackages in explicit order.
    /// No persistent library extraction or project Assets writes are performed here.
    pub fn prepare_zip_unity_import(
        &self,
        version_id: &str,
        selected_ids: &[String],
    ) -> CoreResult<PreparedUnityImport> {
        if selected_ids.is_empty()
            || selected_ids.iter().collect::<HashSet<_>>().len() != selected_ids.len()
        {
            return Err(CoreError::Invalid("INVALID_CONTENT_SELECTION"));
        }
        let (path, expected_hash) = self.zip_source(version_id)?;
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)?;
        if format!("{:x}", hasher.finalize()) != expected_hash {
            return Err(CoreError::Invalid("OBJECT_STORE_CORRUPT"));
        }
        file.seek(SeekFrom::Start(0))?;
        let mut archive = ZipArchive::new(file).map_err(|e| CoreError::Archive(e.to_string()))?;
        let entries = zip_entries(&mut archive, version_id)?;
        let indexed: HashMap<_, _> = entries
            .iter()
            .map(|entry| (entry.entry.id.as_str(), entry))
            .collect();
        let selected = selected_ids
            .iter()
            .map(|id| {
                let entry = indexed
                    .get(id.as_str())
                    .ok_or(CoreError::Invalid("INVALID_CONTENT_SELECTION"))?;
                if entry.entry.kind != "unitypackage" {
                    return Err(CoreError::Invalid("INVALID_CONTENT_SELECTION"));
                }
                if entry.entry.encrypted {
                    return Err(CoreError::Invalid("ARCHIVE_ENCRYPTED"));
                }
                Ok(*entry)
            })
            .collect::<CoreResult<Vec<_>>>()?;
        let mut basenames = HashSet::new();
        for entry in &selected {
            let name = Path::new(&entry.entry.relative_path)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(CoreError::Invalid("INVALID_CONTENT_SELECTION"))?;
            if !basenames.insert(name.to_lowercase()) {
                return Err(CoreError::Invalid("UNITYPACKAGE_NAME_COLLISION"));
            }
        }
        let job_id = Uuid::new_v4().to_string();
        let staging_root = self.app_data_dir.join("unity-staging");
        fs::create_dir_all(&staging_root)?;
        let staging = staging_root.join(&job_id);
        fs::create_dir(&staging)?;
        let result = (|| {
            let mut packages = Vec::new();
            let mut total = 0u64;
            for entry in selected {
                // Retain a safe basename for Unity's package-name completion callbacks.
                let basename = Path::new(&entry.entry.relative_path)
                    .file_name()
                    .ok_or(CoreError::Invalid("INVALID_CONTENT_SELECTION"))?;
                let directory = staging.join(&entry.entry.id);
                fs::create_dir(&directory)?;
                let target = directory.join(basename);
                let mut output = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)?;
                let mut member = archive
                    .by_index(entry.index)
                    .map_err(|e| CoreError::Archive(e.to_string()))?;
                let copied = copy_bounded(
                    &mut member,
                    &mut output,
                    MAX_EXPANDED_BYTES.saturating_sub(total),
                )?;
                if copied != entry.entry.size_bytes {
                    return Err(CoreError::Archive("ARCHIVE_SIZE_MISMATCH".into()));
                }
                total += copied;
                output.sync_all()?;
                packages.push(PreparedUnityPackage {
                    operation_id: entry.entry.id.clone(),
                    path: fs::canonicalize(&target)?,
                    content_hash: hash_file(&target)?,
                });
            }
            let prepared = PreparedUnityImport { job_id, packages };
            let bytes = serde_json::to_vec(&prepared)
                .map_err(|_| CoreError::Invalid("SERIALIZATION_ERROR"))?;
            let mut manifest = File::create(staging.join("prepared.json"))?;
            manifest.write_all(&bytes)?;
            manifest.sync_all()?;
            Ok(prepared)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(staging);
        }
        result
    }

    /// Reloads an outstanding job after restart. File paths are confined to its staging root.
    pub fn load_prepared_unity_import(&self, job_id: &str) -> CoreResult<PreparedUnityImport> {
        let root = self.unity_staging_directory(job_id)?;
        let prepared: PreparedUnityImport =
            serde_json::from_slice(&fs::read(root.join("prepared.json"))?)
                .map_err(|_| CoreError::Invalid("INVALID_IMPORT_STAGING"))?;
        if prepared.job_id != job_id || prepared.packages.is_empty() {
            return Err(CoreError::Invalid("INVALID_IMPORT_STAGING"));
        }
        let canonical_root = fs::canonicalize(root)?;
        for package in &prepared.packages {
            let path = fs::canonicalize(&package.path)?;
            if !path.starts_with(&canonical_root) || hash_file(&path)? != package.content_hash {
                return Err(CoreError::Invalid("INVALID_IMPORT_STAGING"));
            }
        }
        Ok(prepared)
    }

    fn unity_staging_directory(&self, job_id: &str) -> CoreResult<PathBuf> {
        if Uuid::parse_str(job_id).is_err() {
            return Err(CoreError::Invalid("INVALID_IMPORT_JOB"));
        }
        Ok(self.app_data_dir.join("unity-staging").join(job_id))
    }

    /// Called only after the adapter has validated a completed result for this exact job.
    /// Failure, cancellation and unknown outcomes must retain staging for recovery.
    pub fn cleanup_completed_unity_import(&self, job_id: &str) -> CoreResult<()> {
        let root = self.unity_staging_directory(job_id)?;
        match fs::remove_dir_all(root) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

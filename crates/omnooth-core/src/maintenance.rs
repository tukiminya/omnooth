//! Explicit, revalidated cleanup. A shared filesystem lock excludes active library operations.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    pub plan_hash: String,
    pub versions: Vec<(String, String)>,
    pub staging_count: usize,
    pub backup_count: usize,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    pub removed_versions: usize,
    pub removed_directories: usize,
    pub retained_paths: Vec<String>,
}
#[derive(Serialize, Deserialize)]
struct CleanupJournal {
    token: String,
    moves: Vec<(PathBuf, PathBuf, String)>,
}

#[derive(Serialize)]
struct Candidate {
    kind: String,
    id: String,
    title: String,
    paths: Vec<(PathBuf, String, u64)>,
}

fn inventory(path: &Path) -> CoreResult<(BTreeMap<String, Option<String>>, u64)> {
    fn walk(
        root: &Path,
        path: &Path,
        entries: &mut BTreeMap<String, Option<String>>,
        bytes: &mut u64,
    ) -> CoreResult<()> {
        if entries.len() >= MAX_ARCHIVE_ENTRIES {
            return Err(CoreError::Invalid("CLEANUP_UNSAFE"));
        }
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            return Err(CoreError::Invalid("CLEANUP_UNSAFE"));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| CoreError::Invalid("CLEANUP_UNSAFE"))?
            .to_string_lossy()
            .into_owned();
        if metadata.is_file() {
            *bytes = bytes
                .checked_add(metadata.len())
                .ok_or(CoreError::Invalid("CLEANUP_UNSAFE"))?;
            if *bytes > MAX_EXPANDED_BYTES {
                return Err(CoreError::Invalid("CLEANUP_UNSAFE"));
            }
            entries.insert(relative, Some(hash_file(path)?));
        } else {
            entries.insert(relative, None);
            for entry in fs::read_dir(path)? {
                walk(root, &entry?.path(), entries, bytes)?;
            }
        }
        Ok(())
    }
    let mut entries = BTreeMap::new();
    let mut bytes = 0;
    walk(path, path, &mut entries, &mut bytes)?;
    Ok((entries, bytes))
}

fn tree(path: &Path) -> CoreResult<(String, u64)> {
    let (entries, bytes) = inventory(path)?;
    Ok((
        format!("{:x}", Sha256::digest(serde_json::to_vec(&entries)?)),
        bytes,
    ))
}
pub(crate) fn record_extraction(conn: &Connection, version: &str, path: &Path) -> CoreResult<()> {
    let (entries, _) = inventory(path)?;
    conn.execute(
        "DELETE FROM archive_extraction_files WHERE version_id=?1",
        [version],
    )?;
    for (relative, hash) in entries {
        conn.execute("INSERT INTO archive_extraction_files(version_id,relative_path,content_hash) VALUES(?1,?2,?3)",params![version,relative,hash])?;
    }
    Ok(())
}

pub(crate) fn migrate(conn: &mut Connection) -> CoreResult<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let tx = conn.transaction()?;
    if version < 9 {
        tx.execute_batch("CREATE TABLE cleanup_commits(token TEXT PRIMARY KEY);")?;
    }
    tx.execute_batch("CREATE TABLE archive_extraction_files(version_id TEXT NOT NULL REFERENCES package_versions(id) ON DELETE CASCADE,relative_path TEXT NOT NULL,content_hash TEXT CHECK(content_hash IS NULL OR length(content_hash)=64),PRIMARY KEY(version_id,relative_path));")?;
    tx.pragma_update(None, "user_version", 10)?;
    tx.commit()?;
    Ok(())
}

impl Service {
    fn save_cleanup_journal(&self, journal: &CleanupJournal) -> CoreResult<()> {
        let temporary = self.app_data_dir.join("cleanup-journal.tmp");
        let target = self.app_data_dir.join("cleanup-journal.json");
        let mut file = File::create(&temporary)?;
        file.write_all(&serde_json::to_vec(journal)?)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temporary, target)?;
        #[cfg(unix)]
        {
            File::open(&self.app_data_dir)?.sync_all()?;
        }
        Ok(())
    }
    pub(crate) fn recover_cleanup(&self, conn: &Connection) -> CoreResult<()> {
        let path = self.app_data_dir.join("cleanup-journal.json");
        if !path.exists() {
            return Ok(());
        }
        let journal: CleanupJournal = serde_json::from_slice(&fs::read(&path)?)?;
        if Uuid::parse_str(&journal.token).is_err() {
            return Err(CoreError::Invalid("CLEANUP_RECOVERY_REQUIRED"));
        }
        let committed: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM cleanup_commits WHERE token=?1)",
            [&journal.token],
            |r| r.get(0),
        )?;
        let mut retained = false;
        for (original, temporary, stamp) in journal.moves.iter().rev() {
            let expected = original.with_file_name(format!(
                ".omnooth-cleanup-{}-{}",
                journal.token,
                original
                    .file_name()
                    .ok_or(CoreError::Invalid("CLEANUP_RECOVERY_REQUIRED"))?
                    .to_string_lossy()
            ));
            if *temporary != expected {
                return Err(CoreError::Invalid("CLEANUP_RECOVERY_REQUIRED"));
            }
            if !temporary.exists() {
                continue;
            }
            if tree(temporary)?.0 != *stamp {
                if committed {
                    retained = true;
                    continue;
                }
                return Err(CoreError::Invalid("CLEANUP_RECOVERY_REQUIRED"));
            }
            if committed {
                let result = if temporary.is_dir() {
                    fs::remove_dir_all(temporary)
                } else {
                    fs::remove_file(temporary)
                };
                if result.is_err() {
                    retained = true;
                }
            } else if original.exists() {
                return Err(CoreError::Invalid("CLEANUP_RECOVERY_REQUIRED"));
            } else {
                fs::rename(temporary, original)?;
            }
        }
        if !retained {
            fs::remove_file(path)?;
            conn.execute(
                "DELETE FROM cleanup_commits WHERE token=?1",
                [journal.token],
            )?;
        }
        Ok(())
    }

    pub(crate) fn library_access(&self, exclusive: bool) -> CoreResult<File> {
        let path = self.app_data_dir.join("library-maintenance.lock");
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(CoreError::Invalid("LIBRARY_BUSY"));
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        if exclusive {
            file.try_lock()
                .map_err(|_| CoreError::Invalid("LIBRARY_BUSY"))?;
        } else {
            file.try_lock_shared()
                .map_err(|_| CoreError::Invalid("LIBRARY_BUSY"))?;
        }
        Ok(file)
    }
    fn cleanup_candidates(&self, conn: &Connection) -> CoreResult<Vec<Candidate>> {
        if conn.query_row("SELECT EXISTS(SELECT 1 FROM install_jobs WHERE status IN ('preparing','waitingForUnity','recovering'))",[],|r|r.get::<_,bool>(0))? {return Err(CoreError::Invalid("LIBRARY_BUSY"));}
        let mut candidates = Vec::new();
        if !read_settings(conn)?.keep_old_versions {
            // An independent adapter request can outlive its call; retain originals while it exists.
            let staged = self.app_data_dir.join("unity-staging");
            if !staged.exists() || fs::read_dir(staged)?.next().is_none() {
                let mut stmt=conn.prepare("SELECT v.id,v.package_id,p.title,v.content_hash,v.object_path,v.extracted_path FROM package_versions v JOIN packages p ON p.id=v.package_id WHERE v.id<>(SELECT id FROM package_versions newest WHERE newest.package_id=v.package_id ORDER BY imported_at DESC,id LIMIT 1) AND NOT EXISTS(SELECT 1 FROM project_packages WHERE version_id=v.id) AND NOT EXISTS(SELECT 1 FROM package_dependencies WHERE required_version_id=v.id) AND NOT EXISTS(SELECT 1 FROM install_plans,json_tree(install_plans.payload) WHERE atom=v.id) AND NOT EXISTS(SELECT 1 FROM install_jobs,json_tree(install_jobs.journal) WHERE atom=v.id) ORDER BY v.id")?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, Option<String>>(5)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                let mut verified = Vec::new();
                for row in rows {
                    if self.require_item_visible(&row.1).is_err() {
                        continue;
                    }
                    if let Some(path) = &row.5 {
                        let mut stored=conn.prepare("SELECT relative_path,content_hash FROM archive_extraction_files WHERE version_id=?1 ORDER BY relative_path")?;
                        let expected = stored
                            .query_map([&row.0], |r| {
                                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                            })?
                            .collect::<Result<BTreeMap<_, _>, _>>()?;
                        if expected.is_empty() || inventory(Path::new(path))?.0 != expected {
                            continue;
                        }
                    }
                    verified.push(row);
                }
                let rows = verified;
                let eligible: BTreeSet<_> = rows.iter().map(|r| r.0.clone()).collect();
                let mut reference_paths: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
                let mut references =
                    conn.prepare("SELECT id,object_path,extracted_path FROM package_versions")?;
                for row in references.query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                })? {
                    let (id, object, extracted) = row?;
                    for path in std::iter::once(object).chain(extracted) {
                        let path = PathBuf::from(path);
                        let canonical = fs::canonicalize(&path).unwrap_or(path);
                        reference_paths
                            .entry(canonical)
                            .or_default()
                            .push(id.clone());
                    }
                }
                let mut claimed = BTreeSet::new();
                for (id, package, title, hash, object, extracted) in rows {
                    if self.require_item_visible(&package).is_err() {
                        continue;
                    }
                    let mut paths = Vec::new();
                    let object = PathBuf::from(object);
                    for path in std::iter::once(object.clone()).chain(extracted.map(PathBuf::from))
                    {
                        let parent = path.parent().ok_or(CoreError::Invalid("CLEANUP_UNSAFE"))?;
                        if !path.is_absolute()
                            || path.file_name().and_then(|s| s.to_str()) != Some(&hash)
                            || !matches!(
                                parent.file_name().and_then(|s| s.to_str()),
                                Some("objects" | "extracted")
                            )
                            || fs::symlink_metadata(parent)?.file_type().is_symlink()
                        {
                            return Err(CoreError::Invalid("CLEANUP_UNSAFE"));
                        }
                        let canonical = fs::canonicalize(&path)?;
                        if reference_paths
                            .get(&canonical)
                            .is_some_and(|ids| ids.iter().any(|id| !eligible.contains(id)))
                            || !claimed.insert(canonical)
                        {
                            continue;
                        }
                        let (stamp, bytes) = tree(&path)?;
                        if path == object && hash_file(&path)? != hash {
                            return Err(CoreError::Invalid("OBJECT_STORE_CORRUPT"));
                        }
                        paths.push((path, stamp, bytes));
                    }
                    let date: String = conn.query_row(
                        "SELECT version_date FROM package_versions WHERE id=?1",
                        [&id],
                        |r| r.get(0),
                    )?;
                    candidates.push(Candidate {
                        kind: "version".into(),
                        id,
                        title: format!("{title} · {date}@{}", &hash[..8]),
                        paths,
                    });
                }
            }
        }
        let cutoff =
            (Utc::now() - chrono::Duration::days(7)).to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut plans=conn.prepare("SELECT id,payload FROM install_plans WHERE created_at<?1 AND NOT EXISTS(SELECT 1 FROM install_jobs WHERE plan_id=install_plans.id) ORDER BY id")?;
        for (id, payload) in plans
            .query_map([cutoff], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
        {
            let path = self.app_data_dir.join("install-staging").join(&id);
            let mut paths = Vec::new();
            if path.exists() {
                if fs::symlink_metadata(&path)?.file_type().is_symlink() {
                    continue;
                }
                let path = fs::canonicalize(path)?;
                let payload: serde_json::Value = serde_json::from_str(&payload)?;
                let expected: BTreeMap<PathBuf, String> = payload
                    .get("packages")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|v| {
                        Some((
                            PathBuf::from(v.pointer("/file/path")?.as_str()?),
                            v.get("hash")?.as_str()?.into(),
                        ))
                    })
                    .collect();
                if fs::read_dir(&path)?.any(|entry| {
                    entry.is_err()
                        || entry
                            .as_ref()
                            .is_ok_and(|e| !expected.contains_key(&e.path()))
                }) {
                    continue;
                }
                if expected.iter().any(|(p, hash)| {
                    p.parent() != Some(path.as_path()) || hash_file(p).ok().as_ref() != Some(hash)
                }) {
                    continue;
                }
                let (stamp, bytes) = tree(&path)?;
                paths.push((path, stamp, bytes));
            }
            candidates.push(Candidate {
                kind: "staging".into(),
                id,
                title: String::new(),
                paths,
            });
        }
        let cutoff =
            (Utc::now() - chrono::Duration::days(30)).to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut jobs=conn.prepare("SELECT id,journal FROM install_jobs WHERE status IN ('succeeded','restored') AND created_at<?1 ORDER BY id")?;
        for (id, journal) in jobs
            .query_map([cutoff], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
        {
            let path = self.app_data_dir.join("install-backups").join(&id);
            if !path.exists() {
                continue;
            }
            let journal: serde_json::Value = serde_json::from_str(&journal)?;
            let expected: BTreeMap<PathBuf, String> = journal
                .get("entries")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| {
                    Some((
                        PathBuf::from(v.get("backup")?.as_str()?),
                        v.get("before")?.as_str()?.into(),
                    ))
                })
                .collect();
            if fs::read_dir(&path)?.any(|entry| {
                entry.is_err()
                    || entry
                        .as_ref()
                        .is_ok_and(|e| !expected.contains_key(&e.path()))
            }) {
                continue;
            }
            if expected.iter().any(|(p, hash)| {
                p.parent() != Some(path.as_path()) || hash_file(p).ok().as_ref() != Some(hash)
            }) {
                continue;
            }
            let (stamp, bytes) = tree(&path)?;
            candidates.push(Candidate {
                kind: "backup".into(),
                id,
                title: String::new(),
                paths: vec![(path, stamp, bytes)],
            });
        }
        Ok(candidates)
    }
    fn cleanup_summary(candidates: &[Candidate]) -> CoreResult<CleanupPlan> {
        Ok(CleanupPlan {
            plan_hash: format!("{:x}", Sha256::digest(serde_json::to_vec(candidates)?)),
            versions: candidates
                .iter()
                .filter(|c| c.kind == "version")
                .map(|c| (c.id.clone(), c.title.clone()))
                .collect(),
            staging_count: candidates.iter().filter(|c| c.kind == "staging").count(),
            backup_count: candidates.iter().filter(|c| c.kind == "backup").count(),
            bytes: candidates.iter().flat_map(|c| &c.paths).map(|p| p.2).sum(),
        })
    }
    pub fn build_cleanup_plan(&self) -> CoreResult<CleanupPlan> {
        let _lock = self.library_access(true)?;
        Self::cleanup_summary(&self.cleanup_candidates(&self.connect()?)?)
    }
    pub fn execute_cleanup(&self, plan_hash: &str) -> CoreResult<CleanupReport> {
        let _lock = self.library_access(true)?;
        let mut conn = self.connect()?;
        self.recover_cleanup(&conn)?;
        if self.app_data_dir.join("cleanup-journal.json").exists() {
            return Err(CoreError::Invalid("CLEANUP_RECOVERY_REQUIRED"));
        }
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let candidates = self.cleanup_candidates(&tx)?;
        let summary = Self::cleanup_summary(&candidates)?;
        if summary.plan_hash != plan_hash {
            return Err(CoreError::Invalid("PLAN_STALE"));
        }
        let token = Uuid::new_v4().to_string();
        let journal = CleanupJournal {
            token: token.clone(),
            moves: candidates
                .iter()
                .flat_map(|candidate| candidate.paths.iter())
                .map(|(source, stamp, _)| {
                    (
                        source.clone(),
                        source.with_file_name(format!(
                            ".omnooth-cleanup-{token}-{}",
                            source.file_name().unwrap().to_string_lossy()
                        )),
                        stamp.clone(),
                    )
                })
                .collect(),
        };
        self.save_cleanup_journal(&journal)?;
        let mut moved: Vec<(PathBuf, PathBuf, String)> = Vec::new();
        let result = (|| -> CoreResult<()> {
            for candidate in &candidates {
                for (source, stamp, _) in &candidate.paths {
                    let target = source.with_file_name(format!(
                        ".omnooth-cleanup-{token}-{}",
                        source.file_name().unwrap().to_string_lossy()
                    ));
                    fs::rename(source, &target)?;
                    moved.push((source.clone(), target.clone(), stamp.clone()));
                    if tree(&target)?.0 != *stamp {
                        return Err(CoreError::Invalid("PLAN_STALE"));
                    }
                }
                if candidate.kind == "version" {
                    tx.execute(
                        "DELETE FROM package_dependencies WHERE version_id=?1",
                        [&candidate.id],
                    )?;
                    tx.execute("DELETE FROM package_versions WHERE id=?1", [&candidate.id])?;
                } else if candidate.kind == "staging" {
                    tx.execute("DELETE FROM install_plans WHERE id=?1", [&candidate.id])?;
                }
            }
            tx.execute("INSERT INTO cleanup_commits(token) VALUES(?1)", [&token])?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(error) = result {
            for (original, temporary, _) in moved.iter().rev() {
                if fs::rename(temporary, original).is_err() {
                    return Err(CoreError::Invalid("CLEANUP_RECOVERY_REQUIRED"));
                }
            }
            let _ = fs::remove_file(self.app_data_dir.join("cleanup-journal.json"));
            return Err(error);
        }
        let mut retained_paths = Vec::new();
        for (_, path, _) in moved {
            let result = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            if result.is_err() {
                retained_paths.push(path.to_string_lossy().into_owned());
            }
        }
        if retained_paths.is_empty() {
            fs::remove_file(self.app_data_dir.join("cleanup-journal.json"))?;
            conn.execute("DELETE FROM cleanup_commits WHERE token=?1", [token])?;
        }
        Ok(CleanupReport {
            removed_versions: summary.versions.len(),
            removed_directories: summary.staging_count + summary.backup_count,
            retained_paths,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (PathBuf, Service, String, String) {
        let root = std::env::temp_dir().join(format!("cleanup-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let service = Service::open(root.join("app")).unwrap();
        let path = root.join("source.zip");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        zip.start_file("readme", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"old").unwrap();
        zip.finish().unwrap();
        let old = service
            .import_archive(ImportArchiveInput {
                archive_path: path.clone(),
                original_filename: "source.zip".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap()
            .item;
        let old_id = old.versions[0].id.clone();
        fs::write(&path, b"PK\x03\x04new-source").unwrap();
        let new = service
            .import_archive(ImportArchiveInput {
                archive_path: path,
                original_filename: "source.zip".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap()
            .item;
        let new_id = new
            .versions
            .iter()
            .find(|v| v.id != old_id)
            .unwrap()
            .id
            .clone();
        let conn = service.connect().unwrap();
        conn.execute(
            "UPDATE package_versions SET imported_at='2020-01-01T00:00:00Z' WHERE id=?1",
            [&old_id],
        )
        .unwrap();
        conn.execute(
            "UPDATE package_versions SET imported_at='2021-01-01T00:00:00Z' WHERE id=?1",
            [&new_id],
        )
        .unwrap();
        (root, service, old_id, new_id)
    }
    #[test]
    fn retention_is_explicit_revalidated_and_preserves_latest_and_dependencies() {
        let (root, service, old, new) = setup();
        assert!(service.build_cleanup_plan().unwrap().versions.is_empty());
        let mut settings = service.snapshot().unwrap().settings;
        settings.keep_old_versions = false;
        service.update_settings(settings).unwrap();
        let plan = service.build_cleanup_plan().unwrap();
        assert_eq!(plan.versions.len(), 1);
        let conn = service.connect().unwrap();
        conn.execute(
            "INSERT INTO package_dependencies(version_id,required_version_id) VALUES(?1,?2)",
            params![new, old],
        )
        .unwrap();
        assert_eq!(
            service.execute_cleanup(&plan.plan_hash).unwrap_err().code(),
            "PLAN_STALE"
        );
        assert!(service.build_cleanup_plan().unwrap().versions.is_empty());
        conn.execute("DELETE FROM package_dependencies", [])
            .unwrap();
        let plan = service.build_cleanup_plan().unwrap();
        assert_eq!(
            service
                .execute_cleanup(&plan.plan_hash)
                .unwrap()
                .removed_versions,
            1
        );
        assert_eq!(service.snapshot().unwrap().items[0].versions.len(), 1);
        assert!(root.join("source.zip").exists());
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn modified_extracted_files_and_shared_original_aliases_are_preserved() {
        let (root, service, old, _) = setup();
        let conn = service.connect().unwrap();
        service.extract_archive(&old).unwrap();
        let mut settings = service.snapshot().unwrap().settings;
        settings.keep_old_versions = false;
        service.update_settings(settings).unwrap();
        let plan = service.build_cleanup_plan().unwrap();
        assert_eq!(plan.versions.len(), 1);
        let extracted: String = conn
            .query_row(
                "SELECT extracted_path FROM package_versions WHERE id=?1",
                [&old],
                |r| r.get(0),
            )
            .unwrap();
        fs::write(Path::new(&extracted).join("readme"), b"user changed").unwrap();
        assert!(service.build_cleanup_plan().unwrap().versions.is_empty());
        assert_eq!(
            service.execute_cleanup(&plan.plan_hash).unwrap_err().code(),
            "PLAN_STALE"
        );
        fs::write(Path::new(&extracted).join("readme"), b"old").unwrap();
        let object: String = conn
            .query_row(
                "SELECT object_path FROM package_versions WHERE id=?1",
                [&old],
                |r| r.get(0),
            )
            .unwrap();
        let other = service
            .import_archive(ImportArchiveInput {
                archive_path: PathBuf::from(&object),
                original_filename: "other.zip".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap()
            .item;
        let original = PathBuf::from(&object);
        let alias = original
            .parent()
            .unwrap()
            .join("../objects")
            .join(original.file_name().unwrap());
        conn.execute(
            "UPDATE package_versions SET object_path=?2 WHERE id=?1",
            params![other.versions[0].id, alias.to_string_lossy()],
        )
        .unwrap();
        let plan = service.build_cleanup_plan().unwrap();
        service.execute_cleanup(&plan.plan_hash).unwrap();
        assert!(original.exists());
        assert!(alias.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interrupted_filesystem_moves_are_restored_before_the_library_opens() {
        let (root, service, old, _) = setup();
        let conn = service.connect().unwrap();
        let source: PathBuf = conn
            .query_row(
                "SELECT object_path FROM package_versions WHERE id=?1",
                [old],
                |r| r.get::<_, String>(0),
            )
            .map(PathBuf::from)
            .unwrap();
        let token = Uuid::new_v4().to_string();
        let temporary = source.with_file_name(format!(
            ".omnooth-cleanup-{token}-{}",
            source.file_name().unwrap().to_string_lossy()
        ));
        let stamp = tree(&source).unwrap().0;
        service
            .save_cleanup_journal(&CleanupJournal {
                token,
                moves: vec![(source.clone(), temporary.clone(), stamp)],
            })
            .unwrap();
        fs::rename(&source, &temporary).unwrap();
        drop(conn);
        drop(service);
        Service::open(root.join("app")).unwrap();
        assert!(source.exists());
        assert!(!temporary.exists());
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn cleanup_cannot_run_while_a_library_writer_holds_a_shared_lock() {
        let (root, service, _, _) = setup();
        let guard = service.library_access(false).unwrap();
        assert_eq!(
            service.build_cleanup_plan().unwrap_err().code(),
            "LIBRARY_BUSY"
        );
        drop(guard);
        assert!(service.build_cleanup_plan().is_ok());
        let _ = fs::remove_dir_all(root);
    }
}

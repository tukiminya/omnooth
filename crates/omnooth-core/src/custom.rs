//! Immutable custom versions built from user-supplied unitypackages and exact dependencies.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveCustomInput {
    pub package_id: Option<String>,
    pub name: String,
    pub description: String,
    pub display_parent_id: String,
    pub unitypackage_paths: Vec<PathBuf>,
    pub required_version_ids: Vec<String>,
}

pub(crate) fn migrate(conn: &mut Connection) -> CoreResult<()> {
    let tx = conn.transaction()?;
    tx.execute_batch("ALTER TABLE packages ADD COLUMN kind TEXT NOT NULL DEFAULT 'booth' CHECK(kind IN ('booth','custom')); ALTER TABLE packages ADD COLUMN display_parent_id TEXT REFERENCES packages(id); ALTER TABLE packages ADD COLUMN description TEXT NOT NULL DEFAULT '';")?;
    tx.pragma_update(None, "user_version", 7)?;
    tx.commit()?;
    Ok(())
}

impl Service {
    pub fn save_custom_package(&self, mut input: SaveCustomInput) -> CoreResult<LibraryItem> {
        if let Some(id) = input.package_id.as_ref() {
            self.require_item_visible(id)?;
        }
        self.require_item_visible(&input.display_parent_id)?;
        for id in &input.required_version_ids {
            self.require_version_visible(id)?;
        }
        let name = booth_download::sanitize_booth_title(&input.name)
            .ok_or(CoreError::Invalid("INVALID_TITLE"))?;
        if input.description.len() > 32768
            || (input.unitypackage_paths.is_empty() && input.package_id.is_none())
            || input.unitypackage_paths.len() > 1000
            || input.required_version_ids.is_empty()
            || input.required_version_ids.len() > 1000
        {
            return Err(CoreError::Invalid("INVALID_CUSTOM_PACKAGE"));
        }
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let package_id = input
            .package_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        if let Some(id) = input.package_id.as_ref() {
            let kind: Option<String> = tx
                .query_row("SELECT kind FROM packages WHERE id=?1", [id], |r| r.get(0))
                .optional()?;
            if kind.as_deref() != Some("custom") {
                return Err(CoreError::Invalid("CUSTOM_NOT_FOUND"));
            }
        }
        let mut parent = Some(input.display_parent_id.clone());
        let mut seen = BTreeSet::from([package_id.clone()]);
        while let Some(id) = parent {
            if !seen.insert(id.clone()) {
                return Err(CoreError::Invalid("DISPLAY_PARENT_CYCLE"));
            }
            parent = tx
                .query_row(
                    "SELECT display_parent_id FROM packages WHERE id=?1",
                    [id],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?
                .ok_or(CoreError::Invalid("ITEM_NOT_FOUND"))?;
        }
        input.required_version_ids.sort();
        input.required_version_ids.dedup();
        fn visit(
            conn: &Connection,
            version: &str,
            own: &str,
            active: &mut BTreeSet<String>,
            selected: &mut BTreeMap<String, String>,
        ) -> CoreResult<()> {
            if !active.insert(version.into()) {
                return Err(CoreError::Invalid("DEPENDENCY_CYCLE"));
            }
            let package: String = conn
                .query_row(
                    "SELECT package_id FROM package_versions WHERE id=?1",
                    [version],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(CoreError::Invalid("DEPENDENCY_MISSING"))?;
            if package == own || selected.get(&package).is_some_and(|v| v != version) {
                return Err(CoreError::Invalid("DEPENDENCY_VERSION_CONFLICT"));
            }
            selected.insert(package, version.into());
            let mut stmt = conn.prepare(
                "SELECT required_version_id FROM package_dependencies WHERE version_id=?1",
            )?;
            for dep in stmt
                .query_map([version], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
            {
                visit(conn, &dep, own, active, selected)?;
            }
            active.remove(version);
            Ok(())
        }
        let mut selected = BTreeMap::new();
        for version in &input.required_version_ids {
            visit(
                &tx,
                version,
                &package_id,
                &mut BTreeSet::new(),
                &mut selected,
            )?;
        }
        if input.unitypackage_paths.is_empty() {
            let (version,root): (String,String) = tx.query_row("SELECT id,extracted_path FROM package_versions WHERE package_id=?1 ORDER BY imported_at DESC,id LIMIT 1",[&package_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let mut stmt=tx.prepare("SELECT relative_path FROM package_files WHERE version_id=?1 ORDER BY relative_path")?;
            input.unitypackage_paths = stmt
                .query_map([version], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|p| PathBuf::from(&root).join(p))
                .collect();
        }
        let mut sources = BTreeMap::new();
        let mut aliases = BTreeSet::new();
        let mut total = 0u64;
        for path in &input.unitypackage_paths {
            let filename = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or(CoreError::Invalid("UNSUPPORTED_FILE"))?
                .to_owned();
            if !filename.to_ascii_lowercase().ends_with(".unitypackage")
                || !aliases.insert(filename.to_lowercase())
            {
                return Err(CoreError::Invalid("UNSUPPORTED_FILE"));
            }
            safe_member_path(&filename)?;
            if fs::symlink_metadata(path)?.file_type().is_symlink() || !path.is_file() {
                return Err(CoreError::Invalid("FILE_MISSING"));
            }
            crate::unitypackage::inspect(path)?;
            let size = fs::metadata(path)?.len();
            total = total
                .checked_add(size)
                .ok_or(CoreError::Invalid("ARCHIVE_TOO_LARGE"))?;
            if total > MAX_EXPANDED_BYTES {
                return Err(CoreError::Invalid("ARCHIVE_TOO_LARGE"));
            }
            sources.insert(filename, (path.clone(), hash_file(path)?, size));
        }
        let manifest = serde_json::to_vec(
            &serde_json::json!({"format":1,"files":sources.iter().map(|(name,(_,hash,size))|serde_json::json!({"name":name,"sha256":hash,"sizeBytes":size})).collect::<Vec<_>>(),"requiredVersionIds":input.required_version_ids}),
        )?;
        let hash = format!("{:x}", Sha256::digest(&manifest));
        let library = self.current_library_path()?;
        let root = library.join("extracted").join(&hash);
        fs::create_dir_all(&root)?;
        for (filename, (source, hash, _)) in &sources {
            atomic_copy(source, &root.join(filename), hash)?;
        }
        fs::create_dir_all(library.join("objects"))?;
        let temporary = self
            .app_data_dir
            .join(format!("custom-{}.tmp", Uuid::new_v4()));
        fs::write(&temporary, &manifest)?;
        let object = library.join("objects").join(&hash);
        let copied = atomic_copy(&temporary, &object, &hash);
        let _ = fs::remove_file(&temporary);
        copied?;
        if input.package_id.is_none() {
            tx.execute("INSERT INTO packages(id,title,identity_filename,created_at,kind,display_parent_id,description) VALUES(?1,?2,?3,?4,'custom',?5,?6)",params![package_id,name,format!("custom:{package_id}"),now(),input.display_parent_id,input.description])?;
        } else {
            tx.execute(
                "UPDATE packages SET title=?2,description=?3,display_parent_id=?4 WHERE id=?1",
                params![package_id, name, input.description, input.display_parent_id],
            )?;
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM package_versions WHERE package_id=?1 AND content_hash=?2",
                params![package_id, hash],
                |r| r.get(0),
            )
            .optional()?;
        if existing.is_none() {
            let version = Uuid::new_v4().to_string();
            tx.execute("INSERT INTO package_versions(id,package_id,original_filename,content_hash,version_date,state,object_path,extracted_path,imported_at) VALUES(?1,?2,'custom.manifest.json',?3,?4,'extracted',?5,?6,?7)",params![version,package_id,hash,Utc::now().format("%Y-%m-%d").to_string(),object.to_string_lossy(),root.to_string_lossy(),now()])?;
            for (filename, (_, hash, size)) in sources {
                tx.execute("INSERT INTO package_files(id,version_id,relative_path,kind,size_bytes,content_hash) VALUES(?1,?2,?3,'unitypackage',?4,?5)",params![Uuid::new_v4().to_string(),version,filename,size as i64,hash])?;
            }
            for dep in input.required_version_ids {
                tx.execute("INSERT INTO package_dependencies(version_id,required_version_id) VALUES(?1,?2)",params![version,dep])?;
            }
        }
        tx.commit()?;
        self.load_item_by_id(&package_id)
    }
}

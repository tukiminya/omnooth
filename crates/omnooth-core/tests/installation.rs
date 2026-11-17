use flate2::{write::GzEncoder, Compression};
use omnooth_core::*;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use uuid::Uuid;

struct Workspace {
    root: PathBuf,
    service: Service,
    project: Project,
}
impl Workspace {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("omnooth-install-{}", Uuid::new_v4()));
        let project = root.join("Project");
        for name in ["Assets", "Packages", "ProjectSettings"] {
            fs::create_dir_all(project.join(name)).unwrap();
        }
        fs::write(
            project.join("ProjectSettings/ProjectVersion.txt"),
            "m_EditorVersion: 2022.3.22f1\n",
        )
        .unwrap();
        let service = Service::open(root.join("app")).unwrap();
        let project = service.link_project(project).unwrap();
        Self {
            root,
            service,
            project,
        }
    }
    fn import(&self, name: &str, assets: &[(&str, &str, Option<&[u8]>)]) -> LibraryItem {
        let source = self.root.join(format!("{}.unitypackage", Uuid::new_v4()));
        fs::write(&source, package(assets)).unwrap();
        self.service
            .import_archive(ImportArchiveInput {
                archive_path: source,
                original_filename: name.into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap()
            .item
    }
    fn plan(&self, version: &LibraryVersion, ids: Vec<String>) -> InstallPlan {
        self.service
            .build_install_plan(BuildInstallPlanInput {
                project_id: self.project.id.clone(),
                selections: vec![InstallSelection {
                    version_id: version.id.clone(),
                    file_ids: ids,
                }],
                remove_package_ids: vec![],
            })
            .unwrap()
    }
    fn input(&self, plan: &InstallPlan) -> InstallItemInput {
        InstallItemInput {
            project_id: self.project.id.clone(),
            plan_id: plan.id.clone(),
            plan_hash: plan.plan_hash.clone(),
            idempotency_key: Uuid::new_v4().to_string(),
        }
    }
    fn install(&self, version: &LibraryVersion, gateway: &FakeUnity) -> InstallationJob {
        let plan = self.plan(
            version,
            version.files.iter().map(|f| f.id.clone()).collect(),
        );
        let job = self
            .service
            .install_item(self.input(&plan), gateway)
            .unwrap();
        self.service.poll_installation(&job.id, gateway).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn append(tar: &mut tar::Builder<GzEncoder<Vec<u8>>>, path: &str, bytes: &[u8]) {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, path, bytes).unwrap();
}
fn package(assets: &[(&str, &str, Option<&[u8]>)]) -> Vec<u8> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (path, guid, bytes) in assets {
        append(&mut tar, &format!("{guid}/pathname"), path.as_bytes());
        append(
            &mut tar,
            &format!("{guid}/asset.meta"),
            format!(
                "fileFormatVersion: 2\nguid: {guid}\n{}",
                if bytes.is_none() {
                    "folderAsset: yes\n"
                } else {
                    ""
                }
            )
            .as_bytes(),
        );
        if let Some(bytes) = bytes {
            append(&mut tar, &format!("{guid}/asset"), bytes);
        }
    }
    tar.into_inner().unwrap().finish().unwrap()
}
const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "cccccccccccccccccccccccccccccccc";
struct FakeUnity {
    write_files: bool,
    outcome: Mutex<UnityOutcome>,
    published: Mutex<Vec<UnityImportFile>>,
    ready: bool,
}
impl FakeUnity {
    fn success() -> Self {
        Self {
            write_files: true,
            outcome: Mutex::new(UnityOutcome::Completed),
            published: Mutex::new(Vec::new()),
            ready: true,
        }
    }
}
impl UnityGateway for FakeUnity {
    fn ensure_ready(&self, _: &Path) -> CoreResult<()> {
        if self.ready {
            Ok(())
        } else {
            Err(CoreError::Invalid("UNITY_UNAVAILABLE"))
        }
    }
    fn publish(&self, project: &Path, _: &str, packages: &[UnityImportFile]) -> CoreResult<()> {
        *self.published.lock().unwrap() = packages.to_vec();
        if self.write_files {
            for p in packages {
                let mut archive =
                    tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&p.path)?));
                let mut records = std::collections::BTreeMap::<
                    String,
                    std::collections::BTreeMap<String, Vec<u8>>,
                >::new();
                for e in archive.entries()? {
                    use std::io::Read;
                    let mut e = e?;
                    let path = e.path()?.to_string_lossy().into_owned();
                    if !e.header().entry_type().is_file() {
                        continue;
                    }
                    let path = path.trim_start_matches("./");
                    let Some((guid, name)) = path.split_once('/') else {
                        continue;
                    };
                    if guid.len() != 32 || !matches!(name, "pathname" | "asset" | "asset.meta") {
                        continue;
                    }
                    let mut bytes = Vec::new();
                    e.read_to_end(&mut bytes)?;
                    records
                        .entry(guid.into())
                        .or_default()
                        .insert(name.into(), bytes);
                }
                for (_, record) in records {
                    let relative = std::str::from_utf8(&record["pathname"]).unwrap();
                    let target = project.join(relative);
                    fs::create_dir_all(target.parent().unwrap())?;
                    if let Some(bytes) = record.get("asset") {
                        fs::write(target, bytes)?;
                    } else {
                        fs::create_dir_all(target)?;
                    }
                    fs::write(
                        project.join(format!("{relative}.meta")),
                        &record["asset.meta"],
                    )?;
                }
            }
        }
        Ok(())
    }
    fn poll(&self, _: &Path, _: &str, _: &[UnityImportFile]) -> CoreResult<UnityOutcome> {
        Ok(match *self.outcome.lock().unwrap() {
            UnityOutcome::Completed => UnityOutcome::Completed,
            UnityOutcome::Failed => UnityOutcome::Failed,
            UnityOutcome::Cancelled => UnityOutcome::Cancelled,
            UnityOutcome::Pending => UnityOutcome::Pending,
        })
    }
}

#[test]
fn zip_selection_order_restart_idempotency_and_ledger_are_connected() {
    let w = Workspace::new();
    let source = w.root.join("bundle.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&source).unwrap());
    for (name, path, guid) in [
        ("One.unitypackage", "Assets/One.prefab", A),
        ("Two.unitypackage", "Assets/Two.prefab", B),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&package(&[(path, guid, Some(b"content"))]))
            .unwrap();
    }
    zip.start_file(
        "Unchosen.unitypackage",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"invalid but unselected").unwrap();
    zip.finish().unwrap();
    let item = w
        .service
        .import_archive(ImportArchiveInput {
            archive_path: source,
            original_filename: "bundle.zip".into(),
            extract_archive: false,
            item_id: None,
        })
        .unwrap()
        .item;
    let version = &item.versions[0];
    let entries = w.service.inspect_archive(&version.id).unwrap().entries;
    let ids = ["Two.unitypackage", "One.unitypackage"]
        .iter()
        .map(|name| {
            entries
                .iter()
                .find(|e| e.relative_path == *name)
                .unwrap()
                .id
                .clone()
        })
        .collect();
    let plan = w.plan(version, ids);
    assert_eq!(
        plan.import_order,
        vec!["Two.unitypackage", "One.unitypackage"]
    );
    assert!(!Path::new(&w.project.path)
        .join("Assets/One.prefab")
        .exists());
    let gateway = FakeUnity::success();
    *gateway.outcome.lock().unwrap() = UnityOutcome::Pending;
    let input = w.input(&plan);
    let job = w.service.install_item(input.clone(), &gateway).unwrap();
    assert_eq!(job.status, "waitingForUnity");
    assert_eq!(w.service.install_item(input, &gateway).unwrap().id, job.id);
    assert_eq!(
        w.service
            .install_item(w.input(&plan), &gateway)
            .unwrap_err()
            .code(),
        "PROJECT_BUSY"
    );
    assert_eq!(
        w.service
            .poll_installation(&job.id, &gateway)
            .unwrap()
            .status,
        "waitingForUnity"
    );
    assert!(w
        .service
        .get_installations(&w.project.id)
        .unwrap()
        .packages
        .is_empty());
    let restarted = Service::open(w.root.join("app")).unwrap();
    *gateway.outcome.lock().unwrap() = UnityOutcome::Completed;
    assert_eq!(
        restarted
            .poll_installation(&job.id, &gateway)
            .unwrap()
            .status,
        "succeeded"
    );
    assert_eq!(
        restarted.get_installations(&w.project.id).unwrap().packages[0].file_ids,
        plan.selections[0].file_ids
    );
    for p in gateway.published.lock().unwrap().iter() {
        assert!(!p.path.exists());
    }
    // Later permanent extraction preserves IDs referenced by installed selections.
    restarted.extract_archive(&version.id).unwrap();
    let conn = rusqlite::Connection::open(w.root.join("app/omnooth.sqlite")).unwrap();
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM project_files", [], |r| r.get(0))
            .unwrap(),
        4
    );
}

#[test]
fn callback_without_matching_files_never_commits_and_can_be_recovered() {
    let w = Workspace::new();
    let item = w.import(
        "One.unitypackage",
        &[("Assets/One.prefab", A, Some(b"one"))],
    );
    let mut gateway = FakeUnity::success();
    gateway.write_files = false;
    let job = w.install(&item.versions[0], &gateway);
    assert_eq!(job.status, "recovering");
    assert_eq!(job.error_code.as_deref(), Some("INSTALLED_FILE_MISMATCH"));
    assert!(w
        .service
        .get_installations(&w.project.id)
        .unwrap()
        .packages
        .is_empty());
    assert!(
        !w.service
            .recover_installation(&job.id, RecoveryMode::Inspect, &gateway)
            .unwrap()
            .requires_manual_action
    );
    w.service
        .recover_installation(&job.id, RecoveryMode::Restore, &gateway)
        .unwrap();
    assert_eq!(
        w.service.get_installations(&w.project.id).unwrap().jobs[0].status,
        "restored"
    );
}

#[test]
fn updating_and_removing_preserves_unmanaged_siblings_and_shared_folder() {
    let w = Workspace::new();
    let gateway = FakeUnity::success();
    let one = w.import(
        "Product.unitypackage",
        &[
            ("Assets/Shared", A, None),
            ("Assets/Shared/One.prefab", B, Some(b"one")),
        ],
    );
    assert_eq!(w.install(&one.versions[0], &gateway).status, "succeeded");
    fs::write(
        Path::new(&w.project.path).join("Assets/Shared/User.txt"),
        b"keep me",
    )
    .unwrap();
    let two = w.import(
        "Product.unitypackage",
        &[
            ("Assets/Shared", A, None),
            ("Assets/Shared/Two.prefab", C, Some(b"two")),
        ],
    );
    let version = two
        .versions
        .iter()
        .find(|v| v.content_hash != one.versions[0].content_hash)
        .unwrap();
    assert_eq!(w.install(version, &gateway).status, "succeeded");
    assert!(!Path::new(&w.project.path)
        .join("Assets/Shared/One.prefab")
        .exists());
    let plan = w
        .service
        .build_install_plan(BuildInstallPlanInput {
            project_id: w.project.id.clone(),
            selections: vec![],
            remove_package_ids: vec![two.id],
        })
        .unwrap();
    assert!(!plan.removed_paths.contains(&"Assets/Shared.meta".into()));
    let job = w.service.install_item(w.input(&plan), &gateway).unwrap();
    assert_eq!(
        w.service
            .poll_installation(&job.id, &gateway)
            .unwrap()
            .status,
        "succeeded"
    );
    assert!(Path::new(&w.project.path)
        .join("Assets/Shared.meta")
        .exists());
    assert_eq!(
        fs::read(Path::new(&w.project.path).join("Assets/Shared/User.txt")).unwrap(),
        b"keep me"
    );
    assert!(w
        .service
        .get_installations(&w.project.id)
        .unwrap()
        .packages
        .is_empty());
}

#[test]
fn failed_update_retains_old_ledger_and_recovers_only_unmodified_files() {
    let w = Workspace::new();
    let gateway = FakeUnity::success();
    let one = w.import(
        "Product.unitypackage",
        &[("Assets/One.prefab", A, Some(b"old"))],
    );
    assert_eq!(w.install(&one.versions[0], &gateway).status, "succeeded");
    let two = w.import(
        "Product.unitypackage",
        &[("Assets/One.prefab", A, Some(b"new"))],
    );
    let version = two
        .versions
        .iter()
        .find(|v| v.content_hash != one.versions[0].content_hash)
        .unwrap();
    *gateway.outcome.lock().unwrap() = UnityOutcome::Failed;
    let job = w.install(version, &gateway);
    assert_eq!(job.status, "recovering");
    let target = Path::new(&w.project.path).join("Assets/One.prefab");
    fs::write(&target, b"user edit").unwrap();
    let report = w
        .service
        .recover_installation(&job.id, RecoveryMode::Restore, &gateway)
        .unwrap();
    assert!(report.requires_manual_action);
    assert_eq!(fs::read(&target).unwrap(), b"user edit");
    fs::write(&target, b"new").unwrap();
    w.service
        .recover_installation(&job.id, RecoveryMode::Restore, &gateway)
        .unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(
        w.service.get_installations(&w.project.id).unwrap().packages[0].version_id,
        one.versions[0].id
    );
}

#[test]
fn stale_plan_and_unavailable_unity_do_not_touch_assets() {
    let w = Workspace::new();
    let item = w.import(
        "One.unitypackage",
        &[("Assets/One.prefab", A, Some(b"one"))],
    );
    let v = &item.versions[0];
    let plan = w.plan(v, vec![v.files[0].id.clone()]);
    let gateway = FakeUnity::success();
    fs::write(
        Path::new(&w.project.path).join("Assets/User.txt"),
        b"changed",
    )
    .unwrap();
    assert_eq!(
        w.service
            .install_item(w.input(&plan), &gateway)
            .unwrap_err()
            .code(),
        "PLAN_STALE"
    );
    let plan = w.plan(v, vec![v.files[0].id.clone()]);
    let mut gateway = FakeUnity::success();
    gateway.ready = false;
    assert_eq!(
        w.service
            .install_item(w.input(&plan), &gateway)
            .unwrap_err()
            .code(),
        "UNITY_UNAVAILABLE"
    );
    assert!(w
        .service
        .get_installations(&w.project.id)
        .unwrap()
        .jobs
        .is_empty());
    assert!(!Path::new(&w.project.path)
        .join("Assets/One.prefab")
        .exists());
}

#[test]
fn unmanaged_paths_and_guid_collisions_are_rejected() {
    let w = Workspace::new();
    let item = w.import(
        "One.unitypackage",
        &[("Assets/One.prefab", A, Some(b"one"))],
    );
    let v = &item.versions[0];
    let input = || BuildInstallPlanInput {
        project_id: w.project.id.clone(),
        selections: vec![InstallSelection {
            version_id: v.id.clone(),
            file_ids: vec![v.files[0].id.clone()],
        }],
        remove_package_ids: vec![],
    };
    let target = Path::new(&w.project.path).join("Assets/One.prefab");
    fs::write(&target, b"existing").unwrap();
    assert_eq!(
        w.service.build_install_plan(input()).unwrap_err().code(),
        "ASSET_PATH_CONFLICT"
    );
    fs::remove_file(target).unwrap();
    fs::write(
        Path::new(&w.project.path).join("Assets/Other.prefab.meta"),
        format!("guid: {A}\n"),
    )
    .unwrap();
    assert_eq!(
        w.service.build_install_plan(input()).unwrap_err().code(),
        "ASSET_GUID_CONFLICT"
    );
}

#[test]
fn unsafe_package_paths_links_and_mismatched_guids_are_rejected() {
    let w = Workspace::new();
    for path in [
        "../outside",
        "Packages/Thing",
        "Assets/../outside",
        "Assets/C:/thing",
        "Assets//thing",
        "Assets/CON.txt",
    ] {
        let item = w.import(
            &format!("{}.unitypackage", Uuid::new_v4()),
            &[(path, A, Some(b"data"))],
        );
        let v = &item.versions[0];
        assert!(
            w.service
                .build_install_plan(BuildInstallPlanInput {
                    project_id: w.project.id.clone(),
                    selections: vec![InstallSelection {
                        version_id: v.id.clone(),
                        file_ids: vec![v.files[0].id.clone()]
                    }],
                    remove_package_ids: vec![]
                })
                .is_err(),
            "{path}"
        );
    }
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header.set_link_name("/tmp/outside").unwrap();
    header.set_cksum();
    tar.append_data(&mut header, format!("{A}/asset"), &[][..])
        .unwrap();
    let source = w.root.join("unsafe.unitypackage");
    fs::write(&source, tar.into_inner().unwrap().finish().unwrap()).unwrap();
    let item = w
        .service
        .import_archive(ImportArchiveInput {
            archive_path: source,
            original_filename: "unsafe.unitypackage".into(),
            extract_archive: false,
            item_id: None,
        })
        .unwrap()
        .item;
    let v = &item.versions[0];
    assert_eq!(
        w.service
            .build_install_plan(BuildInstallPlanInput {
                project_id: w.project.id.clone(),
                selections: vec![InstallSelection {
                    version_id: v.id.clone(),
                    file_ids: vec![v.files[0].id.clone()]
                }],
                remove_package_ids: vec![]
            })
            .unwrap_err()
            .code(),
        "UNITYPACKAGE_INVALID"
    );
}

#[test]
fn exact_dependencies_are_validated_including_retained_packages_and_cycles() {
    let w = Workspace::new();
    let gateway = FakeUnity::success();
    let base = w.import(
        "Base.unitypackage",
        &[("Assets/Base.prefab", A, Some(b"base"))],
    );
    let dependent = w.import(
        "Dependent.unitypackage",
        &[("Assets/Dependent.prefab", B, Some(b"dependent"))],
    );
    let conn = rusqlite::Connection::open(w.root.join("app/omnooth.sqlite")).unwrap();
    conn.execute(
        "INSERT INTO package_dependencies VALUES(?1,?2)",
        rusqlite::params![dependent.versions[0].id, base.versions[0].id],
    )
    .unwrap();
    let input = BuildInstallPlanInput {
        project_id: w.project.id.clone(),
        selections: vec![InstallSelection {
            version_id: dependent.versions[0].id.clone(),
            file_ids: vec![dependent.versions[0].files[0].id.clone()],
        }],
        remove_package_ids: vec![],
    };
    assert_eq!(
        w.service
            .build_install_plan(input.clone())
            .unwrap_err()
            .code(),
        "DEPENDENCY_SELECTION_REQUIRED"
    );
    let mut combined = input;
    combined.selections.push(InstallSelection {
        version_id: base.versions[0].id.clone(),
        file_ids: vec![base.versions[0].files[0].id.clone()],
    });
    let plan = w.service.build_install_plan(combined.clone()).unwrap();
    assert_eq!(
        plan.import_order,
        vec!["Base.unitypackage", "Dependent.unitypackage"]
    );
    let job = w.service.install_item(w.input(&plan), &gateway).unwrap();
    assert_eq!(
        w.service
            .poll_installation(&job.id, &gateway)
            .unwrap()
            .status,
        "succeeded"
    );
    assert_eq!(
        w.service
            .build_install_plan(BuildInstallPlanInput {
                project_id: w.project.id.clone(),
                selections: vec![],
                remove_package_ids: vec![base.id.clone()]
            })
            .unwrap_err()
            .code(),
        "DEPENDENCY_SELECTION_REQUIRED"
    );
    conn.execute(
        "INSERT INTO package_dependencies VALUES(?1,?2)",
        rusqlite::params![base.versions[0].id, dependent.versions[0].id],
    )
    .unwrap();
    let newer = w.import(
        "Dependent.unitypackage",
        &[("Assets/Dependent.prefab", B, Some(b"updated"))],
    );
    // Existing unchanged selections produce no changes; removing a cycle member still fails dependency validation.
    assert!(w
        .service
        .build_install_plan(BuildInstallPlanInput {
            project_id: w.project.id.clone(),
            selections: vec![],
            remove_package_ids: vec![base.id]
        })
        .is_err());
    assert_eq!(newer.id, dependent.id);
    let extra = w.import(
        "Extra.unitypackage",
        &[("Assets/Extra.prefab", C, Some(b"extra"))],
    );
    let v = &extra.versions[0];
    assert_eq!(
        w.service
            .build_install_plan(BuildInstallPlanInput {
                project_id: w.project.id.clone(),
                selections: vec![InstallSelection {
                    version_id: v.id.clone(),
                    file_ids: vec![v.files[0].id.clone()]
                }],
                remove_package_ids: vec![]
            })
            .unwrap_err()
            .code(),
        "DEPENDENCY_CYCLE"
    );
}

#[test]
fn reselect_removes_only_unchosen_source_files_and_never_reimports_retained_selection() {
    let w = Workspace::new();
    let source = w.root.join("bundle.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&source).unwrap());
    for (name, path, guid) in [
        ("One.unitypackage", "Assets/One.prefab", A),
        ("Two.unitypackage", "Assets/Two.prefab", B),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&package(&[(path, guid, Some(b"content"))]))
            .unwrap();
    }
    zip.finish().unwrap();
    let item = w
        .service
        .import_archive(ImportArchiveInput {
            archive_path: source,
            original_filename: "bundle.zip".into(),
            extract_archive: true,
            item_id: None,
        })
        .unwrap()
        .item;
    let v = &item.versions[0];
    let gateway = FakeUnity::success();
    assert_eq!(w.install(v, &gateway).status, "succeeded");
    let plan = w.plan(v, vec![v.files[0].id.clone()]);
    assert_eq!(plan.operations[0].kind, "reselect");
    assert!(plan.import_order.is_empty());
    let job = w.service.install_item(w.input(&plan), &gateway).unwrap();
    assert_eq!(
        w.service
            .poll_installation(&job.id, &gateway)
            .unwrap()
            .status,
        "succeeded"
    );
    assert!(Path::new(&w.project.path)
        .join("Assets/One.prefab")
        .exists());
    assert!(!Path::new(&w.project.path)
        .join("Assets/Two.prefab")
        .exists());
}

#[test]
fn accepts_real_unity_2022_3_export_with_exact_manifest_verification() {
    let w = Workspace::new();
    let source = w.root.join("real.unitypackage");
    fs::write(
        &source,
        include_bytes!("fixtures/unity-2022.3-text.unitypackage"),
    )
    .unwrap();
    let item = w
        .service
        .import_archive(ImportArchiveInput {
            archive_path: source,
            original_filename: "UnityExport.unitypackage".into(),
            extract_archive: false,
            item_id: None,
        })
        .unwrap()
        .item;
    let job = w.install(&item.versions[0], &FakeUnity::success());
    assert_eq!(job.status, "succeeded");
    assert_eq!(
        fs::read(Path::new(&w.project.path).join("Assets/OmnoothValidation.txt")).unwrap(),
        b"omnooth fixture"
    );
}

#[test]
fn modified_owned_files_and_linked_assets_are_never_overwritten() {
    let w = Workspace::new();
    let gateway = FakeUnity::success();
    let item = w.import(
        "One.unitypackage",
        &[("Assets/One.prefab", A, Some(b"one"))],
    );
    let v = &item.versions[0];
    assert_eq!(w.install(v, &gateway).status, "succeeded");
    fs::write(
        Path::new(&w.project.path).join("Assets/One.prefab"),
        b"user modified",
    )
    .unwrap();
    assert_eq!(
        w.service
            .build_install_plan(BuildInstallPlanInput {
                project_id: w.project.id.clone(),
                selections: vec![],
                remove_package_ids: vec![item.id]
            })
            .unwrap_err()
            .code(),
        "USER_FILE_MODIFIED"
    );
    #[cfg(unix)]
    {
        let outside = w.root.join("outside");
        fs::write(&outside, b"outside").unwrap();
        std::os::unix::fs::symlink(
            &outside,
            Path::new(&w.project.path).join("Assets/Linked.prefab"),
        )
        .unwrap();
        let other = w.import(
            "Other.unitypackage",
            &[("Assets/Other.prefab", B, Some(b"other"))],
        );
        let v = &other.versions[0];
        assert_eq!(
            w.service
                .build_install_plan(BuildInstallPlanInput {
                    project_id: w.project.id.clone(),
                    selections: vec![InstallSelection {
                        version_id: v.id.clone(),
                        file_ids: vec![v.files[0].id.clone()]
                    }],
                    remove_package_ids: vec![]
                })
                .unwrap_err()
                .code(),
            "PROJECT_UNSAFE_PATH"
        );
        assert_eq!(fs::read(outside).unwrap(), b"outside");
    }
}

#[test]
fn recovery_waits_for_terminal_editor_and_preserves_new_folder_with_user_work() {
    let w = Workspace::new();
    let gateway = FakeUnity::success();
    let item = w.import(
        "Folder.unitypackage",
        &[
            ("Assets/Folder", A, None),
            ("Assets/Folder/One.prefab", B, Some(b"one")),
        ],
    );
    *gateway.outcome.lock().unwrap() = UnityOutcome::Pending;
    let plan = w.plan(
        &item.versions[0],
        vec![item.versions[0].files[0].id.clone()],
    );
    let job = w.service.install_item(w.input(&plan), &gateway).unwrap();
    assert_eq!(
        w.service
            .recover_installation(&job.id, RecoveryMode::Restore, &gateway)
            .unwrap_err()
            .code(),
        "UNITY_BUSY"
    );
    assert!(Path::new(&w.project.path)
        .join("Assets/Folder/One.prefab")
        .exists());
    *gateway.outcome.lock().unwrap() = UnityOutcome::Failed;
    w.service.poll_installation(&job.id, &gateway).unwrap();
    fs::write(
        Path::new(&w.project.path).join("Assets/Folder/User.txt"),
        b"user work",
    )
    .unwrap();
    let report = w
        .service
        .recover_installation(&job.id, RecoveryMode::Restore, &gateway)
        .unwrap();
    assert!(report.requires_manual_action);
    assert!(report.conflicts.contains(&"Assets/Folder.meta".into()));
    assert!(Path::new(&w.project.path)
        .join("Assets/Folder.meta")
        .exists());
}

#[test]
fn invalid_gzip_trailer_and_metadata_guid_are_rejected() {
    let w = Workspace::new();
    let mut corrupt = package(&[("Assets/One.prefab", A, Some(b"one"))]);
    let n = corrupt.len();
    corrupt[n - 8] ^= 0xff;
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    append(&mut tar, &format!("{A}/pathname"), b"Assets/One.prefab");
    append(&mut tar, &format!("{A}/asset"), b"one");
    append(
        &mut tar,
        &format!("{A}/asset.meta"),
        format!("guid: {B}\n").as_bytes(),
    );
    for bytes in [corrupt, tar.into_inner().unwrap().finish().unwrap()] {
        let source = w.root.join(format!("{}.unitypackage", Uuid::new_v4()));
        fs::write(&source, bytes).unwrap();
        let item = w
            .service
            .import_archive(ImportArchiveInput {
                archive_path: source,
                original_filename: format!("{}.unitypackage", Uuid::new_v4()),
                extract_archive: false,
                item_id: None,
            })
            .unwrap()
            .item;
        let v = &item.versions[0];
        assert!(w
            .service
            .build_install_plan(BuildInstallPlanInput {
                project_id: w.project.id.clone(),
                selections: vec![InstallSelection {
                    version_id: v.id.clone(),
                    file_ids: vec![v.files[0].id.clone()]
                }],
                remove_package_ids: vec![]
            })
            .is_err());
    }
}

#[test]
fn deferred_ownership_constraints_reject_sources_outside_confirmed_selections() {
    let w = Workspace::new();
    let gateway = FakeUnity::success();
    let item = w.import(
        "One.unitypackage",
        &[("Assets/One.prefab", A, Some(b"one"))],
    );
    assert_eq!(w.install(&item.versions[0], &gateway).status, "succeeded");
    let other = w.import(
        "Other.unitypackage",
        &[("Assets/Other.prefab", B, Some(b"other"))],
    );
    let conn = rusqlite::Connection::open(w.root.join("app/omnooth.sqlite")).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    tx.execute(
        "UPDATE project_files SET source_file_id=?1",
        [&other.versions[0].files[0].id],
    )
    .unwrap();
    assert!(tx.commit().is_err());
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get(0))
            .unwrap(),
        0
    );
}

#[test]
fn pax_path_overrides_are_rejected_even_when_real_headers_are_safe() {
    let w = Workspace::new();
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    tar.append_pax_extensions([("path", b"../outside".as_slice())])
        .unwrap();
    append(&mut tar, &format!("{A}/pathname"), b"Assets/One.prefab");
    append(&mut tar, &format!("{A}/asset"), b"one");
    append(
        &mut tar,
        &format!("{A}/asset.meta"),
        format!("guid: {A}\n").as_bytes(),
    );
    let source = w.root.join("override.unitypackage");
    fs::write(&source, tar.into_inner().unwrap().finish().unwrap()).unwrap();
    let item = w
        .service
        .import_archive(ImportArchiveInput {
            archive_path: source,
            original_filename: "override.unitypackage".into(),
            extract_archive: false,
            item_id: None,
        })
        .unwrap()
        .item;
    let v = &item.versions[0];
    assert!(w
        .service
        .build_install_plan(BuildInstallPlanInput {
            project_id: w.project.id.clone(),
            selections: vec![InstallSelection {
                version_id: v.id.clone(),
                file_ids: vec![v.files[0].id.clone()]
            }],
            remove_package_ids: vec![]
        })
        .is_err());
}

#[test]
fn completed_callback_waits_for_editor_to_finish_compilation_before_commit() {
    let w = Workspace::new();
    let mut gateway = FakeUnity::success();
    let item = w.import(
        "One.unitypackage",
        &[("Assets/One.prefab", A, Some(b"one"))],
    );
    let v = &item.versions[0];
    let plan = w.plan(v, vec![v.files[0].id.clone()]);
    let job = w.service.install_item(w.input(&plan), &gateway).unwrap();
    gateway.ready = false;
    assert_eq!(
        w.service
            .poll_installation(&job.id, &gateway)
            .unwrap()
            .status,
        "waitingForUnity"
    );
    assert!(w
        .service
        .get_installations(&w.project.id)
        .unwrap()
        .packages
        .is_empty());
    gateway.ready = true;
    assert_eq!(
        w.service
            .poll_installation(&job.id, &gateway)
            .unwrap()
            .status,
        "succeeded"
    );
}

#[test]
fn updates_cannot_rewrite_existing_shared_folder_metadata() {
    let w = Workspace::new();
    let gateway = FakeUnity::success();
    let one = w.import(
        "Folder.unitypackage",
        &[
            ("Assets/Folder", A, None),
            ("Assets/Folder/One.prefab", B, Some(b"one")),
        ],
    );
    assert_eq!(w.install(&one.versions[0], &gateway).status, "succeeded");
    let two = w.import(
        "Folder.unitypackage",
        &[
            ("Assets/Folder", C, None),
            ("Assets/Folder/One.prefab", B, Some(b"one")),
        ],
    );
    let v = two
        .versions
        .iter()
        .find(|v| v.id != one.versions[0].id)
        .unwrap();
    assert_eq!(
        w.service
            .build_install_plan(BuildInstallPlanInput {
                project_id: w.project.id.clone(),
                selections: vec![InstallSelection {
                    version_id: v.id.clone(),
                    file_ids: vec![v.files[0].id.clone()]
                }],
                remove_package_ids: vec![]
            })
            .unwrap_err()
            .code(),
        "ASSET_PATH_CONFLICT"
    );
}

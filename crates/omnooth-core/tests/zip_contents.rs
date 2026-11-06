use omnooth_core::{ImportArchiveInput, Service};
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
};
use uuid::Uuid;

struct Fixture {
    root: PathBuf,
    service: Service,
    version_id: String,
    hash: String,
}
impl Fixture {
    fn new(members: &[(&str, &[u8])]) -> Self {
        let root = std::env::temp_dir().join(format!("omnooth-zip-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("Bundle.zip");
        let mut zip = zip::ZipWriter::new(File::create(&source).unwrap());
        for (name, bytes) in members {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let service = Service::open(root.join("app")).unwrap();
        let result = service
            .import_archive(ImportArchiveInput {
                archive_path: source,
                original_filename: "Bundle.zip".into(),
                extract_archive: false,
                item_id: None,
            })
            .unwrap();
        let version = &result.item.versions[0];
        Self {
            root,
            service,
            version_id: version.id.clone(),
            hash: version.content_hash.clone(),
        }
    }
    fn object(&self) -> PathBuf {
        self.root.join("app/Library/objects").join(&self.hash)
    }
    fn selection(&self, names: &[&str]) -> Vec<String> {
        let contents = self.service.inspect_archive(&self.version_id).unwrap();
        names
            .iter()
            .map(|name| {
                contents
                    .entries
                    .iter()
                    .find(|entry| &entry.relative_path == name)
                    .unwrap()
                    .id
                    .clone()
            })
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn lists_every_member_without_inflating_or_changing_library_state() {
    let f = Fixture::new(&[
        ("nested/PC.unitypackage", b"pc"),
        ("readme.txt", b"read me"),
        ("image.png", b"png"),
        ("nested/Quest.UNITYPACKAGE", b"quest"),
    ]);
    let before = fs::read(f.object()).unwrap();
    let contents = f.service.inspect_archive(&f.version_id).unwrap();
    assert_eq!(contents.entries.len(), 4);
    assert_eq!(
        contents
            .entries
            .iter()
            .filter(|entry| entry.kind == "unitypackage")
            .count(),
        2
    );
    assert_eq!(contents, f.service.inspect_archive(&f.version_id).unwrap());
    let snapshot = f.service.snapshot().unwrap();
    assert_eq!(snapshot.items[0].versions[0].state, "stored");
    assert!(snapshot.items[0].versions[0].files.is_empty());
    assert_eq!(
        fs::read_dir(f.root.join("app/Library/extracted"))
            .unwrap()
            .count(),
        0
    );
    assert!(!f.root.join("app/unity-staging").exists());
    assert_eq!(before, fs::read(f.object()).unwrap());
    let reply = f
        .service
        .dispatch(omnooth_core::ApplicationCommand::InspectArchive {
            version_id: f.version_id.clone(),
        })
        .unwrap();
    assert_eq!(reply["versionId"], f.version_id);
    assert_eq!(reply["entries"].as_array().unwrap().len(), 4);
}

#[test]
fn stages_only_selected_packages_in_order_and_can_reload_after_restart() {
    let f = Fixture::new(&[
        ("nested/PC.unitypackage", b"pc"),
        ("image.png", b"png"),
        ("nested/Quest.unitypackage", b"quest"),
    ]);
    let selected = f.selection(&["nested/Quest.unitypackage", "nested/PC.unitypackage"]);
    let prepared = f
        .service
        .prepare_zip_unity_import(&f.version_id, &selected)
        .unwrap();
    assert_eq!(prepared.packages().len(), 2);
    assert_eq!(prepared.packages()[0].operation_id, selected[0]);
    assert_eq!(fs::read(&prepared.packages()[0].path).unwrap(), b"quest");
    assert_eq!(fs::read(&prepared.packages()[1].path).unwrap(), b"pc");
    let stage = f.root.join("app/unity-staging").join(prepared.job_id());
    assert_eq!(fs::read_dir(&stage).unwrap().count(), 3); // two selected files + manifest
    let reopened = Service::open(f.root.join("app")).unwrap();
    assert_eq!(
        reopened
            .load_prepared_unity_import(prepared.job_id())
            .unwrap(),
        prepared
    );
    reopened
        .cleanup_completed_unity_import(prepared.job_id())
        .unwrap();
    assert!(!stage.exists());
    assert!(f.object().is_file());
    assert_eq!(
        reopened.snapshot().unwrap().items[0].versions[0].state,
        "stored"
    );
    assert_eq!(
        fs::read_dir(f.root.join("app/Library/extracted"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn rejects_empty_duplicate_unknown_and_non_package_selections_before_writing() {
    let f = Fixture::new(&[("A.unitypackage", b"a"), ("readme.txt", b"text")]);
    let package = f.selection(&["A.unitypackage"])[0].clone();
    for ids in [
        vec![],
        vec![package.clone(), package],
        vec!["unknown".into()],
        f.selection(&["readme.txt"]),
    ] {
        assert_eq!(
            f.service
                .prepare_zip_unity_import(&f.version_id, &ids)
                .unwrap_err()
                .code(),
            "INVALID_CONTENT_SELECTION"
        );
    }
    assert!(!f.root.join("app/unity-staging").exists());
}

#[test]
fn detects_corrupt_original_before_staging() {
    let f = Fixture::new(&[("A.unitypackage", b"a")]);
    let ids = f.selection(&["A.unitypackage"]);
    fs::write(f.object(), b"tampered").unwrap();
    assert_eq!(
        f.service
            .prepare_zip_unity_import(&f.version_id, &ids)
            .unwrap_err()
            .code(),
        "OBJECT_STORE_CORRUPT"
    );
    assert!(!f.root.join("app/unity-staging").exists());
}

#[test]
fn rejects_unsafe_directory_entries_even_without_extracting_them() {
    for (names, code) in [
        (vec!["../escape.unitypackage"], "ARCHIVE_UNSAFE"),
        (vec!["C:/escape.unitypackage"], "ARCHIVE_UNSAFE"),
        (
            vec!["A.unitypackage", "a.unitypackage"],
            "ARCHIVE_DUPLICATE_PATH",
        ),
        (
            vec!["folder", "folder/A.unitypackage"],
            "ARCHIVE_PATH_COLLISION",
        ),
        (
            vec!["folder/A.unitypackage", "folder"],
            "ARCHIVE_PATH_COLLISION",
        ),
    ] {
        let members: Vec<_> = names.iter().map(|name| (*name, b"x".as_slice())).collect();
        let f = Fixture::new(&members);
        assert_eq!(
            f.service.inspect_archive(&f.version_id).unwrap_err().code(),
            code
        );
        assert!(!f.root.join("app/unity-staging").exists());
    }
}

#[test]
fn directory_listing_does_not_decompress_members_or_claim_their_hashes() {
    let f = Fixture::new(&[("A.unitypackage", b"contents")]);
    let mut bytes = fs::read(f.object()).unwrap();
    let local = bytes
        .windows(4)
        .position(|window| window == b"PK\x03\x04")
        .unwrap();
    let name_length =
        u16::from_le_bytes(bytes[local + 26..local + 28].try_into().unwrap()) as usize;
    let extra_length =
        u16::from_le_bytes(bytes[local + 28..local + 30].try_into().unwrap()) as usize;
    bytes[local + 30 + name_length + extra_length] ^= 0xff;
    fs::write(f.object(), bytes).unwrap();
    let contents = f.service.inspect_archive(&f.version_id).unwrap();
    assert_eq!(contents.entries[0].relative_path, "A.unitypackage");
    let json = serde_json::to_value(contents).unwrap();
    assert!(json["entries"][0].get("contentHash").is_none());
}

#[test]
fn encrypted_members_can_be_listed_but_cannot_be_imported() {
    let f = Fixture::new(&[("Locked.unitypackage", b"locked")]);
    let source = f.root.join("Bundle.zip");
    let mut bytes = fs::read(&source).unwrap();
    let central = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    bytes[6] |= 1;
    bytes[central + 8] |= 1;
    fs::write(&source, bytes).unwrap();
    let result = f
        .service
        .import_archive(ImportArchiveInput {
            archive_path: source,
            original_filename: "Bundle.zip".into(),
            extract_archive: false,
            item_id: None,
        })
        .unwrap();
    let version = result
        .item
        .versions
        .iter()
        .find(|v| v.content_hash != f.hash)
        .unwrap();
    let entries = f.service.inspect_archive(&version.id).unwrap().entries;
    assert!(entries[0].encrypted);
    assert_eq!(
        f.service
            .prepare_zip_unity_import(&version.id, &[entries[0].id.clone()])
            .unwrap_err()
            .code(),
        "ARCHIVE_ENCRYPTED"
    );
    assert!(!f.root.join("app/unity-staging").exists());
}

#[test]
fn crc_failure_during_materialization_cleans_unpublished_partial_files() {
    let f = Fixture::new(&[("Broken.unitypackage", b"contents")]);
    let source = f.root.join("Bundle.zip");
    let mut bytes = fs::read(&source).unwrap();
    let central = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    bytes[central + 16] ^= 0xff; // incorrect CRC, while the original's SHA-256 is valid
    fs::write(&source, bytes).unwrap();
    let result = f
        .service
        .import_archive(ImportArchiveInput {
            archive_path: source,
            original_filename: "Bundle.zip".into(),
            extract_archive: false,
            item_id: None,
        })
        .unwrap();
    let version = result
        .item
        .versions
        .iter()
        .find(|v| v.content_hash != f.hash)
        .unwrap();
    let entries = f.service.inspect_archive(&version.id).unwrap().entries;
    assert!(f
        .service
        .prepare_zip_unity_import(&version.id, &[entries[0].id.clone()])
        .is_err());
    assert_eq!(
        fs::read_dir(f.root.join("app/unity-staging"))
            .unwrap()
            .count(),
        0
    );
    assert!(f.object().is_file());
}

#[test]
fn rejects_ambiguous_unity_callback_names_before_staging() {
    let f = Fixture::new(&[
        ("PC/A.unitypackage", b"pc"),
        ("Quest/A.unitypackage", b"quest"),
    ]);
    let ids = f.selection(&["PC/A.unitypackage", "Quest/A.unitypackage"]);
    assert_eq!(
        f.service
            .prepare_zip_unity_import(&f.version_id, &ids)
            .unwrap_err()
            .code(),
        "UNITYPACKAGE_NAME_COLLISION"
    );
    assert!(!f.root.join("app/unity-staging").exists());
}

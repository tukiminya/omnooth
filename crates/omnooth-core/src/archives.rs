//! Format readers feed a single bounded writer. Archive paths are never used by an unpack API.
use super::*;
use std::collections::HashMap;

struct Output<'a> {
    root: &'a Path,
    seen: HashMap<String, bool>,
    total: u64,
    parents: std::collections::HashSet<String>,
}
impl<'a> Output<'a> {
    fn target(&mut self, name: &str, directory: bool, size: u64) -> CoreResult<PathBuf> {
        if self.seen.len() >= MAX_ARCHIVE_ENTRIES {
            return Err(CoreError::Invalid("ARCHIVE_TOO_MANY_ENTRIES"));
        }
        let relative = safe_member_path(name)?;
        let key = relative.to_string_lossy().replace('\\', "/").to_lowercase();
        if self.seen.contains_key(&key) {
            return Err(CoreError::Invalid("ARCHIVE_DUPLICATE_PATH"));
        }
        if !directory && self.parents.contains(&key) {
            return Err(CoreError::Invalid("ARCHIVE_PATH_COLLISION"));
        }
        for (offset, _) in key.match_indices('/') {
            let parent = &key[..offset];
            if self.seen.get(parent) == Some(&false) {
                return Err(CoreError::Invalid("ARCHIVE_PATH_COLLISION"));
            }
            self.parents.insert(parent.to_owned());
        }
        if size > MAX_EXPANDED_BYTES.saturating_sub(self.total) {
            return Err(CoreError::Invalid("ARCHIVE_TOO_LARGE"));
        }
        self.seen.insert(key, directory);
        let target = self.root.join(relative);
        if directory {
            if size != 0 {
                return Err(CoreError::Invalid("ARCHIVE_INVALID_DIRECTORY"));
            }
            fs::create_dir_all(&target)?;
        } else if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(target)
    }
    fn entry(
        &mut self,
        name: &str,
        directory: bool,
        size: u64,
        reader: &mut dyn Read,
    ) -> CoreResult<()> {
        if directory && matches!(name, "." | "./") {
            return Ok(());
        }
        let path = self.target(name, directory, size)?;
        if directory {
            return Ok(());
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        let copied = copy_bounded(reader, &mut output, MAX_EXPANDED_BYTES - self.total)?;
        self.total += copied;
        if copied != size {
            return Err(CoreError::Invalid("ARCHIVE_SIZE_MISMATCH"));
        }
        output.sync_all()?;
        Ok(())
    }
}

pub(crate) fn extract(
    path: &Path,
    name: &str,
    root: &Path,
    password: Option<&str>,
) -> CoreResult<()> {
    let mut output = Output {
        root,
        seen: HashMap::new(),
        total: 0,
        parents: Default::default(),
    };
    match import_format(name)? {
        ImportFormat::Zip => super::extract_zip_safely_with_password(path, root, password),
        ImportFormat::SevenZip => {
            let mut reader = sevenz_rust2::ArchiveReader::new_with_memory_limit(
                File::open(path)?,
                sevenz_rust2::Password::from(password.unwrap_or("")),
                512 * 1024,
            )
            .map_err(|error| seven_error(error, password))?;
            reader.set_thread_count(1);
            if reader.archive().files.len() > MAX_ARCHIVE_ENTRIES {
                return Err(CoreError::Invalid("ARCHIVE_TOO_MANY_ENTRIES"));
            }
            let mut failure = None;
            reader
                .for_each_entries(|entry, reader| {
                    let kind = (entry.windows_attributes >> 16) & 0o170000;
                    let result = if entry.is_anti_item
                        || (entry.windows_attributes & 0x400) != 0
                        || !matches!(kind, 0 | 0o100000 | 0o040000)
                    {
                        Err(CoreError::Invalid("ARCHIVE_SPECIAL_FILE"))
                    } else {
                        output.entry(&entry.name, entry.is_directory, entry.size, reader)
                    };
                    if let Err(error) = result {
                        failure = Some(error);
                        return Err(std::io::Error::other("ARCHIVE_REJECTED").into());
                    }
                    Ok(true)
                })
                .map_err(|error| failure.unwrap_or_else(|| seven_error(error, password)))
        }
        ImportFormat::Rar => extract_rar(path, &mut output, password),
        format => {
            let file = File::open(path)?;
            let mut reader: Box<dyn Read> = match format {
                ImportFormat::Tar => Box::new(file),
                ImportFormat::TarGzip => Box::new(flate2::read::MultiGzDecoder::new(file)),
                ImportFormat::TarBzip2 => Box::new(bzip2::read::MultiBzDecoder::new(file)),
                ImportFormat::TarXz => Box::new(xz2::read::XzDecoder::new_stream(
                    file,
                    xz2::stream::Stream::new_stream_decoder(
                        512 * 1024 * 1024,
                        xz2::stream::CONCATENATED,
                    )
                    .map_err(|_| CoreError::Invalid("ARCHIVE_INVALID"))?,
                )),
                _ => return Err(CoreError::Invalid("EXTRACTION_UNSUPPORTED")),
            };
            {
                let mut archive = tar::Archive::new(&mut reader);
                let mut pending_name: Option<String> = None;
                let mut pending_size: Option<u64> = None;
                for (index, entry) in archive.entries()?.raw(true).enumerate() {
                    if index >= MAX_ARCHIVE_ENTRIES {
                        return Err(CoreError::Invalid("ARCHIVE_TOO_MANY_ENTRIES"));
                    }
                    let mut entry = entry?;
                    let kind = entry.header().entry_type();
                    if kind.is_gnu_longname() {
                        if entry.size() > 8192 || pending_name.is_some() {
                            return Err(CoreError::Invalid("ARCHIVE_UNSAFE"));
                        }
                        let mut name = String::new();
                        entry.read_to_string(&mut name)?;
                        pending_name = Some(name.trim_end_matches('\0').to_owned());
                        continue;
                    }
                    if kind.is_pax_local_extensions() || kind.is_pax_global_extensions() {
                        if entry.size() > 65536 {
                            return Err(CoreError::Invalid("ARCHIVE_TOO_LARGE"));
                        }
                        if let Some(extensions) = entry.pax_extensions()? {
                            for extension in extensions {
                                let extension = extension?;
                                match extension
                                    .key()
                                    .map_err(|_| CoreError::Invalid("ARCHIVE_UNSAFE"))?
                                {
                                    "path"
                                        if !kind.is_pax_global_extensions()
                                            && pending_name.is_none() =>
                                    {
                                        pending_name = Some(
                                            extension
                                                .value()
                                                .map_err(|_| CoreError::Invalid("ARCHIVE_UNSAFE"))?
                                                .into(),
                                        )
                                    }
                                    "size"
                                        if !kind.is_pax_global_extensions()
                                            && pending_size.is_none() =>
                                    {
                                        pending_size = Some(
                                            extension
                                                .value()
                                                .map_err(|_| CoreError::Invalid("ARCHIVE_UNSAFE"))?
                                                .parse()
                                                .map_err(|_| {
                                                    CoreError::Invalid("ARCHIVE_INVALID")
                                                })?,
                                        )
                                    }
                                    "path" | "size" | "linkpath" => {
                                        return Err(CoreError::Invalid("ARCHIVE_UNSAFE"))
                                    }
                                    _ => {}
                                }
                            }
                        }
                        continue;
                    }
                    if !kind.is_file() && !kind.is_dir() {
                        return Err(CoreError::Invalid("ARCHIVE_SPECIAL_FILE"));
                    }
                    let name = match pending_name.take() {
                        Some(name) => name,
                        None => entry
                            .path()?
                            .to_str()
                            .ok_or(CoreError::Invalid("ARCHIVE_UNSAFE"))?
                            .to_owned(),
                    };
                    let size = entry.size();
                    if pending_size.take().is_some_and(|value| value != size) {
                        return Err(CoreError::Invalid("ARCHIVE_SIZE_MISMATCH"));
                    }
                    output.entry(&name, kind.is_dir(), size, &mut entry)?;
                }
                if pending_name.is_some() || pending_size.is_some() {
                    return Err(CoreError::Invalid("ARCHIVE_INVALID"));
                }
            }
            // Drain compressed trailers so checksum failures cannot be hidden by tar's end markers.
            let mut sink = std::io::sink();
            copy_bounded(&mut reader, &mut sink, 1024 * 1024)?;
            Ok(())
        }
    }
}
fn seven_error(error: sevenz_rust2::Error, password: Option<&str>) -> CoreError {
    match error {
        sevenz_rust2::Error::PasswordRequired => CoreError::Invalid("ARCHIVE_PASSWORD_REQUIRED"),
        sevenz_rust2::Error::MaybeBadPassword(_) => CoreError::Invalid(if password.is_none() {
            "ARCHIVE_PASSWORD_REQUIRED"
        } else {
            "ARCHIVE_PASSWORD_INVALID"
        }),
        sevenz_rust2::Error::MaxMemLimited { .. } => CoreError::Invalid("ARCHIVE_TOO_LARGE"),
        _ => CoreError::Invalid("ARCHIVE_INVALID"),
    }
}

fn extract_rar(path: &Path, output: &mut Output<'_>, password: Option<&str>) -> CoreResult<()> {
    use unrar_sys as rar;
    struct Handle(*const rar::Handle);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                rar::RARCloseArchive(self.0);
            }
        }
    }
    struct Stream {
        file: Option<File>,
        size: u64,
        limit: u64,
        password: Option<String>,
        error: Option<CoreError>,
    }
    extern "C" fn callback(
        message: rar::UINT,
        user: rar::LPARAM,
        p1: rar::LPARAM,
        p2: rar::LPARAM,
    ) -> i32 {
        // The C library calls synchronously on this thread; the context outlives the handle.
        let state = unsafe { &mut *(user as *mut Stream) };
        if message == rar::UCM_PROCESSDATA {
            if p1 == 0 || p2 < 0 || p2 as u64 > state.limit.saturating_sub(state.size) {
                state.error = Some(CoreError::Invalid("ARCHIVE_TOO_LARGE"));
                return -1;
            }
            let bytes = unsafe { std::slice::from_raw_parts(p1 as *const u8, p2 as usize) };
            match state.file.as_mut().map(|file| file.write_all(bytes)) {
                Some(Ok(())) => {
                    state.size += p2 as u64;
                    return 1;
                }
                _ => {
                    state.error = Some(CoreError::Invalid("ARCHIVE_IO_ERROR"));
                    return -1;
                }
            }
        }
        if matches!(message, rar::UCM_NEEDPASSWORD | rar::UCM_NEEDPASSWORDW) {
            let Some(password) = state.password.as_ref() else {
                return -1;
            };
            if p1 == 0 || p2 <= 0 {
                return -1;
            }
            if message == rar::UCM_NEEDPASSWORD {
                if password.len() + 1 > p2 as usize {
                    return -1;
                }
                unsafe {
                    std::ptr::copy_nonoverlapping(password.as_ptr(), p1 as *mut u8, password.len());
                    *((p1 as *mut u8).add(password.len())) = 0;
                }
            } else {
                #[cfg(windows)]
                let wide: Vec<rar::WCHAR> = password.encode_utf16().chain(Some(0)).collect();
                #[cfg(not(windows))]
                let wide: Vec<rar::WCHAR> = password
                    .chars()
                    .map(|c| c as rar::WCHAR)
                    .chain(Some(0))
                    .collect();
                if wide.len() > p2 as usize {
                    return -1;
                }
                unsafe {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), p1 as *mut rar::WCHAR, wide.len());
                }
            }
            return 1;
        }
        -1 // Never prompt for missing volumes or allow writes by the native unpacker.
    }
    fn error(code: i32, password: Option<&str>) -> CoreError {
        match code {
            rar::ERAR_MISSING_PASSWORD => CoreError::Invalid("ARCHIVE_PASSWORD_REQUIRED"),
            rar::ERAR_BAD_PASSWORD => CoreError::Invalid(if password.is_none() {
                "ARCHIVE_PASSWORD_REQUIRED"
            } else {
                "ARCHIVE_PASSWORD_INVALID"
            }),
            _ => CoreError::Invalid("ARCHIVE_INVALID"),
        }
    }
    let mut stream = Stream {
        file: None,
        size: 0,
        limit: 0,
        password: password.map(str::to_owned),
        error: None,
    };
    #[cfg(not(windows))]
    let wide: Vec<rar::WCHAR> = path
        .to_str()
        .ok_or(CoreError::Invalid("INVALID_ARCHIVE_PATH"))?
        .chars()
        .map(|c| c as rar::WCHAR)
        .chain(Some(0))
        .collect();
    #[cfg(windows)]
    let wide: Vec<rar::WCHAR> = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    };
    let mut options = rar::OpenArchiveDataEx::new(wide.as_ptr(), rar::RAR_OM_EXTRACT);
    options.callback = Some(callback);
    options.user_data = (&mut stream as *mut Stream) as rar::LPARAM;
    // UnRAR writes these records despite the bindings declaring const pointers.
    let options_pointer = &mut options as *mut rar::OpenArchiveDataEx;
    let handle = unsafe { rar::RAROpenArchiveEx(options_pointer) };
    if handle.is_null() {
        return Err(error(options.open_result as i32, password));
    }
    let handle = Handle(handle);
    if options.open_result != 0 {
        return Err(error(options.open_result as i32, password));
    }
    loop {
        let mut header = rar::HeaderDataEx::default();
        let header_pointer = &mut header as *mut rar::HeaderDataEx;
        let result = unsafe { rar::RARReadHeaderEx(handle.0, header_pointer) };
        if result == rar::ERAR_END_ARCHIVE {
            break;
        }
        if result != 0 {
            return Err(error(result, password));
        }
        if header.redir_type != 0
            || header.file_attr & 0x400 != 0
            || (header.host_os == 3
                && !matches!(header.file_attr & 0o170000, 0 | 0o100000 | 0o040000))
        {
            return Err(CoreError::Invalid("ARCHIVE_SPECIAL_FILE"));
        }
        if header.dict_size > 512 * 1024
            || header.flags & (rar::RHDF_SPLITBEFORE | rar::RHDF_SPLITAFTER) != 0
        {
            return Err(CoreError::Invalid("ARCHIVE_TOO_LARGE"));
        }
        if header.flags & rar::RHDF_ENCRYPTED != 0 && password.is_none() {
            return Err(CoreError::Invalid("ARCHIVE_PASSWORD_REQUIRED"));
        }
        let directory = header.flags & rar::RHDF_DIRECTORY != 0;
        let size = ((header.unp_size_high as u64) << 32) | header.unp_size as u64;
        #[cfg(windows)]
        let name = String::from_utf16(
            &header
                .filename_w
                .iter()
                .copied()
                .take_while(|c| *c != 0)
                .collect::<Vec<_>>(),
        )
        .map_err(|_| CoreError::Invalid("ARCHIVE_UNSAFE"))?;
        #[cfg(not(windows))]
        let name = header
            .filename_w
            .iter()
            .take_while(|c| **c != 0)
            .map(|c| char::from_u32(*c as u32).ok_or(CoreError::Invalid("ARCHIVE_UNSAFE")))
            .collect::<CoreResult<String>>()?;
        let target = output.target(&name, directory, size)?;
        stream.size = 0;
        stream.limit = MAX_EXPANDED_BYTES - output.total;
        stream.file = if directory {
            None
        } else {
            Some(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(target)?,
            )
        };
        let result = unsafe {
            rar::RARProcessFileW(
                handle.0,
                if directory {
                    rar::RAR_SKIP
                } else {
                    rar::RAR_TEST
                },
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if let Some(error) = stream.error.take() {
            return Err(error);
        }
        if result != 0 {
            return Err(error(result, password));
        }
        if !directory {
            if stream.size != size {
                return Err(CoreError::Invalid("ARCHIVE_SIZE_MISMATCH"));
            }
            output.total += stream.size;
            if let Some(file) = stream.file.take() {
                file.sync_all()?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("archives-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }
    fn tar_bytes() -> Vec<u8> {
        let mut archive = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(7);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, "dir/Selected.unitypackage", &b"fixture"[..])
            .unwrap();
        archive.into_inner().unwrap()
    }
    #[test]
    fn tar_and_compressed_variants_preserve_originals_and_register_files() {
        let root = root();
        let service = Service::open(root.join("app")).unwrap();
        let bytes = tar_bytes();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&bytes).unwrap();
        let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
        bz.write_all(&bytes).unwrap();
        let mut xz = xz2::write::XzEncoder::new(Vec::new(), 1);
        xz.write_all(&bytes).unwrap();
        for (name, bytes) in [
            ("a.tar", bytes),
            ("b.tgz", gz.finish().unwrap()),
            ("c.tbz2", bz.finish().unwrap()),
            ("d.txz", xz.finish().unwrap()),
        ] {
            let path = root.join(name);
            fs::write(&path, &bytes).unwrap();
            let result = service
                .import_archive(ImportArchiveInput {
                    archive_path: path.clone(),
                    original_filename: name.into(),
                    extract_archive: true,
                    item_id: None,
                })
                .unwrap();
            assert_eq!(result.item.versions[0].state, "extracted");
            assert_eq!(result.item.versions[0].files.len(), 1);
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn encrypted_zip_and_sevenz_require_transient_passwords() {
        let root = root();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("Selected.unitypackage"), b"fixture").unwrap();
        let seven = root.join("fixture.7z");
        sevenz_rust2::compress_to_path_encrypted(
            &source,
            &seven,
            sevenz_rust2::Password::from("test-password"),
        )
        .unwrap();
        let zip = root.join("fixture.zip");
        let mut writer = zip::ZipWriter::new(File::create(&zip).unwrap());
        writer
            .start_file(
                "Selected.unitypackage",
                zip::write::SimpleFileOptions::default()
                    .with_aes_encryption(zip::AesMode::Aes256, "test-password"),
            )
            .unwrap();
        writer.write_all(b"fixture").unwrap();
        writer.finish().unwrap();
        let service = Service::open(root.join("app")).unwrap();
        for (path, name) in [(seven, "fixture.7z"), (zip, "fixture.zip")] {
            let item = service
                .import_archive(ImportArchiveInput {
                    archive_path: path,
                    original_filename: name.into(),
                    extract_archive: true,
                    item_id: None,
                })
                .unwrap()
                .item;
            assert_eq!(item.versions[0].state, "stored");
            let version = &item.versions[0].id;
            assert!(service
                .extract_archive_with_password(version, Some("wrong"))
                .is_err());
            assert_eq!(
                service
                    .extract_archive_with_password(version, Some("test-password"))
                    .unwrap()
                    .files
                    .len(),
                1
            );
        }
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn rar_streams_through_bounded_writer() {
        let root = root();
        let out = root.join("out");
        fs::create_dir(&out).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/test_read_format_rar5_stored.rar");
        extract(&source, "fixture.rar", &out, None).unwrap();
        let links =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/libarchive-sample.rar");
        let unsafe_out = root.join("unsafe-out");
        fs::create_dir(&unsafe_out).unwrap();
        assert_eq!(
            extract(&links, "links.rar", &unsafe_out, None)
                .unwrap_err()
                .code(),
            "ARCHIVE_SPECIAL_FILE"
        );
        assert!(fs::read_dir(&out).unwrap().next().is_some());
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn rar_encrypted_headers_require_a_password_and_accept_the_correct_one() {
        let root = root();
        let service = Service::open(root.join("app")).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/test_read_format_rar5_encrypted_filenames.rar");
        let item = service
            .import_archive(ImportArchiveInput {
                archive_path: source,
                original_filename: "encrypted.rar".into(),
                extract_archive: true,
                item_id: None,
            })
            .unwrap()
            .item;
        assert_eq!(item.versions[0].state, "stored");
        assert!(service
            .extract_archive_with_password(&item.versions[0].id, Some("wrong"))
            .is_err());
        assert_eq!(
            service
                .extract_archive_with_password(&item.versions[0].id, Some("password"))
                .unwrap()
                .state,
            "extracted"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn links_and_path_aliases_are_rejected_before_writing() {
        let root = root();
        let mut output = Output {
            root: &root,
            seen: HashMap::new(),
            parents: Default::default(),
            total: 0,
        };
        output.entry("Dir/File", false, 1, &mut &b"a"[..]).unwrap();
        assert!(output.entry("dir/file", false, 1, &mut &b"b"[..]).is_err());
        assert!(output.entry("../escape", false, 1, &mut &b"b"[..]).is_err());
        let mut bytes = Vec::new();
        let mut archive = tar::Builder::new(&mut bytes);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_link_name("/tmp").unwrap();
        header.set_cksum();
        archive
            .append_data(&mut header, "link", std::io::empty())
            .unwrap();
        archive.finish().unwrap();
        drop(archive);
        let path = root.join("unsafe.tar");
        fs::write(&path, bytes).unwrap();
        assert!(extract(&path, "unsafe.tar", &root, None).is_err());
        let _ = fs::remove_dir_all(root);
    }
}

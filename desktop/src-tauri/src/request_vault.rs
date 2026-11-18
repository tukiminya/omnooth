//! Authenticated ciphertext only. The per-data-directory key lives in the OS credential store.
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use omnooth_core::{parse_item_import_deep_link, ParsedItemImport};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub struct RequestVault {
    directory: PathBuf,
    key: zeroize::Zeroizing<[u8; 32]>,
}
impl RequestVault {
    pub fn open(app_data: &Path) -> Result<Self, &'static str> {
        let canonical = fs::canonicalize(app_data).map_err(|_| "CREDENTIAL_STORE_UNAVAILABLE")?;
        // Keep isolated app-data directories from sharing a key.
        let entry = keyring::Entry::new(
            "dev.tuki.omnooth.download-key",
            &canonical.to_string_lossy(),
        )
        .map_err(|_| "CREDENTIAL_STORE_UNAVAILABLE")?;
        let key = match entry.get_secret() {
            Ok(bytes) => {
                let bytes = zeroize::Zeroizing::new(bytes);
                if bytes.len() != 32 {
                    return Err("CREDENTIAL_KEY_INVALID");
                }
                let mut key = [0; 32];
                key.copy_from_slice(&bytes);
                key
            }
            Err(keyring::Error::NoEntry) => {
                let mut key = [0; 32];
                getrandom::fill(&mut key).map_err(|_| "CREDENTIAL_STORE_UNAVAILABLE")?;
                entry
                    .set_secret(&key)
                    .map_err(|_| "CREDENTIAL_STORE_UNAVAILABLE")?;
                key
            }
            Err(_) => return Err("CREDENTIAL_STORE_UNAVAILABLE"),
        };
        Self::with_key(app_data, key)
    }
    pub(crate) fn with_key(app_data: &Path, key: [u8; 32]) -> Result<Self, &'static str> {
        let directory = app_data.join("protected-requests");
        fs::create_dir_all(&directory).map_err(|_| "REQUEST_STORAGE_ERROR")?;
        if fs::symlink_metadata(&directory)
            .map_err(|_| "REQUEST_STORAGE_ERROR")?
            .file_type()
            .is_symlink()
        {
            return Err("REQUEST_STORAGE_ERROR");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                .map_err(|_| "REQUEST_STORAGE_ERROR")?;
        }
        Ok(Self {
            directory,
            key: zeroize::Zeroizing::new(key),
        })
    }
    pub fn save(&self, id: u64, request: &ParsedItemImport) -> Result<(), &'static str> {
        let mut nonce = [0; 12];
        getrandom::fill(&mut nonce).map_err(|_| "REQUEST_STORAGE_ERROR")?;
        let cipher =
            Aes256Gcm::new_from_slice(self.key.as_ref()).map_err(|_| "REQUEST_STORAGE_ERROR")?;
        let aad = format!("dev.tuki.omnooth.download:{id}");
        let link = zeroize::Zeroizing::new(request.protected_resume_link());
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: link.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| "REQUEST_STORAGE_ERROR")?;
        let temporary = self.directory.join(format!("{id}.tmp"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let _ = fs::remove_file(&temporary);
        let mut file = options
            .open(&temporary)
            .map_err(|_| "REQUEST_STORAGE_ERROR")?;
        file.write_all(&nonce)
            .and_then(|_| file.write_all(&encrypted))
            .and_then(|_| file.sync_all())
            .map_err(|_| "REQUEST_STORAGE_ERROR")?;
        let target = self.directory.join(format!("{id}.enc"));
        #[cfg(windows)]
        {
            let _ = fs::remove_file(&target);
        }
        fs::rename(temporary, target).map_err(|_| "REQUEST_STORAGE_ERROR")?;
        #[cfg(unix)]
        {
            std::fs::File::open(&self.directory)
                .and_then(|f| f.sync_all())
                .map_err(|_| "REQUEST_STORAGE_ERROR")?;
        }
        Ok(())
    }
    pub fn restore(&self, id: u64) -> Option<ParsedItemImport> {
        let path = self.directory.join(format!("{id}.enc"));
        let metadata = fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file() || metadata.len() > 128 * 1024 || metadata.len() < 28 {
            return None;
        }
        let data = fs::read(path).ok()?;
        let cipher = Aes256Gcm::new_from_slice(self.key.as_ref()).ok()?;
        let aad = format!("dev.tuki.omnooth.download:{id}");
        let plaintext = zeroize::Zeroizing::new(
            cipher
                .decrypt(
                    Nonce::from_slice(&data[..12]),
                    Payload {
                        msg: &data[12..],
                        aad: aad.as_bytes(),
                    },
                )
                .ok()?,
        );
        let request = parse_item_import_deep_link(std::str::from_utf8(&plaintext).ok()?).ok()?;
        request.can_resume_now().then_some(request)
    }
    pub fn remove(&self, id: u64) {
        let _ = fs::remove_file(self.directory.join(format!("{id}.enc")));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Writes a new isolated test key to the OS credential store"]
    fn native_credential_store_round_trip() {
        let root =
            std::env::temp_dir().join(format!("omnooth-native-vault-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let canonical = fs::canonicalize(&root).unwrap();
        let result = (|| -> Result<(), &'static str> {
            let vault = RequestVault::open(&root)?;
            let reopened = RequestVault::open(&root)?;
            if *vault.key != *reopened.key {
                return Err("CREDENTIAL_KEY_MISMATCH");
            }
            Ok(())
        })();
        if let Ok(entry) = keyring::Entry::new(
            "dev.tuki.omnooth.download-key",
            &canonical.to_string_lossy(),
        ) {
            let _ = entry.delete_credential();
        }
        let _ = fs::remove_dir_all(root);
        assert!(
            result.is_ok(),
            "Native credential store round-trip failed: {:?}",
            result.err()
        );
    }

    #[test]
    fn ciphertext_is_bound_to_job_and_key_and_rejects_tampering() {
        let root = std::env::temp_dir().join(format!("vault-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let vault = RequestVault::with_key(&root, [7; 32]).unwrap();
        let request=parse_item_import_deep_link("booth-library-manager://item-import?dlurl=https%3A%2F%2Fs6.booth.pm%2Ffiles%2Fa.zip%3FX-Amz-Date%3D20990101T000000Z%26X-Amz-Expires%3D86400%26token%3DSYNTHETIC_TOKEN&downloadable_filename=a.zip&item_id=1").unwrap();
        vault.save(1, &request).unwrap();
        let bytes = fs::read(vault.directory.join("1.enc")).unwrap();
        assert!(!bytes.windows(15).any(|v| v == b"SYNTHETIC_TOKEN"));
        assert!(vault.restore(1).is_some());
        fs::copy(vault.directory.join("1.enc"), vault.directory.join("2.enc")).unwrap();
        assert!(vault.restore(2).is_none());
        assert!(RequestVault::with_key(&root, [8; 32])
            .unwrap()
            .restore(1)
            .is_none());
        let mut changed = bytes;
        changed[20] ^= 1;
        fs::write(vault.directory.join("1.enc"), changed).unwrap();
        assert!(vault.restore(1).is_none());
        vault.remove(1);
        assert!(!vault.directory.join("1.enc").exists());
        let _ = fs::remove_dir_all(root);
    }
}

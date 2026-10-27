use serde::Serialize;
use tauri::AppHandle;

const SCHEME: &str = "booth-library-manager";
const APP_ID: &str = "dev.tuki.omnooth";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkRegistration {
    pub scheme: String,
    pub registered: bool,
}

fn registration_status(owner: Option<&str>) -> DeepLinkRegistration {
    DeepLinkRegistration {
        scheme: SCHEME.to_owned(),
        registered: owner == Some(APP_ID),
    }
}

/// Reads whether this application is the default handler for the supported scheme.
pub fn status(app: &AppHandle) -> Result<DeepLinkRegistration, String> {
    #[cfg(target_os = "macos")]
    {
        let owner = macos::default_handler(SCHEME)?;
        let _ = app;
        return Ok(registration_status(owner.as_deref()));
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        use tauri_plugin_deep_link::DeepLinkExt;

        let registered = app
            .deep_link()
            .is_registered(SCHEME)
            .map_err(|_| "REGISTRATION_STATUS_FAILED".to_owned())?;
        return Ok(DeepLinkRegistration {
            scheme: SCHEME.to_owned(),
            registered,
        });
    }

    #[allow(unreachable_code)]
    Err("REGISTRATION_UNSUPPORTED".to_owned())
}

/// Registers this application as the default handler, then verifies the OS readback.
pub fn register(app: &AppHandle) -> Result<DeepLinkRegistration, String> {
    #[cfg(target_os = "macos")]
    {
        macos::register_current_bundle()?;
        return status(app).and_then(|result| {
            if result.registered {
                Ok(result)
            } else {
                Err("REGISTRATION_READBACK_FAILED".to_owned())
            }
        });
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        use tauri_plugin_deep_link::DeepLinkExt;

        app.deep_link()
            .register(SCHEME)
            .map_err(|_| "REGISTRATION_FAILED".to_owned())?;
        return status(app).and_then(|result| {
            if result.registered {
                Ok(result)
            } else {
                Err("REGISTRATION_READBACK_FAILED".to_owned())
            }
        });
    }

    #[allow(unreachable_code)]
    Err("REGISTRATION_UNSUPPORTED".to_owned())
}

#[cfg(target_os = "macos")]
mod macos {
    use super::APP_ID;
    use std::{
        ffi::{c_char, c_void, CStr, CString},
        os::unix::ffi::OsStrExt,
        path::{Path, PathBuf},
        ptr,
    };

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            value: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFStringGetCString(
            value: *const c_void,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            bytes: *const u8,
            length: isize,
            is_directory: u8,
        ) -> *const c_void;
        fn CFRelease(value: *const c_void);
    }

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSRegisterURL(url: *const c_void, update: u8) -> i32;
        fn LSSetDefaultHandlerForURLScheme(scheme: *const c_void, bundle_id: *const c_void) -> i32;
        fn LSCopyDefaultHandlerForURLScheme(scheme: *const c_void) -> *const c_void;
    }

    struct CfOwned(*const c_void);

    impl CfOwned {
        fn new(value: *const c_void) -> Result<Self, String> {
            if value.is_null() {
                Err("LAUNCH_SERVICES_FAILED".to_owned())
            } else {
                Ok(Self(value))
            }
        }

        fn as_ptr(&self) -> *const c_void {
            self.0
        }
    }

    impl Drop for CfOwned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn cf_string(value: &str) -> Result<CfOwned, String> {
        let value = CString::new(value).map_err(|_| "LAUNCH_SERVICES_FAILED".to_owned())?;
        unsafe {
            CfOwned::new(CFStringCreateWithCString(
                ptr::null(),
                value.as_ptr(),
                K_CF_STRING_ENCODING_UTF8,
            ))
        }
    }

    pub(super) fn bundle_root_from_executable(executable: &Path) -> Option<PathBuf> {
        executable
            .ancestors()
            .find(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
            })
            .map(Path::to_path_buf)
    }

    pub(super) fn register_current_bundle() -> Result<(), String> {
        let executable = std::env::current_exe().map_err(|_| "BUNDLE_REQUIRED".to_owned())?;
        let bundle =
            bundle_root_from_executable(&executable).ok_or_else(|| "BUNDLE_REQUIRED".to_owned())?;
        register_bundle(&bundle)
    }

    pub(super) fn register_bundle(bundle: &Path) -> Result<(), String> {
        if !bundle.is_dir() || !bundle.join("Contents/Info.plist").is_file() {
            return Err("BUNDLE_REQUIRED".to_owned());
        }
        let path = bundle.as_os_str().as_bytes();
        let url = unsafe {
            CfOwned::new(CFURLCreateFromFileSystemRepresentation(
                ptr::null(),
                path.as_ptr(),
                path.len() as isize,
                1,
            ))?
        };
        let scheme = cf_string(super::SCHEME)?;
        let bundle_id = cf_string(APP_ID)?;

        let registered = unsafe { LSRegisterURL(url.as_ptr(), 1) };
        if registered != 0 {
            return Err("LAUNCH_SERVICES_REGISTER_FAILED".to_owned());
        }
        let selected =
            unsafe { LSSetDefaultHandlerForURLScheme(scheme.as_ptr(), bundle_id.as_ptr()) };
        if selected != 0 {
            return Err("LAUNCH_SERVICES_SET_DEFAULT_FAILED".to_owned());
        }
        Ok(())
    }

    pub(super) fn default_handler(scheme: &str) -> Result<Option<String>, String> {
        let scheme = cf_string(scheme)?;
        let handler = unsafe { LSCopyDefaultHandlerForURLScheme(scheme.as_ptr()) };
        if handler.is_null() {
            return Ok(None);
        }
        let handler = CfOwned::new(handler)?;
        let mut buffer = [0 as c_char; 512];
        let converted = unsafe {
            CFStringGetCString(
                handler.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
            )
        };
        if converted == 0 {
            return Err("REGISTRATION_STATUS_FAILED".to_owned());
        }
        let value = unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_str()
            .map_err(|_| "REGISTRATION_STATUS_FAILED".to_owned())?;
        Ok(Some(value.to_owned()))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{macos::bundle_root_from_executable, registration_status, APP_ID, SCHEME};
    use std::path::Path;

    #[test]
    fn detects_application_bundle_from_executable_path() {
        assert_eq!(
            bundle_root_from_executable(Path::new(
                "/Applications/omnooth.app/Contents/MacOS/omnooth"
            )),
            Some(Path::new("/Applications/omnooth.app").to_path_buf())
        );
        assert_eq!(
            bundle_root_from_executable(Path::new("/tmp/target/debug/omnooth")),
            None
        );
    }

    #[test]
    fn status_is_registered_only_for_the_expected_bundle_id() {
        assert!(registration_status(Some(APP_ID)).registered);
        assert!(!registration_status(Some("dev.other.app")).registered);
        assert!(!registration_status(None).registered);
        assert_eq!(registration_status(None).scheme, SCHEME);
    }

    #[test]
    #[ignore = "Changes the OS handler; requires OMNOOTH_REGISTRATION_TEST_BUNDLE"]
    fn registers_built_bundle_and_verifies_os_default_owner() {
        let bundle = std::env::var_os("OMNOOTH_REGISTRATION_TEST_BUNDLE")
            .expect("Pass the built omnooth .app path explicitly");
        super::macos::register_bundle(Path::new(&bundle)).unwrap();
        assert_eq!(
            super::macos::default_handler(SCHEME).unwrap().as_deref(),
            Some(APP_ID)
        );
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::{registration_status, APP_ID, SCHEME};

    #[test]
    fn status_is_registered_only_for_the_expected_bundle_id() {
        assert!(registration_status(Some(APP_ID)).registered);
        assert!(!registration_status(Some("dev.other.app")).registered);
        assert!(!registration_status(None).registered);
        assert_eq!(registration_status(None).scheme, SCHEME);
    }
}

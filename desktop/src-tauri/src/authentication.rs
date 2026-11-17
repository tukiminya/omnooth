//! Native OS dialogs; credentials never pass through the webview.
use omnooth_core::{AuthenticationGateway, CoreError, CoreResult};
pub struct NativeAuthentication(pub tauri::AppHandle);
impl AuthenticationGateway for NativeAuthentication {
    fn verify(&self, reason: &str) -> CoreResult<()> {
        #[cfg(target_os = "macos")]
        {
            use objc2::runtime::Bool;
            use objc2_foundation::{NSError, NSString};
            use objc2_local_authentication::{LAContext, LAPolicy};
            let context = unsafe { LAContext::new() };
            let policy = LAPolicy::DeviceOwnerAuthentication;
            unsafe { context.canEvaluatePolicy_error(policy) }
                .map_err(|_| CoreError::Invalid("AUTH_UNAVAILABLE"))?;
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let reply = block2::RcBlock::new(move |success: Bool, _error: *mut NSError| {
                let _ = tx.send(success.as_bool());
            });
            unsafe {
                context.evaluatePolicy_localizedReason_reply(
                    policy,
                    &NSString::from_str(reason),
                    &reply,
                );
            }
            let verified = rx.recv_timeout(std::time::Duration::from_secs(120));
            unsafe {
                context.invalidate();
            }
            return match verified {
                Ok(true) => Ok(()),
                _ => Err(CoreError::Invalid("AUTH_CANCELLED")),
            };
        }
        #[cfg(target_os = "windows")]
        {
            use tauri::Manager;
            use windows::{
                core::{factory, HSTRING},
                Foundation::IAsyncOperation,
                Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier},
                Win32::System::WinRT::IUserConsentVerifierInterop,
            };
            let window = self
                .0
                .get_webview_window("main")
                .ok_or(CoreError::Invalid("AUTH_UNAVAILABLE"))?;
            let hwnd = window
                .hwnd()
                .map_err(|_| CoreError::Invalid("AUTH_UNAVAILABLE"))?;
            let interop: IUserConsentVerifierInterop =
                factory::<UserConsentVerifier, IUserConsentVerifierInterop>()
                    .map_err(|_| CoreError::Invalid("AUTH_UNAVAILABLE"))?;
            let operation: IAsyncOperation<UserConsentVerificationResult> =
                unsafe { interop.RequestVerificationForWindowAsync(hwnd, &HSTRING::from(reason)) }
                    .map_err(|_| CoreError::Invalid("AUTH_UNAVAILABLE"))?;
            return match operation.get() {
                Ok(UserConsentVerificationResult::Verified) => Ok(()),
                _ => Err(CoreError::Invalid("AUTH_CANCELLED")),
            };
        }
        #[allow(unreachable_code)]
        {
            let _ = (&self.0, reason);
            Err(CoreError::Invalid("AUTH_UNAVAILABLE"))
        }
    }
}

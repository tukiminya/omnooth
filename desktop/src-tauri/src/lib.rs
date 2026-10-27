mod deep_link_receiver;
mod deep_link_registration;

use deep_link_receiver::Receiver;
use omnooth_core::{ApplicationCommand, ItemImportReceipt, Service};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;

struct Backend {
    service: Arc<Mutex<Service>>,
    receiver: Mutex<Receiver>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeepLinkStatus {
    scheme: String,
    registered: bool,
    received_count: u64,
    last_receipt: Option<ItemImportReceipt>,
    last_error: Option<String>,
}

fn receiver_status(app: &tauri::AppHandle) -> Result<DeepLinkStatus, String> {
    let registration = deep_link_registration::status(app)?;
    let backend = app.state::<Backend>();
    let receiver = backend.receiver.lock().map_err(|_| "SERVICE_UNAVAILABLE")?;
    Ok(DeepLinkStatus {
        scheme: registration.scheme,
        registered: registration.registered,
        received_count: receiver.received_count,
        last_receipt: receiver.last_receipt.clone(),
        last_error: receiver.last_error.clone(),
    })
}

fn receive_urls(app: &tauri::AppHandle, urls: impl IntoIterator<Item = String>) {
    let backend = app.state::<Backend>();
    let Ok(mut receiver) = backend.receiver.lock() else {
        return;
    };
    let mut changed = false;
    for url in urls {
        changed |= receiver.receive(&url);
    }
    drop(receiver);
    if changed {
        if let Ok(status) = receiver_status(app) {
            let _ = app.emit("deep-link-status-changed", status);
        }
    }
}

#[tauri::command]
async fn get_deep_link_status(app: tauri::AppHandle) -> Result<DeepLinkStatus, String> {
    tauri::async_runtime::spawn_blocking(move || receiver_status(&app))
        .await
        .map_err(|_| "SERVICE_UNAVAILABLE".to_owned())?
}

#[tauri::command]
async fn register_deep_link(app: tauri::AppHandle) -> Result<DeepLinkStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        deep_link_registration::register(&app)?;
        let status = receiver_status(&app)?;
        let _ = app.emit("deep-link-status-changed", &status);
        Ok(status)
    })
    .await
    .map_err(|_| "SERVICE_UNAVAILABLE".to_owned())?
}

#[tauri::command]
async fn open_manual(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.opener()
            .open_url("https://omnooth.tuki.dev/docs", None::<&str>)
            .map_err(|_| "MANUAL_OPEN_FAILED".to_owned())
    })
    .await
    .map_err(|_| "SERVICE_UNAVAILABLE".to_owned())?
}

#[tauri::command]
async fn dispatch(
    state: tauri::State<'_, Backend>,
    command: ApplicationCommand,
) -> Result<serde_json::Value, String> {
    let service = Arc::clone(&state.service);
    tauri::async_runtime::spawn_blocking(move || {
        let service = service
            .lock()
            .map_err(|_| "SERVICE_UNAVAILABLE".to_owned())?;
        service
            .dispatch(command)
            .map_err(|error| format!("{}: {}", error.code(), error))
    })
    .await
    .map_err(|_| "SERVICE_UNAVAILABLE".to_owned())?
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            #[cfg(debug_assertions)]
            let isolated = std::env::var_os("OMNOOTH_DEV_DATA_DIR").map(std::path::PathBuf::from);
            #[cfg(not(debug_assertions))]
            let isolated: Option<std::path::PathBuf> = None;
            let service = if let Some(directory) = isolated {
                Service::open(directory)?
            } else {
                let legacy_data = app_data.with_file_name("jp.tukiminya.omnooth");
                Service::open_migrating_legacy(app_data, legacy_data)?
            };
            app.manage(Backend {
                service: Arc::new(Mutex::new(service)),
                receiver: Mutex::new(Receiver::default()),
            });
            let app_handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                receive_urls(
                    &app_handle,
                    event.urls().into_iter().map(|url| url.to_string()),
                );
            });
            if let Some(urls) = app.deep_link().get_current()? {
                receive_urls(app.handle(), urls.into_iter().map(|url| url.to_string()));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dispatch,
            get_deep_link_status,
            register_deep_link,
            open_manual
        ])
        .run(tauri::generate_context!())
        .expect("omnooth could not start");
}

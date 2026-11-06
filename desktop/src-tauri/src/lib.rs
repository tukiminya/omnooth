mod activity_log;
mod booth_import;
mod deep_link_registration;
mod import_queue;

use activity_log::{ActivityLog, LogLevel, LogSnapshot};
use booth_import::{DeepLinkStatus, ReceiverState};
use import_queue::ImportQueue;
use omnooth_core::{ApplicationCommand, Service};
use std::sync::{atomic::AtomicBool, mpsc::SyncSender, Mutex};
use tauri::Manager;
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;

struct Backend {
    service: Service,
    receiver: Mutex<ReceiverState>,
    queue: Mutex<ImportQueue>,
    logs: Mutex<ActivityLog>,
    worker_running: AtomicBool,
    incoming: SyncSender<Vec<String>>,
}

fn record(
    app: &tauri::AppHandle,
    level: LogLevel,
    operation: &str,
    message: &str,
    correlation: Option<&str>,
) {
    if let Some(backend) = app.try_state::<Backend>() {
        if let Ok(mut logs) = backend.logs.lock() {
            if logs.record(level, operation, message, correlation).is_err() {
                eprintln!("ACTIVITY_LOG_WRITE_FAILED");
            }
        }
    }
}

#[tauri::command]
fn get_logs(state: tauri::State<'_, Backend>) -> Result<LogSnapshot, String> {
    state
        .logs
        .lock()
        .map(|logs| logs.snapshot())
        .map_err(|_| "LOG_UNAVAILABLE".to_owned())
}

#[tauri::command]
fn report_client_error(app: tauri::AppHandle, operation: String, message: String) {
    // All externally supplied fields are redacted and bounded by the logger.
    record(&app, LogLevel::Error, &operation, &message, None);
}

#[tauri::command]
async fn get_deep_link_status(app: tauri::AppHandle) -> Result<DeepLinkStatus, String> {
    tauri::async_runtime::spawn_blocking(move || booth_import::status(&app))
        .await
        .map_err(|_| "SERVICE_UNAVAILABLE".to_owned())?
}

#[tauri::command]
async fn register_deep_link(app: tauri::AppHandle) -> Result<DeepLinkStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        record(
            &app,
            LogLevel::Info,
            "deepLink.register",
            "BOOTH リンクの受信先を設定します",
            None,
        );
        if let Err(code) = deep_link_registration::register(&app) {
            record(&app, LogLevel::Error, "deepLink.register", &code, None);
            return Err(code);
        }
        record(
            &app,
            LogLevel::Info,
            "deepLink.register",
            "OS の既定の受信先を確認しました",
            None,
        );
        booth_import::emit_status(&app);
        booth_import::status(&app)
    })
    .await
    .map_err(|_| "SERVICE_UNAVAILABLE".to_owned())?
}

#[tauri::command]
async fn open_manual(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        record(
            &app,
            LogLevel::Info,
            "manual.open",
            "マニュアルを開きます",
            None,
        );
        app.opener()
            .open_url("https://omnooth.tuki.dev/docs", None::<&str>)
            .map_err(|_| {
                record(
                    &app,
                    LogLevel::Error,
                    "manual.open",
                    "MANUAL_OPEN_FAILED",
                    None,
                );
                "MANUAL_OPEN_FAILED".to_owned()
            })
    })
    .await
    .map_err(|_| "SERVICE_UNAVAILABLE".to_owned())?
}

fn command_name(command: &ApplicationCommand) -> &'static str {
    match command {
        ApplicationCommand::GetSnapshot => "getSnapshot",
        ApplicationCommand::LinkProject { .. } => "linkProject",
        ApplicationCommand::ImportArchive(_) => "importArchive",
        ApplicationCommand::InspectArchive { .. } => "inspectArchive",
        ApplicationCommand::ExtractArchive { .. } => "extractArchive",
        ApplicationCommand::UpdateSettings { .. } => "updateSettings",
    }
}

#[tauri::command]
async fn dispatch(
    app: tauri::AppHandle,
    state: tauri::State<'_, Backend>,
    command: ApplicationCommand,
) -> Result<serde_json::Value, String> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let name = command_name(&command);
        record(&app, LogLevel::Info, name, "処理を開始しました", None);
        match service.dispatch(command) {
            Ok(value) => {
                record(&app, LogLevel::Info, name, "処理が完了しました", None);
                Ok(value)
            }
            Err(error) => {
                let message = format!("{}: {}", error.code(), error);
                record(&app, LogLevel::Error, name, &message, None);
                Err(message)
            }
        }
    })
    .await
    .map_err(|_| "SERVICE_UNAVAILABLE".to_owned())?
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            record(
                app,
                LogLevel::Info,
                "app.activate",
                "起動要求を既存のウィンドウへ渡しました",
                None,
            );
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
            let data_directory = isolated.as_ref().unwrap_or(&app_data);
            let mut logs = ActivityLog::open(data_directory)?;
            logs.record(
                LogLevel::Info,
                "app.start",
                "omnooth 0.1.0 を起動しています",
                None,
            )?;
            let service_result = if isolated.is_some() {
                Service::open(data_directory)
            } else {
                Service::open_migrating_legacy(
                    &app_data,
                    app_data.with_file_name("jp.tukiminya.omnooth"),
                )
            };
            let service = match service_result {
                Ok(service) => service,
                Err(error) => {
                    let _ = logs.record(
                        LogLevel::Error,
                        "service.open",
                        &format!("{}: {}", error.code(), error),
                        None,
                    );
                    return Err(error.into());
                }
            };
            logs.record(
                LogLevel::Info,
                "service.open",
                "ライブラリとデータベースを開きました",
                None,
            )?;
            let queue = match ImportQueue::open(data_directory) {
                Ok(queue) => queue,
                Err(error) => {
                    let _ = logs.record(LogLevel::Error, "queue.restore", error.code(), None);
                    return Err(error.into());
                }
            };
            let restored = queue.snapshot();
            logs.record(
                LogLevel::Info,
                "queue.restore",
                &format!(
                    "保存済みジョブ {}件（待機 {}件）を復元しました",
                    restored.jobs.len(),
                    restored.active_count
                ),
                None,
            )?;
            let (incoming, incoming_rx) = std::sync::mpsc::sync_channel(100);
            app.manage(Backend {
                service,
                receiver: Mutex::new(ReceiverState::from_queue(&restored)),
                queue: Mutex::new(queue),
                logs: Mutex::new(logs),
                worker_running: AtomicBool::new(false),
                incoming,
            });
            let panic_handle = app.handle().clone();
            std::panic::set_hook(Box::new(move |info| {
                // Panic payloads from dependencies may contain credentials; retain only location.
                eprintln!("OMNOOTH_INTERNAL_ERROR");
                if let Some(backend) = panic_handle.try_state::<Backend>() {
                    if let Ok(mut logs) = backend.logs.try_lock() {
                        let location = info
                            .location()
                            .map(|location| location.to_string())
                            .unwrap_or_default();
                        let _ = logs.record(
                            LogLevel::Error,
                            "runtime.panic",
                            &format!("内部エラーが発生しました {location}"),
                            None,
                        );
                    }
                }
            }));
            let admission_handle = app.handle().clone();
            std::thread::Builder::new()
                .name("booth-link-admission".into())
                .spawn(move || {
                    while let Ok(urls) = incoming_rx.recv() {
                        booth_import::receive_urls(&admission_handle, urls);
                    }
                })?;
            let app_handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                booth_import::submit_urls(
                    &app_handle,
                    event.urls().into_iter().map(|url| url.to_string()),
                );
            });
            match app.deep_link().get_current() {
                Ok(Some(urls)) => booth_import::submit_urls(
                    app.handle(),
                    urls.into_iter().map(|url| url.to_string()),
                ),
                Ok(None) => {}
                Err(_) => record(
                    app.handle(),
                    LogLevel::Error,
                    "deepLink.startup",
                    "DEEP_LINK_STARTUP_FAILED",
                    None,
                ),
            }
            record(
                app.handle(),
                LogLevel::Info,
                "app.ready",
                "起動が完了しました。BOOTH リンクを受信できます",
                None,
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dispatch,
            get_deep_link_status,
            register_deep_link,
            open_manual,
            get_logs,
            report_client_error
        ])
        .run(tauri::generate_context!())
        .expect("omnooth could not start; check the application logs");
}

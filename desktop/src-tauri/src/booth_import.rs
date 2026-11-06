use crate::{
    activity_log::LogLevel,
    deep_link_registration,
    import_queue::{EnqueueDisposition, ImportJob, JobStatus, QueueRecord, QueueSnapshot},
    record, Backend,
};
use omnooth_core::{
    download_booth_archive_with_progress, fetch_booth_item_metadata, parse_item_import_deep_link,
    ItemImportReceipt,
};
use serde::Serialize;
use std::{path::Path, sync::atomic::Ordering};
use tauri::{Emitter, Manager};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStatus {
    job_id: u64,
    filename: String,
    state: String,
    error_code: Option<String>,
    item_id: Option<String>,
}

impl ImportStatus {
    fn from_record(record: &QueueRecord) -> Self {
        let state = match record.status {
            JobStatus::Queued => "queued",
            JobStatus::Downloading => "downloading",
            JobStatus::AwaitingUrl => "awaitingUrl",
            JobStatus::Failed => "failed",
            JobStatus::Succeeded => "succeeded",
        };
        Self {
            job_id: record.id,
            filename: record.receipt.filename.clone(),
            state: state.into(),
            error_code: record.error_code.clone(),
            item_id: record.local_item_id.clone(),
        }
    }
}

#[derive(Default)]
pub struct ReceiverState {
    received_count: u64,
    last_receipt: Option<ItemImportReceipt>,
    last_error: Option<String>,
    last_import: Option<ImportStatus>,
}

impl ReceiverState {
    pub fn from_queue(queue: &QueueSnapshot) -> Self {
        let last = queue
            .jobs
            .iter()
            .rev()
            .find(|job| matches!(job.status, JobStatus::AwaitingUrl | JobStatus::Failed));
        Self {
            received_count: 0,
            last_receipt: last.map(|job| job.receipt.clone()),
            last_error: last.and_then(|job| job.error_code.clone()),
            last_import: last.map(ImportStatus::from_record),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkStatus {
    scheme: String,
    registered: bool,
    received_count: u64,
    last_receipt: Option<ItemImportReceipt>,
    last_error: Option<String>,
    last_import: Option<ImportStatus>,
    active_count: usize,
}

pub fn status(app: &tauri::AppHandle) -> Result<DeepLinkStatus, String> {
    let registration = deep_link_registration::status(app)?;
    let backend = app.state::<Backend>();
    let active_count = backend
        .queue
        .lock()
        .map_err(|_| "SERVICE_UNAVAILABLE")?
        .active_count();
    let receiver = backend.receiver.lock().map_err(|_| "SERVICE_UNAVAILABLE")?;
    Ok(DeepLinkStatus {
        scheme: registration.scheme,
        registered: registration.registered,
        received_count: receiver.received_count,
        last_receipt: receiver.last_receipt.clone(),
        last_error: receiver.last_error.clone(),
        last_import: receiver.last_import.clone(),
        active_count,
    })
}

pub fn emit_status(app: &tauri::AppHandle) {
    if let Ok(status) = status(app) {
        let _ = app.emit("deep-link-status-changed", status);
    }
}

fn report_error(app: &tauri::AppHandle, operation: &str, code: &str) {
    record(app, LogLevel::Error, operation, code, None);
    if let Ok(mut receiver) = app.state::<Backend>().receiver.lock() {
        receiver.last_error = Some(code.to_owned());
    }
    emit_status(app);
}

fn update_import(app: &tauri::AppHandle, status: ImportStatus) {
    if let Ok(mut receiver) = app.state::<Backend>().receiver.lock() {
        receiver.last_error = status.error_code.clone();
        receiver.last_import = Some(status);
    }
    emit_status(app);
}

pub fn submit_urls(app: &tauri::AppHandle, urls: impl IntoIterator<Item = String>) {
    let urls: Vec<_> = urls.into_iter().take(101).collect();
    if urls.len() > 100 || urls.iter().any(|url| url.len() > 64 * 1024) {
        report_error(app, "deepLink.receive", "INVALID_DEEP_LINK");
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if app.state::<Backend>().incoming.try_send(urls).is_err() {
        report_error(app, "deepLink.receive", "IMPORT_QUEUE_FULL");
    }
}

pub fn receive_urls(app: &tauri::AppHandle, urls: impl IntoIterator<Item = String>) {
    let backend = app.state::<Backend>();
    for url in urls {
        // Never record this string, its query, or ParsedItemImport's download URL.
        let parsed = match parse_item_import_deep_link(&url) {
            Ok(parsed) => parsed,
            Err(error) => {
                report_error(app, "deepLink.parse", error.code());
                continue;
            }
        };
        let receipt = parsed.receipt().clone();
        record(
            app,
            LogLevel::Info,
            "deepLink.receive",
            &format!(
                "BOOTH 商品 {} の {} を受信しました",
                receipt.booth_item_id, receipt.filename
            ),
            None,
        );
        let settings = match backend.service.snapshot() {
            Ok(snapshot) => snapshot.settings,
            Err(error) => {
                report_error(app, "queue.settings", error.code());
                continue;
            }
        };
        let outcome = match backend.queue.lock() {
            Ok(mut queue) => queue
                .enqueue(parsed, &settings)
                .map_err(|error| error.code().to_owned()),
            Err(_) => Err("SERVICE_UNAVAILABLE".into()),
        };
        match outcome {
            Ok(outcome) => {
                if let Ok(mut receiver) = backend.receiver.lock() {
                    receiver.received_count = receiver.received_count.saturating_add(1);
                    receiver.last_receipt = Some(receipt);
                    receiver.last_error = None;
                }
                let message = match outcome.disposition {
                    EnqueueDisposition::Accepted => "取り込みキューへ保存しました",
                    EnqueueDisposition::Reused => "同じファイルの受付済みジョブを使用します",
                    EnqueueDisposition::AlreadySucceeded => {
                        "このファイルはライブラリに登録済みです"
                    }
                };
                record(
                    app,
                    LogLevel::Info,
                    "queue.enqueue",
                    message,
                    Some(&outcome.record.id.to_string()),
                );
                update_import(app, ImportStatus::from_record(&outcome.record));
            }
            Err(code) => report_error(app, "queue.enqueue", &code),
        }
    }
    start_worker(app);
}

fn start_worker(app: &tauri::AppHandle) {
    let backend = app.state::<Backend>();
    if backend
        .worker_running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || loop {
        let backend = app.state::<Backend>();
        // Clear the flag while holding the queue lock so a concurrent arrival cannot be lost.
        let next = match backend.queue.lock() {
            Ok(mut queue) => match queue.take_next() {
                Ok(Some(job)) => Ok(Some(job)),
                Ok(None) => {
                    backend.worker_running.store(false, Ordering::Release);
                    Ok(None)
                }
                Err(error) => {
                    backend.worker_running.store(false, Ordering::Release);
                    Err(error.code().to_owned())
                }
            },
            Err(_) => {
                backend.worker_running.store(false, Ordering::Release);
                Err("SERVICE_UNAVAILABLE".into())
            }
        };
        match next {
            Ok(Some(job)) => {
                let id = job.id;
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(&app, job)))
                    .is_err()
                {
                    release_failed_job(&app, id, "IMPORT_WORKER_PANIC");
                }
            }
            Ok(None) => break,
            Err(code) => {
                report_error(&app, "queue.next", &code);
                break;
            }
        }
    });
}

fn run_job(app: &tauri::AppHandle, job: ImportJob) {
    let backend = app.state::<Backend>();
    let correlation = job.id.to_string();
    update_import(
        app,
        ImportStatus {
            job_id: job.id,
            filename: job.receipt.filename.clone(),
            state: "downloading".into(),
            error_code: None,
            item_id: None,
        },
    );
    record(
        app,
        LogLevel::Info,
        "download.start",
        &format!("{} をダウンロードしています", job.receipt.filename),
        Some(&correlation),
    );
    let result = (|| {
        let mut last_progress = std::time::Instant::now();
        let downloaded = download_booth_archive_with_progress(
            &job.request,
            &backend.service.booth_download_staging_dir(),
            |bytes, total| {
                if last_progress.elapsed() >= std::time::Duration::from_secs(5) {
                    let message = match total {
                        Some(total) => format!("{bytes} / {total} bytes を受信しました"),
                        None => format!("{bytes} bytes を受信しました"),
                    };
                    record(
                        app,
                        LogLevel::Info,
                        "download.progress",
                        &message,
                        Some(&correlation),
                    );
                    last_progress = std::time::Instant::now();
                }
            },
        )?;
        record(
            app,
            LogLevel::Info,
            "download.complete",
            &format!("{} bytes の原本を受信しました", downloaded.size_bytes()),
            Some(&correlation),
        );
        record(
            app,
            LogLevel::Info,
            "metadata.fetch",
            "BOOTH の公開商品情報を取得します",
            Some(&correlation),
        );
        let metadata = match fetch_booth_item_metadata(&job.request) {
            Ok(metadata) => metadata,
            Err(error) => {
                record(
                    app,
                    LogLevel::Warn,
                    "metadata.fetch",
                    error.code(),
                    Some(&correlation),
                );
                None
            }
        };
        if metadata.is_none() {
            record(
                app,
                LogLevel::Warn,
                "metadata.fetch",
                "商品情報が得られないためファイル名と取り込み日を使います",
                Some(&correlation),
            );
        }
        update_import(
            app,
            ImportStatus {
                job_id: job.id,
                filename: job.receipt.filename.clone(),
                state: "importing".into(),
                error_code: None,
                item_id: None,
            },
        );
        record(
            app,
            LogLevel::Info,
            "library.import",
            "SHA-256 を検証し、原本とライブラリを登録します",
            Some(&correlation),
        );
        backend.service.import_booth_download_with_metadata(
            &job.request,
            downloaded,
            metadata.as_ref(),
            job.extract_archives,
            Some(Path::new(&job.library_path)),
        )
    })();
    let (state, error, item) = match &result {
        Ok(result) => {
            record(
                app,
                LogLevel::Info,
                "library.import",
                &format!("{} を登録しました", result.item.title),
                Some(&correlation),
            );
            for warning in &result.warnings {
                record(
                    app,
                    LogLevel::Warn,
                    "library.import",
                    warning,
                    Some(&correlation),
                );
            }
            (JobStatus::Succeeded, None, Some(result.item.id.as_str()))
        }
        Err(error) => {
            record(
                app,
                LogLevel::Error,
                "import.failed",
                &match error.safe_detail() {
                    Some(detail) => format!("{} (HTTP {detail})", error.code()),
                    None => error.code().to_owned(),
                },
                Some(&correlation),
            );
            let status = if matches!(error.code(), "DOWNLOAD_EXPIRED" | "DOWNLOAD_REJECTED") {
                JobStatus::AwaitingUrl
            } else {
                JobStatus::Failed
            };
            (status, Some(error.code()), None)
        }
    };
    let finished = match backend.queue.lock() {
        Ok(mut queue) => queue
            .finish(job.id, state, error, item)
            .map_err(|error| error.code().to_owned()),
        Err(_) => Err("SERVICE_UNAVAILABLE".into()),
    };
    match finished {
        Ok(record) => update_import(app, ImportStatus::from_record(&record)),
        Err(code) => release_failed_job(app, job.id, &code),
    }
}

fn release_failed_job(app: &tauri::AppHandle, id: u64, code: &str) {
    let backend = app.state::<Backend>();
    let failed = match backend.queue.lock() {
        Ok(mut queue) => queue.release_after_failure(id, code),
        Err(poisoned) => {
            let mut queue = poisoned.into_inner();
            let failed = queue.release_after_failure(id, code);
            backend.queue.clear_poison();
            failed
        }
    };
    record(
        app,
        LogLevel::Error,
        "queue.recover",
        code,
        Some(&id.to_string()),
    );
    if let Some(failed) = failed {
        update_import(app, ImportStatus::from_record(&failed));
    } else {
        report_error(app, "queue.recover", code);
    }
}

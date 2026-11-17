# Omnooth Unity Editor adapter

This local UPM package watches `Library/Omnooth/request.json` in an already-open Unity project. It validates each explicit, ordered `.unitypackage` path and calls `AssetDatabase.ImportPackage(path, false)` one package at a time. It writes `Library/Omnooth/result.json` only after Unity reports completion, failure, or cancellation.

The adapter does not create a project, copy loose assets, move imported assets, or rewrite package paths. The desktop confirmation dialog can install the bundled extension into `Packages/com.omnooth.editor/` as an embedded UPM package. An existing different package is preserved. Alternatively, add this folder through Package Manager’s **Add package from disk** action. Open the project in Unity and finish compilation before applying.

## Transport

The Rust crate writes a request atomically and will not replace it while a result is pending. The protocol is JSON version 1:

```json
{
  "protocol_version": 1,
  "job_id": "job-123",
  "packages": [
    { "operation_id": "op-1", "path": "/absolute/path/first.unitypackage" },
    { "operation_id": "op-2", "path": "/absolute/path/second.unitypackage" }
  ]
}
```

The result has `protocol_version`, matching `job_id`, `status` (`completed`, `failed`, or `cancelled`), ordered `completed_operation_ids`, and an optional `error`. An empty package list requests an AssetDatabase.Refresh (used after owned-file removal). `status.json` publishes a protocol version, Unix-second `updated_at`, and `is_busy` heartbeat every second. The desktop requires a fresh idle Editor before changing assets. A domain reload restores persisted in-flight state; unreadable or mismatched state is failed as outcome-unknown, and a missing terminal callback is reported after ten minutes without repeating the import.

Unity's callback API identifies the imported package by its package name, not the request operation id. To avoid guessing which operation completed, basenames in one request must be unique. A `completed` result confirms Unity's package callback and request order only. It does not inspect imported files, GUID conflicts, or hashes and does not commit an omnooth installation record. Unity may alter assets before a failure or cancellation; the adapter reports the partial completion prefix and does not claim rollback.

## Packages inside ZIP files

The Rust adapter exposes `start_zip_import(service, project_path, version_id, selected_ids)` and `poll_zip_import(service, project_path, job_id)`. The first verifies the stored ZIP's SHA-256, extracts only the explicitly selected unitypackages into app-owned temporary storage, and publishes the ordered request. The second validates the exact published request and terminal result and removes that temporary job directory only on full success. Callers must continue polling pending jobs; failure, cancellation, and unknown outcomes retain the files. Publication errors include the retained job ID. A persisted job can be checked after restarting the application while its request/result remain current.

The original ZIP and any permanent library extraction are retained. Previously completed requests remain verifiable after their temporary input files are removed, so the next import is not blocked. The desktop now uses the core installation service, which validates a confirmed plan, backs up changed files, imports through this transport, verifies actual hashes and GUIDs, and commits the ownership ledger. The older ZIP-only API remains independent and does not commit installation state. See [ZIP implementation and validation](../../docs/zip-import.md) and [desktop installation behavior](../../docs/unity-installation.md).

On 2026-11-17 the packaged adapter was exercised in macOS Unity 2022.3.22f1 using an isolated project and a self-authored text asset: request/operation identity, completion, imported bytes, GUID and exact asset.meta bytes matched. Run `python3 unity/tests/validate-editor.py --unity <Unity executable>` from the repository root to reproduce. Real VRChat assets, domain reload recovery, and Windows/Linux Editors remain unverified.

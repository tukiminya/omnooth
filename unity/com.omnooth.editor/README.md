# Omnooth Unity Editor adapter

This local UPM package watches `Library/Omnooth/request.json` in an already-open Unity project. It validates each explicit, ordered `.unitypackage` path and calls `AssetDatabase.ImportPackage(path, false)` one package at a time. It writes `Library/Omnooth/result.json` only after Unity reports completion, failure, or cancellation.

The adapter does not create a project, copy loose assets, move imported assets, or rewrite package paths. The Editor extension is not installed automatically. To try it, add this repository folder as a local package in the intended Unity project using Package Manager's **Add package from disk** action and select this folder's `package.json`. Remove the package when finished to unlink it.

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

The result has `protocol_version`, matching `job_id`, `status` (`completed`, `failed`, or `cancelled`), ordered `completed_operation_ids`, and an optional `error`. A domain reload restores persisted in-flight state; unreadable or mismatched state is failed as outcome-unknown, and a missing terminal callback is reported after ten minutes without repeating the import.

Unity's callback API identifies the imported package by its package name, not the request operation id. To avoid guessing which operation completed, basenames in one request must be unique. A `completed` result confirms Unity's package callback and request order only. It does not inspect imported files, GUID conflicts, or hashes and does not commit an omnooth installation record. Unity may alter assets before a failure or cancellation; the adapter reports the partial completion prefix and does not claim rollback.

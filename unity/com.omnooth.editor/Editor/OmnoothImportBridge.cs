using System;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using UnityEditor;
using UnityEngine;

namespace Omnooth.Editor
{
    [Serializable]
    internal sealed class ImportRequest
    {
        public int protocol_version;
        public string job_id;
        public ImportEntry[] packages;
    }

    [Serializable]
    internal sealed class ImportEntry
    {
        public string operation_id;
        public string path;
    }

    [Serializable]
    internal sealed class ImportResult
    {
        public int protocol_version = 1;
        public string job_id;
        public string status;
        public string[] completed_operation_ids;
        public string error;
    }

    [Serializable]
    internal sealed class ImportState
    {
        public int protocol_version = 1;
        public string job_id;
        public int next_index;
        public string active_path;
        public string active_operation_id;
        public string request_fingerprint;
        public long active_since_utc_ticks;
        public string[] completed_operation_ids;
        public string terminal_status;
        public string error;
    }

    /// <summary>
    /// Polls a local request manifest and serializes imports through Unity's
    /// official non-interactive ImportPackage API. State is persisted before
    /// entering Unity so a domain reload never repeats an uncertain import.
    /// </summary>
    [InitializeOnLoad]
    internal static class OmnoothImportBridge
    {
        private const int ProtocolVersion = 1;
        private const double PollIntervalSeconds = 1.0;
        private const double ActiveImportTimeoutSeconds = 10 * 60;
        private static readonly string Root = Path.Combine(Directory.GetParent(Application.dataPath).FullName, "Library", "Omnooth");
        private static readonly string RequestPath = Path.Combine(Root, "request.json");
        private static readonly string ResultPath = Path.Combine(Root, "result.json");
        private static readonly string StatePath = Path.Combine(Root, "state.json");
        private static double _nextPoll;
        private static ImportRequest _request;
        private static ImportState _state;
        private static string _requestFingerprint;
        private static bool _stateCorrupt;

        static OmnoothImportBridge()
        {
            AssetDatabase.importPackageCompleted += OnImportCompleted;
            AssetDatabase.importPackageFailed += OnImportFailed;
            AssetDatabase.importPackageCancelled += OnImportCancelled;
            EditorApplication.update += Update;
            LoadState();
            // Give callbacks from an import surviving a script reload a chance to arrive.
            _nextPoll = EditorApplication.timeSinceStartup + 2.0;
        }

        private static void Update()
        {
            if (EditorApplication.timeSinceStartup < _nextPoll) return;
            _nextPoll = EditorApplication.timeSinceStartup + PollIntervalSeconds;
            try
            {
                if (_stateCorrupt)
                {
                    if (!TryReadRequest(out var recoveredRequest, out var fingerprint)) return;
                    _request = recoveredRequest;
                    if (HasMatchingTerminalResult(recoveredRequest.job_id))
                    {
                        _stateCorrupt = false;
                        return;
                    }
                    _state = new ImportState
                    {
                        job_id = recoveredRequest.job_id,
                        request_fingerprint = fingerprint,
                        completed_operation_ids = new string[0]
                    };
                    _stateCorrupt = false;
                    Finish("failed", "IMPORT_OUTCOME_UNKNOWN: persisted adapter state was unreadable; the request was not retried.");
                    return;
                }
                if (_state != null && !string.IsNullOrEmpty(_state.terminal_status))
                {
                    if (!HasMatchingTerminalResult(_state.job_id)) WriteResultFromState();
                    _state = null;
                    _request = null;
                }
                if (_state != null && !string.IsNullOrEmpty(_state.active_path))
                {
                    CheckActiveRequestBinding();
                    if (_state == null || !string.IsNullOrEmpty(_state.terminal_status)) return;
                    CheckActiveTimeout();
                    return;
                }
                _request = null;
                if (!TryLoadRequest()) return;
                if (_state == null || _state.job_id != _request.job_id)
                {
                    if (HasMatchingTerminalResult(_request.job_id)) return;
                    _state = new ImportState
                    {
                        job_id = _request.job_id,
                        request_fingerprint = _requestFingerprint,
                        next_index = 0,
                        completed_operation_ids = new string[0]
                    };
                    SaveState();
                }
                if (!StateMatchesRequest(_state, _request))
                {
                    Finish("failed", "IMPORT_OUTCOME_UNKNOWN: persisted state does not match the requested operation order; no package was retried.");
                    return;
                }
                StartNextImport();
            }
            catch (Exception exception)
            {
                Debug.LogError("Omnooth Unity adapter: " + exception);
                if (_state != null && string.IsNullOrEmpty(_state.terminal_status))
                    Finish("failed", "ADAPTER_ERROR: " + exception.Message);
            }
        }

        private static bool TryLoadRequest()
        {
            if (!TryReadRequest(out var request, out var fingerprint)) return false;
            _requestFingerprint = fingerprint;
            if (_state != null && _state.job_id == request.job_id && string.IsNullOrEmpty(_state.terminal_status))
            {
                _request = request;
                if (_state.request_fingerprint != fingerprint)
                {
                    Finish("failed", "IMPORT_OUTCOME_UNKNOWN: the request changed while its job id was active; the request was not retried.");
                    return false;
                }
                return true;
            }
            if (HasMatchingTerminalResult(request.job_id)) return false;
            _request = request;
            return true;
        }

        private static bool TryReadRequest(out ImportRequest request, out string fingerprint)
        {
            request = null;
            fingerprint = null;
            if (!File.Exists(RequestPath)) return false;
            var bytes = File.ReadAllBytes(RequestPath);
            request = JsonUtility.FromJson<ImportRequest>(System.Text.Encoding.UTF8.GetString(bytes));
            ValidateRequest(request);
            using (var sha256 = SHA256.Create())
                fingerprint = BitConverter.ToString(sha256.ComputeHash(bytes)).Replace("-", string.Empty);
            return true;
        }

        private static bool StateMatchesRequest(ImportState state, ImportRequest request)
        {
            if (state == null || request == null || state.job_id != request.job_id ||
                state.request_fingerprint != _requestFingerprint || state.next_index < 0 ||
                state.next_index > request.packages.Length) return false;
            var completed = state.completed_operation_ids ?? new string[0];
            if (completed.Length != state.next_index) return false;
            for (var index = 0; index < completed.Length; index++)
                if (completed[index] != request.packages[index].operation_id) return false;
            if (string.IsNullOrEmpty(state.active_path)) return state.next_index <= request.packages.Length;
            if (state.next_index >= request.packages.Length) return false;
            var active = request.packages[state.next_index];
            return state.active_path == active.path && state.active_operation_id == active.operation_id;
        }

        private static void CheckActiveRequestBinding()
        {
            try
            {
                if (!TryReadRequest(out var request, out var fingerprint) || request.job_id != _state.job_id ||
                    fingerprint != _state.request_fingerprint)
                {
                    Finish("failed", "IMPORT_OUTCOME_UNKNOWN: the active request or persisted operation state changed; no package was retried.");
                    return;
                }
                _requestFingerprint = fingerprint;
                if (!StateMatchesRequest(_state, request))
                    Finish("failed", "IMPORT_OUTCOME_UNKNOWN: the active request or persisted operation state changed; no package was retried.");
            }
            catch (Exception exception)
            {
                Finish("failed", "IMPORT_OUTCOME_UNKNOWN: the active request could not be verified: " + exception.Message);
            }
        }

        private static void ValidateRequest(ImportRequest request)
        {
            if (request == null || request.protocol_version != ProtocolVersion || !ValidId(request.job_id) ||
                request.packages == null || request.packages.Length == 0)
                throw new InvalidDataException("Request manifest has an invalid version, job id, or empty package list.");
            var operations = new System.Collections.Generic.HashSet<string>(StringComparer.Ordinal);
            var fileNames = new System.Collections.Generic.HashSet<string>(StringComparer.OrdinalIgnoreCase);
            foreach (var entry in request.packages)
            {
                if (entry == null || !ValidId(entry.operation_id) || !operations.Add(entry.operation_id))
                    throw new InvalidDataException("Package operation ids must be valid and unique.");
                if (string.IsNullOrWhiteSpace(entry.path) || !Path.IsPathRooted(entry.path) ||
                    !string.Equals(Path.GetExtension(entry.path), ".unitypackage", StringComparison.OrdinalIgnoreCase) ||
                    !File.Exists(entry.path))
                    throw new InvalidDataException("Every package path must identify an existing absolute .unitypackage file.");
                var canonical = Path.GetFullPath(entry.path);
                if (!string.Equals(canonical, entry.path, StringComparison.Ordinal))
                    throw new InvalidDataException("Package paths must be canonical absolute paths.");
                // Unity callbacks identify the package by its display name, so reject ambiguous basenames.
                if (!fileNames.Add(Path.GetFileNameWithoutExtension(entry.path)))
                    throw new InvalidDataException("Package filenames must be unique within one request.");
            }
        }

        private static void StartNextImport()
        {
            if (_request == null || _state == null || _state.job_id != _request.job_id) return;
            if (_state.next_index >= _request.packages.Length)
            {
                Finish("completed", null);
                return;
            }
            var entry = _request.packages[_state.next_index];
            _state.active_path = entry.path;
            _state.active_operation_id = entry.operation_id;
            _state.active_since_utc_ticks = DateTime.UtcNow.Ticks;
            SaveState(); // Durable before crossing into Unity; do not repeat on reload.
            AssetDatabase.ImportPackage(entry.path, false);
        }

        private static void OnImportCompleted(string packageName)
        {
            CompleteActiveEvent(packageName, null, "completed");
        }

        private static void OnImportFailed(string packageName, string errorMessage)
        {
            CompleteActiveEvent(packageName, errorMessage, "failed");
        }

        private static void OnImportCancelled(string packageName)
        {
            CompleteActiveEvent(packageName, "Import was cancelled in Unity.", "cancelled");
        }

        private static void CompleteActiveEvent(string packageName, string error, string status)
        {
            if (_state == null || string.IsNullOrEmpty(_state.active_path)) return;
            var expected = Path.GetFileNameWithoutExtension(_state.active_path);
            var callbackName = Path.GetFileNameWithoutExtension(packageName ?? string.Empty);
            if (!string.Equals(expected, callbackName, StringComparison.OrdinalIgnoreCase) &&
                !string.Equals(Path.GetFullPath(_state.active_path), packageName, StringComparison.OrdinalIgnoreCase))
            {
                Debug.LogWarning("Omnooth ignored an ImportPackage callback for a different package: " + packageName);
                return;
            }
            if (status == "completed")
            {
                var done = (_state.completed_operation_ids ?? new string[0]).ToList();
                done.Add(_state.active_operation_id);
                _state.completed_operation_ids = done.ToArray();
                _state.next_index++;
                _state.active_path = null;
                _state.active_operation_id = null;
                _state.active_since_utc_ticks = 0;
                SaveState();
                _nextPoll = 0;
            }
            else
            {
                Finish(status, string.IsNullOrEmpty(error) ? "Unity import " + status + "." : error);
            }
        }

        private static void CheckActiveTimeout()
        {
            if (_state.active_since_utc_ticks <= 0) return;
            var started = new DateTime(_state.active_since_utc_ticks, DateTimeKind.Utc);
            if (DateTime.UtcNow - started > TimeSpan.FromSeconds(ActiveImportTimeoutSeconds))
                Finish("failed", "IMPORT_OUTCOME_UNKNOWN: Unity did not report a terminal callback; the import was not retried.");
        }

        private static void Finish(string status, string error)
        {
            if (_state == null || !string.IsNullOrEmpty(_state.terminal_status)) return;
            _state.terminal_status = status;
            _state.error = error;
            _state.active_path = null;
            _state.active_operation_id = null;
            _state.active_since_utc_ticks = 0;
            SaveState();
            WriteResultFromState();
        }

        private static void WriteResultFromState()
        {
            var result = new ImportResult
            {
                job_id = _state.job_id,
                status = _state.terminal_status,
                completed_operation_ids = _state.completed_operation_ids ?? new string[0],
                error = _state.error
            };
            AtomicWrite(ResultPath, JsonUtility.ToJson(result, true));
        }

        private static bool HasMatchingTerminalResult(string jobId)
        {
            if (!File.Exists(ResultPath)) return false;
            try
            {
                var result = JsonUtility.FromJson<ImportResult>(File.ReadAllText(ResultPath));
                return result != null && result.protocol_version == ProtocolVersion && result.job_id == jobId &&
                    (result.status == "completed" || result.status == "failed" || result.status == "cancelled");
            }
            catch { return false; }
        }

        private static void LoadState()
        {
            if (!File.Exists(StatePath)) return;
            try
            {
                _state = JsonUtility.FromJson<ImportState>(File.ReadAllText(StatePath));
                if (_state == null || _state.protocol_version != ProtocolVersion || !ValidId(_state.job_id) ||
                    string.IsNullOrEmpty(_state.request_fingerprint) || _state.request_fingerprint.Length != 64 ||
                    _state.next_index < 0 || _state.completed_operation_ids == null)
                    throw new InvalidDataException("Persisted adapter state is incomplete or has an unsupported version.");
            }
            catch (Exception exception)
            {
                _state = null;
                _stateCorrupt = true;
                Debug.LogError("Omnooth persisted state is unreadable; an uncertain request will be failed without retry: " + exception.Message);
            }
        }

        private static bool ValidId(string value)
        {
            return !string.IsNullOrEmpty(value) && value.Length <= 128 && value != "." && value != ".." &&
                value.All(character => (character >= 'a' && character <= 'z') || (character >= 'A' && character <= 'Z') ||
                    (character >= '0' && character <= '9') || character == '-' || character == '_' || character == '.');
        }

        private static void SaveState()
        {
            Directory.CreateDirectory(Root);
            AtomicWrite(StatePath, JsonUtility.ToJson(_state, true));
        }

        private static void AtomicWrite(string path, string contents)
        {
            Directory.CreateDirectory(Path.GetDirectoryName(path));
            var temp = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
            File.WriteAllText(temp, contents);
            if (File.Exists(path)) File.Replace(temp, path, null);
            else File.Move(temp, path);
        }
    }
}

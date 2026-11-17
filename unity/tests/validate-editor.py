#!/usr/bin/env python3
"""Exercise the packaged adapter in an isolated Unity 2022.3+ project.

No existing project or user library is used. Keep the temporary project/log for
inspection. Unity must already have a working license on the current machine.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import tarfile

parser = argparse.ArgumentParser()
parser.add_argument('--unity', required=True, help='Path to the Unity executable')
parser.add_argument('--timeout', type=int, default=45)
parser.add_argument('--extended', action='store_true', help='Also import Material, Prefab and a script that triggers domain reload')
args = parser.parse_args()
root = Path(tempfile.mkdtemp(prefix='omnooth-unity-validation-'))
(root / 'Assets/Editor').mkdir(parents=True)
(root / 'Packages').mkdir()
(root / 'ProjectSettings').mkdir()
(root / 'Packages/manifest.json').write_text('{"dependencies":{}}\n')
(root / 'ProjectSettings/ProjectVersion.txt').write_text('m_EditorVersion: 2022.3.22f1\n')
shutil.copytree(Path(__file__).resolve().parents[1] / 'com.omnooth.editor', root / 'Packages/com.omnooth.editor')
(root / 'Assets/Editor/OmnoothValidation.cs').write_text(r'''
using System;
using System.IO;
using UnityEditor;
using UnityEngine;
[Serializable] public class OmnoothValidationResult { public string job_id; public string status; public string[] completed_operation_ids; }
[InitializeOnLoad] public static class OmnoothValidation {
    static string Root { get { return Directory.GetParent(Application.dataPath).FullName; } }
    static OmnoothValidation() { EditorApplication.update += Check; }
    const bool Extended = __EXTENDED__;
    public static void Run() {
        File.WriteAllText(Path.Combine(Application.dataPath,"OmnoothValidation.txt"),"omnooth fixture");
        AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
        var guid = AssetDatabase.AssetPathToGUID("Assets/OmnoothValidation.txt");
        File.WriteAllText(Path.Combine(Root,"expected-guid"),guid);
        var package = Path.Combine(Root,"Validation.unitypackage");
        var paths = new System.Collections.Generic.List<string> { "Assets/OmnoothValidation.txt" };
        if (Extended) {
            var material = new Material(Shader.Find("Standard"));
            AssetDatabase.CreateAsset(material, "Assets/OmnoothValidation.mat");
            var cube = GameObject.CreatePrimitive(PrimitiveType.Cube);
            cube.GetComponent<Renderer>().sharedMaterial = material;
            PrefabUtility.SaveAsPrefabAsset(cube, "Assets/OmnoothValidation.prefab");
            UnityEngine.Object.DestroyImmediate(cube);
            File.WriteAllText(Path.Combine(Application.dataPath,"OmnoothReloadFixture.cs"), "public sealed class OmnoothReloadFixture {}\n");
            AssetDatabase.Refresh(ImportAssetOptions.ForceSynchronousImport);
            paths.Add("Assets/OmnoothValidation.mat"); paths.Add("Assets/OmnoothValidation.prefab"); paths.Add("Assets/OmnoothReloadFixture.cs");
        }
        AssetDatabase.ExportPackage(paths.ToArray(),package,ExportPackageOptions.Default);
        if (Extended) {
            AssetDatabase.DeleteAsset("Assets/OmnoothValidation.prefab");
            AssetDatabase.DeleteAsset("Assets/OmnoothValidation.mat");
            AssetDatabase.DeleteAsset("Assets/OmnoothReloadFixture.cs");
        }
        AssetDatabase.DeleteAsset("Assets/OmnoothValidation.txt");
        Directory.CreateDirectory(Path.Combine(Root,"Library/Omnooth"));
        File.WriteAllText(Path.Combine(Root,"Library/Omnooth/request.json"), __REQUEST_JSON__);
    }
    static void Check() {
        if (EditorApplication.isCompiling || EditorApplication.isUpdating) return;
        var path = Path.Combine(Root,"Library/Omnooth/result.json");
        if (!File.Exists(path)) return;
        var result = JsonUtility.FromJson<OmnoothValidationResult>(File.ReadAllText(path));
        var success = result.job_id == "validation-job" && result.status == "completed" &&
            result.completed_operation_ids.Length == 1 && result.completed_operation_ids[0] == "validation-op" &&
            File.ReadAllText(Path.Combine(Application.dataPath,"OmnoothValidation.txt")) == "omnooth fixture" &&
            AssetDatabase.AssetPathToGUID("Assets/OmnoothValidation.txt") == File.ReadAllText(Path.Combine(Root,"expected-guid"));
        if (success && Extended) {
            var prefab = AssetDatabase.LoadAssetAtPath<GameObject>("Assets/OmnoothValidation.prefab");
            var material = AssetDatabase.LoadAssetAtPath<Material>("Assets/OmnoothValidation.mat");
            var scriptLoaded = false;
            foreach (var assembly in AppDomain.CurrentDomain.GetAssemblies()) if (assembly.GetType("OmnoothReloadFixture") != null) scriptLoaded = true;
            success = prefab != null && material != null && prefab.GetComponent<Renderer>().sharedMaterial == material && scriptLoaded;
        }
        File.WriteAllText(Path.Combine(Root,"validation-result"), success ? "PASS" : "FAIL");
        EditorApplication.Exit(success ? 0 : 1);
    }
}
''')
# Insert JSON as a C# string literal; both escaping rules agree for this ASCII path.
script = root / 'Assets/Editor/OmnoothValidation.cs'
request = json.dumps({'protocol_version': 1, 'job_id': 'validation-job', 'packages': [{'operation_id': 'validation-op', 'path': str(root / 'Validation.unitypackage')}]})
script.write_text(script.read_text().replace('__REQUEST_JSON__', json.dumps(request)).replace('__EXTENDED__', 'true' if args.extended else 'false'))
print(f'Project and log: {root}', flush=True)
command = [args.unity, '-batchmode', '-nographics', '-projectPath', str(root), '-executeMethod', 'OmnoothValidation.Run', '-logFile', str(root / 'unity.log')]
with (root / 'launcher.log').open('w') as output:
    process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT)
    try:
        code = process.wait(timeout=args.timeout)
    except subprocess.TimeoutExpired:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        print('TIMEOUT: inspect unity.log; no real import success has been confirmed.')
        raise SystemExit(2)
result = root / 'validation-result'
if code or not result.exists() or result.read_text() != 'PASS':
    print(f'FAILED: Unity exited {code}; inspect unity.log.')
    raise SystemExit(1)
with tarfile.open(root / 'Validation.unitypackage', 'r:gz') as archive:
    for member in archive.getmembers():
        if not member.name.endswith('/pathname'):
            continue
        relative = archive.extractfile(member).read().decode().rstrip('\r\n')
        prefix = member.name.rsplit('/', 1)[0]
        for name, suffix in [('asset', ''), ('asset.meta', '.meta')]:
            expected = archive.extractfile(prefix + '/' + name).read()
            if (root / (relative + suffix)).read_bytes() != expected:
                raise SystemExit('FAIL: imported bytes differ from the exported manifest.')
print('PASS: request, terminal result, exact asset/meta bytes and GUID verified in Unity.' + (' Material/Prefab references and compiled script verified after domain reload.' if args.extended else ''))

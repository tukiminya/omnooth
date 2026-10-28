import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import {
  mkdtempSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const stageScript = path.join(repoRoot, 'desktop/scripts/stage-release-assets.mjs');
const fullSha = 'a'.repeat(40);
const installers = [
  ['macos-arm64-dmg', 'Omnooth.dmg'],
  ['macos-x64-dmg', 'Omnooth.dmg'],
  ['windows-x64-nsis', 'Omnooth-setup.exe'],
  ['linux-x64-deb', 'omnooth.deb'],
  ['linux-x64-appimage', 'omnooth.AppImage'],
];

function makeFixture({ includeApps = false, omit = null, overrideFile = null, extra = null } = {}) {
  const root = mkdtempSync(path.join(os.tmpdir(), 'omnooth-release-assets-'));
  const artifactRoot = path.join(root, 'artifacts');
  const outputDir = path.join(root, 'output');
  const githubOutput = path.join(root, 'github-output');
  mkdirSync(artifactRoot);
  mkdirSync(outputDir);
  const configDirectory = path.join(root, 'desktop/src-tauri');
  mkdirSync(configDirectory, { recursive: true });
  writeFileSync(path.join(configDirectory, 'tauri.conf.json'), JSON.stringify({ version: '1.2.3' }));

  for (const [key, filename] of installers) {
    if (key === omit) continue;
    const name = key === overrideFile?.key ? overrideFile.filename : filename;
    const directory = path.join(artifactRoot, `omnooth-${fullSha}-${key}`);
    mkdirSync(directory);
    writeFileSync(path.join(directory, name), 'fixture bundle');
  }

  if (includeApps) {
    for (const architecture of ['arm64', 'x64']) {
      const app = path.join(artifactRoot, `omnooth-${fullSha}-macos-${architecture}-app`);
      mkdirSync(path.join(app, 'Omnooth.app', 'Contents', 'MacOS'), { recursive: true });
      writeFileSync(path.join(app, 'Omnooth.app', 'Contents', 'MacOS', 'omnooth'), 'fixture app');
    }
  }

  if (extra) mkdirSync(path.join(artifactRoot, `omnooth-${fullSha}-${extra}`));
  return { root, artifactRoot, outputDir, githubOutput };
}

function stage(fixture) {
  return spawnSync(process.execPath, [stageScript, fixture.artifactRoot, fixture.outputDir], {
    cwd: fixture.root,
    encoding: 'utf8',
    env: { ...process.env, GITHUB_SHA: fullSha, GITHUB_OUTPUT: fixture.githubOutput },
  });
}

test('accepts the five installers and ignores the two known macOS app artifacts', (t) => {
  const fixture = makeFixture({ includeApps: true });
  t.after(() => rmSync(fixture.root, { recursive: true, force: true }));

  const result = stage(fixture);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(
    readdirSync(fixture.outputDir).sort(),
    [
      'omnooth-linux-x64-1.2.3.AppImage',
      'omnooth-linux-x64-1.2.3.deb',
      'omnooth-macos-arm64-1.2.3.dmg',
      'omnooth-macos-x64-1.2.3.dmg',
      'omnooth-windows-x64-1.2.3.exe',
    ].sort(),
  );
  assert.match(readFileSync(fixture.githubOutput, 'utf8'), /release_tag=main-a{40}/);
});

test('rejects a missing installer', (t) => {
  const fixture = makeFixture({ omit: 'linux-x64-appimage' });
  t.after(() => rmSync(fixture.root, { recursive: true, force: true }));

  const result = stage(fixture);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Required build artifact is missing: linux-x64-appimage/);
});

test('rejects an installer artifact with the wrong extension', (t) => {
  const fixture = makeFixture({
    overrideFile: { key: 'windows-x64-nsis', filename: 'Omnooth-setup.msi' },
  });
  t.after(() => rmSync(fixture.root, { recursive: true, force: true }));

  const result = stage(fixture);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Expected one \.exe bundle in omnooth-.*windows-x64-nsis/);
});

test('rejects an unknown artifact even when all five installers are present', (t) => {
  const fixture = makeFixture({ extra: 'macos-arm64-pkg' });
  t.after(() => rmSync(fixture.root, { recursive: true, force: true }));

  const result = stage(fixture);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Unexpected build artifact name: .*macos-arm64-pkg/);
});

import {
  copyFileSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';

const [artifactRootArg, outputDirArg] = process.argv.slice(2);
if (!artifactRootArg || !outputDirArg || !process.env.GITHUB_SHA || !process.env.GITHUB_OUTPUT) {
  throw new Error('Usage: stage-release-assets.mjs <downloaded-artifact-root> <output-directory>');
}

const artifactRoot = path.resolve(artifactRootArg);
const outputDir = path.resolve(outputDirArg);
const fullSha = process.env.GITHUB_SHA;
if (!/^[a-f0-9]{40}$/i.test(fullSha)) throw new Error('GITHUB_SHA must be a full Git commit SHA.');

const configPath = path.resolve('desktop/src-tauri/tauri.conf.json');
const config = JSON.parse(readFileSync(configPath, 'utf8'));
if (typeof config.version !== 'string' || !/^\d+\.\d+\.\d+(?:[-+][\w.-]+)?$/.test(config.version)) {
  throw new Error('The Tauri config must contain a valid app version.');
}

const expected = new Map([
  ['macos-arm64-dmg', { asset: 'macos-arm64', extension: '.dmg' }],
  ['macos-x64-dmg', { asset: 'macos-x64', extension: '.dmg' }],
  ['windows-x64-nsis', { asset: 'windows-x64', extension: '.exe' }],
  ['linux-x64-deb', { asset: 'linux-x64', extension: '.deb' }],
  ['linux-x64-appimage', { asset: 'linux-x64', extension: '.AppImage' }],
]);
const ignoredMacAppArtifacts = new Set(['macos-arm64-app', 'macos-x64-app']);

function filesUnder(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const fullPath = path.join(directory, entry.name);
    if (entry.isSymbolicLink()) throw new Error(`Unexpected symbolic link in build artifacts: ${entry.name}`);
    if (entry.isDirectory()) return filesUnder(fullPath);
    if (entry.isFile()) return [fullPath];
    throw new Error(`Unexpected artifact entry: ${entry.name}`);
  });
}

const prefix = `omnooth-${fullSha}-`;
const observed = new Map();
for (const entry of readdirSync(artifactRoot, { withFileTypes: true })) {
  if (!entry.isDirectory() || !entry.name.startsWith(prefix)) {
    throw new Error(`Unexpected downloaded artifact: ${entry.name}`);
  }
  const key = entry.name.slice(prefix.length);
  const descriptor = expected.get(key);
  if (!descriptor && ignoredMacAppArtifacts.has(key)) continue;
  if (!descriptor) throw new Error(`Unexpected build artifact name: ${entry.name}`);
  if (observed.has(key)) throw new Error(`Duplicate build artifact: ${entry.name}`);

  const files = filesUnder(path.join(artifactRoot, entry.name));
  if (files.length !== 1 || path.extname(files[0]).toLowerCase() !== descriptor.extension.toLowerCase()) {
    throw new Error(`Expected one ${descriptor.extension} bundle in ${entry.name}.`);
  }
  observed.set(key, { source: files[0], ...descriptor });
}

for (const key of expected.keys()) {
  if (!observed.has(key)) throw new Error(`Required build artifact is missing: ${key}`);
}

mkdirSync(outputDir, { recursive: true });
if (readdirSync(outputDir).length !== 0) throw new Error('Release staging directory must be empty.');

for (const { source, asset, extension } of observed.values()) {
  copyFileSync(source, path.join(outputDir, `omnooth-${asset}-${config.version}${extension}`));
}

const releaseName = `omnooth ${config.version} (main ${fullSha.slice(0, 12)})`;
const releaseTag = `main-${fullSha}`;
writeFileSync(
  process.env.GITHUB_OUTPUT,
  `app_version=${config.version}\nrelease_name=${releaseName}\nrelease_tag=${releaseTag}\n`,
  { flag: 'a' },
);

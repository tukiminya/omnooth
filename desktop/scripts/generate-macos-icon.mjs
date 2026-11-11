import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const desktop = fileURLToPath(new URL('../', import.meta.url));
const source = readFileSync(path.join(desktop, 'public/brand/app-icon.svg'), 'utf8');
const svg = source.match(/^\s*<svg\b[^>]*\bviewBox="([^"]+)"[^>]*>([\s\S]*)<\/svg>\s*$/u);
if (!svg) throw new Error('The app icon must be an SVG with a viewBox.');

// Legacy ICNS icons need their own transparent corners and Dock-sized inset.
// Keep the bundled brand artwork intact and fit its white tile into an 824px
// rounded rectangle on a transparent 1024px canvas.
const macosSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <defs>
    <clipPath id="macos-tile">
      <rect x="100" y="100" width="824" height="824" rx="185"/>
    </clipPath>
  </defs>
  <g clip-path="url(#macos-tile)">
    <svg x="100" y="100" width="824" height="824" viewBox="${svg[1]}">${svg[2]}</svg>
  </g>
</svg>`;

const temporary = mkdtempSync(path.join(tmpdir(), 'omnooth-macos-icon-'));
try {
  const input = path.join(temporary, 'app-icon-macos.svg');
  const output = path.join(temporary, 'icons');
  writeFileSync(input, macosSvg);
  const result = spawnSync(process.execPath, [
    path.join(desktop, 'node_modules/@tauri-apps/cli/tauri.js'),
    'icon', input, '--output', output,
  ], { cwd: desktop, encoding: 'utf8' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr || result.stdout || 'Icon generation failed.');

  // Tauri emits ICNS layers in arbitrary order. Sort them for reproducible files.
  const generated = readFileSync(path.join(output, 'icon.icns'));
  const layers = [];
  for (let offset = 8; offset < generated.length;) {
    const length = generated.readUInt32BE(offset + 4);
    if (length < 8 || offset + length > generated.length) throw new Error('Invalid ICNS layer.');
    layers.push(generated.subarray(offset, offset + length));
    offset += length;
  }
  layers.sort((a, b) => Buffer.compare(a.subarray(0, 4), b.subarray(0, 4)));

  // Only ICNS is macOS-specific; preserve the Windows/Linux icons and favicon.
  writeFileSync(path.join(desktop, 'src-tauri/icons/icon.icns'), Buffer.concat([
    generated.subarray(0, 8), ...layers,
  ]));
  console.log('Generated desktop/src-tauri/icons/icon.icns');
} finally {
  rmSync(temporary, { recursive: true, force: true });
}

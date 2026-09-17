import { execSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const desktopDir = path.resolve(__dirname, '..');
const rootDir = path.resolve(desktopDir, '../..');
const binariesDir = path.join(desktopDir, 'src-tauri', 'binaries');

const isRelease = process.argv.includes('--release') || process.env.NODE_ENV === 'production';
const profile = isRelease ? 'release' : 'debug';

console.log(`[sidecar] Compiling easyjob-agent (${profile})...`);
execSync(`cargo build -p easyjob-agent ${isRelease ? '--release' : ''}`, {
  cwd: rootDir,
  stdio: 'inherit',
});

// Determine target triple from rustc
const rustcOutput = execSync('rustc -vV', { cwd: rootDir }).toString();
const match = /host: (\S+)/.exec(rustcOutput);
if (!match) {
  throw new Error('Failed to determine host target triple from rustc -vV');
}
const targetTriple = match[1];
const isWindows = process.platform === 'win32';
const ext = isWindows ? '.exe' : '';

const srcBin = path.join(rootDir, 'target', profile, `easyjob-agent${ext}`);
if (!fs.existsSync(srcBin)) {
  throw new Error(`Compiled agent binary not found at ${srcBin}`);
}

if (!fs.existsSync(binariesDir)) {
  fs.mkdirSync(binariesDir, { recursive: true });
}

const destBin = path.join(binariesDir, `easyjob-agent-${targetTriple}${ext}`);
console.log(`[sidecar] Copying ${srcBin} -> ${destBin}`);
fs.copyFileSync(srcBin, destBin);

if (!isWindows) {
  fs.chmodSync(destBin, 0o755);
}

console.log(`[sidecar] Sidecar binary ready: ${destBin}`);

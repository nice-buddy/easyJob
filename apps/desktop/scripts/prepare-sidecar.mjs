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

// Determine target triple from:
// 1. --target argument
// 2. TAURI_ENV_TARGET_TRIPLE (set by Tauri CLI)
// 3. rustc -vV host fallback
const targetArgIdx = process.argv.indexOf('--target');
const targetArg = targetArgIdx !== -1 ? process.argv[targetArgIdx + 1] : null;

const rustcOutput = execSync('rustc -vV', { cwd: rootDir }).toString();
const hostMatch = /host: (\S+)/.exec(rustcOutput);
if (!hostMatch) {
  throw new Error('Failed to determine host target triple from rustc -vV');
}
const hostTargetTriple = hostMatch[1];
const targetTriple = targetArg || process.env.TAURI_ENV_TARGET_TRIPLE || hostTargetTriple;
const isCrossCompile = targetTriple !== hostTargetTriple;

const isWindows = targetTriple.includes('windows') || process.platform === 'win32';
const ext = isWindows ? '.exe' : '';

console.log(
  `[sidecar] Target: ${targetTriple} (host: ${hostTargetTriple}, cross: ${isCrossCompile}, profile: ${profile})`
);
console.log(`[sidecar] Compiling easyjob-agent (${profile})...`);

const targetFlag = isCrossCompile ? `--target ${targetTriple}` : '';
execSync(`cargo build -p easyjob-agent ${targetFlag} ${isRelease ? '--release' : ''}`.trim(), {
  cwd: rootDir,
  stdio: 'inherit',
});

const srcBin = isCrossCompile
  ? path.join(rootDir, 'target', targetTriple, profile, `easyjob-agent${ext}`)
  : path.join(rootDir, 'target', profile, `easyjob-agent${ext}`);

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


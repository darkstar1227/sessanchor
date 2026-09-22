'use strict';
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const root = path.join(__dirname, '..');

function arg(flag) {
  const i = process.argv.indexOf(flag);
  return i === -1 ? undefined : process.argv[i + 1];
}

const rustTarget = arg('--target');
const nodePlatform = arg('--platform') || process.platform;
const nodeArch = arg('--arch') || process.arch;
if (rustTarget && (!arg('--platform') || !arg('--arch'))) {
  throw new Error('--target requires --platform and --arch (the node platform/arch it produces a binary for)');
}

const cargoArgs = ['build', '--release', '--locked', '--bin', 'sanc'];
if (rustTarget) cargoArgs.push('--target', rustTarget);
const result = spawnSync('cargo', cargoArgs, { cwd: root, stdio: 'inherit' });
if (result.error || result.status !== 0) process.exit(result.status || 1);

const name = nodePlatform === 'win32' ? 'sanc.exe' : 'sanc';
const builtPath = rustTarget
  ? path.join(root, 'target', rustTarget, 'release', name)
  : path.join(root, 'target', 'release', name);

const platformDir = path.join(root, 'npm', `${nodePlatform}-${nodeArch}`);
fs.mkdirSync(platformDir, { recursive: true });
const executable = path.join(platformDir, name);
fs.copyFileSync(builtPath, executable);
fs.chmodSync(executable, 0o755);
const manifest = { platform: nodePlatform, arch: nodeArch, sha256: crypto.createHash('sha256').update(fs.readFileSync(executable)).digest('hex') };
fs.writeFileSync(path.join(platformDir, 'manifest.json'), JSON.stringify(manifest) + '\n');
console.log(`Built ${nodePlatform}-${nodeArch} into npm/${nodePlatform}-${nodeArch}/`);

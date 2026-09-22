#!/usr/bin/env node
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawn } = require('node:child_process');
const root = path.join(__dirname, '..', 'native');
try {
  const manifest = JSON.parse(fs.readFileSync(path.join(root, 'manifest.json'), 'utf8'));
  if (manifest.platform !== process.platform || manifest.arch !== process.arch) {
    throw new Error('This preview package was built for a different platform/architecture.');
  }
  const executable = path.join(root, process.platform === 'win32' ? 'sanc.exe' : 'sanc');
  const digest = crypto.createHash('sha256').update(fs.readFileSync(executable)).digest('hex');
  if (digest !== manifest.sha256) throw new Error('Native executable checksum mismatch.');
  const child = spawn(executable, process.argv.slice(2), { stdio: 'inherit', windowsHide: true });
  child.on('error', () => { console.error('Unable to start native executable.'); process.exitCode = 1; });
  child.on('exit', (code, signal) => { process.exitCode = code ?? (signal ? 1 : 0); });
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => child.kill(signal));
} catch (error) {
  console.error(`SessAnchor: ${error.message}`);
  process.exitCode = 1;
}

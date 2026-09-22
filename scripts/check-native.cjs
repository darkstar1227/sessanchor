'use strict';
const { spawnSync } = require('node:child_process');
const path = require('node:path');
const result = spawnSync(process.execPath, [path.join(__dirname, '..', 'bin', 'sanc.cjs'), 'capabilities'], { encoding: 'utf8' });
if (result.status !== 0) {
  console.error('Build native executable first: npm run build:native');
  process.exit(1);
}
const capabilities = JSON.parse(result.stdout);
if (capabilities.schema_version !== 1) throw new Error('Unexpected binary response schema.');

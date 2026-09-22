'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const path = require('node:path');
test('npm launcher passes structured capabilities through unchanged', () => {
  const out = spawnSync(process.execPath, [path.join(__dirname, '..', 'bin', 'sanc.cjs'), 'capabilities'], { encoding: 'utf8' });
  assert.equal(out.status, 0, out.stderr);
  assert.equal(JSON.parse(out.stdout).schema_version, 1);
});
test('npm package is explicitly private and has both CLI aliases', () => {
  const pkg = require('../package.json');
  assert.equal(pkg.private, true);
  assert.equal(pkg.bin.sanc, pkg.bin.sessanchor);
  assert.equal(pkg.scripts.postinstall, undefined);
});

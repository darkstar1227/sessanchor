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
test('npm package is publishable and has both CLI aliases', () => {
  const pkg = require('../package.json');
  assert.equal(pkg.private, undefined);
  assert.equal(pkg.bin.sanc, pkg.bin.sessanchor);
  assert.equal(pkg.scripts.postinstall, undefined);
});

test('every platform optional dependency has a matching npm/ subpackage', () => {
  const fs = require('node:fs');
  const pkg = require('../package.json');
  for (const name of Object.keys(pkg.optionalDependencies)) {
    const dir = name.replace('@sessanchor/', '');
    const sub = require(`../npm/${dir}/package.json`);
    assert.equal(sub.name, name);
    assert.equal(sub.version, pkg.optionalDependencies[name]);
    assert.ok(fs.existsSync(path.join(__dirname, '..', 'npm', dir, 'package.json')));
  }
});

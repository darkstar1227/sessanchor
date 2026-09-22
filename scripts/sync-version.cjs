'use strict';
// Stamps package.json (root + every npm/<platform>/) with one version, derived
// from the release tag in CI. Keeps root optionalDependencies pinned to the
// same version so a release always publishes a consistent, installable set.
const fs = require('node:fs');
const path = require('node:path');
const root = path.join(__dirname, '..');

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(version)) {
  throw new Error('Usage: node scripts/sync-version.cjs X.Y.Z (no leading "v")');
}

function readJson(file) { return JSON.parse(fs.readFileSync(file, 'utf8')); }
function writeJson(file, data) { fs.writeFileSync(file, JSON.stringify(data, null, 2) + '\n'); }

const rootPkgPath = path.join(root, 'package.json');
const rootPkg = readJson(rootPkgPath);
rootPkg.version = version;

const npmDir = path.join(root, 'npm');
for (const dir of fs.readdirSync(npmDir)) {
  const pkgPath = path.join(npmDir, dir, 'package.json');
  if (!fs.existsSync(pkgPath)) continue;
  const pkg = readJson(pkgPath);
  pkg.version = version;
  writeJson(pkgPath, pkg);
  if (rootPkg.optionalDependencies && rootPkg.optionalDependencies[pkg.name] !== undefined) {
    rootPkg.optionalDependencies[pkg.name] = version;
  }
  console.log(`${pkg.name}@${version}`);
}

writeJson(rootPkgPath, rootPkg);
console.log(`sessanchor@${version}`);

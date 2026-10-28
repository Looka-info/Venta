#!/usr/bin/env node

/**
 * Vanta Native Binary Provisioner for NPM
 *
 * Automatically provisions the official native Vanta toolchain (vantac, vanta,
 * runtime archive, and standard library) for Linux x86-64.
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const https = require('https');

const VERSION = 'v1.0-RC2';
const TARBALL_NAME = `vanta-${VERSION}-x86_64-unknown-linux-gnu.tar.gz`;
const RELEASE_URL = `https://github.com/Looka-info/Venta/releases/download/${VERSION}/${TARBALL_NAME}`;

const PKG_ROOT = path.resolve(__dirname, '..');
const DIST_DIR = path.join(PKG_ROOT, 'dist');
const BIN_DIR = path.join(DIST_DIR, 'bin');

function log(msg) {
  console.log(`[vanta-lang] ${msg}`);
}

function ensureDir(dir) {
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
}

function findLocalBinaries() {
  const repoRoot = path.resolve(PKG_ROOT, '../..');
  const candidates = [
    path.join(repoRoot, 'dist', `vanta-${VERSION}-x86_64-unknown-linux-gnu`),
    path.join(repoRoot, 'target', 'release'),
    path.join(repoRoot, 'bin'),
  ];

  for (const c of candidates) {
    const vantac = path.join(c, 'vantac');
    const vanta = path.join(c, 'vanta');
    if (fs.existsSync(vantac) && fs.existsSync(vanta)) {
      return { dir: c, vantac, vanta };
    }
  }
  return null;
}

function copyRecursive(src, dest) {
  const stats = fs.statSync(src);
  if (stats.isDirectory()) {
    ensureDir(dest);
    for (const file of fs.readdirSync(src)) {
      copyRecursive(path.join(src, file), path.join(dest, file));
    }
  } else {
    ensureDir(path.dirname(dest));
    fs.copyFileSync(src, dest);
  }
}

async function downloadFile(url, dest) {
  return new Promise((resolve, reject) => {
    https.get(url, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return resolve(downloadFile(res.headers.location, dest));
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`Download failed with HTTP ${res.statusCode}`));
      }
      const fileStream = fs.createWriteStream(dest);
      res.pipe(fileStream);
      fileStream.on('finish', () => {
        fileStream.close();
        resolve();
      });
    }).on('error', reject);
  });
}

async function main() {
  log(`Provisioning Vanta Toolchain (${VERSION})...`);

  ensureDir(DIST_DIR);
  ensureDir(BIN_DIR);

  // 1. Check for local repository build
  const local = findLocalBinaries();
  if (local) {
    log(`Found local Vanta binaries in ${local.dir}, copying...`);
    fs.copyFileSync(local.vantac, path.join(BIN_DIR, 'vantac'));
    fs.copyFileSync(local.vanta, path.join(BIN_DIR, 'vanta'));
    fs.chmodSync(path.join(BIN_DIR, 'vantac'), 0o755);
    fs.chmodSync(path.join(BIN_DIR, 'vanta'), 0o755);

    const repoRoot = path.resolve(PKG_ROOT, '../..');
    const stdSrc = path.join(repoRoot, 'library', 'std');
    if (fs.existsSync(stdSrc)) {
      copyRecursive(stdSrc, path.join(DIST_DIR, 'lib', 'vanta', 'std'));
    }
    const rtSrc = path.join(repoRoot, 'runtime', 'libvanta_rt.a');
    if (fs.existsSync(rtSrc)) {
      ensureDir(path.join(DIST_DIR, 'lib', 'vanta'));
      fs.copyFileSync(rtSrc, path.join(DIST_DIR, 'lib', 'vanta', 'libvanta_rt.a'));
    }

    log(`Local binaries provisioned successfully.`);
    return;
  }

  // 2. Download from official GitHub Releases
  const tempTarball = path.join(DIST_DIR, TARBALL_NAME);
  log(`Downloading official release from: ${RELEASE_URL}`);

  try {
    await downloadFile(RELEASE_URL, tempTarball);
    log(`Extracting ${TARBALL_NAME}...`);
    execSync(`tar -xzf "${tempTarball}" -C "${DIST_DIR}" --strip-components=1`, { stdio: 'inherit' });
    fs.unlinkSync(tempTarball);

    // Ensure permissions
    const vantacBin = path.join(BIN_DIR, 'vantac');
    const vantaBin = path.join(BIN_DIR, 'vanta');
    if (fs.existsSync(vantacBin)) fs.chmodSync(vantacBin, 0o755);
    if (fs.existsSync(vantaBin)) fs.chmodSync(vantaBin, 0o755);

    log(`Vanta toolchain provisioned successfully.`);
  } catch (err) {
    log(`Warning: Automatic download failed (${err.message}).`);
    log(`Run './scripts/build_self_hosted_compiler.sh' or install from GitHub Releases.`);
  }
}

main().catch((err) => {
  console.error(`[vanta-lang] Installation warning:`, err.message);
  process.exit(0); // Do not fail npm install
});

#!/usr/bin/env node

/**
 * Vanta Native Binary Provisioner for NPM
 *
 * Automatically provisions the official native Vanta toolchain (vantac, vanta,
 * runtime archive, and standard library) for Linux, Windows, and macOS.
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const https = require('https');

const VERSION = 'v1.0-RC2';

const platform = process.platform;
const arch = process.arch;

let target = 'x86_64-unknown-linux-gnu';
let isZip = false;

if (platform === 'win32') {
  target = 'x86_64-pc-windows-msvc';
  isZip = true;
} else if (platform === 'darwin') {
  target = (arch === 'arm64') ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
  isZip = false;
} else {
  target = (arch === 'arm64') ? 'aarch64-unknown-linux-gnu' : 'x86_64-unknown-linux-gnu';
  isZip = false;
}

const ARCHIVE_EXT = isZip ? '.zip' : '.tar.gz';
const ARCHIVE_NAME = `vanta-${VERSION}-${target}${ARCHIVE_EXT}`;
const RELEASE_URL = `https://github.com/Looka-info/Venta/releases/download/${VERSION}/${ARCHIVE_NAME}`;

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
  const isWin = process.platform === 'win32';
  const exts = isWin ? ['.exe', ''] : [''];
  const dirs = [
    path.join(repoRoot, 'dist', `vanta-${VERSION}-${target}`, 'bin'),
    path.join(repoRoot, 'dist', `vanta-${VERSION}-${target}`),
    path.join(repoRoot, 'target', 'release'),
    path.join(repoRoot, 'target', 'debug'),
    path.join(repoRoot, 'bin'),
  ];

  let vantac = null;
  let vanta = null;

  for (const d of dirs) {
    for (const ext of exts) {
      if (!vantac) {
        if (fs.existsSync(path.join(d, `ventac${ext}`))) vantac = path.join(d, `ventac${ext}`);
        else if (fs.existsSync(path.join(d, `vantac${ext}`))) vantac = path.join(d, `vantac${ext}`);
      }
      if (!vanta) {
        if (fs.existsSync(path.join(d, `venta${ext}`))) vanta = path.join(d, `venta${ext}`);
        else if (fs.existsSync(path.join(d, `vanta${ext}`))) vanta = path.join(d, `vanta${ext}`);
      }
    }
  }

  if (vantac && vanta) {
    return { dir: path.dirname(vantac), vantac, vanta };
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
  log(`Provisioning Vanta Toolchain (${VERSION}) for target: ${target}...`);

  ensureDir(DIST_DIR);
  ensureDir(BIN_DIR);

  // 1. Check for local repository build
  const local = findLocalBinaries();
  if (local) {
    log(`Found local Vanta binaries in ${local.dir}, copying...`);
    const isWin = process.platform === 'win32';
    const ext = isWin && local.vantac.endsWith('.exe') ? '.exe' : '';

    fs.copyFileSync(local.vantac, path.join(BIN_DIR, `vantac${ext}`));
    fs.copyFileSync(local.vantac, path.join(BIN_DIR, `ventac${ext}`));
    fs.copyFileSync(local.vanta, path.join(BIN_DIR, `vanta${ext}`));
    fs.copyFileSync(local.vanta, path.join(BIN_DIR, `venta${ext}`));

    if (!isWin) {
      fs.chmodSync(path.join(BIN_DIR, 'vantac'), 0o755);
      fs.chmodSync(path.join(BIN_DIR, 'ventac'), 0o755);
      fs.chmodSync(path.join(BIN_DIR, 'vanta'), 0o755);
      fs.chmodSync(path.join(BIN_DIR, 'venta'), 0o755);
    }

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
  const tempArchive = path.join(DIST_DIR, ARCHIVE_NAME);
  log(`Downloading official release from: ${RELEASE_URL}`);

  try {
    await downloadFile(RELEASE_URL, tempArchive);
    log(`Extracting ${ARCHIVE_NAME}...`);
    if (isZip) {
      execSync(`powershell -Command "Expand-Archive -Path '${tempArchive}' -DestinationPath '${DIST_DIR}' -Force"`, { stdio: 'inherit' });
      const extractedFolder = path.join(DIST_DIR, `vanta-${VERSION}-${target}`);
      if (fs.existsSync(extractedFolder)) {
        copyRecursive(extractedFolder, DIST_DIR);
        fs.rmSync(extractedFolder, { recursive: true, force: true });
      }
    } else {
      execSync(`tar -xzf "${tempArchive}" -C "${DIST_DIR}" --strip-components=1`, { stdio: 'inherit' });
    }
    fs.unlinkSync(tempArchive);

    // Ensure permissions on POSIX
    if (process.platform !== 'win32') {
      const vantacBin = path.join(BIN_DIR, 'vantac');
      const vantaBin = path.join(BIN_DIR, 'vanta');
      if (fs.existsSync(vantacBin)) fs.chmodSync(vantacBin, 0o755);
      if (fs.existsSync(vantaBin)) fs.chmodSync(vantaBin, 0o755);
    }

    log(`Vanta toolchain provisioned successfully.`);
  } catch (err) {
    log(`Warning: Automatic download failed (${err.message}).`);
    log(`Build from source via 'cargo build --release' or install from GitHub Releases.`);
  }
}

main().catch((err) => {
  console.error(`[vanta-lang] Installation warning:`, err.message);
  process.exit(0); // Do not fail npm install
});

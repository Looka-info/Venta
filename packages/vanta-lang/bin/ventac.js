#!/usr/bin/env node

/**
 * Ventac CLI Wrapper (Node.js)
 * Executes the native Venta compiler binary transparently on Linux, Windows, and macOS.
 */

const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

const PKG_ROOT = path.resolve(__dirname, '..');

function getCandidates() {
  const home = os.homedir();
  const isWin = process.platform === 'win32';
  const exts = isWin ? ['.exe', ''] : [''];

  const names = ['ventac', 'vantac'];
  const candidates = [];

  for (const name of names) {
    for (const ext of exts) {
      const binName = `${name}${ext}`;
      candidates.push(
        path.join(PKG_ROOT, 'dist', 'bin', binName),
        path.join(PKG_ROOT, 'dist', binName),
        path.join(home, '.venta', 'bin', binName),
        path.join(home, '.vanta', 'bin', binName),
        path.join(home, '.local', 'bin', binName),
        path.join(PKG_ROOT, '..', '..', 'target', 'release', binName),
        path.join(PKG_ROOT, '..', '..', 'target', 'debug', binName),
        path.join(PKG_ROOT, '..', '..', 'bin', binName)
      );
    }
  }
  return candidates;
}

function findBinary() {
  for (const c of getCandidates()) {
    if (fs.existsSync(c)) {
      return c;
    }
  }
  return null;
}

function toWslPath(p) {
  if (typeof p !== 'string') return p;
  return p
    .replace(/^([A-Za-z]):/, (_, d) => `/mnt/${d.toLowerCase()}`)
    .replace(/\\/g, '/');
}

const nativeBin = findBinary();

if (process.platform === 'win32') {
  if (nativeBin && (nativeBin.endsWith('.exe') || fs.existsSync(nativeBin + '.exe'))) {
    const exe = nativeBin.endsWith('.exe') ? nativeBin : nativeBin + '.exe';
    const child = spawn(exe, process.argv.slice(2), {
      stdio: 'inherit',
      env: {
        ...process.env,
        VANTA_STD_PATH: process.env.VANTA_STD_PATH || path.join(PKG_ROOT, 'dist', 'lib', 'vanta', 'std'),
      },
    });
    child.on('close', (code) => process.exit(code === null ? 1 : code));
    child.on('error', (err) => {
      console.error(`Failed to execute Vanta compiler native Windows binary: ${err.message}`);
      process.exit(1);
    });
  } else {
    const wslBin = nativeBin ? toWslPath(nativeBin) : 'ventac';
    const convertedArgs = process.argv.slice(2).map(arg => {
      if (arg.includes('\\') || /^[A-Za-z]:/.test(arg)) {
        return toWslPath(arg);
      }
      return arg;
    });

    const child = spawn('wsl', [wslBin, ...convertedArgs], { stdio: 'inherit' });
    child.on('close', (code) => process.exit(code === null ? 1 : code));
    child.on('error', (err) => {
      console.error(`Failed to invoke WSL for ventac: ${err.message}`);
      process.exit(1);
    });
  }
} else {
  if (!nativeBin) {
    console.error('Error: Vanta compiler (ventac) native binary not found.');
    console.error('Run: npm run postinstall or install via:');
    console.error('  curl -fsSL https://raw.githubusercontent.com/Looka-info/Venta/main/scripts/install.sh | bash');
    process.exit(1);
  }

  const child = spawn(nativeBin, process.argv.slice(2), {
    stdio: 'inherit',
    env: {
      ...process.env,
      VANTA_STD_PATH: process.env.VANTA_STD_PATH || path.join(PKG_ROOT, 'dist', 'lib', 'vanta', 'std'),
    },
  });

  child.on('close', (code) => {
    process.exit(code === null ? 1 : code);
  });

  child.on('error', (err) => {
    console.error(`Failed to execute ventac binary: ${err.message}`);
    process.exit(1);
  });
}

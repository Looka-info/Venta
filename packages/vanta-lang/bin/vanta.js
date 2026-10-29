#!/usr/bin/env node

/**
 * Vanta CLI Wrapper (Node.js)
 * Executes the native Vanta compiler binary transparently on Linux and Windows (via WSL).
 */

const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

const PKG_ROOT = path.resolve(__dirname, '..');

function getCandidates() {
  const home = os.homedir();
  return [
    path.join(PKG_ROOT, 'dist', 'bin', 'venta'),
    path.join(PKG_ROOT, 'dist', 'bin', 'vanta'),
    path.join(PKG_ROOT, 'dist', 'venta'),
    path.join(PKG_ROOT, 'dist', 'vanta'),
    path.join(home, '.venta', 'bin', 'venta'),
    path.join(home, '.vanta', 'bin', 'vanta'),
    path.join(home, '.local', 'bin', 'venta'),
    path.join(home, '.local', 'bin', 'vanta'),
    path.join(PKG_ROOT, '..', '..', 'target', 'release', 'venta'),
    path.join(PKG_ROOT, '..', '..', 'target', 'release', 'vanta'),
    path.join(PKG_ROOT, '..', '..', 'target', 'debug', 'venta'),
    path.join(PKG_ROOT, '..', '..', 'target', 'debug', 'vanta'),
    path.join(PKG_ROOT, '..', '..', 'bin', 'venta'),
    path.join(PKG_ROOT, '..', '..', 'bin', 'vanta'),
  ];
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
  // On Windows, invoke the ELF binary via WSL
  const wslBin = nativeBin ? toWslPath(nativeBin) : 'vanta';
  
  // Convert any path arguments with Windows backslashes to WSL-compatible forward slashes
  const convertedArgs = process.argv.slice(2).map(arg => {
    // If it looks like a path or file, convert backslashes
    if (arg.includes('\\') || /^[A-Za-z]:/.test(arg)) {
      return toWslPath(arg);
    }
    return arg;
  });

  const child = spawn('wsl', [wslBin, ...convertedArgs], { stdio: 'inherit' });
  child.on('close', (code) => process.exit(code === null ? 1 : code));
  child.on('error', (err) => {
    console.error(`Failed to invoke WSL for Vanta: ${err.message}`);
    process.exit(1);
  });
} else {
  if (!nativeBin) {
    console.error('Error: Vanta native binary not found.');
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
    console.error(`Failed to execute Vanta binary: ${err.message}`);
    process.exit(1);
  });
}

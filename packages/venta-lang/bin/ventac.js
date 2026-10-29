#!/usr/bin/env node

/**
 * Venta Compiler CLI Wrapper (Node.js)
 * Executes the native Venta compiler binary (ventac) transparently on Linux and Windows (via WSL).
 */

const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

const PKG_ROOT = path.resolve(__dirname, '..');

function getCandidates() {
  const home = os.homedir();
  return [
    path.join(PKG_ROOT, 'dist', 'bin', 'ventac'),
    path.join(PKG_ROOT, 'dist', 'bin', 'vantac'),
    path.join(PKG_ROOT, 'dist', 'ventac'),
    path.join(PKG_ROOT, 'dist', 'vantac'),
    path.join(home, '.venta', 'bin', 'ventac'),
    path.join(home, '.vanta', 'bin', 'vantac'),
    path.join(home, '.local', 'bin', 'ventac'),
    path.join(home, '.local', 'bin', 'vantac'),
    path.join(PKG_ROOT, '..', '..', 'target', 'release', 'ventac'),
    path.join(PKG_ROOT, '..', '..', 'target', 'release', 'vantac'),
    path.join(PKG_ROOT, '..', '..', 'target', 'debug', 'ventac'),
    path.join(PKG_ROOT, '..', '..', 'target', 'debug', 'vantac'),
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
    console.error('Error executing Ventac binary via WSL:', err.message);
    process.exit(1);
  });
} else {
  if (!nativeBin) {
    console.error('Ventac binary not found. Run: npm install -g venta-lang or cargo build --release --bin ventac');
    process.exit(1);
  }

  const child = spawn(nativeBin, process.argv.slice(2), { stdio: 'inherit' });
  child.on('close', (code) => process.exit(code === null ? 1 : code));
  child.on('error', (err) => {
    console.error('Error executing Ventac binary:', err.message);
    process.exit(1);
  });
}

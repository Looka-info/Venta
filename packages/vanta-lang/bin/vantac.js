#!/usr/bin/env node

/**
 * Vantac CLI Wrapper (Node.js)
 * Executes the native Vanta reference compiler (vantac) binary transparently on Linux and Windows (via WSL).
 */

const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');

const PKG_ROOT = path.resolve(__dirname, '..');

function getCandidates() {
  const home = os.homedir();
  return [
    path.join(PKG_ROOT, 'dist', 'bin', 'vantac'),
    path.join(PKG_ROOT, 'dist', 'vantac'),
    path.join(home, '.vanta', 'bin', 'vantac'),
    path.join(home, '.local', 'bin', 'vantac'),
    path.join(PKG_ROOT, '..', '..', 'target', 'release', 'vantac'),
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
  // On Windows, invoke the ELF binary via WSL
  const wslBin = nativeBin ? toWslPath(nativeBin) : 'vantac';

  const convertedArgs = process.argv.slice(2).map(arg => {
    if (arg.includes('\\') || /^[A-Za-z]:/.test(arg)) {
      return toWslPath(arg);
    }
    return arg;
  });

  const child = spawn('wsl', [wslBin, ...convertedArgs], { stdio: 'inherit' });
  child.on('close', (code) => process.exit(code === null ? 1 : code));
  child.on('error', (err) => {
    console.error(`Failed to invoke WSL for vantac: ${err.message}`);
    process.exit(1);
  });
} else {
  if (!nativeBin) {
    console.error('Error: Vanta reference compiler (vantac) not found.');
    console.error('Run: npm run postinstall');
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
    console.error(`Failed to execute vantac binary: ${err.message}`);
    process.exit(1);
  });
}

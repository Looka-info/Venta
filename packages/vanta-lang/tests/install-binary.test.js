const fs = require('fs');
const path = require('path');
const os = require('os');
const assert = require('assert');
const { extractArchive, getTargetTriple, getBinaryNames } = require('../scripts/install-binary');

function crc32(buf) {
  let crc = -1;
  for (let i = 0; i < buf.length; i++) {
    crc = crc ^ buf[i];
    for (let j = 0; j < 8; j++) {
      crc = (crc >>> 1) ^ (-(crc & 1) & 0xEDB88320);
    }
  }
  return (crc ^ -1) >>> 0;
}

function createZipBuffer(entries) {
  const parts = [];
  const cdEntries = [];
  let offset = 0;

  for (const entry of entries) {
    const nameBuf = Buffer.from(entry.name, 'utf8');
    const dataBuf = Buffer.from(entry.data || '');
    const crc = crc32(dataBuf);

    const localHdr = Buffer.alloc(30);
    localHdr.writeUInt32LE(0x04034b50, 0);
    localHdr.writeUInt16LE(20, 4);
    localHdr.writeUInt16LE(0, 6);
    localHdr.writeUInt16LE(0, 8);
    localHdr.writeUInt16LE(0, 10);
    localHdr.writeUInt16LE(0, 12);
    localHdr.writeUInt32LE(crc, 14);
    localHdr.writeUInt32LE(dataBuf.length, 18);
    localHdr.writeUInt32LE(dataBuf.length, 22);
    localHdr.writeUInt16LE(nameBuf.length, 26);
    localHdr.writeUInt16LE(0, 28);
    parts.push(localHdr, nameBuf, dataBuf);

    const cdHdr = Buffer.alloc(46);
    cdHdr.writeUInt32LE(0x02014b50, 0);
    cdHdr.writeUInt16LE(20, 4);
    cdHdr.writeUInt16LE(20, 6);
    cdHdr.writeUInt16LE(0, 8);
    cdHdr.writeUInt16LE(0, 10);
    cdHdr.writeUInt16LE(0, 12);
    cdHdr.writeUInt16LE(0, 14);
    cdHdr.writeUInt32LE(crc, 16);
    cdHdr.writeUInt32LE(dataBuf.length, 20);
    cdHdr.writeUInt32LE(dataBuf.length, 24);
    cdHdr.writeUInt16LE(nameBuf.length, 28);
    cdHdr.writeUInt16LE(0, 30);
    cdHdr.writeUInt16LE(0, 32);
    cdHdr.writeUInt16LE(0, 34);
    cdHdr.writeUInt16LE(0, 36);
    cdHdr.writeUInt32LE(0, 38);
    cdHdr.writeUInt32LE(offset, 42);
    cdEntries.push(cdHdr, nameBuf);

    offset += localHdr.length + nameBuf.length + dataBuf.length;
  }

  const cdOffset = offset;
  let cdSize = 0;
  for (const part of cdEntries) {
    parts.push(part);
    cdSize += part.length;
  }

  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(0, 4);
  eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(cdSize, 12);
  eocd.writeUInt32LE(cdOffset, 16);
  eocd.writeUInt16LE(0, 20);
  parts.push(eocd);

  return Buffer.concat(parts);
}

function testTargetTripleResolution() {
  console.log('[TEST] Target triple resolution...');
  assert.strictEqual(getTargetTriple('win32', 'x64'), 'x86_64-pc-windows-msvc');
  assert.strictEqual(getTargetTriple('darwin', 'arm64'), 'aarch64-apple-darwin');
  assert.strictEqual(getTargetTriple('darwin', 'x64'), 'x86_64-apple-darwin');
  assert.strictEqual(getTargetTriple('linux', 'arm64'), 'aarch64-unknown-linux-gnu');
  assert.strictEqual(getTargetTriple('linux', 'x64'), 'x86_64-unknown-linux-gnu');

  const winBins = getBinaryNames('win32');
  assert.strictEqual(winBins.vantac, 'vantac.exe');
  assert.strictEqual(winBins.vanta, 'vanta.exe');

  const linBins = getBinaryNames('linux');
  assert.strictEqual(linBins.vantac, 'vantac');
  assert.strictEqual(linBins.vanta, 'vanta');
  console.log('  -> Target triple resolution passed.');
}

function testSafeExtractionWithSpacesAndSpecialChars() {
  console.log('[TEST] Safe extraction with spaces and special characters in paths...');
  const tmpBase = fs.mkdtempSync(path.join(os.tmpdir(), 'vanta test space (spec)! '));
  const zipPath = path.join(tmpBase, 'package with spaces.zip');
  const destDir = path.join(tmpBase, 'destination folder (v1.0) #1');

  try {
    const zipData = createZipBuffer([
      { name: 'bin/vantac.txt', data: 'compiler payload' },
      { name: 'lib/runtime.a', data: 'archive payload' },
    ]);
    fs.writeFileSync(zipPath, zipData);

    extractArchive(zipPath, destDir, true);

    const extractedVantac = path.join(destDir, 'bin', 'vantac.txt');
    const extractedRt = path.join(destDir, 'lib', 'runtime.a');

    assert.ok(fs.existsSync(extractedVantac), 'vantac.txt must exist in target');
    assert.strictEqual(fs.readFileSync(extractedVantac, 'utf8'), 'compiler payload');
    assert.ok(fs.existsSync(extractedRt), 'runtime.a must exist in target');
    assert.strictEqual(fs.readFileSync(extractedRt, 'utf8'), 'archive payload');
    console.log('  -> Safe extraction with spaces and special characters passed.');
  } finally {
    fs.rmSync(tmpBase, { recursive: true, force: true });
  }
}

function testZipSlipRejection() {
  console.log('[TEST] Zip Slip path traversal rejection...');
  const tmpBase = fs.mkdtempSync(path.join(os.tmpdir(), 'vanta-zipslip-'));
  const zipPath = path.join(tmpBase, 'malicious.zip');
  const destDir = path.join(tmpBase, 'extract_target');
  const canaryFile = path.join(tmpBase, 'canary_escaped.txt');

  try {
    // Malicious zip trying to escape into tmpBase
    const maliciousZip = createZipBuffer([
      { name: 'valid.txt', data: 'safe file' },
      { name: '../canary_escaped.txt', data: 'PWNED' },
    ]);
    fs.writeFileSync(zipPath, maliciousZip);

    let rejected = false;
    try {
      extractArchive(zipPath, destDir, true);
    } catch (err) {
      rejected = true;
    }

    assert.ok(rejected, 'Malicious Zip Slip archive must be rejected');
    assert.ok(!fs.existsSync(canaryFile), 'Canary file outside destination must NOT be written');
    console.log('  -> Zip Slip path traversal correctly rejected.');
  } finally {
    fs.rmSync(tmpBase, { recursive: true, force: true });
  }
}

function runAll() {
  console.log('=== Vanta NPM Installer Security Test Suite ===');
  testTargetTripleResolution();
  testSafeExtractionWithSpacesAndSpecialChars();
  testZipSlipRejection();
  console.log('=== All install-binary tests passed successfully! ===');
}

runAll();

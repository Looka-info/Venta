# Vanta Release Artifact Integrity & Verification Guide

## 1. Release Candidate Artifacts & Cryptographic Checksums

For **Vanta v1.0-RC1 (Linux x86-64)**, the official release artifacts and their verified SHA-256 digests are:

| File Name | File Size | SHA-256 Checksum | Purpose |
|:---|:---:|:---|:---|
| `vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz` | ~2.18 MB | `6268c37ad04b0013fa3879464315b634e83726b740b3c45941201c62ab860199` | Primary release tarball (binaries, runtime, std, examples, scripts) |
| `vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz.sha256` | 113 B | N/A (Contains hash line) | Standard SHA-256 hash manifest |

---

## 2. Checksum Verification Procedure

To verify that the downloaded archive has not been corrupted in transit or modified:

```bash
# Verify using sha256sum
sha256sum -c vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz.sha256
```

**Expected Successful Output:**
```
vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz: OK
```

If the output displays `FAILED` or the hash does not match, the archive must be discarded and re-downloaded.

---

## 3. Security Dimensions: Checksums, Authenticity, and Signatures

Understanding the security properties of release artifacts requires distinguishing between three separate concepts:

### 3.1 Checksum Verification (Data Integrity)
- **What it provides**: Guarantees that the downloaded file is bit-for-bit identical to the file from which the checksum was computed. It protects against transmission errors, disk corruption, and incomplete downloads.
- **What it does NOT provide**: It does not prove the identity of the author or protect against an attacker who controls both the download file and the checksum manifest.

### 3.2 Transport Authenticity (Channel Security)
- **What it provides**: Transport Layer Security (TLS/HTTPS) and cryptographic Git hashes verify that the artifacts were obtained from the authentic repository host (`https://github.com/vanta-lang/vanta`).
- **What it does NOT provide**: Independent offline verification after download.

### 3.3 Cryptographic Signatures (Provenance & Non-Repudiation)
- **What it provides**: An asymmetric cryptographic signature (e.g. using GPG/OpenPGP, Minisign, or Sigstore cosign) allows anyone to mathematically verify that the archive was signed with the private key of an authorized release manager.
- **Current Status for v1.0-RC1**: The project currently relies on Git commit hashes and SHA-256 checksums published over authenticated TLS channels. In accordance with project security policy, **no fabricated GPG signatures or ad-hoc private keys have been generated or asserted without formal key-management infrastructure**. Cryptographic signing via Sigstore / GPG is scheduled for the v1.0 General Availability release pipeline.

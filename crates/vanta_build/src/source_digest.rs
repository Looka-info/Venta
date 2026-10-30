//! Deterministic source-tree hashing for compiler identity.
//!
//! Provides SHA-256 digesting of all compiler sources, manifests, build scripts,
//! runtime, and standard-library inputs affecting code generation.

use std::fs;
use std::io;
use std::path::Path;

/// 256-bit cryptographic digest of a compiler source tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceTreeDigest {
    pub bytes: [u8; 32],
}

#[allow(dead_code)]
impl SourceTreeDigest {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(64);
        for byte in &self.bytes {
            use std::fmt::Write;
            let _ = write!(&mut hex, "{:02x}", byte);
        }
        hex
    }
}

/// Standalone FIPS 180-4 compliant SHA-256 implementation.
struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buf_len: usize,
    total_bytes: u64,
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0u8; 64],
            buf_len: 0,
            total_bytes: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_bytes += data.len() as u64;

        if self.buf_len > 0 {
            let needed = 64 - self.buf_len;
            if data.len() >= needed {
                self.buffer[self.buf_len..64].copy_from_slice(&data[..needed]);
                let block = self.buffer;
                Self::compress(&mut self.state, &block);
                self.buf_len = 0;
                data = &data[needed..];
            } else {
                self.buffer[self.buf_len..self.buf_len + data.len()].copy_from_slice(data);
                self.buf_len += data.len();
                return;
            }
        }

        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            Self::compress(&mut self.state, &block);
            data = &data[64..];
        }

        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_bytes * 8;
        self.buffer[self.buf_len] = 0x80;
        self.buf_len += 1;

        if self.buf_len > 56 {
            for i in self.buf_len..64 {
                self.buffer[i] = 0;
            }
            let block = self.buffer;
            Self::compress(&mut self.state, &block);
            self.buf_len = 0;
        }

        for i in self.buf_len..56 {
            self.buffer[i] = 0;
        }

        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buffer;
        Self::compress(&mut self.state, &block);

        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    #[inline(always)]
    fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
}

/// Checks if a directory should be skipped during compiler source scanning.
pub fn should_ignore_dir(dir_name: &str) -> bool {
    dir_name == ".git"
        || dir_name == "target"
        || dir_name == ".venta_cache"
        || dir_name == "node_modules"
        || dir_name == ".idea"
        || dir_name == ".vscode"
        || dir_name == "tmp"
        || dir_name.starts_with('.')
}

/// Checks if a file should be skipped during compiler source scanning.
pub fn should_ignore_file(file_name: &str) -> bool {
    if file_name.starts_with('.') {
        return true;
    }
    // Temporary backup/editor files
    if file_name.ends_with('~')
        || file_name.ends_with(".tmp")
        || file_name.ends_with(".bak")
        || file_name.ends_with(".swp")
        || file_name.ends_with(".swo")
    {
        return true;
    }
    // Binary/compiled artifacts
    if file_name.ends_with(".o")
        || file_name.ends_with(".obj")
        || file_name.ends_with(".a")
        || file_name.ends_with(".lib")
        || file_name.ends_with(".so")
        || file_name.ends_with(".dylib")
        || file_name.ends_with(".dll")
        || file_name.ends_with(".exe")
        || file_name.ends_with(".out")
        || file_name.ends_with(".pdb")
        || file_name.ends_with(".d")
    {
        return true;
    }
    false
}

/// Checks if a file qualifies as a relevant compiler input.
pub fn is_relevant_compiler_file(file_name: &str) -> bool {
    if should_ignore_file(file_name) {
        return false;
    }
    file_name == "Cargo.toml"
        || file_name == "Cargo.lock"
        || file_name.ends_with(".rs")
        || file_name.ends_with(".toml")
        || file_name.ends_with(".c")
        || file_name.ends_with(".h")
        || file_name.ends_with(".va")
        || file_name.ends_with(".s")
        || file_name.ends_with(".asm")
        || file_name.ends_with(".json")
        || file_name.ends_with(".tbl")
        || file_name.ends_with(".def")
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn walk_dir(root: &Path, dir: &Path, files: &mut Vec<String>) -> Result<(), io::Error> {
    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    if should_ignore_dir(&dir_name) {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if !should_ignore_dir(&name_str) {
                walk_dir(root, &path, files)?;
            }
        } else if path.is_file() {
            if is_relevant_compiler_file(&name_str) {
                if let Ok(rel) = path.strip_prefix(root) {
                    files.push(normalize_path(rel));
                }
            }
        }
    }

    Ok(())
}

/// Scans the given source tree root for all compiler components that affect generated artifacts.
///
/// Returns relative, forward-slash-normalized paths sorted lexicographically.
pub fn scan_compiler_source_files(root: &Path) -> Result<Vec<String>, io::Error> {
    let mut files = Vec::new();

    // 1. Workspace manifests and lockfile at root
    for root_file in &["Cargo.toml", "Cargo.lock"] {
        let p = root.join(root_file);
        if p.is_file() {
            files.push(root_file.to_string());
        }
    }

    // 2. Standard compiler subdirectories
    let standard_dirs = ["crates", "runtime", "library", "src"];
    let has_any_standard = standard_dirs.iter().any(|d| root.join(d).is_dir());

    if has_any_standard {
        for d in &standard_dirs {
            let dir_path = root.join(d);
            if dir_path.is_dir() {
                walk_dir(root, &dir_path, &mut files)?;
            }
        }
    } else {
        // Fallback for custom mock test trees
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let p = entry.path();
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if p.is_dir() && !should_ignore_dir(&name_str) {
                    walk_dir(root, &p, &mut files)?;
                } else if p.is_file() && is_relevant_compiler_file(&name_str) {
                    if let Ok(rel) = p.strip_prefix(root) {
                        files.push(normalize_path(rel));
                    }
                }
            }
        }
    }

    files.sort();
    files.dedup();
    Ok(files)
}

/// Computes a deterministic SHA-256 source-tree digest covering all compiler components
/// that affect generated artifacts.
///
/// Includes relative paths, entry types, lengths, and file contents.
/// Excludes `.git`, `target`, `.venta_cache`, temporary files, and binaries.
pub fn compute_source_tree_digest(root: &Path) -> Result<SourceTreeDigest, io::Error> {
    let files = scan_compiler_source_files(root)?;

    let mut hasher = Sha256::new();
    // Length-prefixed domain separation tag
    let domain_tag = b"VENTA:COMPILER_SOURCE_TREE:v1";
    hasher.update(&(domain_tag.len() as u32).to_le_bytes());
    hasher.update(domain_tag);

    for rel_path in &files {
        let full_path = root.join(rel_path);
        let metadata = fs::metadata(&full_path)?;
        let file_len = metadata.len();
        let content = fs::read(&full_path)?;

        // 1. Relative path (length-prefixed)
        let path_bytes = rel_path.as_bytes();
        hasher.update(&(path_bytes.len() as u32).to_le_bytes());
        hasher.update(path_bytes);

        // 2. Entry type: 1u8 for regular file
        hasher.update(&[1u8]);

        // 3. Length: 64-bit unsigned integer little endian
        hasher.update(&file_len.to_le_bytes());

        // 4. File content (length-prefixed)
        hasher.update(&(content.len() as u64).to_le_bytes());
        hasher.update(&content);
    }

    let bytes = hasher.finalize();
    Ok(SourceTreeDigest { bytes })
}

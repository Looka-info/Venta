//! Deterministic content-addressed cryptographic fingerprinting for Venta build artifacts.
//!
//! Provides pure-Rust, FIPS 180-4 compliant SHA-256 fingerprinting with domain separation,
//! strict length-prefixed encoding, compiler identity hashing, and serialization.

use serde::{Deserialize, Serialize};
use std::fmt;

/// 256-bit cryptographic content fingerprint.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ContentFingerprint(pub [u8; 32]);

impl ContentFingerprint {
    pub const ZERO: Self = ContentFingerprint([0u8; 32]);

    pub fn new(bytes: [u8; 32]) -> Self {
        ContentFingerprint(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(64);
        for byte in &self.0 {
            use std::fmt::Write;
            let _ = write!(&mut hex, "{:02x}", byte);
        }
        hex
    }

    pub fn from_hex(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.len() != 64 {
            return Err(format!("Expected 64 hex characters, got {}", s.len()));
        }
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|e| format!("Invalid hex byte at index {}: {}", i * 2, e))?;
        }
        Ok(ContentFingerprint(bytes))
    }

    /// Compute fingerprint of arbitrary raw bytes with domain separation.
    pub fn for_bytes(domain: Domain, data: &[u8]) -> Self {
        let mut builder = FingerprintBuilder::new(domain);
        builder.update(data);
        builder.finish()
    }
}

impl fmt::Debug for ContentFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl fmt::Display for ContentFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Domain separation tags to prevent cross-artifact hash collisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Source,
    ModuleInterface,
    FunctionMir,
    FunctionCodegen,
    TypeLayout,
    GenericInstance,
    TraitResolution,
    Object,
    Archive,
    Link,
    CompilerIdentity,
    CompilerConfig,
    Manifest,
}

impl Domain {
    pub fn as_str(&self) -> &'static str {
        match self {
            Domain::Source => "VENTA:SRC:v1",
            Domain::ModuleInterface => "VENTA:MOD_IFACE:v1",
            Domain::FunctionMir => "VENTA:FN_MIR:v1",
            Domain::FunctionCodegen => "VENTA:FN_CODEGEN:v1",
            Domain::TypeLayout => "VENTA:TYPE_LAYOUT:v1",
            Domain::GenericInstance => "VENTA:GENERIC_INST:v1",
            Domain::TraitResolution => "VENTA:TRAIT_RES:v1",
            Domain::Object => "VENTA:OBJECT:v1",
            Domain::Archive => "VENTA:ARCHIVE:v1",
            Domain::Link => "VENTA:LINK:v1",
            Domain::CompilerIdentity => "VENTA:COMPILER_ID:v1",
            Domain::CompilerConfig => "VENTA:COMPILER_CONFIG:v1",
            Domain::Manifest => "VENTA:MANIFEST:v1",
        }
    }
}

/// Incremental SHA-256 fingerprint builder with length-prefixed domain separation.
pub struct FingerprintBuilder {
    hasher: Sha256,
}

impl FingerprintBuilder {
    pub fn new(domain: Domain) -> Self {
        let mut hasher = Sha256::new();
        let domain_bytes = domain.as_str().as_bytes();
        hasher.update(&(domain_bytes.len() as u32).to_le_bytes());
        hasher.update(domain_bytes);
        Self { hasher }
    }

    /// Update with arbitrary slice of bytes.
    pub fn update(&mut self, data: &[u8]) {
        self.hasher.update(&(data.len() as u64).to_le_bytes());
        self.hasher.update(data);
    }

    /// Update with a UTF-8 string slice.
    pub fn update_str(&mut self, s: &str) {
        self.update(s.as_bytes());
    }

    /// Update with a 32-bit unsigned integer.
    pub fn update_u32(&mut self, val: u32) {
        self.hasher.update(&val.to_le_bytes());
    }

    /// Update with a 64-bit unsigned integer.
    pub fn update_u64(&mut self, val: u64) {
        self.hasher.update(&val.to_le_bytes());
    }

    /// Update with a boolean.
    pub fn update_bool(&mut self, b: bool) {
        self.hasher.update(&[if b { 1u8 } else { 0u8 }]);
    }

    /// Update with another content fingerprint.
    pub fn update_fingerprint(&mut self, fp: &ContentFingerprint) {
        self.hasher.update(&fp.0);
    }

    /// Complete computation and produce 256-bit fingerprint.
    pub fn finish(self) -> ContentFingerprint {
        ContentFingerprint(self.hasher.finalize())
    }
}

/// Compiler identity tracking version, source-tree digest, git revision, and schema versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerIdentity {
    pub compiler_version: String,
    pub cache_schema_version: u32,
    pub git_revision: String,
    pub target_triple: String,
    #[serde(default)]
    pub source_digest: String,
}

impl CompilerIdentity {
    pub const CURRENT_CACHE_SCHEMA_VERSION: u32 = 1;

    pub fn current(target_triple: &str) -> Self {
        let rev = option_env!("VENTA_GIT_REV")
            .or(option_env!("VENTA_BUILD_GIT_REV"))
            .unwrap_or("unknown-untrusted-rev")
            .to_string();
        let source_digest = option_env!("VENTA_COMPILER_SOURCE_DIGEST")
            .unwrap_or("")
            .to_string();
        Self {
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            cache_schema_version: Self::CURRENT_CACHE_SCHEMA_VERSION,
            git_revision: rev,
            target_triple: target_triple.to_string(),
            source_digest,
        }
    }

    /// Compute compiler identity directly from a source tree on disk.
    pub fn from_source_tree(
        target_triple: &str,
        root: &std::path::Path,
    ) -> Result<Self, std::io::Error> {
        let digest = crate::source_digest::compute_source_tree_digest(root)?;
        let rev = option_env!("VENTA_GIT_REV")
            .or(option_env!("VENTA_BUILD_GIT_REV"))
            .unwrap_or("unknown-untrusted-rev")
            .to_string();
        Ok(Self {
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            cache_schema_version: Self::CURRENT_CACHE_SCHEMA_VERSION,
            git_revision: rev,
            target_triple: target_triple.to_string(),
            source_digest: digest.to_hex(),
        })
    }

    pub fn fingerprint(&self) -> ContentFingerprint {
        let mut builder = FingerprintBuilder::new(Domain::CompilerIdentity);
        builder.update_str(&self.compiler_version);
        builder.update_u32(self.cache_schema_version);
        builder.update_str(&self.source_digest);
        builder.update_str(&self.git_revision);
        builder.update_str(&self.target_triple);
        builder.finish()
    }
}

/// Target CPU feature flags affecting instruction selection and code generation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetFeaturesConfig {
    pub avx: bool,
    pub avx2: bool,
    pub fma: bool,
    pub avx512f: bool,
    pub bmi1: bool,
    pub bmi2: bool,
}

/// Canonical representation of all output-affecting compiler and linker configuration flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildConfiguration {
    pub target_triple: String,
    pub arch: String,
    pub os: String,
    pub abi: String,
    pub object_format: String,
    pub opt_level: String,
    pub target_features: TargetFeaturesConfig,
    pub enable_reg_alloc: bool,
    pub enable_tail_calls: bool,
    pub enable_block_layout: bool,
    pub enable_code_align: bool,
    pub enable_mem2reg: bool,
    pub enable_gvn: bool,
    pub enable_inline: bool,
    pub enable_ipa_constprop: bool,
    pub enable_licm: bool,
    pub enable_indvar: bool,
    pub enable_strength_reduction: bool,
    pub enable_sroa: bool,
    pub enable_loop_peel: bool,
    pub enable_loop_unroll: bool,
    pub enable_vectorize: bool,
    pub enable_instruction_schedule: bool,
    pub enable_global_dce: bool,
    pub debug_info: bool,
    pub std_fingerprint: Option<ContentFingerprint>,
    pub runtime_fingerprint: Option<ContentFingerprint>,
}

impl BuildConfiguration {
    pub fn fingerprint(&self, compiler_id: &CompilerIdentity) -> ContentFingerprint {
        let mut builder = FingerprintBuilder::new(Domain::CompilerConfig);
        builder.update_fingerprint(&compiler_id.fingerprint());
        builder.update_str(&self.target_triple);
        builder.update_str(&self.arch);
        builder.update_str(&self.os);
        builder.update_str(&self.abi);
        builder.update_str(&self.object_format);
        builder.update_str(&self.opt_level);
        builder.update_bool(self.target_features.avx);
        builder.update_bool(self.target_features.avx2);
        builder.update_bool(self.target_features.fma);
        builder.update_bool(self.target_features.avx512f);
        builder.update_bool(self.target_features.bmi1);
        builder.update_bool(self.target_features.bmi2);
        builder.update_bool(self.enable_reg_alloc);
        builder.update_bool(self.enable_tail_calls);
        builder.update_bool(self.enable_block_layout);
        builder.update_bool(self.enable_code_align);
        builder.update_bool(self.enable_mem2reg);
        builder.update_bool(self.enable_gvn);
        builder.update_bool(self.enable_inline);
        builder.update_bool(self.enable_ipa_constprop);
        builder.update_bool(self.enable_licm);
        builder.update_bool(self.enable_indvar);
        builder.update_bool(self.enable_strength_reduction);
        builder.update_bool(self.enable_sroa);
        builder.update_bool(self.enable_loop_peel);
        builder.update_bool(self.enable_loop_unroll);
        builder.update_bool(self.enable_vectorize);
        builder.update_bool(self.enable_instruction_schedule);
        builder.update_bool(self.enable_global_dce);
        builder.update_bool(self.debug_info);
        if let Some(fp) = &self.std_fingerprint {
            builder.update_bool(true);
            builder.update_fingerprint(fp);
        } else {
            builder.update_bool(false);
        }
        if let Some(fp) = &self.runtime_fingerprint {
            builder.update_bool(true);
            builder.update_fingerprint(fp);
        } else {
            builder.update_bool(false);
        }
        builder.finish()
    }
}

/// Pure-Rust, zero-dependency, FIPS 180-4 compliant SHA-256 implementation.
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
        // Pad 0x80
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_empty_string() {
        let hasher = Sha256::new();
        let digest = hasher.finalize();
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        // SHA-256 of empty string is e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_standard_vector() {
        let mut hasher = Sha256::new();
        hasher.update(b"abc");
        let digest = hasher.finalize();
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        // SHA-256 of "abc" is ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_domain_separation() {
        let fp1 = ContentFingerprint::for_bytes(Domain::Source, b"hello");
        let fp2 = ContentFingerprint::for_bytes(Domain::Object, b"hello");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_hex_roundtrip() {
        let fp = ContentFingerprint::for_bytes(Domain::Source, b"test content");
        let hex = fp.to_hex();
        let parsed = ContentFingerprint::from_hex(&hex).expect("parse hex");
        assert_eq!(fp, parsed);
    }

    #[test]
    fn test_compiler_identity_invalidation() {
        let id1 = CompilerIdentity {
            compiler_version: "2.1.0".into(),
            cache_schema_version: 1,
            git_revision: "commit_a".into(),
            target_triple: "x86_64-unknown-linux-gnu".into(),
            source_digest: "digest_1".into(),
        };
        let id2 = CompilerIdentity {
            compiler_version: "2.1.0".into(),
            cache_schema_version: 1,
            git_revision: "commit_a".into(),
            target_triple: "x86_64-unknown-linux-gnu".into(),
            source_digest: "digest_2".into(),
        };
        assert_ne!(id1.fingerprint(), id2.fingerprint());

        // Also test git revision invalidation
        let id3 = CompilerIdentity {
            compiler_version: "2.1.0".into(),
            cache_schema_version: 1,
            git_revision: "commit_b".into(),
            target_triple: "x86_64-unknown-linux-gnu".into(),
            source_digest: "digest_1".into(),
        };
        assert_ne!(id1.fingerprint(), id3.fingerprint());
    }

    #[test]
    fn test_build_configuration_options_invalidation() {
        let id = CompilerIdentity {
            compiler_version: "2.1.0".into(),
            cache_schema_version: 1,
            git_revision: "commit_base".into(),
            target_triple: "x86_64-unknown-linux-gnu".into(),
            source_digest: "digest_base".into(),
        };
        let base_cfg = BuildConfiguration {
            target_triple: "x86_64-unknown-linux-gnu".into(),
            arch: "x86_64".into(),
            os: "linux".into(),
            abi: "systemv".into(),
            object_format: "elf64".into(),
            opt_level: "O1".into(),
            target_features: TargetFeaturesConfig::default(),
            enable_reg_alloc: true,
            enable_tail_calls: true,
            enable_block_layout: true,
            enable_code_align: true,
            enable_mem2reg: true,
            enable_gvn: true,
            enable_inline: true,
            enable_ipa_constprop: true,
            enable_licm: true,
            enable_indvar: true,
            enable_strength_reduction: true,
            enable_sroa: true,
            enable_loop_peel: true,
            enable_loop_unroll: true,
            enable_vectorize: false,
            enable_instruction_schedule: true,
            enable_global_dce: true,
            debug_info: false,
            std_fingerprint: None,
            runtime_fingerprint: None,
        };

        let base_fp = base_cfg.fingerprint(&id);

        // 1. Opt level -O0 vs -O1
        let mut cfg_o0 = base_cfg.clone();
        cfg_o0.opt_level = "O0".into();
        assert_ne!(base_fp, cfg_o0.fingerprint(&id));

        // 2. Vectorization toggled
        let mut cfg_vec = base_cfg.clone();
        cfg_vec.enable_vectorize = true;
        assert_ne!(base_fp, cfg_vec.fingerprint(&id));

        // 3. AVX2 toggled
        let mut cfg_avx2 = base_cfg.clone();
        cfg_avx2.target_features.avx2 = true;
        assert_ne!(base_fp, cfg_avx2.fingerprint(&id));

        // 4. AVX-512 toggled
        let mut cfg_avx512 = base_cfg.clone();
        cfg_avx512.target_features.avx512f = true;
        assert_ne!(base_fp, cfg_avx512.fingerprint(&id));

        // 5. Reg alloc toggled
        let mut cfg_noreg = base_cfg.clone();
        cfg_noreg.enable_reg_alloc = false;
        assert_ne!(base_fp, cfg_noreg.fingerprint(&id));

        // 6. Debug info toggled
        let mut cfg_dbg = base_cfg.clone();
        cfg_dbg.debug_info = true;
        assert_ne!(base_fp, cfg_dbg.fingerprint(&id));

        // 7. Tail calls toggled
        let mut cfg_tail = base_cfg.clone();
        cfg_tail.enable_tail_calls = false;
        assert_ne!(base_fp, cfg_tail.fingerprint(&id));

        // 8. Block layout toggled
        let mut cfg_layout = base_cfg.clone();
        cfg_layout.enable_block_layout = false;
        assert_ne!(base_fp, cfg_layout.fingerprint(&id));

        // 9. Instruction schedule toggled
        let mut cfg_sched = base_cfg.clone();
        cfg_sched.enable_instruction_schedule = false;
        assert_ne!(base_fp, cfg_sched.fingerprint(&id));

        // 10. Target ABI changed
        let mut cfg_abi = base_cfg.clone();
        cfg_abi.abi = "msvc".into();
        assert_ne!(base_fp, cfg_abi.fingerprint(&id));
    }
}

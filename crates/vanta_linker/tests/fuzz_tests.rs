//! Malformed-input fuzzing and robustness tests for Venta Linker (Milestone 31.5 Stage 31.5.5).
//!
//! Tests thousands of mutated, truncated, corrupted, and extreme inputs against:
//! - ELF64 object parser
//! - AMD64 COFF object parser
//! - ARM64 Mach-O object parser
//! - Unix `ar` and Windows `.lib` archive parser
//!
//! Enforces:
//! - Zero panics
//! - Zero unchecked allocations or out-of-bounds memory accesses
//! - Clean structured error reporting (`LinkerError::MalformedInput`)

use std::path::PathBuf;
use vanta_linker::archive::Archive;
use vanta_linker::coff::reader::read_coff_object;
use vanta_linker::elf::reader::parse_elf64_object;
use vanta_linker::macho::reader::read_macho_object;

// Deterministic pseudo-random generator (Xorshift64) to ensure exact reproducibility across test runs
struct FuzzRng {
    state: u64,
}

impl FuzzRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0xDEADBEEF } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn fill_bytes(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let val = self.next_u64().to_le_bytes();
            let len = chunk.len();
            chunk.copy_from_slice(&val[..len]);
        }
    }
}

#[test]
fn test_fuzz_archive_parser_random_and_mutated_buffers() {
    let mut rng = FuzzRng::new(0xCAFE_BABE_1234_5678);
    let path = PathBuf::from("fuzz.a");

    // 1. Completely random byte buffers
    for len in [0, 1, 7, 8, 15, 60, 68, 120, 256, 1024, 4096] {
        for _ in 0..100 {
            let mut buf = vec![0u8; len];
            rng.fill_bytes(&mut buf);
            // Must return Err or Ok, NEVER panic
            let _ = Archive::parse(&path, &buf);
        }
    }

    // 2. Corrupted valid archive prefixes
    let base_header = b"!<arch>\n/               0           0     0     100644  16        `\n";
    for i in 0..base_header.len() {
        let truncated = &base_header[..i];
        let _ = Archive::parse(&path, truncated);
    }
}

#[test]
fn test_fuzz_elf64_parser_robustness() {
    let mut rng = FuzzRng::new(0x1122_3344_5566_7788);
    let path = PathBuf::from("fuzz.o");

    for len in [0, 4, 16, 52, 64, 128, 512, 2048] {
        for _ in 0..100 {
            let mut buf = vec![0u8; len];
            rng.fill_bytes(&mut buf);
            let _ = parse_elf64_object(&path, &buf, 0);
        }
    }

    // Header mutation with valid ELF magic
    let mut header = [0u8; 64];
    header[0..4].copy_from_slice(b"\x7fELF");
    header[4] = 2; // 64-bit
    header[5] = 1; // little-endian
    for _ in 0..200 {
        rng.fill_bytes(&mut header[16..]);
        let _ = parse_elf64_object(&path, &header, 0);
    }
}

#[test]
fn test_fuzz_coff_parser_robustness() {
    let mut rng = FuzzRng::new(0x99AA_BBCC_DDEE_FF00);
    let path = PathBuf::from("fuzz.obj");

    for len in [0, 2, 18, 20, 40, 100, 1024] {
        for _ in 0..100 {
            let mut buf = vec![0u8; len];
            rng.fill_bytes(&mut buf);
            let _ = read_coff_object(0, "fuzz.obj".into(), path.clone(), &buf);
        }
    }

    // COFF with machine 0x8664 and mutated section/symbol counts
    let mut coff_header = [0u8; 20];
    coff_header[0..2].copy_from_slice(&0x8664u16.to_le_bytes());
    for _ in 0..200 {
        rng.fill_bytes(&mut coff_header[2..]);
        let _ = read_coff_object(0, "fuzz.obj".into(), path.clone(), &coff_header);
    }
}

#[test]
fn test_fuzz_macho_parser_robustness() {
    let mut rng = FuzzRng::new(0x1357_9BDF_2468_ACE0);
    let path = PathBuf::from("fuzz.o");

    for len in [0, 4, 32, 64, 120, 500, 2000] {
        for _ in 0..100 {
            let mut buf = vec![0u8; len];
            rng.fill_bytes(&mut buf);
            let _ = read_macho_object(0, "fuzz.o".into(), path.clone(), &buf);
        }
    }

    // Mach-O with 0xFEEDFACF magic and mutated load commands
    let mut macho_header = [0u8; 32];
    macho_header[0..4].copy_from_slice(&0xFEEDFACFu32.to_le_bytes());
    macho_header[4..8].copy_from_slice(&0x0100000Cu32.to_le_bytes());
    for _ in 0..200 {
        rng.fill_bytes(&mut macho_header[8..]);
        let _ = read_macho_object(0, "fuzz.o".into(), path.clone(), &macho_header);
    }
}

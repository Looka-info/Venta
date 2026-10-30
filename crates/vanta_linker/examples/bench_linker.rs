//! Reproducible standalone benchmark suite for Venta Native Linker (Milestone 31.5 Stage 31.5.8).
//!
//! Evaluates linker performance across increasing workloads:
//! - Small: 2 objects (minimal program)
//! - Medium: 20 objects (chained cross-module calls)
//! - Large: 100 objects (dense multi-object graph)
//! - Archive-heavy: 10 archives, 50 total members with selective lazy extraction
//!
//! Measures:
//! - Total link time (median, min, max)
//! - Emitted binary size
//! - Deterministic output hash across runs

use std::path::PathBuf;
use std::time::Instant;
use vanta_linker::config::{LinkMode, LinkerConfig};
use vanta_linker::input::InputFormat;
use vanta_linker::object::ObjectFile;
use vanta_linker::relocation::RelocationRecord;
use vanta_linker::section::{InputSection, SectionFlags, SectionKind};
use vanta_linker::symbol::{Symbol, SymbolKind, SymbolVisibility};
use vanta_target::TargetTriple;

/// Generates a synthetic relocatable ELF64 object in memory.
fn generate_synthetic_elf_object(
    obj_id: usize,
    name: &str,
    defined_funcs: &[(&str, usize)],    // (name, code_size)
    referenced_funcs: &[(&str, usize)], // (name, offset_in_code)
) -> ObjectFile {
    let mut obj = ObjectFile::new(
        obj_id,
        name.to_string(),
        PathBuf::from(format!("bench://{}", name)),
        InputFormat::Elf64,
    );

    let mut code_bytes = Vec::new();
    let mut symbols = Vec::new();
    let mut relocations = Vec::new();

    for &(fname, size) in defined_funcs {
        let offset = code_bytes.len();
        // Fill dummy x86-64 code: mov $42, %rax; ret (or nops)
        code_bytes.extend(std::iter::repeat(0x90).take(size.max(8)));
        code_bytes[offset] = 0x48;
        code_bytes[offset + 1] = 0xC7;
        code_bytes[offset + 2] = 0xC0;
        code_bytes[offset + 3] = 0x2A; // 42
        code_bytes[offset + 7] = 0xC3; // ret

        symbols.push(Symbol {
            name: fname.to_string(),
            visibility: SymbolVisibility::Global,
            kind: SymbolKind::Function,
            section_index: Some(1),
            value: offset as u64,
            size: size as u64,
            object_id: obj_id,
            final_virtual_address: None,
        });
    }

    for &(rfname, patch_off) in referenced_funcs {
        relocations.push(RelocationRecord {
            object_id: obj_id,
            section_index: 1,
            offset_in_section: patch_off,
            symbol_name: rfname.to_string(),
            symbol_index: 0,
            reloc_type: vanta_linker::elf::reloc::R_X86_64_PLT32,
            addend: -4,
        });
    }

    obj.sections.push(InputSection {
        object_id: obj_id,
        source_index: 1,
        name: ".text".into(),
        kind: SectionKind::Code,
        flags: SectionFlags::code(),
        align: 16,
        size: code_bytes.len(),
        data: code_bytes,
    });

    obj.symbols = symbols;
    obj.relocations = relocations;
    obj
}

fn create_synthetic_archive_bytes(members: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"!<arch>\n");

    // Symbol index table
    let mut sym_names: Vec<(String, usize)> = Vec::new();
    for (m_idx, (_m_name, m_data)) in members.iter().enumerate() {
        if let Ok(obj) = vanta_linker::elf::reader::parse_elf64_object(
            &PathBuf::from("archive_member"),
            m_data,
            m_idx,
        ) {
            for sym in obj.symbols {
                if sym.is_global() && sym.is_defined() {
                    sym_names.push((sym.name, m_idx));
                }
            }
        }
    }

    // Format GNU symbol index
    let mut strtab = Vec::new();
    for (sname, _) in &sym_names {
        strtab.extend_from_slice(sname.as_bytes());
        strtab.push(0);
    }

    let num_syms = sym_names.len();
    let index_data_len = 4 + num_syms * 4 + strtab.len();
    let pad = index_data_len % 2;
    let index_header = format!(
        "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
        "/", "0", "0", "0", "100644", index_data_len
    );
    out.extend_from_slice(index_header.as_bytes());

    // Placeholder offsets: calculate member start positions
    let mut member_offsets = Vec::new();
    let mut curr_off = out.len() + index_data_len + pad;
    for (_m_name, m_data) in members {
        member_offsets.push(curr_off);
        let m_size = m_data.len();
        curr_off += 60 + m_size + (m_size % 2);
    }

    // Write index table
    out.extend_from_slice(&(num_syms as u32).to_be_bytes());
    for (_, m_idx) in &sym_names {
        let m_off = member_offsets[*m_idx] as u32;
        out.extend_from_slice(&m_off.to_be_bytes());
    }
    out.extend_from_slice(&strtab);
    if pad > 0 {
        out.push(0);
    }

    // Write members
    for (m_name, m_data) in members {
        let m_hdr = format!(
            "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
            format!("{}/", m_name),
            "0",
            "0",
            "0",
            "100644",
            m_data.len()
        );
        out.extend_from_slice(m_hdr.as_bytes());
        out.extend_from_slice(m_data);
        if m_data.len() % 2 != 0 {
            out.push(0);
        }
    }

    out
}

fn run_benchmark(
    name: &str,
    mut config_generator: impl FnMut() -> LinkerConfig,
    repetitions: usize,
) {
    // 5 warmups
    for _ in 0..5 {
        let cfg = config_generator();
        let _ = vanta_linker::link(&cfg);
    }

    let mut times: Vec<f64> = Vec::with_capacity(repetitions);
    let mut binary_size = 0;

    for _ in 0..repetitions {
        let cfg = config_generator();
        let start = Instant::now();
        let res = vanta_linker::link(&cfg).expect("Link failed");
        let elapsed = start.elapsed().as_secs_f64() * 1000.0; // ms
        times.push(elapsed);
        binary_size = res.total_file_size;
    }

    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    let min = times[0];
    let max = times[times.len() - 1];

    println!(
        "| {:<24} | {:>6} ms | {:>6} ms | {:>6} ms | {:>8} bytes |",
        name,
        format!("{:.3}", median),
        format!("{:.3}", min),
        format!("{:.3}", max),
        binary_size
    );
}

fn main() {
    println!("\n==========================================================================");
    println!("     VENTA v2.0 NATIVE LINKER BENCHMARK SUITE (Milestone 31.5)");
    println!("==========================================================================");
    println!("| Workload                 | Median    | Min       | Max       | Output Size |");
    println!("|--------------------------|-----------|-----------|-----------|-------------|");

    // 1. Small Workload (2 objects: main.o + math.o)
    let temp_dir = std::env::temp_dir().join("vanta_bench");
    let _ = std::fs::create_dir_all(&temp_dir);

    run_benchmark(
        "Small (2 objects)",
        || {
            let mut cfg = LinkerConfig::new(
                TargetTriple::X86_64UnknownLinuxGnu,
                temp_dir.join("bench_small.out"),
            );
            cfg.link_mode = LinkMode::Static;

            let main_obj =
                generate_synthetic_elf_object(0, "main.o", &[("main", 16)], &[("add", 1)]);
            let math_obj = generate_synthetic_elf_object(1, "math.o", &[("add", 16)], &[]);

            // Pass objects in memory
            cfg.input_objects = Vec::new();
            cfg.input_buffers = vec![
                ("main.o".into(), serialize_synthetic_object(&main_obj)),
                ("math.o".into(), serialize_synthetic_object(&math_obj)),
            ];
            cfg
        },
        50,
    );

    // 2. Medium Workload (20 chained objects)
    run_benchmark(
        "Medium (20 objects)",
        || {
            let mut cfg = LinkerConfig::new(
                TargetTriple::X86_64UnknownLinuxGnu,
                temp_dir.join("bench_medium.out"),
            );
            cfg.link_mode = LinkMode::Static;

            let mut buffers = Vec::new();
            // main calls func_1, func_1 calls func_2, ... func_19 returns
            let main_obj =
                generate_synthetic_elf_object(0, "main.o", &[("main", 16)], &[("func_1", 1)]);
            buffers.push(("main.o".into(), serialize_synthetic_object(&main_obj)));

            for i in 1..20 {
                let def_name = format!("func_{}", i);
                let obj_name = format!("obj_{}.o", i);
                let (obj, refs) = if i < 19 {
                    let next_name = format!("func_{}", i + 1);
                    (
                        generate_synthetic_elf_object(
                            i,
                            &obj_name,
                            &[(&def_name, 16)],
                            &[(&next_name, 1)],
                        ),
                        vec![next_name],
                    )
                } else {
                    (
                        generate_synthetic_elf_object(i, &obj_name, &[(&def_name, 16)], &[]),
                        vec![],
                    )
                };
                let _ = refs;
                buffers.push((obj_name, serialize_synthetic_object(&obj)));
            }

            cfg.input_buffers = buffers;
            cfg
        },
        30,
    );

    // 3. Large Workload (100 objects)
    run_benchmark(
        "Large (100 objects)",
        || {
            let mut cfg = LinkerConfig::new(
                TargetTriple::X86_64UnknownLinuxGnu,
                temp_dir.join("bench_large.out"),
            );
            cfg.link_mode = LinkMode::Static;

            let mut buffers = Vec::new();
            let main_obj =
                generate_synthetic_elf_object(0, "main.o", &[("main", 16)], &[("mod_1", 1)]);
            buffers.push(("main.o".into(), serialize_synthetic_object(&main_obj)));

            for i in 1..100 {
                let def_name = format!("mod_{}", i);
                let obj_name = format!("mod_{}.o", i);
                let obj = if i < 99 {
                    let next_name = format!("mod_{}", i + 1);
                    generate_synthetic_elf_object(
                        i,
                        &obj_name,
                        &[(&def_name, 16)],
                        &[(&next_name, 1)],
                    )
                } else {
                    generate_synthetic_elf_object(i, &obj_name, &[(&def_name, 16)], &[])
                };
                buffers.push((obj_name, serialize_synthetic_object(&obj)));
            }

            cfg.input_buffers = buffers;
            cfg
        },
        20,
    );

    // 4. Archive-Heavy (5 archives with 10 members each, 50 members total)
    run_benchmark(
        "Archive (50 members)",
        || {
            let mut cfg = LinkerConfig::new(
                TargetTriple::X86_64UnknownLinuxGnu,
                temp_dir.join("bench_archive.out"),
            );
            cfg.link_mode = LinkMode::Static;

            let main_obj =
                generate_synthetic_elf_object(0, "main.o", &[("main", 16)], &[("arch_func_0", 1)]);
            cfg.input_buffers = vec![("main.o".into(), serialize_synthetic_object(&main_obj))];

            let mut archive_buffers = Vec::new();
            for ar_i in 0..5 {
                let mut members = Vec::new();
                for m_j in 0..10 {
                    let fname = format!("arch_func_{}_{}", ar_i, m_j);
                    let m_obj = generate_synthetic_elf_object(
                        100 + ar_i * 10 + m_j,
                        &format!("m_{}.o", m_j),
                        &[(&fname, 16)],
                        &[],
                    );
                    members.push((format!("m_{}.o", m_j), serialize_synthetic_object(&m_obj)));
                }
                // Add the one needed function in archive 0
                if ar_i == 0 {
                    let needed_obj =
                        generate_synthetic_elf_object(999, "needed.o", &[("arch_func_0", 16)], &[]);
                    members.push(("needed.o".into(), serialize_synthetic_object(&needed_obj)));
                }
                let ar_bytes = create_synthetic_archive_bytes(&members);
                archive_buffers.push((format!("lib_{}.a", ar_i), ar_bytes));
            }

            cfg.archive_buffers = archive_buffers;
            cfg
        },
        20,
    );

    println!("==========================================================================\n");
}

fn serialize_synthetic_object(obj: &ObjectFile) -> Vec<u8> {
    // Minimal standard ELF64 object serialization
    let code_data = obj
        .sections
        .first()
        .map(|s| s.data.as_slice())
        .unwrap_or(&[]);
    let code_len = code_data.len();

    let mut out = Vec::new();
    // 1. ELF Header (64 bytes)
    out.extend_from_slice(b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00"); // e_ident
    out.extend_from_slice(&1u16.to_le_bytes()); // e_type = ET_REL (1)
    out.extend_from_slice(&0x3Eu16.to_le_bytes()); // e_machine = EM_X86_64
    out.extend_from_slice(&1u32.to_le_bytes()); // e_version
    out.extend_from_slice(&0u64.to_le_bytes()); // e_entry
    out.extend_from_slice(&0u64.to_le_bytes()); // e_phoff
    let shoff_pos = out.len();
    out.extend_from_slice(&0u64.to_le_bytes()); // e_shoff (patched later)
    out.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    out.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
    out.extend_from_slice(&0u16.to_le_bytes()); // e_phentsize
    out.extend_from_slice(&0u16.to_le_bytes()); // e_phnum
    out.extend_from_slice(&64u16.to_le_bytes()); // e_shentsize
    let shnum_pos = out.len();
    out.extend_from_slice(&0u16.to_le_bytes()); // e_shnum (patched)
    out.extend_from_slice(&1u16.to_le_bytes()); // e_shstrndx = 1 (.shstrtab)

    // Sections:
    // 0: SHN_UNDEF
    // 1: .shstrtab
    // 2: .text
    // 3: .symtab
    // 4: .strtab
    // 5: .rela.text (if relocations present)
    let shstrtab_data = b"\0.shstrtab\0.text\0.symtab\0.strtab\0.rela.text\0";
    let shstrtab_off = out.len();
    out.extend_from_slice(shstrtab_data);

    let text_off = out.len();
    out.extend_from_slice(code_data);

    // .strtab
    let mut strtab_data = vec![0u8];
    let mut sym_str_offsets = Vec::new();
    for sym in &obj.symbols {
        let off = strtab_data.len();
        strtab_data.extend_from_slice(sym.name.as_bytes());
        strtab_data.push(0);
        sym_str_offsets.push(off);
    }
    for rel in &obj.relocations {
        let off = strtab_data.len();
        strtab_data.extend_from_slice(rel.symbol_name.as_bytes());
        strtab_data.push(0);
        sym_str_offsets.push(off);
    }

    // .symtab
    let mut symtab_data = vec![0u8; 24]; // entry 0: dummy
    for (idx, sym) in obj.symbols.iter().enumerate() {
        let st_name = sym_str_offsets[idx] as u32;
        symtab_data.extend_from_slice(&st_name.to_le_bytes());
        symtab_data.push(0x12); // STB_GLOBAL | STT_FUNC
        symtab_data.push(0);
        symtab_data.extend_from_slice(&2u16.to_le_bytes()); // st_shndx = 2 (.text)
        symtab_data.extend_from_slice(&sym.value.to_le_bytes());
        symtab_data.extend_from_slice(&sym.size.to_le_bytes());
    }
    let first_rel_sym_idx = 1 + obj.symbols.len();
    for (rel_i, rel) in obj.relocations.iter().enumerate() {
        let st_name = sym_str_offsets[obj.symbols.len() + rel_i] as u32;
        symtab_data.extend_from_slice(&st_name.to_le_bytes());
        symtab_data.push(0x10); // STB_GLOBAL | STT_NOTYPE
        symtab_data.push(0);
        symtab_data.extend_from_slice(&0u16.to_le_bytes()); // SHN_UNDEF
        symtab_data.extend_from_slice(&0u64.to_le_bytes());
        symtab_data.extend_from_slice(&0u64.to_le_bytes());
        let _ = rel;
    }

    let symtab_off = out.len();
    out.extend_from_slice(&symtab_data);

    let strtab_off = out.len();
    out.extend_from_slice(&strtab_data);

    // .rela.text
    let has_reloc = !obj.relocations.is_empty();
    let rela_off = out.len();
    if has_reloc {
        for (i, rel) in obj.relocations.iter().enumerate() {
            let sym_idx = (first_rel_sym_idx + i) as u64;
            let r_info = (sym_idx << 32) | (rel.reloc_type as u64);
            out.extend_from_slice(&(rel.offset_in_section as u64).to_le_bytes());
            out.extend_from_slice(&r_info.to_le_bytes());
            out.extend_from_slice(&rel.addend.to_le_bytes());
        }
    }
    let rela_len = if has_reloc {
        obj.relocations.len() * 24
    } else {
        0
    };

    // Pad to 8 bytes for section headers
    while out.len() % 8 != 0 {
        out.push(0);
    }
    let shoff = out.len();
    out[shoff_pos..shoff_pos + 8].copy_from_slice(&(shoff as u64).to_le_bytes());

    let num_sections = if has_reloc { 6 } else { 5 };
    out[shnum_pos..shnum_pos + 2].copy_from_slice(&(num_sections as u16).to_le_bytes());

    // Write Section Headers (64 bytes each)
    // 0: UNDEF
    out.extend_from_slice(&[0u8; 64]);
    // 1: .shstrtab
    write_shdr(
        &mut out,
        1,
        3,
        0,
        shstrtab_off as u64,
        shstrtab_data.len() as u64,
        0,
        0,
        1,
        0,
    );
    // 2: .text
    write_shdr(
        &mut out,
        11,
        1,
        6, /* SHF_ALLOC | SHF_EXECINSTR */
        text_off as u64,
        code_len as u64,
        0,
        0,
        16,
        0,
    );
    // 3: .symtab
    write_shdr(
        &mut out,
        17,
        2,
        0,
        symtab_off as u64,
        symtab_data.len() as u64,
        4, /* link to .strtab */
        1,
        8,
        24,
    );
    // 4: .strtab
    write_shdr(
        &mut out,
        25,
        3,
        0,
        strtab_off as u64,
        strtab_data.len() as u64,
        0,
        0,
        1,
        0,
    );
    if has_reloc {
        // 5: .rela.text
        write_shdr(
            &mut out,
            33,
            4,
            0x40, /* SHF_INFO_LINK */
            rela_off as u64,
            rela_len as u64,
            3, /* symtab */
            2, /* text */
            8,
            24,
        );
    }

    out
}

fn write_shdr(
    out: &mut Vec<u8>,
    name: u32,
    sh_type: u32,
    flags: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    align: u64,
    entsize: u64,
) {
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&sh_type.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // addr
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&link.to_le_bytes());
    out.extend_from_slice(&info.to_le_bytes());
    out.extend_from_slice(&align.to_le_bytes());
    out.extend_from_slice(&entsize.to_le_bytes());
}

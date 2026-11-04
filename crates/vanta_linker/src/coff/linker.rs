//! Windows AMD64 PE/COFF to PE32+ executable linker coordinator.

use crate::archive::{extract_required_members, Archive, ArchiveMember};
use crate::coff::pe_writer::{
    PeSectionDesc, PeWriter, IMAGE_REL_BASED_DIR64, IMAGE_SCN_CNT_CODE,
    IMAGE_SCN_CNT_INITIALIZED_DATA, IMAGE_SCN_MEM_DISCARDABLE, IMAGE_SCN_MEM_EXECUTE,
    IMAGE_SCN_MEM_READ, IMAGE_SCN_MEM_WRITE,
};
use crate::coff::reader::read_coff_object;
use crate::coff::reloc::apply_coff_relocation;
use crate::config::LinkerConfig;
use crate::error::{LinkResult, LinkerError};
use crate::map_file::generate_link_map;
use crate::object::ObjectFile;
use crate::resolve::SymbolResolver;
use crate::section::{MergedSection, SectionFlags, SectionKind};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

/// Outcome of a successful PE32+ link session.
#[derive(Debug, Clone)]
pub struct CoffLinkOutput {
    pub output_path: PathBuf,
    pub entry_point_rva: u32,
    pub total_file_size: usize,
    pub input_objects_count: usize,
    pub extracted_archive_members_count: usize,
    pub map_content: Option<String>,
}

/// Links multiple AMD64 COFF relocatable objects (.obj) and archives (.lib) into a PE32+ executable.
pub fn link_coff_executable(config: &LinkerConfig) -> LinkResult<CoffLinkOutput> {
    let mut objects: Vec<ObjectFile> = Vec::new();
    let mut next_obj_id = 0;

    // 1. Load explicit input object files
    for path in &config.input_objects {
        let bytes = fs::read(path).map_err(|e| LinkerError::Io {
            path: path.clone(),
            message: format!("Failed to read input object: {}", e),
        })?;
        let obj = read_coff_object(
            next_obj_id,
            path.display().to_string(),
            path.clone(),
            &bytes,
        )?;
        next_obj_id += 1;
        objects.push(obj);
    }

    // 2. Load explicit in-memory input buffers
    for (name, bytes) in &config.input_buffers {
        let obj = read_coff_object(
            next_obj_id,
            name.clone(),
            PathBuf::from(format!("memory://{}", name)),
            bytes,
        )?;
        next_obj_id += 1;
        objects.push(obj);
    }

    // 3. Load static archives (.lib)
    let mut archives: Vec<Archive> = Vec::new();
    for path in &config.archives {
        let bytes = fs::read(path).map_err(|e| LinkerError::Io {
            path: path.clone(),
            message: format!("Failed to read archive: {}", e),
        })?;
        let ar = Archive::parse(path, &bytes)?;
        archives.push(ar);
    }

    for (name, bytes) in &config.archive_buffers {
        let p = PathBuf::from(format!("memory://{}", name));
        let ar = Archive::parse(&p, bytes)?;
        archives.push(ar);
    }

    // 4. Lazy archive extraction
    let mut extracted_members: Vec<ArchiveMember> = Vec::new();
    if !archives.is_empty() {
        let mut defined_symbols: HashSet<String> = HashSet::new();
        let mut referenced_symbols: HashSet<String> = HashSet::new();

        for obj in &objects {
            for sym in &obj.symbols {
                if sym.is_defined() && !sym.is_local() {
                    defined_symbols.insert(sym.name.clone());
                } else if sym.is_undefined() {
                    referenced_symbols.insert(sym.name.clone());
                }
            }
            for rel in &obj.relocations {
                if !defined_symbols.contains(&rel.symbol_name) {
                    referenced_symbols.insert(rel.symbol_name.clone());
                }
            }
        }

        let mut unresolved: HashSet<String> = referenced_symbols
            .difference(&defined_symbols)
            .cloned()
            .collect();

        extract_required_members(&archives, &mut unresolved, &mut extracted_members);

        for member in &extracted_members {
            let obj = read_coff_object(
                next_obj_id,
                member.name.clone(),
                PathBuf::from(format!("archive://{}", member.name)),
                &member.data,
            )?;
            next_obj_id += 1;
            objects.push(obj);
        }
    }

    // 5. Global symbol resolution
    let mut resolver = SymbolResolver::new();
    resolver.register_definitions(&objects)?;

    // 5b. Collect unresolved external symbols and determine providing DLLs
    let mut unresolved_external_symbols: HashSet<String> = HashSet::new();
    for obj in &objects {
        for rel in &obj.relocations {
            let is_local = obj
                .symbols
                .iter()
                .any(|s| s.is_local() && s.name == rel.symbol_name);
            let is_sec = obj.sections.iter().any(|s| s.name == rel.symbol_name);
            let is_global = resolver.strong_symbols.contains_key(&rel.symbol_name)
                || resolver.weak_symbols.contains_key(&rel.symbol_name);
            if !is_local && !is_sec && !is_global {
                unresolved_external_symbols.insert(rel.symbol_name.clone());
            }
        }
    }

    let mut dll_map: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();
    for sym in &unresolved_external_symbols {
        if let Some(dll) = lookup_dll_for_symbol(sym) {
            let fn_name = sym.strip_prefix("__imp_").unwrap_or(sym).to_string();
            dll_map.entry(dll).or_default().insert(fn_name);
        } else {
            for obj in &objects {
                for rel in &obj.relocations {
                    if rel.symbol_name == *sym {
                        return Err(LinkerError::UndefinedSymbol {
                            symbol: sym.clone(),
                            referencing_file: obj.path.clone(),
                            referencing_section: rel.section_index.to_string(),
                        });
                    }
                }
            }
            return Err(LinkerError::UndefinedSymbol {
                symbol: sym.clone(),
                referencing_file: PathBuf::from("linker"),
                referencing_section: "import".into(),
            });
        }
    }

    if dll_map.is_empty() {
        let mut default_kernel32 = BTreeSet::new();
        default_kernel32.insert("ExitProcess".into());
        default_kernel32.insert("GetStdHandle".into());
        default_kernel32.insert("WriteFile".into());
        dll_map.insert("KERNEL32.dll", default_kernel32);
    }

    // 6. Section merging
    let mut text_merged =
        MergedSection::new(".text".into(), SectionKind::Code, SectionFlags::code());
    let mut rdata_merged = MergedSection::new(
        ".rdata".into(),
        SectionKind::ReadOnlyData,
        SectionFlags::readonly(),
    );
    let mut data_merged =
        MergedSection::new(".data".into(), SectionKind::Data, SectionFlags::readwrite());
    let mut bss_merged =
        MergedSection::new(".bss".into(), SectionKind::Bss, SectionFlags::readwrite());
    let mut pdata_merged = MergedSection::new(
        ".pdata".into(),
        SectionKind::UnwindPdata,
        SectionFlags::readonly(),
    );
    let mut xdata_merged = MergedSection::new(
        ".xdata".into(),
        SectionKind::UnwindXdata,
        SectionFlags::readonly(),
    );

    for obj in &objects {
        for sec in &obj.sections {
            match sec.kind {
                SectionKind::Code => {
                    text_merged.append_contribution(sec);
                }
                SectionKind::ReadOnlyData => {
                    rdata_merged.append_contribution(sec);
                }
                SectionKind::Data => {
                    data_merged.append_contribution(sec);
                }
                SectionKind::Bss => {
                    bss_merged.append_contribution(sec);
                }
                SectionKind::UnwindPdata => {
                    pdata_merged.append_contribution(sec);
                }
                SectionKind::UnwindXdata => {
                    xdata_merged.append_contribution(sec);
                }
                _ => {}
            }
        }
    }

    // Append import thunks to .text
    let mut thunks: Vec<ThunkInfo> = Vec::new();
    for (dll, funcs) in &dll_map {
        for func in funcs {
            let thunk_offset = text_merged.data.len();
            text_merged
                .data
                .extend_from_slice(&[0xFF, 0x25, 0x00, 0x00, 0x00, 0x00, 0x90, 0x90]);
            thunks.push(ThunkInfo {
                dll,
                name: func.clone(),
                thunk_offset,
            });
        }
    }

    let mut merged_sections: Vec<MergedSection> = Vec::new();
    if !text_merged.contributions.is_empty() || !text_merged.data.is_empty() {
        merged_sections.push(text_merged);
    }
    if !rdata_merged.contributions.is_empty() {
        merged_sections.push(rdata_merged);
    }
    if !data_merged.contributions.is_empty() {
        merged_sections.push(data_merged);
    }
    if !bss_merged.contributions.is_empty() {
        merged_sections.push(bss_merged);
    }
    if !pdata_merged.contributions.is_empty() {
        merged_sections.push(pdata_merged);
    }
    if !xdata_merged.contributions.is_empty() {
        merged_sections.push(xdata_merged);
    }

    // 7. Calculate RVAs and Virtual Addresses for each section
    // SectionAlignment = 4096, FileAlignment = 512
    let image_base = PeWriter::DEFAULT_IMAGE_BASE;
    let mut current_rva = 0x1000u32; // First section starts at RVA 0x1000

    for sec in &mut merged_sections {
        sec.virtual_address = image_base + current_rva as u64;
        sec.file_offset = current_rva as u64; // will be aligned by writer
        let sec_len = if sec.kind.is_zero_init() {
            sec.size
        } else {
            sec.data.len()
        };
        current_rva = (current_rva + sec_len as u32 + 0xFFF) & !0xFFF;
    }

    let text_rva = if !merged_sections.is_empty() && merged_sections[0].name == ".text" {
        (merged_sections[0]
            .virtual_address
            .saturating_sub(image_base)) as u32
    } else {
        0x1000
    };

    // 8. Symbol virtual address assignment
    resolver.compute_symbol_addresses(&objects, &merged_sections);

    // 9. Entry point lookup
    let entry_name = config.resolved_entry_symbol();
    let entry_va = if let Some(def) = resolver.find_definition(entry_name, &objects) {
        resolver.resolve_symbol_address(entry_name, def.0, &merged_sections)
    } else if let Some(def) = resolver.find_definition("main", &objects) {
        // Fall back to "main" if "_start" not explicitly provided
        resolver.resolve_symbol_address("main", def.0, &merged_sections)
    } else {
        None
    };

    let entry_va = entry_va.ok_or_else(|| LinkerError::UndefinedSymbol {
        symbol: entry_name.to_string(),
        referencing_file: PathBuf::from("linker"),
        referencing_section: "entry".into(),
    })?;

    let entry_rva = (entry_va.saturating_sub(image_base)) as u32;

    // 10. Synthesize dynamic .idata section
    let (idata_bytes, idata_rva, idata_size, iat_rva, iat_size) = build_dynamic_idata_section(
        current_rva,
        &dll_map,
        image_base,
        text_rva,
        &thunks,
        &mut merged_sections[0].data,
        &mut resolver,
    );
    current_rva = (current_rva + idata_bytes.len() as u32 + 0xFFF) & !0xFFF;

    // 11. Apply COFF Relocations
    let mut base_reloc_rvas: Vec<u32> = Vec::new();
    let contrib_map = crate::section::ContributionMap::build(&merged_sections);

    for obj in &objects {
        for rel in &obj.relocations {
            let sym_va = resolver
                .resolve_symbol_address_with_map(
                    &rel.symbol_name,
                    obj,
                    &merged_sections,
                    &contrib_map,
                )
                .ok_or_else(|| LinkerError::UndefinedSymbol {
                    symbol: rel.symbol_name.clone(),
                    referencing_file: obj.path.clone(),
                    referencing_section: rel.section_index.to_string(),
                })?;

            if let Some(loc) = contrib_map.get_by_index(obj.id, rel.section_index) {
                let merged_sec = &mut merged_sections[loc.merged_section_idx];
                let patch_offset = loc.offset_in_merged + rel.offset_in_section;
                let place_va = merged_sec.virtual_address + patch_offset as u64;

                apply_coff_relocation(
                    &obj.path,
                    &merged_sec.name,
                    rel,
                    patch_offset,
                    &mut merged_sec.data,
                    sym_va,
                    place_va,
                    image_base,
                )?;

                // If 64-bit absolute address, record for .reloc base relocations
                if rel.reloc_type == 1 {
                    let rva = (place_va.saturating_sub(image_base)) as u32;
                    base_reloc_rvas.push(rva);
                }
            }
        }
    }

    // 12. Synthesize .reloc section (Base Relocations)
    let (reloc_bytes, reloc_rva, reloc_size) = build_reloc_section(current_rva, &base_reloc_rvas);

    // 13. Build final PeSectionDesc list
    let mut pe_sections = Vec::new();
    let mut pdata_rva_and_size = None;

    for sec in merged_sections {
        let rva = (sec.virtual_address.saturating_sub(image_base)) as u32;
        let v_size = if sec.kind.is_zero_init() {
            sec.size as u32
        } else {
            sec.data.len() as u32
        };

        let chars = match sec.kind {
            SectionKind::Code => IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_MEM_READ,
            SectionKind::ReadOnlyData | SectionKind::UnwindPdata | SectionKind::UnwindXdata => {
                IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ
            }
            SectionKind::Data => {
                IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ | IMAGE_SCN_MEM_WRITE
            }
            SectionKind::Bss => {
                IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ | IMAGE_SCN_MEM_WRITE
            }
            _ => IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ,
        };

        if sec.name == ".pdata" {
            pdata_rva_and_size = Some((rva, v_size));
        }

        pe_sections.push(PeSectionDesc {
            name: sec.name,
            virtual_size: v_size,
            virtual_address: rva,
            data: sec.data,
            characteristics: chars,
        });
    }

    // Add .idata section
    if !idata_bytes.is_empty() {
        pe_sections.push(PeSectionDesc {
            name: ".idata".into(),
            virtual_size: idata_size,
            virtual_address: idata_rva,
            data: idata_bytes,
            characteristics: IMAGE_SCN_CNT_INITIALIZED_DATA
                | IMAGE_SCN_MEM_READ
                | IMAGE_SCN_MEM_WRITE,
        });
    }

    // Add .reloc section
    if !reloc_bytes.is_empty() {
        pe_sections.push(PeSectionDesc {
            name: ".reloc".into(),
            virtual_size: reloc_size,
            virtual_address: reloc_rva,
            data: reloc_bytes,
            characteristics: IMAGE_SCN_CNT_INITIALIZED_DATA
                | IMAGE_SCN_MEM_READ
                | IMAGE_SCN_MEM_DISCARDABLE,
        });
    }

    // 14. Emit PE32+ image
    let binary_bytes = PeWriter::write_image(
        entry_rva,
        pe_sections,
        pdata_rva_and_size,
        Some((idata_rva, idata_size)),
        Some((iat_rva, iat_size)),
        Some((reloc_rva, reloc_size)),
    );

    // 15. Write to output file
    if let Some(parent) = config.output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| LinkerError::Io {
                path: parent.to_path_buf(),
                message: format!("Failed to create output directory: {}", e),
            })?;
        }
    }

    fs::write(&config.output_path, &binary_bytes).map_err(|e| LinkerError::Io {
        path: config.output_path.clone(),
        message: format!("Failed to write PE output: {}", e),
    })?;

    // 16. Optional link map
    let mut map_content = None;
    if let Some(ref map_path) = config.map_file {
        let dummy_merged = Vec::new();
        let map_str = generate_link_map(
            &objects,
            &extracted_members,
            &[],
            &dummy_merged,
            &resolver.resolved_addresses,
            entry_va,
            entry_name,
        );
        fs::write(map_path, &map_str).map_err(|e| LinkerError::Io {
            path: map_path.clone(),
            message: format!("Failed to write map file: {}", e),
        })?;
        map_content = Some(map_str);
    }

    Ok(CoffLinkOutput {
        output_path: config.output_path.clone(),
        entry_point_rva: entry_rva,
        total_file_size: binary_bytes.len(),
        input_objects_count: objects.len(),
        extracted_archive_members_count: extracted_members.len(),
        map_content,
    })
}

#[derive(Debug, Clone)]
pub struct ThunkInfo {
    pub dll: &'static str,
    pub name: String,
    pub thunk_offset: usize,
}

pub fn lookup_dll_for_symbol(sym: &str) -> Option<&'static str> {
    let name = sym.strip_prefix("__imp_").unwrap_or(sym);
    match name {
        // KERNEL32.dll
        "ExitProcess"
        | "GetStdHandle"
        | "WriteFile"
        | "ReadFile"
        | "CloseHandle"
        | "CreateFileA"
        | "CreateFileW"
        | "GetLastError"
        | "SetLastError"
        | "GetProcAddress"
        | "LoadLibraryA"
        | "LoadLibraryW"
        | "FreeLibrary"
        | "GetModuleHandleA"
        | "GetModuleHandleW"
        | "GetCurrentProcess"
        | "GetCurrentThread"
        | "GetCurrentProcessId"
        | "GetCurrentThreadId"
        | "Sleep"
        | "VirtualAlloc"
        | "VirtualFree"
        | "VirtualProtect"
        | "QueryPerformanceCounter"
        | "QueryPerformanceFrequency"
        | "GetSystemTimeAsFileTime"
        | "InitializeCriticalSection"
        | "DeleteCriticalSection"
        | "EnterCriticalSection"
        | "LeaveCriticalSection"
        | "CreateEventA"
        | "SetEvent"
        | "ResetEvent"
        | "WaitForSingleObject"
        | "MultiByteToWideChar"
        | "WideCharToMultiByte"
        | "FormatMessageA"
        | "LocalFree"
        | "TlsAlloc"
        | "TlsGetValue"
        | "TlsSetValue"
        | "TlsFree"
        | "CreateThread"
        | "TerminateProcess"
        | "GetCommandLineA"
        | "GetCommandLineW" => Some("KERNEL32.dll"),

        // ws2_32.dll
        "WSAStartup" | "WSACleanup" | "WSAGetLastError" | "socket" | "closesocket" | "bind"
        | "listen" | "accept" | "connect" | "send" | "recv" | "sendto" | "recvfrom" | "select"
        | "ioctlsocket" | "gethostbyname" | "getaddrinfo" | "freeaddrinfo" | "setsockopt"
        | "getsockopt" | "htons" | "ntohs" | "htonl" | "ntohl" | "inet_addr" | "inet_ntoa"
        | "__WSAFDIsSet" => Some("ws2_32.dll"),

        // msvcrt.dll (C runtime)
        "malloc"
        | "free"
        | "calloc"
        | "realloc"
        | "memcpy"
        | "memmove"
        | "memset"
        | "memcmp"
        | "memchr"
        | "printf"
        | "sprintf"
        | "snprintf"
        | "vsnprintf"
        | "puts"
        | "putchar"
        | "getchar"
        | "strlen"
        | "strcpy"
        | "strncpy"
        | "strcmp"
        | "strncmp"
        | "strcat"
        | "strncat"
        | "strchr"
        | "strrchr"
        | "strstr"
        | "strdup"
        | "strtol"
        | "strtoll"
        | "strtoul"
        | "strtoull"
        | "strtod"
        | "atof"
        | "atoi"
        | "atol"
        | "sin"
        | "cos"
        | "tan"
        | "asin"
        | "acos"
        | "atan"
        | "atan2"
        | "sinh"
        | "cosh"
        | "tanh"
        | "exp"
        | "log"
        | "log10"
        | "log2"
        | "pow"
        | "sqrt"
        | "ceil"
        | "floor"
        | "round"
        | "fabs"
        | "fmod"
        | "modf"
        | "fopen"
        | "fclose"
        | "fread"
        | "fwrite"
        | "fseek"
        | "ftell"
        | "rewind"
        | "fflush"
        | "feof"
        | "ferror"
        | "fgets"
        | "fputs"
        | "fprintf"
        | "fscanf"
        | "sscanf"
        | "remove"
        | "rename"
        | "exit"
        | "_exit"
        | "abort"
        | "getenv"
        | "system"
        | "time"
        | "clock"
        | "difftime"
        | "gmtime"
        | "localtime"
        | "strftime"
        | "_write"
        | "_read"
        | "_close"
        | "_open"
        | "_lseek"
        | "_isatty"
        | "_fileno"
        | "_errno"
        | "__acrt_iob_func"
        | "__stdio_common_vfprintf"
        | "__stdio_common_vsprintf"
        | "__chkstk"
        | "_fstat"
        | "_fstat64"
        | "_fstati64"
        | "_setmode"
        | "qsort"
        | "bsearch" => Some("msvcrt.dll"),

        _ => None,
    }
}

/// Builds `.idata` section dynamically for the required imported DLLs and functions.
pub fn build_dynamic_idata_section(
    start_rva: u32,
    dll_map: &BTreeMap<&'static str, BTreeSet<String>>,
    image_base: u64,
    text_rva: u32,
    thunks: &[ThunkInfo],
    text_data: &mut [u8],
    resolver: &mut SymbolResolver,
) -> (Vec<u8>, u32, u32, u32, u32) {
    let mut idata = Vec::new();
    let num_dlls = dll_map.len();
    if num_dlls == 0 {
        return (Vec::new(), start_rva, 0, 0, 0);
    }

    // 1. Import Directory Table (20 bytes per DLL, terminated by 20 zero bytes)
    let dir_table_size = (num_dlls + 1) * 20;
    idata.resize(dir_table_size, 0);

    // 2. Compute ILT and IAT sizes & RVAs
    let mut ilt_rvas = Vec::new();
    let mut iat_rvas = Vec::new();
    let mut dll_order = Vec::new();

    // ILTs
    for (dll, funcs) in dll_map {
        dll_order.push((*dll, funcs.iter().cloned().collect::<Vec<_>>()));
        let ilt_offset = idata.len();
        ilt_rvas.push(start_rva + ilt_offset as u32);
        let ilt_size = (funcs.len() + 1) * 8;
        idata.resize(ilt_offset + ilt_size, 0);
    }

    // IATs
    let iat_start_offset = idata.len();
    let iat_start_rva = start_rva + iat_start_offset as u32;
    for (_dll, funcs) in &dll_order {
        let iat_offset = idata.len();
        iat_rvas.push(start_rva + iat_offset as u32);
        let iat_size = (funcs.len() + 1) * 8;
        idata.resize(iat_offset + iat_size, 0);
    }
    let total_iat_size = (idata.len() - iat_start_offset) as u32;

    // 3. DLL Name Strings
    let mut dll_name_rvas = Vec::new();
    for (dll, _) in &dll_order {
        let name_offset = idata.len();
        dll_name_rvas.push(start_rva + name_offset as u32);
        idata.extend_from_slice(dll.as_bytes());
        idata.push(0);
        if idata.len() % 2 != 0 {
            idata.push(0);
        }
    }

    // 4. Hint/Name Table Entries & Wiring ILT/IAT
    let mut ilt_cursor = dir_table_size;
    let mut iat_cursor = iat_start_offset;
    let mut func_iat_rva: HashMap<String, u32> = HashMap::new();

    for (dll_idx, (_dll, funcs)) in dll_order.iter().enumerate() {
        for (f_idx, func) in funcs.iter().enumerate() {
            let hn_offset = idata.len();
            let hn_rva = start_rva + hn_offset as u32;

            idata.extend_from_slice(&0u16.to_le_bytes()); // hint = 0
            idata.extend_from_slice(func.as_bytes());
            idata.push(0);
            if idata.len() % 2 != 0 {
                idata.push(0);
            }

            let entry_val = hn_rva as u64; // bit 63 = 0 -> import by name
            let ilt_pos = ilt_cursor + f_idx * 8;
            let iat_pos = iat_cursor + f_idx * 8;

            idata[ilt_pos..ilt_pos + 8].copy_from_slice(&entry_val.to_le_bytes());
            idata[iat_pos..iat_pos + 8].copy_from_slice(&entry_val.to_le_bytes());

            let iat_entry_rva = iat_rvas[dll_idx] + (f_idx * 8) as u32;
            func_iat_rva.insert(func.clone(), iat_entry_rva);
        }

        ilt_cursor += (funcs.len() + 1) * 8;
        iat_cursor += (funcs.len() + 1) * 8;
    }

    // 5. Populate Directory Table entries
    for (i, (_dll, _)) in dll_order.iter().enumerate() {
        let dir = &mut idata[i * 20..(i + 1) * 20];
        dir[0..4].copy_from_slice(&ilt_rvas[i].to_le_bytes()); // OriginalFirstThunk (ILT)
        dir[4..8].copy_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
        dir[8..12].copy_from_slice(&0u32.to_le_bytes()); // ForwarderChain
        dir[12..16].copy_from_slice(&dll_name_rvas[i].to_le_bytes()); // Name RVA
        dir[16..20].copy_from_slice(&iat_rvas[i].to_le_bytes()); // FirstThunk (IAT)
    }

    // 6. Register symbol addresses and patch trampolines
    for thunk in thunks {
        if let Some(&iat_entry_rva) = func_iat_rva.get(&thunk.name) {
            let iat_entry_va = image_base + iat_entry_rva as u64;
            resolver
                .resolved_addresses
                .insert(format!("__imp_{}", thunk.name), iat_entry_va);

            let thunk_rva = text_rva + thunk.thunk_offset as u32;
            let thunk_va = image_base + thunk_rva as u64;
            resolver
                .resolved_addresses
                .insert(thunk.name.clone(), thunk_va);

            // Patch disp32 in text_data: rip at end of instruction is thunk_rva + 6
            let disp32 = (iat_entry_rva as i64) - ((thunk_rva + 6) as i64);
            let disp32_bytes = (disp32 as i32).to_le_bytes();
            if thunk.thunk_offset + 6 <= text_data.len() {
                text_data[thunk.thunk_offset + 2..thunk.thunk_offset + 6]
                    .copy_from_slice(&disp32_bytes);
            }
        }
    }

    // Also register __imp_ for functions without trampolines
    for (fname, &iat_entry_rva) in &func_iat_rva {
        let iat_entry_va = image_base + iat_entry_rva as u64;
        resolver
            .resolved_addresses
            .entry(format!("__imp_{}", fname))
            .or_insert(iat_entry_va);
    }

    let total_size = idata.len() as u32;
    (idata, start_rva, total_size, iat_start_rva, total_iat_size)
}

/// Builds `.reloc` section with base relocation blocks.
fn build_reloc_section(start_rva: u32, rvas: &[u32]) -> (Vec<u8>, u32, u32) {
    if rvas.is_empty() {
        // Dummy 8-byte empty block
        let mut empty = Vec::new();
        empty.extend_from_slice(&start_rva.to_le_bytes()); // Page RVA
        empty.extend_from_slice(&8u32.to_le_bytes()); // Block Size
        let sz = empty.len() as u32;
        return (empty, start_rva, sz);
    }

    // Group RVAs by 4KB page (RVA & !0xFFF)
    let mut pages: HashMap<u32, Vec<u16>> = HashMap::new();
    for &rva in rvas {
        let page = rva & !0xFFF;
        let offset = (rva & 0xFFF) as u16;
        pages.entry(page).or_default().push(offset);
    }

    let mut sorted_pages: Vec<u32> = pages.keys().copied().collect();
    sorted_pages.sort_unstable();

    let mut out = Vec::new();
    for page in sorted_pages {
        let mut offsets = pages.remove(&page).unwrap();
        offsets.sort_unstable();

        // Each entry is 2 bytes: type (4 bits) | offset (12 bits)
        // IMAGE_REL_BASED_DIR64 = 10 (0xA)
        let block_entries_len = offsets.len();
        let pad = !block_entries_len.is_multiple_of(2); // Each block must be 4-byte aligned
        let block_size = 8 + (block_entries_len + if pad { 1 } else { 0 }) * 2;

        out.extend_from_slice(&page.to_le_bytes());
        out.extend_from_slice(&(block_size as u32).to_le_bytes());

        for off in offsets {
            let entry = (IMAGE_REL_BASED_DIR64 << 12) | (off & 0x0FFF);
            out.extend_from_slice(&entry.to_le_bytes());
        }

        if pad {
            out.extend_from_slice(&0u16.to_le_bytes()); // IMAGE_REL_BASED_ABSOLUTE padding
        }
    }

    let total_size = out.len() as u32;
    (out, start_rva, total_size)
}

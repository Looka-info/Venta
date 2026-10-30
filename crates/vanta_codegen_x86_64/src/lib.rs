//! Custom x86-64 native code generator, ELF64, and COFF object writers for Vanta.

pub mod abi;
pub mod coff_writer;
pub mod dwarf;
pub mod elf_writer;
pub mod encoding;
pub mod frame;
pub mod instruction_select;
pub mod machine_ir;
pub mod reg_alloc;
pub mod register;
pub mod simd_alloc;
pub mod target;

use crate::dwarf::{
    DwarfAbbrevTable, DwarfInfoWriter, DwarfLineTable, DwarfRelocTarget, FunctionDebugInfo,
    VariableDebugInfo,
};
use crate::elf_writer::DebugSections;
use crate::encoding::Relocation;
pub use abi::{classify_type, AbiClass};
pub use coff_writer::CoffWriter;
pub use elf_writer::{Elf64Writer, FunctionSymbol, RodataSymbol};
pub use encoding::{CodeEmitter, CodegenError};
pub use frame::FrameLayout;
pub use instruction_select::InstructionSelector;
pub use machine_ir::{MachineInst, Operand, SimdOperand};
pub use reg_alloc::{AllocationResult, Location, RegisterAllocator};
pub use register::{Reg, SimdReg, VectorWidth};
pub use simd_alloc::{
    SimdAllocResult, SimdLiveInterval, SimdLocation, SimdRegisterAllocator, SimdValueId,
};
use std::collections::HashMap;
pub use target::TargetFeatures;
use vanta_mir::{Instruction, MirProgram};
use vanta_sema::Type;
use vanta_target::{ObjectFormat, TargetAbi, TargetSpec};

pub struct EmittedProgram {
    pub text_bytes: Vec<u8>,
    pub rodata_bytes: Vec<u8>,
    pub text_relocations: Vec<Relocation>,
    pub rodata_relocations: Vec<Relocation>,
    pub functions: Vec<FunctionSymbol>,
    pub rodata_symbols: Vec<RodataSymbol>,
    pub func_debug_infos: Vec<FunctionDebugInfo>,
    pub line_entries: Vec<crate::encoding::LineEntry>,
}

pub struct X86_64Backend;

impl X86_64Backend {
    /// Compiles a MIR program into a valid Linux ELF64 relocatable object file (.o) byte buffer,
    /// with physical register allocation enabled by default on baseline target.
    pub fn compile_to_elf(program: &MirProgram) -> Vec<u8> {
        Self::compile_to_elf_with_options(program, true)
    }

    /// Compiles a MIR program with specified target features.
    pub fn compile_to_elf_with_target(
        program: &MirProgram,
        target_features: TargetFeatures,
    ) -> Result<Vec<u8>, CodegenError> {
        Self::compile_to_elf_full_with_target(program, true, true, true, true, target_features)
    }

    /// Compiles a MIR program into an ELF64 object with an explicit register allocation flag.
    pub fn compile_to_elf_with_options(program: &MirProgram, enable_reg_alloc: bool) -> Vec<u8> {
        Self::compile_to_elf_full(program, enable_reg_alloc, true, true, true)
    }

    /// Compiles a MIR program into an ELF64 object with fine-grained backend controls:
    /// register allocation, sibling tail-calls, branch-aware code layout, and loop code alignment.
    pub fn compile_to_elf_full(
        program: &MirProgram,
        enable_reg_alloc: bool,
        enable_tail_calls: bool,
        enable_block_layout: bool,
        enable_code_align: bool,
    ) -> Vec<u8> {
        Self::compile_to_elf_full_with_target(
            program,
            enable_reg_alloc,
            enable_tail_calls,
            enable_block_layout,
            enable_code_align,
            TargetFeatures::baseline(),
        )
        .expect("backend code generation failed")
    }

    /// Compiles a MIR program into an ELF64 object with fine-grained backend controls
    /// and explicit target feature validation.
    pub fn compile_to_elf_full_with_target(
        program: &MirProgram,
        enable_reg_alloc: bool,
        enable_tail_calls: bool,
        enable_block_layout: bool,
        enable_code_align: bool,
        target_features: TargetFeatures,
    ) -> Result<Vec<u8>, CodegenError> {
        Self::compile_to_elf_full_with_debug(
            program,
            enable_reg_alloc,
            enable_tail_calls,
            enable_block_layout,
            enable_code_align,
            target_features,
            false,
            None,
        )
    }

    /// Compiles a MIR program into an ELF64 object with fine-grained backend controls,
    /// explicit target feature validation, and optional DWARF 5 source-level debug information.
    pub fn compile_to_elf_full_with_debug(
        program: &MirProgram,
        enable_reg_alloc: bool,
        enable_tail_calls: bool,
        enable_block_layout: bool,
        enable_code_align: bool,
        target_features: TargetFeatures,
        emit_debug: bool,
        source_map: Option<&vanta_source::SourceMap>,
    ) -> Result<Vec<u8>, CodegenError> {
        let emitted = Self::emit_program_code(
            program,
            TargetAbi::SystemVAmd64,
            enable_reg_alloc,
            enable_tail_calls,
            enable_block_layout,
            enable_code_align,
            target_features,
            emit_debug,
            source_map,
        )?;

        // Generate DWARF 5 debug sections if enabled
        let debug_sections = if emit_debug {
            let comp_dir = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string());

            let mut files_list = Vec::new();
            if let Some(sm) = source_map {
                let mut fid = 1u32;
                while let Some(file) = sm.get_file(vanta_source::FileId(fid)) {
                    files_list.push(file.path.clone());
                    fid += 1;
                }
            }
            if files_list.is_empty() {
                files_list.push("main.va".to_string());
            }
            let primary_source = &files_list[0];

            let line_table = DwarfLineTable::build(
                &comp_dir,
                &files_list,
                &emitted.line_entries,
                emitted.text_bytes.len(),
            );
            let abbrev_table = DwarfAbbrevTable::build();
            let info_writer = DwarfInfoWriter::build(
                primary_source,
                &comp_dir,
                &emitted.func_debug_infos,
                emitted.text_bytes.len(),
            );

            let rela_debug_info: Vec<Relocation> = info_writer
                .relocations
                .into_iter()
                .map(|r| {
                    let symbol = match r.target {
                        DwarfRelocTarget::Text => ".text",
                        DwarfRelocTarget::DebugAbbrev => ".debug_abbrev",
                        DwarfRelocTarget::DebugLine => ".debug_line",
                        DwarfRelocTarget::DebugInfo => ".debug_info",
                    };
                    Relocation {
                        offset: r.offset,
                        symbol: symbol.to_string(),
                        addend: r.addend,
                        r_type: r.r_type,
                    }
                })
                .collect();

            let rela_debug_line: Vec<Relocation> = line_table
                .relocations
                .into_iter()
                .map(|r| {
                    let symbol = match r.target {
                        DwarfRelocTarget::Text => ".text",
                        DwarfRelocTarget::DebugAbbrev => ".debug_abbrev",
                        DwarfRelocTarget::DebugLine => ".debug_line",
                        DwarfRelocTarget::DebugInfo => ".debug_info",
                    };
                    Relocation {
                        offset: r.offset,
                        symbol: symbol.to_string(),
                        addend: r.addend,
                        r_type: r.r_type,
                    }
                })
                .collect();

            Some(DebugSections {
                debug_info: info_writer.section_data,
                debug_abbrev: abbrev_table,
                debug_line: line_table.section_data,
                rela_debug_info,
                rela_debug_line,
            })
        } else {
            None
        };

        // Write complete ELF64 object file
        let writer = Elf64Writer::new(
            emitted.text_bytes,
            emitted.rodata_bytes,
            emitted.text_relocations,
            emitted.functions,
            emitted.rodata_symbols,
        )
        .with_rodata_relocations(emitted.rodata_relocations)
        .with_debug(debug_sections);

        Ok(writer.write())
    }

    /// Compiles a MIR program into a valid Microsoft PE/COFF 64-bit object (.obj) byte buffer.
    pub fn compile_to_coff(program: &MirProgram) -> Result<Vec<u8>, CodegenError> {
        Self::compile_to_coff_with_target(program, TargetFeatures::baseline())
    }

    /// Compiles a MIR program into a COFF object with specified target features.
    pub fn compile_to_coff_with_target(
        program: &MirProgram,
        target_features: TargetFeatures,
    ) -> Result<Vec<u8>, CodegenError> {
        Self::compile_to_coff_full_with_target(program, true, true, true, true, target_features)
    }

    /// Compiles a MIR program into a COFF object with fine-grained backend controls.
    pub fn compile_to_coff_full_with_target(
        program: &MirProgram,
        enable_reg_alloc: bool,
        enable_tail_calls: bool,
        enable_block_layout: bool,
        enable_code_align: bool,
        target_features: TargetFeatures,
    ) -> Result<Vec<u8>, CodegenError> {
        let emitted = Self::emit_program_code(
            program,
            TargetAbi::MicrosoftX64,
            enable_reg_alloc,
            enable_tail_calls,
            enable_block_layout,
            enable_code_align,
            target_features,
            false,
            None,
        )?;

        let writer = CoffWriter::new(
            emitted.text_bytes,
            emitted.rodata_bytes,
            emitted.text_relocations,
            emitted.functions,
            emitted.rodata_symbols,
        )
        .with_rdata_relocations(emitted.rodata_relocations);

        Ok(writer.write())
    }

    /// Dispatches compilation to either ELF64 or COFF according to target specification.
    pub fn compile_to_object(
        program: &MirProgram,
        target_spec: &TargetSpec,
        target_features: TargetFeatures,
    ) -> Result<Vec<u8>, CodegenError> {
        Self::compile_to_object_full_with_debug(
            program,
            target_spec,
            true,
            true,
            true,
            true,
            target_features,
            false,
            None,
        )
    }

    /// Dispatches compilation to either ELF64 or COFF according to target specification
    /// with full backend pipeline flags and debug info support.
    pub fn compile_to_object_full_with_debug(
        program: &MirProgram,
        target_spec: &TargetSpec,
        enable_reg_alloc: bool,
        enable_tail_calls: bool,
        enable_block_layout: bool,
        enable_code_align: bool,
        target_features: TargetFeatures,
        emit_debug: bool,
        source_map: Option<&vanta_source::SourceMap>,
    ) -> Result<Vec<u8>, CodegenError> {
        match target_spec.object_format {
            ObjectFormat::Elf64 => Self::compile_to_elf_full_with_debug(
                program,
                enable_reg_alloc,
                enable_tail_calls,
                enable_block_layout,
                enable_code_align,
                target_features,
                emit_debug,
                source_map,
            ),
            ObjectFormat::Coff => Self::compile_to_coff_full_with_target(
                program,
                enable_reg_alloc,
                enable_tail_calls,
                enable_block_layout,
                enable_code_align,
                target_features,
            ),
            ObjectFormat::MachO64 => Err(CodegenError::InvalidTarget(
                "Mach-O 64-bit object generation requires the AArch64 backend".into(),
            )),
        }
    }

    /// Lowers and encodes MIR functions to machine instructions under the specified ABI.
    pub fn emit_program_code(
        program: &MirProgram,
        abi: TargetAbi,
        enable_reg_alloc: bool,
        enable_tail_calls: bool,
        enable_block_layout: bool,
        enable_code_align: bool,
        target_features: TargetFeatures,
        emit_debug: bool,
        source_map: Option<&vanta_source::SourceMap>,
    ) -> Result<EmittedProgram, CodegenError> {
        let mut emitter = CodeEmitter::with_target(target_features);
        let mut functions = Vec::new();
        let mut func_debug_infos = Vec::new();

        let mut fn_signatures: HashMap<String, (Vec<Type>, Type)> = HashMap::new();
        for f in &program.functions {
            let p_types: Vec<Type> = f.params.iter().map(|(_, ty)| ty.clone()).collect();
            fn_signatures.insert(f.name.clone(), (p_types, f.ret_type.clone()));
        }
        // Well-known external C functions
        fn_signatures.insert(
            "vanta_c_vector_add".into(),
            (
                vec![Type::Struct("v256".into()), Type::Struct("v256".into())],
                Type::Struct("v256".into()),
            ),
        );
        fn_signatures.insert(
            "vanta_c_vector_add512".into(),
            (
                vec![Type::Struct("v512".into()), Type::Struct("v512".into())],
                Type::Struct("v512".into()),
            ),
        );

        for func in &program.functions {
            // Align function entry to 16 bytes
            while !emitter.bytes.len().is_multiple_of(16) {
                emitter.emit_u8(0x90); // nop
            }

            let func_start = emitter.bytes.len();

            // 1. Compute CFG dataflow intervals for function
            let scalar_intervals = RegisterAllocator::compute_live_intervals(func);

            // 2. Identify virtual SIMD values
            let (simd_val_map, simd_width_map) =
                simd_alloc::identify_simd_values_with_width(func, &program.structs, &program.enums);

            // 3. Construct SIMD intervals and allocate physical vector registers
            let mut real_call_points = Vec::new();
            let mut pt = 0usize;
            for block in &func.blocks {
                for inst in &block.instructions {
                    pt += 2;
                    if let Instruction::Call { callee, .. } = inst {
                        if !callee.starts_with("venta_simd_") && !callee.starts_with("simd_") {
                            real_call_points.push(pt);
                        }
                    }
                }
                if block.terminator.is_some() {
                    pt += 2;
                }
            }

            let is_avx512 = target_features.avx512f;
            let mut simd_intervals = Vec::new();
            for iv in &scalar_intervals {
                if let Some(&simd_id) = simd_val_map.get(&iv.val) {
                    let width = simd_width_map
                        .get(&simd_id)
                        .copied()
                        .unwrap_or(VectorWidth::V256);
                    let does_cross_real_call = real_call_points
                        .iter()
                        .any(|&cp| cp > iv.start && cp <= iv.end);
                    simd_intervals.push(SimdLiveInterval {
                        val: simd_id,
                        width,
                        start: iv.start,
                        end: iv.end,
                        does_cross_call: does_cross_real_call,
                    });
                }
            }
            let simd_alloc_res =
                SimdRegisterAllocator::allocate_with_target(simd_intervals, is_avx512);

            // 4. Separate scalar intervals from SIMD intervals
            let non_simd_intervals: Vec<reg_alloc::LiveInterval> = scalar_intervals
                .into_iter()
                .filter(|iv| !simd_val_map.contains_key(&iv.val))
                .collect();
            let alloc_res = if enable_reg_alloc {
                Some(RegisterAllocator::allocate_intervals_with_abi(
                    non_simd_intervals,
                    abi,
                ))
            } else {
                None
            };

            // 5. Compute FrameLayout accounting for ABI and SIMD slots
            let frame = FrameLayout::compute_with_abi(
                func,
                &program.structs,
                &program.enums,
                alloc_res.as_ref(),
                Some(&simd_alloc_res),
                abi,
            );

            // 6. Lower to machine instructions with SIMD register allocation
            let selector = InstructionSelector::with_simd(
                func,
                &frame,
                &program.structs,
                &program.enums,
                alloc_res.as_ref(),
                Some(&simd_alloc_res),
                Some(&simd_val_map),
                Some(&fn_signatures),
                enable_tail_calls,
                enable_block_layout,
            )
            .with_source_map(if emit_debug { source_map } else { None });

            let insts = selector.select_instructions();

            for inst in &insts {
                if enable_code_align {
                    if let MachineInst::Label(lbl) = inst {
                        if lbl.contains("loop_header") {
                            while !emitter.bytes.len().is_multiple_of(16) {
                                emitter.emit_u8(0x90);
                            }
                        }
                    }
                }
                emitter.encode_instruction_checked(inst, &target_features)?;
            }

            let func_end = emitter.bytes.len();
            let func_size = func_end - func_start;
            functions.push(FunctionSymbol {
                name: func.name.clone(),
                offset: func_start,
                size: func_size,
                is_global: true,
            });

            if emit_debug {
                let (file_idx, decl_line) = if !func.span.is_dummy() {
                    if let Some(f) = source_map.and_then(|sm| sm.get_file(func.span.file_id)) {
                        let (l, _) = f.line_col(func.span.start);
                        (func.span.file_id.0, l as u32)
                    } else {
                        (1, 1)
                    }
                } else {
                    (1, 1)
                };

                let mut variables = Vec::new();
                for (slot_id, ty) in &func.params {
                    let slot = func.slots.iter().find(|s| s.id == *slot_id);
                    let name = slot
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| format!("p_{}", slot_id.0));
                    let (var_file, var_line) = if let Some(sp) = func.slot_spans.get(slot_id) {
                        if let Some(f) = source_map.and_then(|sm| sm.get_file(sp.file_id)) {
                            let (l, _) = f.line_col(sp.start);
                            (sp.file_id.0, l as u32)
                        } else {
                            (file_idx, decl_line)
                        }
                    } else {
                        (file_idx, decl_line)
                    };
                    let stack_offset = frame.slot_offsets.get(slot_id).copied().unwrap_or(8);
                    variables.push(VariableDebugInfo {
                        name,
                        type_name: format!("{}", ty),
                        file_idx: var_file,
                        line: var_line,
                        stack_offset,
                        is_param: true,
                    });
                }

                for slot in &func.slots {
                    if func.params.iter().any(|(p_id, _)| *p_id == slot.id) {
                        continue;
                    }
                    if slot.name.starts_with('.')
                        || slot.name.starts_with("_iter_tmp_")
                        || slot.name.starts_with("_res_")
                        || (slot.name.starts_with('_') && slot.name.contains("tmp"))
                    {
                        continue;
                    }
                    let (var_file, var_line) = if let Some(sp) = func.slot_spans.get(&slot.id) {
                        if let Some(f) = source_map.and_then(|sm| sm.get_file(sp.file_id)) {
                            let (l, _) = f.line_col(sp.start);
                            (sp.file_id.0, l as u32)
                        } else {
                            (file_idx, decl_line)
                        }
                    } else {
                        (file_idx, decl_line)
                    };
                    let stack_offset = frame.slot_offsets.get(&slot.id).copied().unwrap_or(8);
                    variables.push(VariableDebugInfo {
                        name: slot.name.clone(),
                        type_name: format!("{}", slot.ty),
                        file_idx: var_file,
                        line: var_line,
                        stack_offset,
                        is_param: false,
                    });
                }

                let ret_ty_str = match &func.ret_type {
                    Type::Unit => None,
                    other => Some(format!("{}", other)),
                };

                func_debug_infos.push(FunctionDebugInfo {
                    name: func.name.clone(),
                    file_idx,
                    line: decl_line,
                    code_offset: func_start,
                    size: func_size,
                    return_type: ret_ty_str,
                    variables,
                });
            }
        }

        // Patch local jumps
        emitter.resolve_fixups();

        // Construct rodata section from string literals and vtables
        let mut rodata_section = Vec::new();
        let mut rodata_symbols = Vec::new();
        let mut rodata_relocations = Vec::new();

        for lit in &program.string_literals {
            let offset = rodata_section.len();
            rodata_section.extend_from_slice(&lit.bytes);
            rodata_section.push(0);
            rodata_symbols.push(RodataSymbol {
                name: lit.symbol.clone(),
                offset,
                size: lit.bytes.len(),
            });
        }

        for vtable in &program.vtables {
            while !rodata_section.len().is_multiple_of(8) {
                rodata_section.push(0);
            }
            let offset = rodata_section.len();
            let size = vtable.methods.len() * 8;
            rodata_symbols.push(RodataSymbol {
                name: vtable.symbol.clone(),
                offset,
                size,
            });
            for (idx, method_symbol) in vtable.methods.iter().enumerate() {
                let entry_offset = offset + idx * 8;
                rodata_relocations.push(Relocation {
                    offset: entry_offset,
                    symbol: method_symbol.clone(),
                    addend: 0,
                    r_type: 1, // R_X86_64_64 / IMAGE_REL_AMD64_ADDR64
                });
                rodata_section.extend_from_slice(&0u64.to_le_bytes());
            }
        }

        while !rodata_section.len().is_multiple_of(16) {
            rodata_section.push(0);
        }

        Ok(EmittedProgram {
            text_bytes: emitter.bytes,
            rodata_bytes: rodata_section,
            text_relocations: emitter.relocations,
            rodata_relocations,
            functions,
            rodata_symbols,
            func_debug_infos,
            line_entries: emitter.line_entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vanta_diagnostics::DiagnosticBag;
    use vanta_lexer::Lexer;
    use vanta_mir::{MirLowerer, MirVerifier};
    use vanta_parser::Parser;
    use vanta_sema::SemanticAnalyzer;
    use vanta_source::SourceMap;

    #[test]
    fn test_x86_64_elf_generation() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "test_add.va".into(),
            r#"
            pub fn add(a: i64, b: i64) -> i64 {
                return a + b;
            }
            "#
            .into(),
        );
        let file = sm.get_file(id).unwrap();
        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);
        let sema = SemanticAnalyzer::new();
        let ctx = sema.analyze(&program, &mut diags);
        assert!(!diags.has_errors());

        let mut lowerer = MirLowerer::new(&ctx);
        let mir = lowerer.lower_program(&program);
        MirVerifier::verify_program(&mir, &mut diags);
        assert!(!diags.has_errors());

        let elf_bytes = X86_64Backend::compile_to_elf(&mir);
        assert!(elf_bytes.len() > 64);
        // Verify ELF Magic: 0x7F, 'E', 'L', 'F'
        assert_eq!(&elf_bytes[0..4], &[0x7F, b'E', b'L', b'F']);
        // Verify 64-bit ELF
        assert_eq!(elf_bytes[4], 2);
        // Verify little-endian
        assert_eq!(elf_bytes[5], 1);
        // Verify machine = EM_X86_64 (62)
        assert_eq!(u16::from_le_bytes([elf_bytes[18], elf_bytes[19]]), 62);
    }

    #[test]
    fn test_x86_64_coff_generation() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "test_coff.va".into(),
            r#"
            pub fn multiply(a: i64, b: i64) -> i64 {
                return a * b;
            }
            "#
            .into(),
        );
        let file = sm.get_file(id).unwrap();
        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);
        let sema = SemanticAnalyzer::new();
        let ctx = sema.analyze(&program, &mut diags);
        assert!(!diags.has_errors());

        let mut lowerer = MirLowerer::new(&ctx);
        let mir = lowerer.lower_program(&program);
        MirVerifier::verify_program(&mir, &mut diags);
        assert!(!diags.has_errors());

        let coff_bytes = X86_64Backend::compile_to_coff(&mir).expect("coff generation failed");
        assert!(coff_bytes.len() > 64);
        // Verify COFF Machine: IMAGE_FILE_MACHINE_AMD64 (0x8664)
        assert_eq!(
            u16::from_le_bytes([coff_bytes[0], coff_bytes[1]]),
            crate::coff_writer::IMAGE_FILE_MACHINE_AMD64
        );
        // Verify 4 sections (.text, .rdata, .pdata, .xdata)
        assert_eq!(u16::from_le_bytes([coff_bytes[2], coff_bytes[3]]), 4);
    }
}

//! Custom x86-64 native code generator and ELF64 object writer for Vanta.

pub mod abi;
pub mod elf_writer;
pub mod encoding;
pub mod frame;
pub mod instruction_select;
pub mod machine_ir;
pub mod reg_alloc;
pub mod register;

pub use abi::{classify_type, AbiClass};
pub use elf_writer::{Elf64Writer, FunctionSymbol, RodataSymbol};
pub use encoding::CodeEmitter;
pub use frame::FrameLayout;
pub use instruction_select::InstructionSelector;
pub use machine_ir::MachineInst;
pub use reg_alloc::{AllocationResult, Location, RegisterAllocator};
pub use register::Reg;
use vanta_mir::MirProgram;

pub struct X86_64Backend;

impl X86_64Backend {
    /// Compiles a MIR program into a valid Linux ELF64 relocatable object file (.o) byte buffer,
    /// with physical register allocation enabled by default.
    pub fn compile_to_elf(program: &MirProgram) -> Vec<u8> {
        Self::compile_to_elf_with_options(program, true)
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
        let mut emitter = CodeEmitter::new();
        let mut functions = Vec::new();

        for func in &program.functions {
            // Align function entry to 16 bytes
            while !emitter.bytes.len().is_multiple_of(16) {
                emitter.emit_u8(0x90); // nop
            }

            let func_start = emitter.bytes.len();
            let alloc_res = if enable_reg_alloc {
                Some(RegisterAllocator::allocate(func))
            } else {
                None
            };
            let frame = FrameLayout::compute(
                func,
                &program.structs,
                &program.enums,
                alloc_res.as_ref(),
            );
            let selector = InstructionSelector::with_options(
                func,
                &frame,
                &program.structs,
                &program.enums,
                alloc_res.as_ref(),
                enable_tail_calls,
                enable_block_layout,
            );
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
                emitter.encode_instruction(inst);
            }

            let func_end = emitter.bytes.len();
            functions.push(FunctionSymbol {
                name: func.name.clone(),
                offset: func_start,
                size: func_end - func_start,
                is_global: true, // Mark all top-level functions global for C linkage
            });
        }

        // Patch local jumps
        emitter.resolve_fixups();

        // Construct rodata section from string literals
        let mut rodata_section = Vec::new();
        let mut rodata_symbols = Vec::new();

        for lit in &program.string_literals {
            let offset = rodata_section.len();
            rodata_section.extend_from_slice(&lit.bytes);
            rodata_section.push(0); // NUL terminator byte for standard C/ELF convention
            rodata_symbols.push(RodataSymbol {
                name: lit.symbol.clone(),
                offset,
                size: lit.bytes.len(),
            });
        }

        while !rodata_section.len().is_multiple_of(16) {
            rodata_section.push(0);
        }

        // Write complete ELF64 object file
        let writer = Elf64Writer::new(
            emitter.bytes,
            rodata_section,
            emitter.relocations,
            functions,
            rodata_symbols,
        );
        writer.write()
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
}

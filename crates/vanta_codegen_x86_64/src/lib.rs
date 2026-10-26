//! Custom x86-64 native code generator and ELF64 object writer for Vanta.

pub mod elf_writer;
pub mod encoding;
pub mod frame;
pub mod instruction_select;
pub mod machine_ir;
pub mod register;

pub use elf_writer::{Elf64Writer, FunctionSymbol, RodataSymbol};
pub use encoding::CodeEmitter;
pub use frame::FrameLayout;
pub use instruction_select::InstructionSelector;
pub use machine_ir::MachineInst;
pub use register::Reg;
use vanta_mir::MirProgram;

pub struct X86_64Backend;

impl X86_64Backend {
    /// Compiles a MIR program into a valid Linux ELF64 relocatable object file (.o) byte buffer.
    pub fn compile_to_elf(program: &MirProgram) -> Vec<u8> {
        let mut emitter = CodeEmitter::new();
        let mut functions = Vec::new();

        for func in &program.functions {
            // Align function entry to 16 bytes
            while !emitter.bytes.len().is_multiple_of(16) {
                emitter.emit_u8(0x90); // nop
            }

            let func_start = emitter.bytes.len();
            let frame = FrameLayout::compute(func);
            let selector = InstructionSelector::new(func, &frame);
            let insts = selector.select_instructions();

            for inst in &insts {
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

//! Vanta AArch64 / Apple Silicon native code generation backend.
//!
//! Provides pure-Rust machine code emission and Mach-O 64-bit relocatable object
//! file generation for the `aarch64-apple-darwin` target architecture.

pub mod abi;
pub mod encoding;
pub mod frame;
pub mod instruction_select;
pub mod macho_writer;
pub mod reg_alloc;
pub mod register;

pub use abi::{classify_arm_type, compute_arm_fn_call_layout, ArmAbiClass, ArmFnCallLayout};
pub use encoding::{ArmCodeEmitter, ArmCond, ArmInst, ArmRelocation, ARM64_RELOC_UNSIGNED};
pub use frame::ArmFrameLayout;
pub use instruction_select::ArmInstructionSelector;
pub use macho_writer::{MachODataSymbol, MachOFuncSymbol, MachOWriter};
pub use reg_alloc::{ArmAllocationResult, ArmLocation, ArmRegisterAllocator};
pub use register::AArch64Reg;
use vanta_mir::MirProgram;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AArch64CodegenError {
    InvalidInstruction(String),
}

impl std::fmt::Display for AArch64CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AArch64CodegenError::InvalidInstruction(msg) => {
                write!(f, "AArch64 codegen error: {}", msg)
            }
        }
    }
}

impl std::error::Error for AArch64CodegenError {}

pub struct AArch64Backend;

impl AArch64Backend {
    /// Compiles a MIR program into a valid Apple Silicon macOS Mach-O 64-bit object (.o) byte buffer.
    pub fn compile_to_macho(program: &MirProgram) -> Result<Vec<u8>, AArch64CodegenError> {
        let mut emitter = ArmCodeEmitter::new();
        let mut functions = Vec::new();

        for func in &program.functions {
            // Align function entry to 16 bytes
            while !emitter.bytes.len().is_multiple_of(16) {
                emitter.emit_u32(0xD503201F); // nop in AArch64
            }

            let func_start = emitter.bytes.len();

            // 1. Dataflow liveness and register allocation
            let intervals = ArmRegisterAllocator::compute_live_intervals(func);
            let alloc_res = ArmRegisterAllocator::allocate_intervals(intervals);

            // 2. Compute AAPCS64 stack frame layout
            let frame =
                ArmFrameLayout::compute(func, &program.structs, &program.enums, Some(&alloc_res));

            // 3. Instruction selection
            let selector = ArmInstructionSelector::new(
                func,
                &frame,
                &program.structs,
                &program.enums,
                Some(&alloc_res),
            );
            selector.select_instructions(&mut emitter);

            let func_end = emitter.bytes.len();
            let func_size = func_end - func_start;
            functions.push(MachOFuncSymbol {
                name: func.name.clone(),
                offset: func_start,
                size: func_size,
                is_global: true,
            });
        }

        // Patch local jumps
        emitter.resolve_fixups();

        // Construct __const section for string literals and vtables
        let mut const_section = Vec::new();
        let mut data_symbols = Vec::new();
        let mut const_relocations = Vec::new();

        for lit in &program.string_literals {
            let offset = const_section.len();
            const_section.extend_from_slice(&lit.bytes);
            const_section.push(0); // NUL terminator
            data_symbols.push(MachODataSymbol {
                name: format!("_{}", lit.symbol),
                offset,
                size: lit.bytes.len(),
            });
        }

        for vtable in &program.vtables {
            while !const_section.len().is_multiple_of(8) {
                const_section.push(0);
            }
            let offset = const_section.len();
            let size = vtable.methods.len() * 8;
            data_symbols.push(MachODataSymbol {
                name: format!("_{}", vtable.symbol),
                offset,
                size,
            });
            for (idx, method_symbol) in vtable.methods.iter().enumerate() {
                let entry_offset = offset + idx * 8;
                let callee_symbol = if !method_symbol.starts_with('_') {
                    format!("_{}", method_symbol)
                } else {
                    method_symbol.clone()
                };
                const_relocations.push(ArmRelocation {
                    offset: entry_offset,
                    symbol: callee_symbol,
                    addend: 0,
                    r_type: ARM64_RELOC_UNSIGNED,
                });
                const_section.extend_from_slice(&0u64.to_le_bytes());
            }
        }

        while !const_section.len().is_multiple_of(16) {
            const_section.push(0);
        }

        let writer = MachOWriter::new(
            emitter.bytes,
            const_section,
            emitter.relocations,
            functions,
            data_symbols,
        )
        .with_const_relocations(const_relocations);

        Ok(writer.write())
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
    fn test_aarch64_macho_generation() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "test_macho.va".into(),
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

        let macho_bytes = AArch64Backend::compile_to_macho(&mir).expect("macho generation failed");
        assert!(macho_bytes.len() > 64);
        // Verify Mach-O Magic (0xFEEDFACF)
        assert_eq!(
            u32::from_le_bytes([
                macho_bytes[0],
                macho_bytes[1],
                macho_bytes[2],
                macho_bytes[3]
            ]),
            macho_writer::MH_MAGIC_64
        );
        // Verify CPU Type ARM64 (0x0100000C)
        assert_eq!(
            u32::from_le_bytes([
                macho_bytes[4],
                macho_bytes[5],
                macho_bytes[6],
                macho_bytes[7]
            ]),
            macho_writer::CPU_TYPE_ARM64
        );
    }
}

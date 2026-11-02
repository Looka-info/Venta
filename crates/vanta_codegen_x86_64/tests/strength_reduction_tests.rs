use vanta_codegen_x86_64::encoding::CodeEmitter;
use vanta_codegen_x86_64::machine_ir::{MachineInst, Operand};
use vanta_codegen_x86_64::register::Reg;
use vanta_codegen_x86_64::target::TargetFeatures;
use vanta_codegen_x86_64::X86_64Backend;
use vanta_diagnostics::DiagnosticBag;
use vanta_lexer::Lexer;
use vanta_mir::{MirLowerer, MirVerifier};
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

#[test]
fn test_sar_instruction_encoding() {
    let target = TargetFeatures::default();

    // 1. sar %rax, 1 -> 48 d1 f8 (or 48 c1 f8 01)
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Sar {
                dest: Reg::Rax,
                count: Operand::Imm(1),
            },
            &target,
        )
        .expect("sar imm1 should encode");
    assert_eq!(emitter.bytes, vec![0x48, 0xc1, 0xf8, 0x01]);

    // 2. sar %rdx, 4 -> 48 c1 fa 04
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Sar {
                dest: Reg::Rdx,
                count: Operand::Imm(4),
            },
            &target,
        )
        .expect("sar imm4 should encode");
    assert_eq!(emitter.bytes, vec![0x48, 0xc1, 0xfa, 0x04]);

    // 3. sar %r10, %cl -> 49 d3 fa
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Sar {
                dest: Reg::R10,
                count: Operand::Reg(Reg::Rcx),
            },
            &target,
        )
        .expect("sar cl should encode");
    assert_eq!(emitter.bytes, vec![0x49, 0xd3, 0xfa]);
}

#[test]
fn test_signed_division_and_modulo_by_power_of_two() {
    let mut sm = SourceMap::new();
    let id = sm.add_file(
        "test_div_rem.va".into(),
        r#"
        pub fn div_by_2(x: i64) -> i64 {
            return x / 2;
        }
        pub fn rem_by_2(x: i64) -> i64 {
            return x % 2;
        }
        pub fn div_by_16(x: i64) -> i64 {
            return x / 16;
        }
        pub fn rem_by_16(x: i64) -> i64 {
            return x % 16;
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
    assert_eq!(&elf_bytes[0..4], &[0x7F, b'E', b'L', b'F']);
}

#[test]
fn test_signed_arithmetic_equivalence_simulation() {
    // Exact semantics of x / 2^k and x % 2^k
    let test_values: Vec<i64> = vec![
        0,
        1,
        2,
        3,
        4,
        7,
        8,
        15,
        16,
        17,
        31,
        32,
        63,
        64,
        100,
        1024,
        -1,
        -2,
        -3,
        -4,
        -7,
        -8,
        -15,
        -16,
        -17,
        -31,
        -32,
        -63,
        -64,
        -100,
        -1024,
        i64::MAX,
        i64::MAX - 1,
        i64::MIN,
        i64::MIN + 1,
    ];

    let divisors: Vec<i64> = vec![2, 4, 8, 16, 32, 64, 128, 256, 1024];

    for &c in &divisors {
        let k = c.trailing_zeros();
        for &x in &test_values {
            // Expected Rust/Venta semantics:
            let expected_div = x / c;
            let expected_rem = x % c;

            // Simulated SAR sequence for div:
            // if k == 1:
            //   t = (x as u64) >> 63;
            //   res = (x + t) >> 1 (arithmetic)
            // if k > 1:
            //   t = (x >> 63) >> (64 - k)
            //   res = (x + t) >> k (arithmetic)
            let bias = if k == 1 {
                ((x as u64) >> 63) as i64
            } else {
                (((x >> 63) as u64) >> (64 - k)) as i64
            };
            let actual_div = (x.wrapping_add(bias)) >> k;

            // Simulated Rem sequence:
            // q = div
            // rem = x - (q << k)
            let actual_rem = x.wrapping_sub(actual_div << k);

            assert_eq!(
                actual_div, expected_div,
                "Div failed for x={}, c={} (k={}): got {}, expected {}",
                x, c, k, actual_div, expected_div
            );
            assert_eq!(
                actual_rem, expected_rem,
                "Rem failed for x={}, c={} (k={}): got {}, expected {}",
                x, c, k, actual_rem, expected_rem
            );
        }
    }
}

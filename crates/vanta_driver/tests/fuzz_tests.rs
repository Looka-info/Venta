//! Bounded, Deterministic Robustness and Fuzzing Harness for Vanta.
//!
//! Uses a reproducible pseudo-random number generator (xorshift64) with fixed seeds
//! to stress-test compiler subsystems:
//! 1. Lexer robustness: arbitrary byte sequences, boundary characters, unicode edge cases.
//! 2. Parser robustness: truncated expressions, unbalanced delimiters, randomized token streams.
//! 3. Type/Borrowchecker robustness: malformed type declarations and cyclic structures.
//! 4. MIR Verifier robustness: malformed basic block graphs and invalid phi edges.

use vanta_diagnostics::DiagnosticBag;
use vanta_lexer::Lexer;
use vanta_mir::{
    BasicBlock, BlockId, Instruction, MirBinaryOp, MirFunction, MirVerifier, Terminator, ValueId,
};
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

struct SimplePrng {
    state: u64,
}

impl SimplePrng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0xDEADBEEF42 } else { seed },
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

    fn next_usize(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next_u64() as usize) % bound
        }
    }

    fn next_byte(&mut self) -> u8 {
        (self.next_u64() & 0xFF) as u8
    }
}

#[test]
fn test_fuzz_lexer_random_byte_sequences_no_panic() {
    let mut rng = SimplePrng::new(0x1337_CAFE_BABE);

    for iteration in 0..200 {
        let len = rng.next_usize(128) + 1;
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(rng.next_byte());
        }

        let input_str = String::from_utf8_lossy(&bytes).to_string();

        let mut sm = SourceMap::new();
        let fid = sm.add_file(format!("fuzz_lex_{}.va", iteration), input_str);
        let file = sm.get_file(fid).unwrap();

        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let _tokens = lexer.tokenize(&mut diags);
        // Invariant: Lexer must NEVER panic, crash, or abort.
    }
}

#[test]
fn test_fuzz_parser_nested_delimiters_and_garbage_no_panic() {
    let mut rng = SimplePrng::new(0x4242_9999_AABB);

    let fragments = [
        "fn main() {",
        "let x = ",
        "(",
        ")",
        "{",
        "}",
        "[",
        "]",
        "123",
        "+",
        "-",
        "*",
        "/",
        "%",
        "if",
        "else",
        "while",
        "return",
        "struct Foo {",
        "enum Bar {",
        "let mut a: i64 = 5;",
        ";",
        "\"",
        "ident",
        "==",
        "!=",
        ">=",
        "<=",
    ];

    for iteration in 0..200 {
        let count = rng.next_usize(40) + 1;
        let mut generated = String::new();
        for _ in 0..count {
            let idx = rng.next_usize(fragments.len());
            generated.push_str(fragments[idx]);
            generated.push(' ');
        }

        let mut sm = SourceMap::new();
        let fid = sm.add_file(format!("fuzz_parse_{}.va", iteration), generated);
        let file = sm.get_file(fid).unwrap();

        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);

        let mut parser = Parser::new(tokens);
        let _program = parser.parse_program(&mut diags);
        // Invariant: Parser must NEVER panic; malformed programs cleanly emit errors.
    }
}

#[test]
fn test_fuzz_sema_randomized_ast_no_panic() {
    let mut rng = SimplePrng::new(0x9876_5432_10FE);

    let templates = [
        "fn main() -> i64 { return X; }",
        "fn foo(x: i64) -> i64 { return x + Y; }",
        "struct S { a: i64, b: S }",
        "enum E { A(i64), B(E) }",
        "fn test() { let x = 5; let r = &mut x; let r2 = &mut x; }",
        "fn calc() -> i64 { let a = true; return a + 1; }",
    ];

    for (i, tpl) in templates.iter().enumerate() {
        let val_x = (rng.next_u64() % 1000) as i64;
        let val_y = (rng.next_u64() % 1000) as i64;
        let src = tpl
            .replace("X", &val_x.to_string())
            .replace("Y", &val_y.to_string());

        let mut sm = SourceMap::new();
        let fid = sm.add_file(format!("fuzz_sema_{}.va", i), src);
        let file = sm.get_file(fid).unwrap();

        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);

        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);

        let sema = SemanticAnalyzer::new();
        let _ctx = sema.analyze(&program, &mut diags);
        // Invariant: Semantic analyzer must catch semantic errors without compiler aborts.
    }
}

#[test]
fn test_fuzz_mir_verifier_catches_invalid_inputs_no_panic() {
    let mut diags = DiagnosticBag::new();

    // 1. Function without basic blocks
    let empty_func = MirFunction {
        name: "empty".into(),
        params: vec![],
        ret_type: vanta_sema::Type::Unit,
        slots: vec![],
        blocks: vec![],
    };
    MirVerifier::verify_function(&empty_func, &mut diags);
    assert!(diags.has_errors());
    diags = DiagnosticBag::new();

    // 2. Block with missing terminator
    let unterminated_func = MirFunction {
        name: "unterminated".into(),
        params: vec![],
        ret_type: vanta_sema::Type::Unit,
        slots: vec![],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            name: "entry".into(),
            instructions: vec![],
            terminator: None,
        }],
    };
    MirVerifier::verify_function(&unterminated_func, &mut diags);
    assert!(diags.has_errors());
    diags = DiagnosticBag::new();

    // 3. Instruction using an undefined SSA ValueId
    let unbound_val_func = MirFunction {
        name: "unbound_val".into(),
        params: vec![],
        ret_type: vanta_sema::Type::Unit,
        slots: vec![],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            name: "entry".into(),
            instructions: vec![Instruction::Binary {
                dest: ValueId(2),
                op: MirBinaryOp::Add,
                lhs: ValueId(999), // Undefined!
                rhs: ValueId(999),
                ty: vanta_sema::Type::I64,
            }],
            terminator: Some(Terminator::Return(None)),
        }],
    };
    MirVerifier::verify_function(&unbound_val_func, &mut diags);
    assert!(diags.has_errors());
    diags = DiagnosticBag::new();

    // 4. Phi instruction referencing non-existent predecessor block
    let invalid_phi_func = MirFunction {
        name: "invalid_phi".into(),
        params: vec![],
        ret_type: vanta_sema::Type::Unit,
        slots: vec![],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            name: "entry".into(),
            instructions: vec![Instruction::Phi {
                dest: ValueId(1),
                inputs: vec![(BlockId(99), ValueId(1))], // Non-existent block 99!
                ty: vanta_sema::Type::I64,
            }],
            terminator: Some(Terminator::Return(None)),
        }],
    };
    MirVerifier::verify_function(&invalid_phi_func, &mut diags);
    assert!(diags.has_errors());
    diags = DiagnosticBag::new();

    // 5. Phi instruction placed after non-Phi instruction
    let misordered_phi_func = MirFunction {
        name: "misordered_phi".into(),
        params: vec![],
        ret_type: vanta_sema::Type::Unit,
        slots: vec![],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            name: "entry".into(),
            instructions: vec![
                Instruction::ConstInt {
                    dest: ValueId(1),
                    val: 10,
                    ty: vanta_sema::Type::I64,
                },
                Instruction::Phi {
                    dest: ValueId(2),
                    inputs: vec![],
                    ty: vanta_sema::Type::I64,
                },
            ],
            terminator: Some(Terminator::Return(None)),
        }],
    };
    MirVerifier::verify_function(&misordered_phi_func, &mut diags);
    assert!(diags.has_errors());
}

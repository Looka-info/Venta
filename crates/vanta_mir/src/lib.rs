//! Vanta Mid-Level Intermediate Representation (MIR) crate.

pub mod ir;
pub mod lower;
pub mod opt;
pub mod verify;

pub use ir::*;
pub use lower::MirLowerer;
pub use opt::{ConstantPropagation, DeadCodeElimination, OptLevel, OptimizationPipeline};
pub use verify::MirVerifier;

#[cfg(test)]
mod tests {
    use super::*;
    use vanta_diagnostics::DiagnosticBag;
    use vanta_lexer::Lexer;
    use vanta_parser::Parser;
    use vanta_sema::SemanticAnalyzer;
    use vanta_source::SourceMap;

    #[test]
    fn test_lower_and_verify_mir() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "mir_test.va".into(),
            r#"
            fn compute(x: i64) -> i64 {
                let mut total: i64 = 0;
                let mut i: i64 = 0;
                while i < x {
                    total += i;
                    i += 1;
                }
                return total;
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
        if diags.has_errors() {
            eprintln!("Sema errors:\n{}", diags.render_all(&sm, false));
        }
        assert!(!diags.has_errors());

        let mut lowerer = MirLowerer::new(&ctx);
        let mir = lowerer.lower_program(&program);
        assert_eq!(mir.functions.len(), 1);

        MirVerifier::verify_program(&mir, &mut diags);
        if diags.has_errors() {
            eprintln!("{}", diags.render_all(&sm, false));
        }
        assert!(!diags.has_errors());
    }
}

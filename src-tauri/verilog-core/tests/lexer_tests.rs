use verilog_core::{lexer, parse_file, SourceFile, TokenKind};

#[test]
fn lexes_basic_module_header_tokens() {
    let src = r#"
module top #(parameter WIDTH = 8) (
    input [3:0] a,
    output b
);
endmodule
"#;

    let res = parse_file("top.v", src);
    assert!(res.diagnostics.is_empty(), "unexpected diagnostics: {:?}", res.diagnostics);

    // Smoke-test that we see expected keywords and punctuation in order in the token stream.
    let file = SourceFile::new("top.v", src);
    let tokens = lexer::lex(&file);
    let kinds: Vec<TokenKind> = tokens.into_iter().map(|t| t.kind).collect();

    assert!(
        kinds.contains(&TokenKind::Module),
        "expected to see 'module' keyword"
    );
    assert!(
        kinds.contains(&TokenKind::Parameter),
        "expected to see 'parameter' keyword"
    );
    assert!(
        kinds.contains(&TokenKind::LBracket) && kinds.contains(&TokenKind::RBracket),
        "expected to see range brackets in port declaration"
    );
    assert!(
        kinds.contains(&TokenKind::Endmodule),
        "expected to see 'endmodule' keyword"
    );
}


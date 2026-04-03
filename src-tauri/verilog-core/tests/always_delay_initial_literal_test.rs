//! `always #delay` clock generators and `initial x = 'd0` based literals.
use verilog_core::build_ir_for_file;

#[test]
fn always_hash_fractional_delay_blocking_assign() {
    let src = r#"
module tb;
  reg CLK;
  always #0.5 CLK = ~CLK;
endmodule
"#;
    let ir = build_ir_for_file("TestBench6.v", src);
    assert!(
        ir.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        ir.diagnostics
    );
}

#[test]
fn initial_blocking_assign_with_unsized_decimal_literal() {
    let src = r#"
module CD;
  reg [3:0] Counter;
  initial Counter = 'd0;
endmodule
"#;
    let ir = build_ir_for_file("Clock_Div.v", src);
    assert!(
        ir.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        ir.diagnostics
    );
}

#[test]
fn always_hash_integer_delay() {
    let src = r#"
module m;
  reg x;
  always #10 x = ~x;
endmodule
"#;
    let ir = build_ir_for_file("m.v", src);
    assert!(ir.diagnostics.is_empty(), "{:?}", ir.diagnostics);
}

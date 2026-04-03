//! Regression: `always @*` must consume `*`; `localparam` must not be parsed as a module instance.
use verilog_core::build_ir_for_file;

#[test]
fn traffic_light_controller_style_always_at_star_and_localparam() {
    let src = r#"
module TLC(
  input Clock,
  input Reset,
  input E, NL, EL, W,
  output [6:0] TimerMSD, TimerLSD,
  output reg [6:0] ETL, NLTL, ELTL, WTL
  );

localparam flowA = 2'b00;
localparam flowB = 2'b01;
localparam flowC = 2'b10;
localparam red = 7'b0101111;

reg [1:0] flow, next_flow;

always @* begin
    case(flow)
    flowA: if(E) next_flow <= flowB; else next_flow <= flowA;
    flowB: next_flow <= flowC;
    flowC: next_flow <= flowA;
    endcase
end

always @* begin
    case(flow)
    flowA: begin ETL <= red; NLTL <= red; end
    default: begin ETL <= red; NLTL <= red; end
    endcase
end

B4to7SEG conv(.Blank(1'b0), .N(4'b0), .MSD(TimerMSD), .LSD(TimerLSD));

endmodule
"#;

    let ir = build_ir_for_file("TLC.v", src);
    assert!(
        ir.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        ir.diagnostics
    );
}

//! `#delay` sums in `initial` → simulation length.
use std::path::Path;

use verilog_core::{
    build_ir_for_file, clock_half_period_fine_ticks, num_cycles_from_initial_delay_sum,
    num_cycles_from_initial_delay_sum_fine, optimize_project,
    sum_initial_delay_literals_for_source_file, sum_initial_delays_for_source_files,
    unit_per_precision_ratio,
};

#[test]
fn sum_delays_constant_for_loop_multiplies_body() {
    let src = r#"
`timescale 1s / 100ms
module tb;
  integer i;
  initial begin
    for (i = 0; i < 700; i = i + 1) #1;
  end
endmodule
"#;
    let path = "/tmp/for_delays_tb.v";
    let mut ir = build_ir_for_file(path, src);
    optimize_project(&mut ir);
    let sum = sum_initial_delay_literals_for_source_file(&ir, Path::new(path));
    assert_eq!(sum, 700, "700 iterations of #1");
    assert_eq!(num_cycles_from_initial_delay_sum(sum, 5), 70);
}

#[test]
fn sum_delays_in_initial_linear_block() {
    let src = r#"
`timescale 1ns/1ps
module tb;
  reg x;
  initial begin
    #10 x = 0;
    #90 x = 1;
  end
endmodule
"#;
    let path = "/tmp/DoesNotMatterForSum.v";
    let mut ir = build_ir_for_file(path, src);
    optimize_project(&mut ir);
    let sum = sum_initial_delay_literals_for_source_file(&ir, Path::new(path));
    assert_eq!(sum, 100, "10 + 90 in one initial");
    let k = unit_per_precision_ratio("1ns", "1ps");
    let h = clock_half_period_fine_ticks(5, k, false, "1ns");
    let n = num_cycles_from_initial_delay_sum_fine(sum, k, h);
    assert_eq!(n, 10, "100ns delay vs 10ns clock period (2×5ns half in ps ticks)");
}

#[test]
fn sum_zero_yields_default_cycles_rule() {
    assert_eq!(num_cycles_from_initial_delay_sum(0, 5), 100);
}

/// Only files whose **preamble** declares the project `timescale` contribute to the delay budget.
/// DUT has `timescale` and no `initial`; TB holds stimulus but no preamble `timescale` — only DUT is summed (0).
#[test]
fn delay_sum_is_timescale_files_only_tb_delays_ignored_without_preamble_timescale() {
    let dut = r#"
`timescale 1ns/1ps
module dut(input wire a, output wire y);
  assign y = a;
endmodule
"#;
    let tb = r#"
module tb;
  reg a;
  wire y;
  dut d(.a(a), .y(y));
  initial begin
    #10 a = 0;
    #90 a = 1;
  end
endmodule
"#;
    let dut_path = "/tmp/csverilog_dut_fallback.v";
    let tb_path = "/tmp/csverilog_tb_fallback.v";
    let mut ir = build_ir_for_file(dut_path, dut);
    ir.modules.extend(build_ir_for_file(tb_path, tb).modules);
    optimize_project(&mut ir);
    let sum_dut_file = sum_initial_delays_for_source_files(
        &ir,
        &[Path::new(dut_path).to_path_buf()],
    );
    assert_eq!(sum_dut_file, 0);
    let k = unit_per_precision_ratio("1ns", "1ps");
    let h = clock_half_period_fine_ticks(5, k, false, "1ns");
    assert_eq!(num_cycles_from_initial_delay_sum_fine(sum_dut_file, k, h), 100);

    let sum_both_with_timescale = sum_initial_delays_for_source_files(
        &ir,
        &[
            Path::new(dut_path).to_path_buf(),
            Path::new(tb_path).to_path_buf(),
        ],
    );
    assert_eq!(sum_both_with_timescale, 100);
}

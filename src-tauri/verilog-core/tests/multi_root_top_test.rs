//! When several modules are uninstantiated (e.g. coursework `Project6` + `TestBench6`),
//! **`run_csverilog_pipeline`** must pick the testbench, not the first module of the first file.
use tempfile::tempdir;
use verilog_core::run_csverilog_pipeline;

#[test]
fn pipeline_picks_testbench_over_parallel_board_top() {
    let dir = tempdir().unwrap();
    let p = dir.path();

    let dut = p.join("DUT_min.v");
    std::fs::write(
        &dut,
        r"module DUT_min(input wire x, output wire y);
  assign y = x;
endmodule
",
    )
    .unwrap();

    // “Board” top: often present alongside TB; alphabetically before TestBench*.v
    let board = p.join("AAProject6Wrap.v");
    std::fs::write(
        &board,
        r"module Project6Wrap;
  wire a, b;
  DUT_min d(.x(a), .y(b));
endmodule
",
    )
    .unwrap();

    let tb = p.join("TestBench6.v");
    std::fs::write(
        &tb,
        r"module TestBench6;
  wire xa, xb;
  DUT_min d1(.x(xa), .y(xb));
  initial begin xa = 0; end
endmodule
",
    )
    .unwrap();

    // Same order `list_verilog_source_paths` would use: sorted paths (AA… first).
    let paths = vec![dut, board, tb];
    let out = p.join("out.vcd");
    let vcd = run_csverilog_pipeline(
        &paths,
        &out,
        "multi_root_top_test",
        Default::default(),
    )
    .expect("vcd");

    assert!(
        vcd.contains("$scope module TestBench6 $end"),
        "expected TestBench6 scope, not board wrapper; header excerpt:\n{}",
        vcd.lines().take(55).collect::<Vec<_>>().join("\n")
    );
}

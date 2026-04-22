//! # Known-bug regression suite
//!
//! These tests run with **`cargo test`** (including CI) and target bugs found during EECS 270 /
//! `verilog-core` development. They use repo **fixtures** where possible; course-only checks **skip**
//! if the tree is not present.
//!
//! | Bug / symptom | Fix area | How we test |
//! |---------------|----------|-------------|
//! | `c[i+1]` indices are `IrExpr::Binary` after `generate` — flatten dropped `cout → carry` | `codegen::flatten` + `ir_try_eval_const_index_expr` | `ripple_addsub_w4_*` in this file |
//! | Same issue inside **`optimize_project` inlining** (wrong 5+3=6) | `optimizer::module_inlining` + `inline_drive_lhs` | W=4 fixture with **`optimize_project`** |
//! | `11'sd3` parsed as 0 (ignored **`s`**) | `parse_verilog_number` | `expr_const` unit tests + `signed_literal_sw_slice` fixture in this file |
//! | `Result@90/170` / `TestBench7` | Full project sim | `p7_testbench7_*` in `project7_regression` (skips if no checkout) |
//! | `AddSub` W=11 + course `FullAdder` / `AddSub` | end-to-end | `addsub_const_inputs_*` in `project7_regression` (skips if no checkout) |
//!
//! Unit tests (always run, no fixtures): `ir::ir_try_eval_const_index_tests`, `expr_const::parse_number_tests`,
//! `optimizer::tests::module_inline_merges_output_into_packed_vec_when_partselect_msb_is_binary_add`.
//!
//! From the repo root, **`npm run test:verilog-core`** runs the full `verilog-core` `cargo test`.

use std::path::PathBuf;

mod common;

use common::{find_var_code_in_scope, parse_binary_changes_for_code, value_at_or_before};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

use verilog_core::{
    build_ir_for_file, generate_vcd, optimize_project, resolve_instance_port_connections, SimConfig,
};

/// W=4 self-contained `AddSub` + `FullAdder` in `tests/fixtures/ripple_addsub_w4_tb.v` — `c[i+1]`
/// carry chain + **`optimize_project`** inlining.
#[test]
fn bug_carry_chain_binary_index_flatten_and_inline_ripple_addsub_w4_five_plus_three_is_eight() {
    let path = fixture_path("ripple_addsub_w4_tb.v");
    assert!(path.is_file(), "missing fixture {}", path.display());
    let src = std::fs::read_to_string(&path).unwrap();
    let mut project = build_ir_for_file(path.to_string_lossy(), &src);
    resolve_instance_port_connections(&mut project).expect("resolve ports");
    optimize_project(&mut project);

    let config = SimConfig {
        top_module: "ripple_addsub_w4_tb".into(),
        num_cycles: 2,
        timescale: "1ns".into(),
        clock_half_period: 5,
        ..Default::default()
    };
    let vcd = generate_vcd(&project, &config).expect("vcd");
    let code = find_var_code_in_scope(&vcd, "ripple_addsub_w4_tb", "S").expect("S in tb scope");
    let samples = parse_binary_changes_for_code(&vcd, &code);
    assert_eq!(
        value_at_or_before(&samples, 1),
        Some(8),
        "5+3=8 with carry; samples={:?}",
        samples
    );
}

/// `SW[10:0] = -11'sd3` must not parse RHS as 0; exercises Project 7 load pattern.
#[test]
fn bug_signed_sized_literal_s_marker_initial_slice_assign_sw() {
    let path = fixture_path("signed_literal_sw_slice_tb.v");
    assert!(path.is_file(), "missing {}", path.display());
    let src = std::fs::read_to_string(&path).unwrap();
    let mut project = build_ir_for_file(path.to_string_lossy(), &src);
    optimize_project(&mut project);

    let config = SimConfig {
        top_module: "signed_literal_sw_slice_tb".into(),
        num_cycles: 4,
        timescale: "1ns".into(),
        clock_half_period: 5,
        ..Default::default()
    };
    let vcd = generate_vcd(&project, &config).expect("vcd");
    let code = find_var_code_in_scope(&vcd, "signed_literal_sw_slice_tb", "SW").expect("SW");
    let samples = parse_binary_changes_for_code(&vcd, &code);

    let eleven_mask = (1i64 << 11) - 1;
    let neg_three_11 = (-3i64) & eleven_mask;
    let expected_sw = (1i64 << 17) | neg_three_11;

    assert_eq!(
        value_at_or_before(&samples, 10),
        Some(expected_sw),
        "SW = sign bit + low 11b -3; samples={:?}",
        samples
    );
}

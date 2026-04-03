use std::path::PathBuf;
use verilog_core::{build_ir_for_file, generate_vcd, optimize_project, SimConfig};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("codegen_fixtures")
        .join(name)
}

fn build_and_gen(filename: &str, top: &str, cycles: usize) -> String {
    let path = fixture(filename);
    let src = std::fs::read_to_string(&path).unwrap();
    let mut ir = build_ir_for_file(path.to_string_lossy(), &src);
    optimize_project(&mut ir);
    let config = SimConfig {
        top_module: top.into(),
        num_cycles: cycles,
        timescale: "1ns".into(),
        clock_half_period: 5,
        ..Default::default()
    };
    generate_vcd(&ir, &config).expect("VCD generation should succeed")
}

// ═══════════════════════════════════════════════════════════════════════
// VCD format structure
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn vcd_has_required_sections() {
    let vcd = build_and_gen("comb_logic.v", "comb_logic", 2);
    assert!(vcd.contains("$date"), "missing $date");
    assert!(vcd.contains("$version"), "missing $version");
    assert!(
        vcd.contains("$timescale")
            && (vcd.contains("1ns") || vcd.contains("1 ns"))
            && vcd.contains("$end"),
        "missing $timescale block"
    );
    assert!(vcd.contains("$scope module comb_logic $end"), "missing $scope");
    assert!(vcd.contains("$upscope $end"), "missing $upscope");
    assert!(vcd.contains("$enddefinitions $end"), "missing $enddefs");
    assert!(vcd.contains("$dumpvars"), "missing $dumpvars");
    assert!(vcd.contains("#0"), "missing initial timestamp");
    assert!(vcd.contains("$comment"), "debug $comment headers should be present");
    assert!(
        vcd.contains("top_source_file:"),
        "IR-derived header should list top source path"
    );
    assert!(
        !vcd.contains("command_line:"),
        "fixture runs omit command line until caller sets VcdRunMeta"
    );
}

/// VCD **`$timescale`** uses the precision operand; **`#`** timestamps are **precision** ticks (IEEE-style).
#[test]
fn vcd_timescale_header_uses_precision_cosmetic() {
    let path = fixture("counter.v");
    let src = std::fs::read_to_string(&path).unwrap();
    let mut ir = build_ir_for_file(path.to_string_lossy(), &src);
    optimize_project(&mut ir);
    let config = SimConfig {
        top_module: "counter".into(),
        num_cycles: 3,
        timescale: "1ns".into(),
        timescale_precision: "1ps".into(),
        clock_half_period: 5,
        ..Default::default()
    };
    let vcd = generate_vcd(&ir, &config).expect("VCD generation should succeed");
    assert!(
        vcd.contains("1 ps"),
        "expected normalized precision on $timescale line"
    );
    assert!(
        vcd.contains("#5000") && vcd.contains("#10000"),
        "1ns unit with 1ps precision: half-period 5ns → 5000ps grid steps"
    );
}

#[test]
fn vcd_declares_all_signals() {
    let vcd = build_and_gen("comb_logic.v", "comb_logic", 2);
    assert!(vcd.contains("$var wire"), "should declare wire variables");
    for sig in &["a", "b", "y", "z"] {
        assert!(vcd.contains(sig), "missing signal {}", sig);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Combinational logic
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn comb_logic_generates_valid_vcd() {
    let vcd = build_and_gen("comb_logic.v", "comb_logic", 3);
    // Pure combinational — no clock, so values settle at time 0.
    assert!(vcd.contains("#0"));
    // Should have valid VCD content (not empty after header).
    assert!(vcd.len() > 100);
}

// ═══════════════════════════════════════════════════════════════════════
// Sequential counter
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn counter_produces_clock_edges() {
    let vcd = build_and_gen("counter.v", "counter", 5);
    // Clock from `always #5 clk = ~clk` in the fixture (no synthetic driver).
    assert!(vcd.contains("clk"), "clock signal should be in VCD");
    // Should have timestamps at half-period intervals.
    assert!(vcd.contains("#5"));
    assert!(vcd.contains("#10"));
    assert!(vcd.contains("#15"));
}

#[test]
fn counter_declares_reg_for_sequential() {
    let vcd = build_and_gen("counter.v", "counter", 3);
    assert!(vcd.contains("$var reg"), "count should be declared as reg");
}

#[test]
fn counter_has_multiple_value_changes() {
    let vcd = build_and_gen("counter.v", "counter", 10);
    let timestamp_count = vcd.matches("\n#").count();
    assert!(
        timestamp_count >= 5,
        "should have many timestamps, got {}",
        timestamp_count
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Hierarchy with instance
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hierarchy_produces_vcd() {
    let vcd = build_and_gen("hierarchy.v", "top_hier", 3);
    assert!(vcd.contains("$scope module top_hier $end"));
    // After inlining, z should be assigned.
    assert!(vcd.contains("z"));
    assert!(vcd.contains("x"));
}

// ═══════════════════════════════════════════════════════════════════════
// Error handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn missing_top_module_is_error() {
    let path = fixture("comb_logic.v");
    let src = std::fs::read_to_string(&path).unwrap();
    let ir = build_ir_for_file(path.to_string_lossy(), &src);
    let config = SimConfig {
        top_module: "nonexistent".into(),
        ..SimConfig::default()
    };
    let result = generate_vcd(&ir, &config);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// Multi-bit signals
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn multibit_declares_width_and_range() {
    let vcd = build_and_gen("multibit.v", "multibit", 3);
    assert!(vcd.contains("[7:0]"), "should declare 8-bit range for data_in/data_out");
    assert!(vcd.contains("[3:0]"), "should declare 4-bit range for nibble");
}

#[test]
fn multibit_uses_binary_format() {
    let vcd = build_and_gen("multibit.v", "multibit", 3);
    assert!(vcd.contains("b"), "should use binary format for multi-bit signals");
}

// ═══════════════════════════════════════════════════════════════════════
// Initial blocks with #delay
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn initial_block_produces_timed_events() {
    let vcd = build_and_gen("initial_tb.v", "initial_tb", 10);
    assert!(vcd.contains("$dumpvars"), "should have $dumpvars");
    assert!(vcd.contains("#0"), "should have time 0");
    assert!(vcd.contains("#10"), "should have time 10 for first delay");
}

#[test]
fn initial_block_signals_are_reg() {
    let vcd = build_and_gen("initial_tb.v", "initial_tb", 5);
    assert!(vcd.contains("$var reg"), "initial block signals should be reg");
}

// ═══════════════════════════════════════════════════════════════════════
// x-initialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn undriven_signals_show_x() {
    let vcd = build_and_gen("comb_logic.v", "comb_logic", 1);
    assert!(vcd.contains("x"), "undriven inputs should initially be x");
}

// ═══════════════════════════════════════════════════════════════════════
// Nested hierarchical scopes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nested_hierarchy_has_scopes() {
    let vcd = build_and_gen("nested_hier.v", "top_nested", 3);
    assert!(vcd.contains("$scope module top_nested $end"), "missing top scope");
    let upscope_count = vcd.matches("$upscope $end").count();
    assert!(upscope_count >= 2, "should have nested $upscope entries, got {}", upscope_count);
}

// ═══════════════════════════════════════════════════════════════════════
// VCD polish: $dumpvars after #0, real date
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dumpvars_placement() {
    let vcd = build_and_gen("counter.v", "counter", 2);
    let t0 = vcd.find("#0").unwrap();
    let dv = vcd.find("$dumpvars").unwrap();
    assert!(dv > t0, "$dumpvars should come after #0");
}

#[test]
fn vcd_has_date_section() {
    let vcd = build_and_gen("counter.v", "counter", 1);
    assert!(vcd.contains("$date"));
    assert!(vcd.contains("$end"));
}

// ═══════════════════════════════════════════════════════════════════════
// All fixtures smoke test
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_codegen_fixtures_produce_valid_vcd() {
    let fixtures = [
        ("comb_logic.v", "comb_logic"),
        ("counter.v", "counter"),
        ("hierarchy.v", "top_hier"),
        ("multibit.v", "multibit"),
        ("initial_tb.v", "initial_tb"),
        ("nested_hier.v", "top_nested"),
    ];
    for (file, top) in &fixtures {
        let vcd = build_and_gen(file, top, 5);
        assert!(
            vcd.contains("$enddefinitions"),
            "{} missing $enddefinitions",
            file
        );
        assert!(vcd.contains("#0"), "{} missing #0", file);
    }
}

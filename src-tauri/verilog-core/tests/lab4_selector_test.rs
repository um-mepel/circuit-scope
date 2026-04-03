//! Regression: Lab4 selector bench + DUT must keep [6:0] through IR and VCD.
//! The `lab4_vcd_declares_seven_bit_vars` test uses `build_ir_for_root` + `generate_vcd` directly.
//! End-to-end **menu / `csverilog`** behavior is covered in `tests/simulate_path_test.rs`.
use std::path::PathBuf;

use verilog_core::{build_ir_for_root, generate_vcd, optimize_project, SimConfig};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn lab4_modules_parse_with_seven_bit_ports() {
    let root = repo_root();
    let tb = root.join("Lab4TestBench.v");
    let dut = root.join("Lab4.v");
    if !tb.is_file() || !dut.is_file() {
        eprintln!("skip: Lab4TestBench.v / Lab4.v not at repo root");
        return;
    }

    let project = build_ir_for_root(&root).expect("build_ir_for_root");
    let tb_mod = project
        .modules
        .iter()
        .find(|m| m.name == "SelectorTestbench")
        .expect("SelectorTestbench");
    let set_a = tb_mod.nets.iter().find(|n| n.name == "Set_A").expect("Set_A");
    let out_f = tb_mod.nets.iter().find(|n| n.name == "Out_F").expect("Out_F");
    assert_eq!(set_a.width, 7, "Set_A must stay [6:0]");
    assert_eq!(out_f.width, 7, "Out_F must stay [6:0]");

    let sel_mod = project.modules.iter().find(|m| m.name == "Selector").expect("Selector");
    let port_a = sel_mod
        .ports
        .iter()
        .find(|p| p.name == "Set_A")
        .expect("Selector.Set_A port");
    assert_eq!(port_a.width, 7);
}

#[test]
fn lab4_vcd_declares_seven_bit_vars() {
    let root = repo_root();
    if !root.join("Lab4TestBench.v").is_file() {
        eprintln!("skip: lab files missing");
        return;
    }

    let mut project = build_ir_for_root(&root).expect("build_ir_for_root");
    optimize_project(&mut project);
    let vcd = generate_vcd(
        &project,
        &SimConfig {
            top_module: "SelectorTestbench".into(),
            num_cycles: 5,
            timescale: "1ns".into(),
            clock_half_period: 5,
            ..Default::default()
        },
    )
    .expect("VCD");

    assert!(
        vcd.contains("$var reg 7 ") && vcd.contains("Set_A [6:0]"),
        "VCD should declare 7-bit Set_A: {}",
        &vcd[..vcd.len().min(400)]
    );
    assert!(vcd.contains("n_sel_and_B [6:0]") || vcd.contains("n_sel_and_B"),
            "inlined DUT internals should appear in VCD"
    );
}

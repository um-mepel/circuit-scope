//! **File → Generate VCD** and **`csverilog &lt;out&gt;`** use `list_verilog_source_paths` + [`run_csverilog_pipeline`].
//! This test locks that behavior for the repo layout (Lab4 + MagnitudeComp, etc.).
use std::path::PathBuf;

use verilog_core::{list_verilog_source_paths, run_csverilog_pipeline};

#[test]
fn repo_scan_pipeline_emits_seven_bit_lab4_vcd() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if !repo.join("Lab4TestBench.v").is_file() {
        eprintln!("skip: Lab4 fixtures not at repo root");
        return;
    }

    let paths = list_verilog_source_paths(&repo).expect("list sources");
    assert!(
        paths.iter().any(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| n == "Lab4TestBench.v")
                .unwrap_or(false)
        }),
        "scan should include Lab4TestBench.v; got {} files",
        paths.len()
    );

    let out = std::env::temp_dir().join("circuit_scope_simulate_path_test.vcd");
    let label = "repo_scan_pipeline_emits_seven_bit_lab4_vcd";
    let vcd = run_csverilog_pipeline(&paths, &out, label, Default::default()).expect("pipeline");

    assert!(
        vcd.contains("$scope module SelectorTestbench")
            && vcd.contains("$var reg 7 ")
            && vcd.contains("Set_A [6:0]"),
        "expected SelectorTestbench + 7-bit vectors:\n{}",
        vcd.lines().take(45).collect::<Vec<_>>().join("\n")
    );

    assert!(
        vcd.contains("$comment")
            && vcd.contains("verilog-core")
            && vcd.contains("command_line:")
            && vcd.contains(&format!("command_line: {}", label)),
        "production VCD should include full debug headers (see codegen `$comment` blocks)"
    );

    let _ = std::fs::remove_file(&out);
}

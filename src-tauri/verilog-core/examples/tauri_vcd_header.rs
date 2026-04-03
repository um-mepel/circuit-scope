//! Print the VCD header from **`list_verilog_source_paths` + `run_csverilog_pipeline`**
//! (same pipeline as **File → Generate VCD** / `csverilog <out>`).
use std::env;
use std::path::PathBuf;

use verilog_core::{list_verilog_source_paths, run_csverilog_pipeline};

fn main() {
    let repo = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let out_name = env::args()
        .nth(2)
        .unwrap_or_else(|| "circuit_scope.vcd".to_string());

    eprintln!("repo root: {}", repo.display());
    eprintln!("output file hint: {}", out_name);

    let paths = list_verilog_source_paths(&repo).expect("list verilog under repo");
    let out = repo.join(&out_name);
    let vcd = run_csverilog_pipeline(
        &paths,
        &out,
        "tauri_vcd_header example (list + run_csverilog_pipeline)",
        Default::default(),
    )
    .expect("pipeline");

    let body = if let Some(pos) = vcd.find("$enddefinitions $end") {
        &vcd[..pos + "$enddefinitions $end".len()]
    } else {
        vcd.as_str()
    };
    println!("{}", body);
}

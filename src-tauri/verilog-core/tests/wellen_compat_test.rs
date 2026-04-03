//! Ensure generated VCD parses with wellen (same stack VaporView uses).
use std::io::Write;
use std::path::PathBuf;

use tempfile::NamedTempFile;
use verilog_core::{build_ir_for_file, generate_vcd, optimize_project, SimConfig};

#[test]
fn wellen_parses_comb_logic_vcd() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/codegen_fixtures/comb_logic.v");
    let src = std::fs::read_to_string(&root).unwrap();
    let mut ir = build_ir_for_file(root.to_string_lossy(), &src);
    optimize_project(&mut ir);
    let vcd = generate_vcd(
        &ir,
        &SimConfig {
            top_module: "comb_logic".into(),
            num_cycles: 3,
            timescale: "1ns".into(),
            clock_half_period: 5,
            ..Default::default()
        },
    )
    .expect("vcd");

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(vcd.as_bytes()).unwrap();
    tmp.flush().unwrap();

    let opts = wellen::LoadOptions {
        multi_thread: false,
        remove_scopes_with_empty_name: false,
    };
    let header =
        wellen::viewers::read_header_from_file(tmp.path(), &opts).expect("wellen header parse");
    let _body = wellen::viewers::read_body(header.body, &header.hierarchy, None)
        .expect("wellen body parse");
}

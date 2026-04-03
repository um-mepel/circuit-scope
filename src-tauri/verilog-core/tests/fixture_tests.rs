use std::fs;
use std::path::PathBuf;

use verilog_core::{analyze_project, parse_file};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn parses_simple_top_module_from_file() {
    let path = fixture("simple_top.v");
    let src = fs::read_to_string(&path).expect("read simple_top.v");
    let res = parse_file(path.to_string_lossy(), &src);
    assert!(
        res.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        res.diagnostics
    );
    assert_eq!(res.modules.len(), 1);
    assert_eq!(res.modules[0].name, "simple_top");
}

#[test]
fn parses_parameterized_ports_and_ranges_from_file() {
    let path = fixture("param_and_ranges.v");
    let src = fs::read_to_string(&path).expect("read param_and_ranges.v");
    let res = parse_file(path.to_string_lossy(), &src);
    assert!(
        res.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        res.diagnostics
    );
    assert_eq!(res.modules.len(), 1);
    let m = &res.modules[0];
    assert_eq!(m.name, "param_and_ranges");
    assert_eq!(m.ports.len(), 3);
}

#[test]
fn semantic_analyzer_identifies_top_in_hierarchy_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    let project = analyze_project(&root).expect("analyze_project on fixtures");
    assert!(
        project
            .modules
            .iter()
            .any(|m| m.name == "leaf" || m.name == "mid" || m.name == "top"),
        "expected modules from hierarchy.v"
    );
    assert!(
        project.top_modules.contains(&"top".to_string()),
        "expected 'top' as top-level, got {:?}",
        project.top_modules
    );
}

#[test]
fn reports_diagnostics_for_bad_header_fixture() {
    let path = fixture("bad_header.v");
    let src = fs::read_to_string(&path).expect("read bad_header.v");
    let res = parse_file(path.to_string_lossy(), &src);
    assert!(
        !res.diagnostics.is_empty(),
        "expected diagnostics for malformed header"
    );
    assert_eq!(res.modules.len(), 1, "should still recover a module");
    assert_eq!(res.modules[0].name, "bad_header");
}


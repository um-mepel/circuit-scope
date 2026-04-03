use std::fs;
use std::path::PathBuf;

use verilog_core::{build_ir_for_file, build_ir_for_root};

fn ir_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("ir_fixtures")
        .join(name)
}

#[test]
fn ir_for_comb_simple_has_single_assign() {
    let path = ir_fixture("ir_comb_simple.v");
    let src = fs::read_to_string(&path).expect("read ir_comb_simple.v");
    let ir = build_ir_for_file(path.to_string_lossy(), &src);
    assert!(ir.diagnostics.is_empty(), "IR diagnostics: {:?}", ir.diagnostics);
    assert_eq!(ir.modules.len(), 1);
    let m = &ir.modules[0];
    assert_eq!(m.name, "ir_comb_simple");
    assert_eq!(m.assigns.len(), 1);
}

#[test]
fn ir_for_comb_chain_has_single_assign() {
    let path = ir_fixture("ir_comb_chain.v");
    let src = fs::read_to_string(&path).expect("read ir_comb_chain.v");
    let ir = build_ir_for_file(path.to_string_lossy(), &src);
    assert!(ir.diagnostics.is_empty(), "IR diagnostics: {:?}", ir.diagnostics);
    assert_eq!(ir.modules.len(), 1);
    let m = &ir.modules[0];
    assert_eq!(m.name, "ir_comb_chain");
    assert_eq!(m.assigns.len(), 1);
}

#[test]
fn ir_for_hierarchy_collects_leaf_assign() {
    let path = ir_fixture("ir_hierarchy.v");
    let src = fs::read_to_string(&path).expect("read ir_hierarchy.v");
    let ir = build_ir_for_file(path.to_string_lossy(), &src);
    assert!(ir.diagnostics.is_empty(), "IR diagnostics: {:?}", ir.diagnostics);
    // Expect three modules: ir_leaf, ir_mid, ir_top.
    assert_eq!(ir.modules.len(), 3);
    let leaf = ir
        .modules
        .iter()
        .find(|m| m.name == "ir_leaf")
        .expect("ir_leaf");
    assert_eq!(leaf.assigns.len(), 1);
}

#[test]
fn ir_for_all_ir_fixtures_builds_without_fatal_errors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("ir_fixtures");
    let ir = build_ir_for_root(&root).expect("build_ir_for_root");
    // We at least expect some modules to be present; exact structure will be
    // refined as the IR evolves.
    assert!(
        !ir.modules.is_empty(),
        "expected some modules in IR project for fixtures"
    );
}


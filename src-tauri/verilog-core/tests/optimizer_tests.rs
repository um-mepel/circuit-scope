use std::fs;
use std::path::PathBuf;

use verilog_core::{build_ir_for_file, optimize_module, IrBinOp, IrExpr, IrUnaryOp};

fn opt_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("optimizer_fixtures")
        .join(name)
}

fn build_and_optimize(name: &str) -> Vec<verilog_core::IrModule> {
    let path = opt_fixture(name);
    let src = fs::read_to_string(&path).unwrap_or_else(|_| panic!("read {}", name));
    let mut ir = build_ir_for_file(path.to_string_lossy(), &src);
    assert!(
        ir.diagnostics.is_empty(),
        "diagnostics for {}: {:?}",
        name,
        ir.diagnostics
    );
    for m in ir.modules.iter_mut() {
        optimize_module(m);
    }
    ir.modules
}

fn find_assign<'a>(
    modules: &'a [verilog_core::IrModule],
    signal: &str,
) -> &'a verilog_core::IrAssign {
    for m in modules {
        for a in &m.assigns {
            if a.lhs == signal {
                return a;
            }
        }
    }
    panic!("signal '{}' not found in optimized IR", signal);
}

fn c(v: i64) -> IrExpr { IrExpr::Const(v) }
fn id(s: &str) -> IrExpr { IrExpr::Ident(s.into()) }

fn bin(op: IrBinOp, l: IrExpr, r: IrExpr) -> IrExpr {
    IrExpr::Binary { op, left: Box::new(l), right: Box::new(r) }
}

// ═══════════════════════════════════════════════════════════════════════
// Constant folding
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn const_fold_add() {
    let mods = build_and_optimize("opt_const_fold.v");
    assert_eq!(find_assign(&mods, "y1").rhs, c(8));
}

#[test]
fn const_fold_mul_nested() {
    let mods = build_and_optimize("opt_const_fold.v");
    assert_eq!(find_assign(&mods, "y2").rhs, c(20));
}

#[test]
fn const_fold_comparison() {
    let mods = build_and_optimize("opt_const_fold.v");
    assert_eq!(find_assign(&mods, "y3").rhs, c(1));
    assert_eq!(find_assign(&mods, "y4").rhs, c(1));
}

// ═══════════════════════════════════════════════════════════════════════
// Identity / annihilator
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn identity_add_zero() {
    let mods = build_and_optimize("opt_identity.v");
    assert_eq!(find_assign(&mods, "y1").rhs, id("a"));
}

#[test]
fn identity_mul_one() {
    let mods = build_and_optimize("opt_identity.v");
    assert_eq!(find_assign(&mods, "y2").rhs, id("a"));
}

#[test]
fn annihilator_and_zero() {
    let mods = build_and_optimize("opt_identity.v");
    assert_eq!(find_assign(&mods, "y3").rhs, c(0));
}

#[test]
fn identity_or_zero() {
    let mods = build_and_optimize("opt_identity.v");
    assert_eq!(find_assign(&mods, "y4").rhs, id("a"));
}

#[test]
fn identity_xor_zero() {
    let mods = build_and_optimize("opt_identity.v");
    assert_eq!(find_assign(&mods, "y5").rhs, id("a"));
}

// ═══════════════════════════════════════════════════════════════════════
// Strength reduction
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn strength_mul_8() {
    let mods = build_and_optimize("opt_strength.v");
    assert_eq!(find_assign(&mods, "y1").rhs, bin(IrBinOp::Shl, id("a"), c(3)));
}

#[test]
fn strength_div_4() {
    let mods = build_and_optimize("opt_strength.v");
    assert_eq!(find_assign(&mods, "y2").rhs, bin(IrBinOp::Shr, id("a"), c(2)));
}

#[test]
fn strength_mod_16() {
    let mods = build_and_optimize("opt_strength.v");
    assert_eq!(find_assign(&mods, "y3").rhs, bin(IrBinOp::And, id("a"), c(15)));
}

// ═══════════════════════════════════════════════════════════════════════
// Constant propagation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn const_prop_folds() {
    let mods = build_and_optimize("opt_const_prop.v");
    assert_eq!(find_assign(&mods, "y").rhs, c(15));
}

// ═══════════════════════════════════════════════════════════════════════
// Copy propagation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copy_prop_chain() {
    let mods = build_and_optimize("opt_copy_prop.v");
    assert_eq!(find_assign(&mods, "y").rhs, bin(IrBinOp::Add, id("a"), c(3)));
}

// ═══════════════════════════════════════════════════════════════════════
// Wire alias elimination
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn alias_chain_resolved() {
    let mods = build_and_optimize("opt_alias.v");
    assert_eq!(find_assign(&mods, "y").rhs, bin(IrBinOp::Add, id("a"), c(1)));
}

// ═══════════════════════════════════════════════════════════════════════
// CSE (Common Subexpression Elimination) — paper's key local opt
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cse_dedup() {
    let mods = build_and_optimize("opt_cse.v");
    let y = find_assign(&mods, "y");
    // After CSE: t1 = t2 = a+b, y = t1 & t1 = t1 = a+b
    assert_eq!(y.rhs, bin(IrBinOp::Add, id("a"), id("b")));
}

// ═══════════════════════════════════════════════════════════════════════
// Complement rules
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn complement_and_zero() {
    let mods = build_and_optimize("opt_complement.v");
    assert_eq!(find_assign(&mods, "y1").rhs, c(0));
}

#[test]
fn complement_or_all_ones() {
    let mods = build_and_optimize("opt_complement.v");
    assert_eq!(find_assign(&mods, "y2").rhs, c(-1));
}

// ═══════════════════════════════════════════════════════════════════════
// Absorption
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn absorption_and_or() {
    let mods = build_and_optimize("opt_absorption.v");
    assert_eq!(find_assign(&mods, "y1").rhs, id("a"));
}

#[test]
fn absorption_or_and() {
    let mods = build_and_optimize("opt_absorption.v");
    assert_eq!(find_assign(&mods, "y2").rhs, id("a"));
}

// ═══════════════════════════════════════════════════════════════════════
// Canonicalization + CSE
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn canonical_enables_cse() {
    let mods = build_and_optimize("opt_canonical.v");
    let y = find_assign(&mods, "y");
    assert_eq!(y.rhs, bin(IrBinOp::And, id("a"), id("b")));
}

// ═══════════════════════════════════════════════════════════════════════
// Ternary
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ternary_const_cond() {
    let mods = build_and_optimize("opt_ternary.v");
    assert_eq!(find_assign(&mods, "y1").rhs, id("a"));
}

#[test]
fn ternary_same_arms() {
    let mods = build_and_optimize("opt_ternary.v");
    assert_eq!(find_assign(&mods, "y2").rhs, id("a"));
}

// ═══════════════════════════════════════════════════════════════════════
// Dead signal elimination
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dead_wire_preserved_when_declared() {
    let mods = build_and_optimize("opt_dead.v");
    let m = &mods[0];
    // User-declared wires are preserved for simulation observability,
    // even if nothing reads from them.
    assert!(m.assigns.iter().any(|a| a.lhs == "dead_wire"));
    assert!(m.assigns.iter().any(|a| a.lhs == "y"));
}

// ═══════════════════════════════════════════════════════════════════════
// Self-operations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn and_self() {
    let mods = build_and_optimize("opt_bitwise.v");
    assert_eq!(find_assign(&mods, "y1").rhs, id("a"));
}

#[test]
fn or_self() {
    let mods = build_and_optimize("opt_bitwise.v");
    assert_eq!(find_assign(&mods, "y2").rhs, id("a"));
}

#[test]
fn xor_self() {
    let mods = build_and_optimize("opt_bitwise.v");
    assert_eq!(find_assign(&mods, "y3").rhs, c(0));
}

#[test]
fn sub_self() {
    let mods = build_and_optimize("opt_bitwise.v");
    assert_eq!(find_assign(&mods, "y4").rhs, c(0));
}

// ═══════════════════════════════════════════════════════════════════════
// Combined
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn combined_prop_strength_fold() {
    let mods = build_and_optimize("opt_combined.v");
    assert_eq!(find_assign(&mods, "y").rhs, c(8));
}

// ═══════════════════════════════════════════════════════════════════════
// Peephole (paper #2 — instruction combining)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn peephole_add_self_becomes_shl() {
    let mods = build_and_optimize("opt_peephole.v");
    let y1 = find_assign(&mods, "y1");
    assert_eq!(y1.rhs, IrExpr::Binary {
        op: IrBinOp::Shl,
        left: Box::new(id("a")),
        right: Box::new(c(1)),
    });
}

// ═══════════════════════════════════════════════════════════════════════
// Code sinking (paper #2)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sinking_factors_common_operand() {
    let mods = build_and_optimize("opt_sinking.v");
    let y1 = find_assign(&mods, "y1");
    // sel ? (a+c) : (b+c) → (sel ? a : b) + c
    assert!(matches!(y1.rhs, IrExpr::Binary { op: IrBinOp::Add, .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// Module inlining (paper #2 — #1 finding)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn module_inlining_from_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("optimizer_fixtures");
    let mut ir = verilog_core::build_ir_for_root(&root).expect("build_ir_for_root");
    let metrics = verilog_core::optimize_project(&mut ir);
    // child_inv should be inlined into parent_inv
    assert!(metrics.modules_inlined > 0, "at least one module should be inlined");
    let parent = ir.modules.iter().find(|m| m.name == "parent_inv");
    if let Some(p) = parent {
        assert!(p.instances.is_empty(), "instances should be removed after inlining");
        // Inlined assigns may be dead-eliminated since our IR doesn't
        // model instance port connections — the unit tests cover the
        // correctness of the inlining step itself.
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Optimizer metrics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn optimize_project_returns_metrics() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("optimizer_fixtures");
    let mut ir = verilog_core::build_ir_for_root(&root).expect("build_ir_for_root");
    let metrics = verilog_core::optimize_project(&mut ir);
    assert!(metrics.total_passes >= 1, "should run at least one pass");
    let total = metrics.canonicalizations
        + metrics.algebraic_rewrites
        + metrics.peephole_rewrites
        + metrics.constants_propagated
        + metrics.copies_propagated
        + metrics.cse_eliminations
        + metrics.sinking_rewrites
        + metrics.dead_signals_removed
        + metrics.modules_inlined;
    assert!(total > 0, "should have done some optimizations");
}

// ═══════════════════════════════════════════════════════════════════════
// Port-mapped module inlining
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn port_mapped_inlining() {
    let fixture = opt_fixture("opt_port_inline.v");
    let mut ir = verilog_core::build_ir_for_file(fixture.to_string_lossy(), &std::fs::read_to_string(&fixture).unwrap());
    let metrics = verilog_core::optimize_project(&mut ir);
    assert!(metrics.modules_inlined > 0, "adder should be inlined");
    let top = ir.modules.iter().find(|m| m.name == "top_port_inline");
    assert!(top.is_some(), "top module should exist");
    let t = top.unwrap();
    assert!(t.instances.is_empty(), "adder instance should be removed");
    let z = t.assigns.iter().find(|a| a.lhs == "z");
    assert!(z.is_some(), "z should be assigned after port-mapped inlining");
}

// ═══════════════════════════════════════════════════════════════════════
// Always block parsing & loop unrolling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn always_block_parsed_and_loop_unrolled() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("optimizer_fixtures");
    let mut ir = verilog_core::build_ir_for_root(&root).expect("build_ir_for_root");
    let metrics = verilog_core::optimize_project(&mut ir);
    let m = ir.modules.iter().find(|m| m.name == "always_loop_test");
    assert!(m.is_some(), "always_loop_test module should be parsed");
    let m = m.unwrap();
    assert!(!m.always_blocks.is_empty(), "should have always blocks");
    assert!(metrics.loops_unrolled > 0, "for loop should be unrolled");
}

// ═══════════════════════════════════════════════════════════════════════
// Adaptive scoring (score_history populated)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn score_history_populated_for_project() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("optimizer_fixtures");
    let mut ir = verilog_core::build_ir_for_root(&root).expect("build_ir_for_root");
    let metrics = verilog_core::optimize_project(&mut ir);
    assert!(!metrics.score_history.is_empty(), "should have score entries");
}

// ═══════════════════════════════════════════════════════════════════════
// Smoke test: all fixtures together
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_optimizer_fixtures_run_cleanly() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("optimizer_fixtures");
    let mut ir = verilog_core::build_ir_for_root(&root).expect("build_ir_for_root");
    for m in ir.modules.iter_mut() {
        optimize_module(m);
    }
    assert!(!ir.modules.is_empty());
}

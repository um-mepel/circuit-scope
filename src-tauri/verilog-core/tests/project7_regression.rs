//! Progressive regressions for EECS 270 Project 7 / FourFuncCalc-style FSMs.
//! - `p7_case_nba_fsm_tb` is repo-local (always runs).
//! - **Known-bug** coverage for carry chain / signed literals: see `regression_known_bugs.rs` (repo fixtures, always on).
//! - **Course tree** cases (`TestBench7`, `AddSub`+`FullAdder` from the project folder) run when files exist; else **skip** (no `#[ignore]`).
mod common;

use std::path::{Path, PathBuf};

use common::{find_var_code_in_scope, parse_binary_changes_for_code, value_at_or_before};

use verilog_core::{
    build_ir_for_file, build_ir_for_path_bufs, clock_half_period_fine_ticks, find_top_module,
    generate_vcd, num_cycles_from_initial_delay_sum_fine, optimize_project,
    resolve_instance_port_connections, scan_timescale_project, sum_initial_delays_for_source_files,
    unit_per_precision_ratio, IrProject, SimConfig,
};

fn simulate_project_files(paths: &[PathBuf]) -> Result<String, String> {
    let mut project = build_ir_for_path_bufs(paths).map_err(|e| e.to_string())?;
    optimize_project(&mut project);
    sim_config_for_project(&project, paths)
        .and_then(|cfg| generate_vcd(&project, &cfg))
}

fn sim_config_for_project(project: &IrProject, verilog_paths: &[PathBuf]) -> Result<SimConfig, String> {
    let top_name = find_top_module(project)?;
    let clock_half_period = 5usize;
    let clock_half_period_explicit = false;
    let ts_decl = scan_timescale_project(verilog_paths)?;
    let delay_files: Vec<PathBuf> = if ts_decl.timescale_source_files.is_empty() {
        project
            .modules
            .iter()
            .find(|m| m.name == top_name)
            .map(|m| PathBuf::from(&m.path))
            .into_iter()
            .collect()
    } else {
        ts_decl.timescale_source_files.clone()
    };
    let delay_sum = sum_initial_delays_for_source_files(project, &delay_files);
    let k = unit_per_precision_ratio(&ts_decl.time_unit, &ts_decl.time_precision);
    let h_fine = clock_half_period_fine_ticks(
        clock_half_period,
        k,
        clock_half_period_explicit,
        &ts_decl.time_unit,
    );
    let num_cycles = num_cycles_from_initial_delay_sum_fine(delay_sum, k, h_fine);
    Ok(SimConfig {
        top_module: top_name,
        num_cycles,
        timescale: ts_decl.time_unit.clone(),
        timescale_precision: ts_decl.time_precision.clone(),
        clock_half_period,
        clock_half_period_is_explicit: clock_half_period_explicit,
        initial_delay_sum_units: (delay_sum > 0).then_some(delay_sum),
        vcd_meta: None,
        ..Default::default()
    })
}

/// Repo-local minimal FSM (FourFuncCalc-style `always @*` + `<=` + `posedge` update).
#[test]
fn p7_minimal_case_nba_fsm_reaches_known_states() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let vpath = dir.join("tests/fixtures/p7_case_nba_fsm_tb.v");
    let src = std::fs::read_to_string(&vpath).unwrap();
    let mut project = build_ir_for_file(vpath.to_string_lossy(), &src);
    optimize_project(&mut project);
    let cfg = sim_config_for_project(&project, &[vpath.clone()]).expect("sim config");
    let vcd = generate_vcd(&project, &cfg).expect("vcd");
    let code = find_var_code_in_scope(&vcd, "p7_case_nba_fsm_tb", "X").expect("X var");
    let samples = parse_binary_changes_for_code(&vcd, &code);
    assert!(
        samples.iter().any(|&(_, v)| v == 4),
        "FSM should visit XDisp=4; got {:?}",
        samples
    );
    assert!(
        samples.iter().any(|&(_, v)| v == 3),
        "FSM should visit XAdd=3; got {:?}",
        samples
    );
}

/// Full testbench: **Icarus reference** has `Result==5` at **~90** and `Result==8` at **~170** (5+3 add).
/// **Skips** if `EECS270_PROJECT7` (or default path) does not contain the project sources.
#[test]
fn p7_testbench7_result_matches_icarus_reference_checkpoints() {
    let root = std::env::var("EECS270_PROJECT7").unwrap_or_else(|_| {
        "/Users/mihirepel/eecs270/Project 7".to_string()
    });
    let root = Path::new(&root);
    let paths: Vec<PathBuf> = [
        "FullAdder.v",
        "AddSub.v",
        "SM2TC.v",
        "TC2SM.v",
        "FourFuncCalc.v",
        "Binary_to_7SEG.v",
        "TestBench7.v",
    ]
    .iter()
    .map(|p| root.join(p))
    .collect();
    if paths.iter().any(|p| !p.is_file()) {
        eprintln!(
            "skip p7_testbench7_result_matches_icarus_reference_checkpoints: Project 7 tree not at {} (set EECS270_PROJECT7)",
            root.display()
        );
        return;
    }
    let vcd = simulate_project_files(&paths).expect("simulate");
    let code = find_var_code_in_scope(&vcd, "TestBench7", "Result").expect("Result var");
    let samples = parse_binary_changes_for_code(&vcd, &code);

    assert_eq!(
        value_at_or_before(&samples, 90),
        Some(5),
        "Icarus: Result is 5 after loading 5 (t≈90); samples={:?}",
        samples
    );
    assert_eq!(
        value_at_or_before(&samples, 170),
        Some(8),
        "Icarus: Result is 8 (5+3) at t≈170; csverilog samples={:?}",
        samples
    );
}

/// Isolated AddSub with constant wire inputs (EECS 270 `AddSub.v` + `FullAdder.v` + local TB).
/// **Skips** if course `FullAdder.v` / `AddSub.v` are not on disk. Also covered by
/// `regression_known_bugs::bug_carry_chain_*` with the repo-only W=4 fixture.
#[test]
fn addsub_const_inputs_five_plus_three_is_eight() {
    let root = std::env::var("EECS270_PROJECT7").unwrap_or_else(|_| {
        "/Users/mihirepel/eecs270/Project 7".to_string()
    });
    let root = Path::new(&root);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tb = dir.join("tests/fixtures/addsub_const_inputs_tb.v");
    let paths = vec![
        root.join("FullAdder.v"),
        root.join("AddSub.v"),
        tb.clone(),
    ];
    if paths.iter().any(|p| !p.is_file()) {
        eprintln!(
            "skip addsub_const_inputs_five_plus_three_is_eight: need FullAdder.v + AddSub.v at {} and {}",
            root.display(),
            tb.display()
        );
        return;
    }
    let mut project = build_ir_for_path_bufs(&paths).expect("build");
    let addsub = project
        .modules
        .iter()
        .find(|m| m.name.starts_with("AddSub__p_W_11"))
        .expect("AddSub specialized");
    let cnet = addsub.nets.iter().find(|n| n.name == "c").expect("carry c");
    assert_eq!(cnet.width, 12, "wire [W:0] c must be 12 bits for W=11");
    resolve_instance_port_connections(&mut project).expect("resolve");
    optimize_project(&mut project);
    let vcd = sim_config_for_project(&project, &paths)
        .and_then(|cfg| generate_vcd(&project, &cfg))
        .expect("simulate");
    let code = find_var_code_in_scope(&vcd, "addsub_const_inputs_tb", "S").expect("S var");
    let samples = parse_binary_changes_for_code(&vcd, &code);
    assert_eq!(
        value_at_or_before(&samples, 1),
        Some(8),
        "S should be 5+3=8 after comb settles; samples={:?}",
        samples
    );
}

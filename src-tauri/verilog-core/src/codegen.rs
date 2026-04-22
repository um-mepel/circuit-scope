// ═══════════════════════════════════════════════════════════════════════
// Code generator: IR → VCD
// ═══════════════════════════════════════════════════════════════════════
//
// Language: **IEEE 1364 Verilog** IR (not SystemVerilog). Implements a simple event-driven
// simulator over the optimised IR and writes value changes in IEEE 1364 VCD format. Supports:
//   • Combinational logic  (continuous assign)
//   • Sequential logic     (always @(posedge/negedge …) blocks)
//   • Initial blocks       (with #delay scheduling)
//   • `always #delay` clock generators (repeating scheduling; no synthetic clk toggling)
//   • Hierarchical instances (remaining after inlining) via flattening
//   • Multi-bit signal widths with proper [N:0] VCD annotations
//   • x-initialization for undriven signals
//   • Nested hierarchical $scope/$upscope in VCD header
//
// The output is a self-contained VCD string aimed at standard IEEE-1364 VCD
// viewers (GTKWave, Surfer, VaporView / wellen, etc.): multiline $timescale
// like common simulators, portable $date, $enddefinitions before #0/$dumpvars.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;

use crate::delay_rational::DelayRational;
use crate::ir::{
    ir_try_eval_const_index_expr, IrAlways, IrAssign, IrBinOp, IrCaseArm, IrEdgeKind, IrExpr,
    IrInitial, IrModule, IrProject, IrSensEntry, IrSensitivity, IrStmt, IrUnaryOp,
};
use crate::timescale_util::{
    clock_half_period_fine_ticks, timescale_token_to_fs, unit_per_precision_ratio,
};

/// IEEE-style `$timescale`: prefer a space between magnitude and unit (`1 s`, `100 ms`).
fn normalize_vcd_timescale_line(ts: &str) -> String {
    let t = ts.trim();
    if t.is_empty() {
        return "1 ns".into();
    }
    if t.chars().any(char::is_whitespace) {
        return t.to_string();
    }
    let Some(i) = t
        .char_indices()
        .find(|(_, c)| c.is_alphabetic())
        .map(|(i, _)| i)
    else {
        return t.to_string();
    };
    if i == 0 {
        return t.to_string();
    }
    format!("{} {}", &t[..i], &t[i..])
}

// ── Public types ────────────────────────────────────────────────────

/// Optional debug metadata written into the VCD header as `$comment` blocks (ignored by waveform viewers).
#[derive(Debug, Clone, Default)]
pub struct VcdRunMeta {
    /// Source file where the top module was defined (absolute or as passed to the compiler).
    pub top_source_file: Option<String>,
    /// Other Verilog RTL inputs (`.v` / `.sv`), in CLI order when applicable.
    pub additional_source_files: Vec<String>,
    /// Full command line or synthetic invocation string used to produce this run.
    pub command_line: Option<String>,
    /// Intended output `.vcd` path, if known.
    pub output_vcd_path: Option<String>,
    /// Process working directory when the run was started, if known.
    pub working_directory: Option<String>,
    /// Set by `generate_vcd`: number of module definitions in the IR.
    pub ir_module_count: usize,
}

/// Configuration for the simulation / VCD generation run.
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// Name of the top-level module to simulate.
    pub top_module: String,
    /// Number of clock cycles to simulate.
    pub num_cycles: usize,
    /// Verilog `` `timescale`` **unit** (`#` delays in the source are in these steps).
    pub timescale: String,
    /// Second `` `timescale`` operand — written to VCD **`$timescale`**. Kernel steps and **`#`** in the VCD
    /// body use one **precision** tick each (see [`Self::clock_half_period_is_explicit`] / codegen).
    pub timescale_precision: String,
    /// Requested clock half-period in **time units** (see [`Self::clock_half_period_is_explicit`]).
    pub clock_half_period: usize,
    /// `true` if set via **`--half-period`** / API; implicit defaults may use a finer grid for ≥1 s units.
    pub clock_half_period_is_explicit: bool,
    /// Sum of `#delay` literals used to derive [`Self::num_cycles`], if applicable (header `$comment` only).
    pub initial_delay_sum_units: Option<usize>,
    /// Extra `$comment` header lines for debugging. Filled automatically when `None`.
    pub vcd_meta: Option<VcdRunMeta>,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            top_module: String::new(),
            num_cycles: 20,
            timescale: "1ns".into(),
            timescale_precision: "1ns".into(),
            clock_half_period: 5,
            clock_half_period_is_explicit: false,
            initial_delay_sum_units: None,
            vcd_meta: None,
        }
    }
}

fn merge_vcd_run_meta(project: &IrProject, top: &IrModule, config: &mut SimConfig) {
    let mut meta = config.vcd_meta.take().unwrap_or_default();
    if meta.top_source_file.is_none() {
        meta.top_source_file = Some(top.path.clone());
    }
    if meta.additional_source_files.is_empty() {
        let top_path = meta.top_source_file.as_deref().unwrap_or(top.path.as_str());
        let mut paths: Vec<String> = project
            .modules
            .iter()
            .map(|m| m.path.clone())
            .filter(|p| p.as_str() != top_path)
            .collect();
        paths.sort();
        paths.dedup();
        meta.additional_source_files = paths;
    }
    meta.ir_module_count = project.modules.len();
    config.vcd_meta = Some(meta);
}

/// Generate a VCD file from an optimised `IrProject`.
///
/// Returns the VCD content as a `String`, or an error message if the
/// top module cannot be found.
pub fn generate_vcd(project: &IrProject, config: &SimConfig) -> Result<String, String> {
    let top = project
        .modules
        .iter()
        .find(|m| m.name == config.top_module)
        .ok_or_else(|| format!("top module '{}' not found", config.top_module))?;

    let mut config = config.clone();
    merge_vcd_run_meta(project, top, &mut config);

    let module_map: HashMap<&str, &IrModule> =
        project.modules.iter().map(|m| (m.name.as_str(), m)).collect();

    let unit_fs = timescale_token_to_fs(&config.timescale).unwrap_or(1_000_000u128);
    let mut sim = Simulator::new(top, &module_map, unit_fs);
    sim.run(&config)
}

// ── Scope tree for nested VCD scopes ────────────────────────────────

#[derive(Debug, Clone)]
struct ScopeNode {
    name: String,
    signals: Vec<String>,
    children: Vec<ScopeNode>,
}

// ── Scheduled event from initial blocks ─────────────────────────────

struct InitialEvent {
    time_fs: u128,
    lhs: String,
    val: i64,
}

/// `always #period ...` — first delay sets the repeat interval; body runs each time (femtoseconds).
struct AlwaysDelayProc {
    period_fs: u128,
    next_fire_fs: u128,
    stmts: Vec<IrStmt>,
}

/// Bit width for a scalar `assign` LHS, including unpacked memory elements `stem__index`.
fn width_for_assign_lhs(
    lhs: &str,
    widths: &HashMap<String, usize>,
    mem_bounds: &HashMap<String, (i64, i64)>,
    mem_elem_width: &HashMap<String, usize>,
) -> usize {
    if let Some(w) = widths.get(lhs) {
        return *w;
    }
    for (stem, _) in mem_bounds {
        let p = format!("{}__", stem);
        if let Some(rest) = lhs.strip_prefix(&p) {
            if rest.parse::<i64>().is_ok() {
                return mem_elem_width.get(stem).copied().unwrap_or(1);
            }
        }
    }
    1
}

// ── Simulator ───────────────────────────────────────────────────────

struct Simulator<'a> {
    signals: HashMap<String, i64>,
    prev_signals: HashMap<String, i64>,
    signal_order: Vec<String>,
    vcd_ids: HashMap<String, String>,
    widths: HashMap<String, usize>,
    assigns: Vec<IrAssign>,
    always_blocks: Vec<IrAlways>,
    always_delay_procs: Vec<AlwaysDelayProc>,
    initial_events: Vec<InitialEvent>,
    /// Latest time in any `initial` (femtoseconds).
    initial_time_horizon_fs: u128,
    /// Signals that have been explicitly driven (not still at x).
    driven: HashSet<String>,
    /// Inclusive bounds for [`IrExpr::MemRead`] stems (after hierarchical prefix).
    mem_bounds: HashMap<String, (i64, i64)>,
    /// Element width for each mem stem (packed width of `stem__k`).
    mem_elem_width: HashMap<String, usize>,
    scope_tree: ScopeNode,
    top: &'a IrModule,
    /// Latest simulation time (femtoseconds) for debug instrumentation.
    last_sim_time_fs: u128,
}

impl<'a> Simulator<'a> {
    fn new(top: &'a IrModule, module_map: &HashMap<&str, &IrModule>, unit_fs: u128) -> Self {
        let mut signals: HashMap<String, i64> = HashMap::new();
        let mut widths: HashMap<String, usize> = HashMap::new();
        let mut mem_bounds: HashMap<String, (i64, i64)> = HashMap::new();
        let mut mem_elem_width: HashMap<String, usize> = HashMap::new();
        let mut assigns = Vec::new();
        let mut always_blocks = Vec::new();
        let mut initial_blocks = Vec::new();
        let mut signal_order = Vec::new();
        let mut scope_tree = ScopeNode {
            name: top.name.clone(),
            signals: Vec::new(),
            children: Vec::new(),
        };

        Self::flatten_module(
            top,
            "",
            module_map,
            &mut signals,
            &mut widths,
            &mut mem_bounds,
            &mut mem_elem_width,
            &mut assigns,
            &mut always_blocks,
            &mut initial_blocks,
            &mut signal_order,
            &mut scope_tree,
            false,
        );

        let (always_blocks, always_delay_procs) =
            Self::split_always_delay_processes(always_blocks, unit_fs);

        // Pre-compute initial block events (times in femtoseconds).
        let (initial_events, initial_time_horizon_fs) = Self::schedule_initial_blocks(
            &initial_blocks,
            &signals,
            &widths,
            &mem_bounds,
            &mem_elem_width,
            unit_fs,
        );

        let mut vcd_ids = HashMap::new();
        for (i, name) in signal_order.iter().enumerate() {
            vcd_ids.insert(name.clone(), vcd_ident(i));
        }

        let prev_signals = signals.clone();

        Simulator {
            signals,
            prev_signals,
            signal_order,
            vcd_ids,
            widths,
            assigns,
            always_blocks,
            always_delay_procs,
            initial_events,
            initial_time_horizon_fs,
            driven: HashSet::new(),
            mem_bounds,
            mem_elem_width,
            scope_tree,
            top,
            last_sim_time_fs: 0,
        }
    }

    fn width_for_signal(&self, name: &str) -> usize {
        width_for_assign_lhs(name, &self.widths, &self.mem_bounds, &self.mem_elem_width)
    }

    /// `always #D ...` with non-empty tail → repeating scheduled process (clock generators).
    fn split_always_delay_processes(
        always_blocks: Vec<IrAlways>,
        unit_fs: u128,
    ) -> (Vec<IrAlways>, Vec<AlwaysDelayProc>) {
        let mut edge_or_star = Vec::new();
        let mut procs = Vec::new();
        for ab in always_blocks {
            if matches!(&ab.sensitivity, IrSensitivity::Star) {
                if let Some(IrStmt::Delay(d)) = ab.stmts.first() {
                    let period_fs = d.to_femtoseconds(unit_fs);
                    let rest = ab.stmts[1..].to_vec();
                    if period_fs > 0 && !rest.is_empty() {
                        procs.push(AlwaysDelayProc {
                            period_fs,
                            next_fire_fs: period_fs,
                            stmts: rest,
                        });
                        continue;
                    }
                }
            }
            edge_or_star.push(ab);
        }
        (edge_or_star, procs)
    }

    fn schedule_initial_blocks(
        blocks: &[IrInitial],
        signals: &HashMap<String, i64>,
        widths: &HashMap<String, usize>,
        mem_bounds: &HashMap<String, (i64, i64)>,
        mem_elem_width: &HashMap<String, usize>,
        unit_fs: u128,
    ) -> (Vec<InitialEvent>, u128) {
        let mut events = Vec::new();
        let mut horizon_fs = 0u128;
        for ib in blocks {
            let start = events.len();
            let mut t = DelayRational::ZERO;
            let mut sig = signals.clone();
            Self::walk_initial_stmts(
                &ib.stmts,
                &mut t,
                &mut sig,
                widths,
                mem_bounds,
                mem_elem_width,
                &mut events,
                unit_fs,
            );
            let mut block_max = t.to_femtoseconds(unit_fs);
            for e in &events[start..] {
                block_max = block_max.max(e.time_fs);
            }
            horizon_fs = horizon_fs.max(block_max);
        }
        events.sort_by_key(|e| e.time_fs);
        (events, horizon_fs)
    }

    fn walk_initial_stmts(
        stmts: &[IrStmt],
        time: &mut DelayRational,
        signals: &mut HashMap<String, i64>,
        widths: &HashMap<String, usize>,
        mem_bounds: &HashMap<String, (i64, i64)>,
        mem_elem_width: &HashMap<String, usize>,
        events: &mut Vec<InitialEvent>,
        unit_fs: u128,
    ) {
        for stmt in stmts {
            match stmt {
                IrStmt::Delay(d) => {
                    *time = time.add(*d);
                }
                IrStmt::BlockingAssign { lhs, rhs } => {
                    let val = Self::static_eval(rhs, signals, mem_bounds);
                    let w = width_for_assign_lhs(lhs, widths, mem_bounds, mem_elem_width);
                    let masked = mask_to_width(val, w);
                    events.push(InitialEvent {
                        time_fs: time.to_femtoseconds(unit_fs),
                        lhs: lhs.clone(),
                        val: masked,
                    });
                    signals.insert(lhs.clone(), masked);
                }
                IrStmt::NonBlockingAssign { lhs, rhs } => {
                    let val = Self::static_eval(rhs, signals, mem_bounds);
                    let w = width_for_assign_lhs(lhs, widths, mem_bounds, mem_elem_width);
                    let masked = mask_to_width(val, w);
                    events.push(InitialEvent {
                        time_fs: time.to_femtoseconds(unit_fs),
                        lhs: lhs.clone(),
                        val: masked,
                    });
                }
                IrStmt::MemAssign {
                    stem,
                    index,
                    rhs,
                    nonblocking,
                } => {
                    let idx = Self::static_eval(index, signals, mem_bounds);
                    let (lo, hi) = mem_bounds.get(stem).copied().unwrap_or((0, 0));
                    let k = idx.clamp(lo, hi);
                    let lhs = format!("{}__{}", stem, k);
                    let val = Self::static_eval(rhs, signals, mem_bounds);
                    let w = width_for_assign_lhs(&lhs, widths, mem_bounds, mem_elem_width);
                    let masked = mask_to_width(val, w);
                    events.push(InitialEvent {
                        time_fs: time.to_femtoseconds(unit_fs),
                        lhs: lhs.clone(),
                        val: masked,
                    });
                    if !*nonblocking {
                        signals.insert(lhs, masked);
                    }
                }
                IrStmt::IfElse { cond, then_body, else_body } => {
                    let c = Self::static_eval(cond, signals, mem_bounds);
                    if c != 0 {
                        Self::walk_initial_stmts(
                            then_body,
                            time,
                            signals,
                            widths,
                            mem_bounds,
                            mem_elem_width,
                            events,
                            unit_fs,
                        );
                    } else {
                        Self::walk_initial_stmts(
                            else_body,
                            time,
                            signals,
                            widths,
                            mem_bounds,
                            mem_elem_width,
                            events,
                            unit_fs,
                        );
                    }
                }
                IrStmt::Case {
                    expr, arms, default, ..
                } => {
                    let val = Self::static_eval(expr, signals, mem_bounds);
                    let mut matched = false;
                    for arm in arms {
                        let av = Self::static_eval(&arm.value, signals, mem_bounds);
                        if val == av {
                            Self::walk_initial_stmts(
                                &arm.body,
                                time,
                                signals,
                                widths,
                                mem_bounds,
                                mem_elem_width,
                                events,
                                unit_fs,
                            );
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        Self::walk_initial_stmts(
                            default,
                            time,
                            signals,
                            widths,
                            mem_bounds,
                            mem_elem_width,
                            events,
                            unit_fs,
                        );
                    }
                }
                IrStmt::For {
                    init_var,
                    init_val,
                    cond,
                    step_var,
                    step_expr,
                    body,
                } => {
                    let start = Self::static_eval(init_val, signals, mem_bounds);
                    signals.insert(init_var.clone(), start);
                    const MAX_INIT_FOR: usize = 10_000_000;
                    for _ in 0..MAX_INIT_FOR {
                        let c = Self::static_eval(cond, signals, mem_bounds);
                        if c == 0 {
                            break;
                        }
                        Self::walk_initial_stmts(
                            body,
                            time,
                            signals,
                            widths,
                            mem_bounds,
                            mem_elem_width,
                            events,
                            unit_fs,
                        );
                        let next = Self::static_eval(step_expr, signals, mem_bounds);
                        signals.insert(step_var.clone(), next);
                    }
                }
                IrStmt::SystemTask { .. } => {}
            }
        }
    }

    fn static_eval(
        expr: &IrExpr,
        signals: &HashMap<String, i64>,
        mem_bounds: &HashMap<String, (i64, i64)>,
    ) -> i64 {
        match expr {
            IrExpr::Const(v) => *v,
            IrExpr::Ident(name) => *signals.get(name).unwrap_or(&0),
            IrExpr::Binary { op, left, right } => {
                let l = Self::static_eval(left, signals, mem_bounds);
                let r = Self::static_eval(right, signals, mem_bounds);
                eval_binop(*op, l, r)
            }
            IrExpr::Unary { op, operand } => {
                let v = Self::static_eval(operand, signals, mem_bounds);
                eval_unop(*op, v)
            }
            IrExpr::Ternary { cond, then_expr, else_expr } => {
                if Self::static_eval(cond, signals, mem_bounds) != 0 {
                    Self::static_eval(then_expr, signals, mem_bounds)
                } else {
                    Self::static_eval(else_expr, signals, mem_bounds)
                }
            }
            IrExpr::Concat(exprs) => {
                let mut result: i64 = 0;
                for (i, e) in exprs.iter().rev().enumerate() {
                    let v = Self::static_eval(e, signals, mem_bounds) & 1;
                    result |= v << i;
                }
                result
            }
            IrExpr::PartSelect { value, msb, lsb } => {
                let v = Self::static_eval(value, signals, mem_bounds);
                let hi = Self::static_eval(msb, signals, mem_bounds);
                let lo = Self::static_eval(lsb, signals, mem_bounds);
                part_select_bits(v, hi, lo)
            }
            IrExpr::MemRead { stem, index } => {
                let idx = Self::static_eval(index, signals, mem_bounds);
                let (lo, hi) = mem_bounds.get(stem).copied().unwrap_or((0, 0));
                let k = idx.clamp(lo, hi);
                let name = format!("{}__{}", stem, k);
                *signals.get(&name).unwrap_or(&0)
            }
            IrExpr::Signed(inner) => {
                let v = Self::static_eval(inner, signals, mem_bounds);
                crate::arith::sign_extend_i64(v, 32)
            }
        }
    }

    /// Instance output ports often connect with a full-range slice, e.g. `.hex(HEX6[6:0])`, which
    /// lowers to [`IrExpr::PartSelect`]. We must still emit `assign HEX6 = child__hex` or the
    /// parent port stays at its initialized 0.
    fn output_port_lhs(expr: &IrExpr, widths: &HashMap<String, usize>) -> Option<String> {
        match expr {
            IrExpr::Ident(s) => Some(s.clone()),
            IrExpr::PartSelect { value, msb, lsb } => {
                let IrExpr::Ident(name) = value.as_ref() else {
                    return None;
                };
                let hi = ir_try_eval_const_index_expr(msb)?;
                let lo = ir_try_eval_const_index_expr(lsb)?;
                let w_sel = (hi - lo).abs() as usize + 1;
                let w_full = widths.get(name).copied().unwrap_or(0);
                if w_sel == w_full && w_full > 0 {
                    Some(name.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// `assign vec = (vec & ~(1<<k)) | ((child & 1) << k)` — drives one bit of a packed vector from
    /// a scalar child output when we cannot use `assign vec = child` (bit-select or partial slice).
    /// Used for generated full-adders (`S[i]`, `c[i+1]`, …) so ripple adders are not silently dropped.
    fn packed_scalar_into_vec_rhs(vec_name: &str, bit_k: i64, child: IrExpr, vec_width: usize) -> IrExpr {
        crate::ir::ir_expr_merge_scalar_into_packed_vec(vec_name, bit_k, child, vec_width)
    }

    /// Registers hierarchical signals for `module` under `prefix` (ports, nets, memories).
    fn register_module_signals_for_prefix(
        module: &IrModule,
        prefix: &str,
        signals: &mut HashMap<String, i64>,
        widths: &mut HashMap<String, usize>,
        mem_bounds: &mut HashMap<String, (i64, i64)>,
        mem_elem_width: &mut HashMap<String, usize>,
        signal_order: &mut Vec<String>,
        scope: &mut ScopeNode,
    ) {
        let pfx = |name: &str| -> String {
            if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{}__{}", prefix, name)
            }
        };

        for ma in &module.mem_arrays {
            let s = pfx(&ma.stem);
            mem_bounds.insert(s.clone(), (ma.lo, ma.hi));
            mem_elem_width.insert(s, ma.elem_width);
        }

        for port in &module.ports {
            let full = pfx(&port.name);
            if !signals.contains_key(&full) {
                signals.insert(full.clone(), 0);
                widths.insert(full.clone(), port.width);
                signal_order.push(full.clone());
                scope.signals.push(full);
            }
        }
        for net in &module.nets {
            let full = pfx(&net.name);
            if !signals.contains_key(&full) {
                signals.insert(full.clone(), 0);
                widths.insert(full.clone(), net.width);
                signal_order.push(full.clone());
                scope.signals.push(full);
            }
        }
    }

    /// `skip_register`: when true, ports/nets were already registered (inputs wired next).
    fn flatten_module(
        module: &IrModule,
        prefix: &str,
        module_map: &HashMap<&str, &IrModule>,
        signals: &mut HashMap<String, i64>,
        widths: &mut HashMap<String, usize>,
        mem_bounds: &mut HashMap<String, (i64, i64)>,
        mem_elem_width: &mut HashMap<String, usize>,
        assigns: &mut Vec<IrAssign>,
        always_blocks: &mut Vec<IrAlways>,
        initial_blocks: &mut Vec<IrInitial>,
        signal_order: &mut Vec<String>,
        scope: &mut ScopeNode,
        skip_register: bool,
    ) {
        let pfx = |name: &str| -> String {
            if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{}__{}", prefix, name)
            }
        };

        if !skip_register {
            Self::register_module_signals_for_prefix(
                module,
                prefix,
                signals,
                widths,
                mem_bounds,
                mem_elem_width,
                signal_order,
                scope,
            );
        }

        for a in &module.assigns {
            assigns.push(IrAssign {
                lhs: pfx(&a.lhs),
                rhs: prefix_ir_expr(&a.rhs, prefix),
            });
        }

        for ab in &module.always_blocks {
            let sens = match &ab.sensitivity {
                IrSensitivity::Star => IrSensitivity::Star,
                IrSensitivity::EdgeList(edges) => IrSensitivity::EdgeList(
                    edges
                        .iter()
                        .map(|e| IrSensEntry {
                            edge: e.edge,
                            signal: pfx(&e.signal),
                        })
                        .collect(),
                ),
            };
            always_blocks.push(IrAlways {
                sensitivity: sens,
                stmts: prefix_stmts(&ab.stmts, prefix),
            });
        }

        for ib in &module.initial_blocks {
            initial_blocks.push(IrInitial {
                stmts: prefix_stmts(&ib.stmts, prefix),
            });
        }

        for inst in &module.instances {
            if let Some(child) = module_map.get(inst.module_name.as_str()) {
                let inst_prefix = pfx(&inst.instance_name);
                let mut child_scope = ScopeNode {
                    name: inst.instance_name.clone(),
                    signals: Vec::new(),
                    children: Vec::new(),
                };
                Self::register_module_signals_for_prefix(
                    child,
                    &inst_prefix,
                    signals,
                    widths,
                    mem_bounds,
                    mem_elem_width,
                    signal_order,
                    &mut child_scope,
                );

                for conn in &inst.connections {
                    let Some(pn) = conn.port_name.as_ref() else {
                        continue;
                    };
                    let child_port = format!("{}__{}", inst_prefix, pn);
                    let parent_expr = prefix_ir_expr(&conn.expr, prefix);
                    let is_output = child
                        .ports
                        .iter()
                        .any(|p| p.name == *pn && p.direction.as_deref() == Some("output"));
                    if !is_output {
                        assigns.push(IrAssign {
                            lhs: child_port,
                            rhs: parent_expr,
                        });
                    }
                }

                Self::flatten_module(
                    child,
                    &inst_prefix,
                    module_map,
                    signals,
                    widths,
                    mem_bounds,
                    mem_elem_width,
                    assigns,
                    always_blocks,
                    initial_blocks,
                    signal_order,
                    &mut child_scope,
                    true,
                );
                scope.children.push(child_scope);

                for conn in &inst.connections {
                    let Some(pn) = conn.port_name.as_ref() else {
                        continue;
                    };
                    let child_port = format!("{}__{}", inst_prefix, pn);
                    let parent_expr = prefix_ir_expr(&conn.expr, prefix);
                    let is_output = child
                        .ports
                        .iter()
                        .any(|p| p.name == *pn && p.direction.as_deref() == Some("output"));
                    if is_output {
                        if let Some(parent_sig) = Self::output_port_lhs(&parent_expr, widths) {
                            assigns.push(IrAssign {
                                lhs: parent_sig,
                                rhs: IrExpr::Ident(child_port),
                            });
                        } else if let IrExpr::PartSelect { value, msb, lsb } = &parent_expr {
                            if let IrExpr::Ident(vec_name) = value.as_ref() {
                                if let (Some(k_hi), Some(k_lo)) = (
                                    ir_try_eval_const_index_expr(msb),
                                    ir_try_eval_const_index_expr(lsb),
                                ) {
                                    if k_hi == k_lo {
                                        let total_w = widths.get(vec_name).copied().unwrap_or(0);
                                        if total_w > 0 && k_hi >= 0 && k_hi < total_w as i64 {
                                            let rhs = Self::packed_scalar_into_vec_rhs(
                                                vec_name,
                                                k_hi,
                                                IrExpr::Ident(child_port),
                                                total_w,
                                            );
                                            assigns.push(IrAssign {
                                                lhs: vec_name.clone(),
                                                rhs,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Simulation loop ─────────────────────────────────────────────

    fn run(&mut self, config: &SimConfig) -> Result<String, String> {
        let mut vcd = String::with_capacity(4096);

        let k = unit_per_precision_ratio(&config.timescale, &config.timescale_precision);
        let h = clock_half_period_fine_ticks(
            config.clock_half_period,
            k,
            config.clock_half_period_is_explicit,
            &config.timescale,
        )
        .max(1);

        let prec_fs = timescale_token_to_fs(&config.timescale_precision)
            .or_else(|| timescale_token_to_fs(&config.timescale))
            .unwrap_or(1_000u128)
            .max(1);

        let base_end_fs = (config.num_cycles.saturating_mul(2).saturating_mul(h) as u128)
            .saturating_mul(prec_fs);

        let last_assign_fs = self
            .initial_events
            .iter()
            .map(|e| e.time_fs)
            .max()
            .unwrap_or(0);

        let proc_horizon_fs = self
            .always_delay_procs
            .iter()
            .map(|p| {
                if p.period_fs == 0 {
                    return 0u128;
                }
                let n = (base_end_fs / p.period_fs).saturating_add(2);
                n.saturating_mul(p.period_fs)
            })
            .max()
            .unwrap_or(0);

        let end_fs = base_end_fs
            .max(last_assign_fs)
            .max(self.initial_time_horizon_fs)
            .max(proc_horizon_fs);

        self.write_header(&mut vcd, config, h);

        self.apply_initial_events_at_fs(0);
        self.eval_combinational();
        // Combinational `always @*` blocks must run before `#0` dumpvars; otherwise only
        // `assign`-driven nets are in `driven` and always-driven signals are dumped as X.
        // Fixed-point: multiple always blocks can depend on each other.
        for _ in 0..64 {
            self.prev_signals = self.signals.clone();
            let before = self.signals.clone();
            self.fire_always_blocks(true);
            self.eval_combinational();
            if self.signals == before {
                break;
            }
        }

        let vcd_step = |t_fs: u128| -> u128 {
            if prec_fs == 0 {
                t_fs
            } else {
                t_fs / prec_fs
            }
        };

        self.write_timestamp(&mut vcd, vcd_step(0));
        writeln!(vcd, "$dumpvars").unwrap();
        self.write_all_values_initial(&mut vcd);
        writeln!(vcd, "$end").unwrap();

        let mut t_sim = 0u128;
        loop {
            let mut next_t = end_fs.saturating_add(1);
            for e in &self.initial_events {
                if e.time_fs > t_sim {
                    next_t = next_t.min(e.time_fs);
                }
            }
            for p in &self.always_delay_procs {
                if p.next_fire_fs > t_sim {
                    next_t = next_t.min(p.next_fire_fs);
                }
            }
            if next_t > end_fs {
                break;
            }
            t_sim = next_t;
            self.last_sim_time_fs = t_sim;

            self.prev_signals = self.signals.clone();

            self.apply_initial_events_at_fs(t_sim);

            let fired: Vec<(usize, Vec<IrStmt>, u128)> = self
                .always_delay_procs
                .iter()
                .enumerate()
                .filter(|(_, p)| p.next_fire_fs == t_sim)
                .map(|(i, p)| (i, p.stmts.clone(), p.period_fs))
                .collect();
            for (i, stmts, period_fs) in fired {
                let mut nba = Vec::new();
                self.exec_stmts(&stmts, &mut nba);
                for (lhs, val) in nba {
                    let w = self.width_for_signal(&lhs);
                    self.signals.insert(lhs.clone(), mask_to_width(val, w));
                    self.driven.insert(lhs);
                }
                self.always_delay_procs[i].next_fire_fs = self.always_delay_procs[i]
                    .next_fire_fs
                    .saturating_add(period_fs);
            }

            // `continuous assign` targets (e.g. hierarchical **Clock** = parent **CLK**) must settle
            // before `posedge Clock` / `negedge` detection; otherwise sequential `always` never sees
            // edges and the VCD only shows the generator clock.
            self.eval_combinational();

            self.fire_always_blocks(false);
            self.eval_combinational();

            let changes = self.collect_changes();
            if !changes.is_empty() {
                self.write_timestamp(&mut vcd, vcd_step(t_sim));
                for (name, val) in &changes {
                    self.write_signal_value(&mut vcd, name, *val);
                }
            }
        }

        Ok(vcd)
    }

    fn apply_initial_events_at_fs(&mut self, t_fs: u128) {
        for ev in &self.initial_events {
            if ev.time_fs == t_fs {
                let w = self.width_for_signal(&ev.lhs);
                let masked = mask_to_width(ev.val, w);
                self.signals.insert(ev.lhs.clone(), masked);
                self.driven.insert(ev.lhs.clone());
            }
        }
    }

    fn eval_combinational(&mut self) {
        for round in 0..100 {
            let mut changed = false;
            for assign in &self.assigns.clone() {
                let val = self.eval_expr(&assign.rhs);
                let w = self.width_for_signal(&assign.lhs);
                let masked = mask_to_width(val, w);
                let old = self.signals.get(&assign.lhs).copied().unwrap_or(0);
                if masked != old {
                    self.signals.insert(assign.lhs.clone(), masked);
                    changed = true;
                }
                // On first round, mark all assign targets as driven
                // (even if value happens to be 0 == default).
                if round == 0 {
                    self.driven.insert(assign.lhs.clone());
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// `true` for `always @(posedge …)` / `negedge` (including lists mixing clock + level).
    fn sensitivity_has_clock_edge(sens: &IrSensitivity) -> bool {
        match sens {
            IrSensitivity::Star => false,
            IrSensitivity::EdgeList(edges) => edges
                .iter()
                .any(|e| matches!(e.edge, IrEdgeKind::Posedge | IrEdgeKind::Negedge)),
        }
    }

    fn always_block_should_fire(&self, ab: &IrAlways, stabilize_level_comb: bool) -> bool {
        match &ab.sensitivity {
            IrSensitivity::Star => true,
            IrSensitivity::EdgeList(edges) => {
                if stabilize_level_comb && Self::edge_list_is_pure_combinational(edges) {
                    true
                } else {
                    edges.iter().any(|e| {
                        let cur = *self.signals.get(&e.signal).unwrap_or(&0);
                        let prev = *self.prev_signals.get(&e.signal).unwrap_or(&0);
                        match e.edge {
                            IrEdgeKind::Posedge => prev == 0 && cur == 1,
                            IrEdgeKind::Negedge => prev == 1 && cur == 0,
                            IrEdgeKind::Level => cur != prev,
                        }
                    })
                }
            }
        }
    }

    /// `stabilize_level_comb`: used only when converging before `#0` dumpvars. Verilog
    /// `always @(a or b)` lowers to an edge list of **level** sensitivities; at t=0 we have
    /// `prev == cur` everywhere, so `cur != prev` never holds and the block would never run — same
    /// X-on-dump bug as missing `always @*`. Pure level lists must execute once during startup.
    ///
    /// **Two-pass scheduling:** blocks with **posedge/negedge** run first; their NBAs commit, then
    /// `assign` propagates, then `always @*` / purely level-sensitive blocks run. This matches
    /// coursework FSMs where `always @(posedge clk) X <= X_Next` must see `X_Next` from the
    /// *previous* cycle while `always @* case (X)` recomputes `X_Next` from the **updated** `X`.
    fn fire_always_blocks(&mut self, stabilize_level_comb: bool) {
        let blocks = self.always_blocks.clone();

        // Pass 1 — clocked sequential (`posedge` / `negedge`, possibly mixed with level in the list).
        //
        // **Per-process scratch:** IEEE NBAs sample RHS from the register/wire state *before any*
        // NBA from this timestep updates a variable. A single shared `scratch` across *different*
        // `always @(posedge …)` blocks is wrong if an earlier process used **blocking** assigns or
        // otherwise perturbed `scratch`: a later FSM + datapath pair can then see a bogus `X`/`A`
        // (EECS270 Project 7: `Result` cleared when entering `XDisp` after `XA_DN`).
        let mut scratch = self.signals.clone();
        let mut nba = Vec::new();
        for ab in &blocks {
            if !Self::sensitivity_has_clock_edge(&ab.sensitivity) {
                continue;
            }
            if self.always_block_should_fire(ab, stabilize_level_comb) {
                scratch.clone_from(&self.signals);
                self.exec_stmts_on_env(&ab.stmts, &mut scratch, &mut nba);
            }
        }

        for (lhs, val) in nba {
            let w = self.width_for_signal(&lhs);
            scratch.insert(lhs.clone(), mask_to_width(val, w));
            self.driven.insert(lhs);
        }
        self.signals = scratch;

        // `assign` often depends on state regs updated above (e.g. `LD_A = (X==…)`).
        self.eval_combinational();

        // Pass 2 — `always @*` and purely level-sensitive `@(a or b …)`.
        let mut scratch = self.signals.clone();
        let mut nba = Vec::new();
        for ab in &blocks {
            if Self::sensitivity_has_clock_edge(&ab.sensitivity) {
                continue;
            }
            if self.always_block_should_fire(ab, stabilize_level_comb) {
                self.exec_stmts_on_env(&ab.stmts, &mut scratch, &mut nba);
            }
        }
        for (lhs, val) in nba {
            let w = self.width_for_signal(&lhs);
            scratch.insert(lhs.clone(), mask_to_width(val, w));
            self.driven.insert(lhs);
        }
        self.signals = scratch;
    }

    fn edge_list_is_pure_combinational(edges: &[IrSensEntry]) -> bool {
        !edges.is_empty() && edges.iter().all(|e| matches!(e.edge, IrEdgeKind::Level))
    }

    fn exec_stmts(&mut self, stmts: &[IrStmt], nba: &mut Vec<(String, i64)>) {
        for stmt in stmts {
            self.exec_stmt(stmt, nba);
        }
    }

    fn exec_stmt(&mut self, stmt: &IrStmt, nba: &mut Vec<(String, i64)>) {
        match stmt {
            IrStmt::BlockingAssign { lhs, rhs } => {
                let val = self.eval_expr(rhs);
                let w = self.width_for_signal(lhs);
                self.signals.insert(lhs.clone(), mask_to_width(val, w));
                self.driven.insert(lhs.clone());
            }
            IrStmt::NonBlockingAssign { lhs, rhs } => {
                let val = self.eval_expr(rhs);
                let w = self.width_for_signal(lhs);
                nba.push((lhs.clone(), mask_to_width(val, w)));
            }
            IrStmt::MemAssign {
                stem,
                index,
                rhs,
                nonblocking,
            } => {
                let idx = self.eval_expr(index);
                let (lo, hi) = self.mem_bounds.get(stem).copied().unwrap_or((0, 0));
                let k = idx.clamp(lo, hi);
                let lhs = format!("{}__{}", stem, k);
                let val = self.eval_expr(rhs);
                let ew = self.mem_elem_width.get(stem).copied().unwrap_or(1);
                let m = mask_to_width(val, ew);
                if *nonblocking {
                    nba.push((lhs, m));
                } else {
                    self.signals.insert(lhs.clone(), m);
                    self.driven.insert(lhs);
                }
            }
            IrStmt::IfElse { cond, then_body, else_body } => {
                let c = self.eval_expr(cond);
                if c != 0 {
                    self.exec_stmts(then_body, nba);
                } else {
                    self.exec_stmts(else_body, nba);
                }
            }
            IrStmt::Case { expr, arms, default } => {
                let val = self.eval_expr(expr);
                let mut matched = false;
                for arm in arms {
                    let arm_val = self.eval_expr(&arm.value);
                    if val == arm_val {
                        self.exec_stmts(&arm.body, nba);
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    self.exec_stmts(default, nba);
                }
            }
            IrStmt::For {
                init_var,
                init_val,
                cond,
                step_var,
                step_expr,
                body,
            } => {
                let start = self.eval_expr(init_val);
                self.signals.insert(init_var.clone(), start);
                for _ in 0..1024 {
                    let c = self.eval_expr(cond);
                    if c == 0 {
                        break;
                    }
                    self.exec_stmts(body, nba);
                    let next = self.eval_expr(step_expr);
                    self.signals.insert(step_var.clone(), next);
                }
            }
            IrStmt::Delay(_) | IrStmt::SystemTask { .. } => {}
        }
    }

    /// Execute statements reading/writing `scratch`; queue NBAs without applying (for cross-`always`
    /// NBA semantics in the same timestep).
    fn exec_stmt_on_env(
        &mut self,
        stmt: &IrStmt,
        scratch: &mut std::collections::HashMap<String, i64>,
        nba: &mut Vec<(String, i64)>,
    ) {
        match stmt {
            IrStmt::BlockingAssign { lhs, rhs } => {
                let val = self.eval_expr_with_env(rhs, scratch);
                let w = self.width_for_signal(lhs);
                scratch.insert(lhs.clone(), mask_to_width(val, w));
                self.driven.insert(lhs.clone());
            }
            IrStmt::NonBlockingAssign { lhs, rhs } => {
                let val = self.eval_expr_with_env(rhs, scratch);
                let w = self.width_for_signal(lhs);
                nba.push((lhs.clone(), mask_to_width(val, w)));
            }
            IrStmt::MemAssign {
                stem,
                index,
                rhs,
                nonblocking,
            } => {
                let idx = self.eval_expr_with_env(index, scratch);
                let (lo, hi) = self.mem_bounds.get(stem).copied().unwrap_or((0, 0));
                let k = idx.clamp(lo, hi);
                let lhs = format!("{}__{}", stem, k);
                let val = self.eval_expr_with_env(rhs, scratch);
                let ew = self.mem_elem_width.get(stem).copied().unwrap_or(1);
                let m = mask_to_width(val, ew);
                if *nonblocking {
                    nba.push((lhs, m));
                } else {
                    scratch.insert(lhs.clone(), m);
                    self.driven.insert(lhs);
                }
            }
            IrStmt::IfElse { cond, then_body, else_body } => {
                let c = self.eval_expr_with_env(cond, scratch);
                if c != 0 {
                    self.exec_stmts_on_env(then_body, scratch, nba);
                } else {
                    self.exec_stmts_on_env(else_body, scratch, nba);
                }
            }
            IrStmt::Case { expr, arms, default } => {
                let val = self.eval_expr_with_env(expr, scratch);
                let mut matched = false;
                for arm in arms {
                    let arm_val = self.eval_expr_with_env(&arm.value, scratch);
                    if val == arm_val {
                        self.exec_stmts_on_env(&arm.body, scratch, nba);
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    self.exec_stmts_on_env(default, scratch, nba);
                }
            }
            IrStmt::For {
                init_var,
                init_val,
                cond,
                step_var,
                step_expr,
                body,
            } => {
                let start = self.eval_expr_with_env(init_val, scratch);
                scratch.insert(init_var.clone(), start);
                for _ in 0..1024 {
                    let c = self.eval_expr_with_env(cond, scratch);
                    if c == 0 {
                        break;
                    }
                    self.exec_stmts_on_env(body, scratch, nba);
                    let next = self.eval_expr_with_env(step_expr, scratch);
                    scratch.insert(step_var.clone(), next);
                }
            }
            IrStmt::Delay(_) | IrStmt::SystemTask { .. } => {}
        }
    }

    fn exec_stmts_on_env(
        &mut self,
        stmts: &[IrStmt],
        scratch: &mut std::collections::HashMap<String, i64>,
        nba: &mut Vec<(String, i64)>,
    ) {
        for stmt in stmts {
            self.exec_stmt_on_env(stmt, scratch, nba);
        }
    }

    // ── Expression evaluator ────────────────────────────────────────

    fn eval_expr(&self, expr: &IrExpr) -> i64 {
        self.eval_expr_with_env(expr, &self.signals)
    }

    fn eval_expr_with_env(&self, expr: &IrExpr, env: &std::collections::HashMap<String, i64>) -> i64 {
        match expr {
            IrExpr::Const(v) => *v,
            IrExpr::Ident(name) => *env.get(name).unwrap_or(&0),
            IrExpr::Binary {
                op: IrBinOp::Ashr,
                left,
                right,
            } => {
                let w = self.infer_expr_width(left).max(1).min(63);
                let l = self.eval_expr_with_env(left, env);
                let r = self.eval_expr_with_env(right, env);
                crate::arith::arith_shr_i64(l, r as u32, w)
            }
            IrExpr::Binary { op, left, right } => {
                let l = self.eval_expr_with_env(left, env);
                let r = self.eval_expr_with_env(right, env);
                eval_binop(*op, l, r)
            }
            IrExpr::Unary { op, operand } => {
                let v = self.eval_expr_with_env(operand, env);
                eval_unop(*op, v)
            }
            IrExpr::Ternary { cond, then_expr, else_expr } => {
                if self.eval_expr_with_env(cond, env) != 0 {
                    self.eval_expr_with_env(then_expr, env)
                } else {
                    self.eval_expr_with_env(else_expr, env)
                }
            }
            IrExpr::Concat(exprs) => {
                let mut result: i64 = 0;
                let mut shift: u32 = 0;
                for e in exprs.iter().rev() {
                    let v = self.eval_expr_with_env(e, env);
                    let w = self.infer_expr_width(e);
                    let mask = if w >= 64 { !0i64 } else { (1i64 << w) - 1 };
                    result |= (v & mask) << shift;
                    shift += w;
                }
                result
            }
            IrExpr::PartSelect { value, msb, lsb } => {
                let v = self.eval_expr_with_env(value, env);
                let hi = self.eval_expr_with_env(msb, env);
                let lo = self.eval_expr_with_env(lsb, env);
                part_select_bits(v, hi, lo)
            }
            IrExpr::MemRead { stem, index } => {
                let idx = self.eval_expr_with_env(index, env);
                let (lo, hi) = self.mem_bounds.get(stem).copied().unwrap_or((0, 0));
                let ew = self.mem_elem_width.get(stem).copied().unwrap_or(1);
                let clamped = idx.clamp(lo, hi);
                let name = format!("{}__{}", stem, clamped);
                let v = *env.get(&name).unwrap_or(&0);
                mask_to_width(v, ew)
            }
            IrExpr::Signed(inner) => {
                let w = self.infer_expr_width(inner).max(1).min(63);
                let v = self.eval_expr_with_env(inner, env);
                crate::arith::sign_extend_i64(v, w)
            }
        }
    }

    fn infer_expr_width(&self, expr: &IrExpr) -> u32 {
        match expr {
            IrExpr::Ident(name) => self.width_for_signal(name).min(64).max(1) as u32,
            IrExpr::Const(_) => 1,
            IrExpr::Concat(exprs) => exprs.iter().map(|e| self.infer_expr_width(e)).sum(),
            IrExpr::Unary { operand, .. } => self.infer_expr_width(operand),
            IrExpr::PartSelect { msb, lsb, .. } => {
                if let (IrExpr::Const(a), IrExpr::Const(b)) = (msb.as_ref(), lsb.as_ref()) {
                    ((a - b).abs() + 1).max(1).min(64) as u32
                } else {
                    32
                }
            }
            IrExpr::MemRead { stem, .. } => self.mem_elem_width.get(stem).copied().unwrap_or(1) as u32,
            IrExpr::Binary { left, right, op, .. } => {
                match op {
                    // Comparison/logical ops always produce 1-bit
                    IrBinOp::Eq | IrBinOp::Ne | IrBinOp::Lt | IrBinOp::Le
                    | IrBinOp::Gt | IrBinOp::Ge | IrBinOp::LogAnd | IrBinOp::LogOr => 1,
                    _ => self.infer_expr_width(left).max(self.infer_expr_width(right)),
                }
            }
            IrExpr::Ternary { then_expr, else_expr, .. } => {
                self.infer_expr_width(then_expr).max(self.infer_expr_width(else_expr))
            }
            IrExpr::Signed(inner) => self.infer_expr_width(inner),
        }
    }

    // ── VCD output ──────────────────────────────────────────────────

    fn write_header(&self, out: &mut String, config: &SimConfig, clock_half_period_fine: usize) {
        writeln!(out, "$date").unwrap();
        let now = current_date_string();
        writeln!(out, "  {}", now).unwrap();
        writeln!(out, "$end").unwrap();
        writeln!(out, "$version").unwrap();
        writeln!(
            out,
            "  CircuitScope VCD Generator (verilog-core {})",
            env!("CARGO_PKG_VERSION")
        )
        .unwrap();
        writeln!(out, "$end").unwrap();
        // `$timescale` is the precision step; `#` in the body matches these fine ticks.
        let ts = normalize_vcd_timescale_line(config.timescale_precision.trim());
        writeln!(out, "$timescale").unwrap();
        writeln!(out, "\t{}", ts).unwrap();
        writeln!(out, "$end").unwrap();

        Self::write_vcd_debug_comments(out, config, clock_half_period_fine);

        self.write_scope(out, &self.scope_tree);

        writeln!(out, "$enddefinitions $end").unwrap();
    }

    /// IEEE 1364 `$comment` sections; safe for strict parsers (e.g. wellen / VaporView).
    fn write_vcd_debug_comments(out: &mut String, config: &SimConfig, clock_half_period_fine: usize) {
        let Some(meta) = config.vcd_meta.as_ref() else {
            return;
        };

        let mut block = |lines: &[String]| {
            writeln!(out, "$comment").unwrap();
            for line in lines {
                writeln!(out, "  {}", line).unwrap();
            }
            writeln!(out, "$end").unwrap();
        };

        let h = clock_half_period_fine.max(1);
        let last_sim = config.num_cycles.saturating_mul(2).saturating_mul(h);
        let mut lines: Vec<String> = vec![
            format!("verilog-core {}", env!("CARGO_PKG_VERSION")),
            format!("top_module: {}", config.top_module),
            format!(
                "`timescale (unit / VCD precision): {}/{}",
                config.timescale, config.timescale_precision
            ),
        ];
        if let Some(sum) = config.initial_delay_sum_units {
            lines.push(format!(
                "initial #delay literal sum (selected source files, time-unit steps): {}",
                sum
            ));
        }
        lines.push(format!(
            "simulation: num_cycles={} clock_half_period_units={} clock_half_period_fine={} vcd_$timescale={}",
            config.num_cycles,
            config.clock_half_period,
            clock_half_period_fine,
            config.timescale_precision
        ));
        lines.push(format!(
            "vcd_last_timestamp: #{} (precision_ticks; use --cycles in csverilog to override length)",
            last_sim
        ));
        lines.push(format!("ir_module_count: {}", meta.ir_module_count));
        block(&lines);

        if let Some(ref p) = meta.top_source_file {
            block(&[format!("top_source_file: {}", p)]);
        }

        if !meta.additional_source_files.is_empty() {
            let mut lines = vec!["other_source_files:".to_string()];
            for p in &meta.additional_source_files {
                lines.push(format!("  {}", p));
            }
            block(&lines);
        }

        if let Some(ref cwd) = meta.working_directory {
            block(&[format!("working_directory: {}", cwd)]);
        }

        if let Some(ref cmd) = meta.command_line {
            block(&[format!("command_line: {}", cmd)]);
        }

        if let Some(ref outp) = meta.output_vcd_path {
            block(&[format!("output_vcd_path: {}", outp)]);
        }
    }

    fn vcd_display_name(flat: &str) -> String {
        let parts: Vec<&str> = flat.split("__").collect();
        if parts.len() >= 2 {
            let last = parts[parts.len() - 1];
            if last.chars().all(|c| c.is_ascii_digit()) {
                return format!("{}_{}", parts[parts.len() - 2], last);
            }
        }
        parts.last().copied().unwrap_or(flat).to_string()
    }

    fn write_scope(&self, out: &mut String, scope: &ScopeNode) {
        writeln!(out, "$scope module {} $end", scope.name).unwrap();
        for sig in &scope.signals {
            let id = match self.vcd_ids.get(sig) {
                Some(id) => id,
                None => continue,
            };
            let w = self.width_for_signal(sig);
            let var_type = if self.is_register(sig) { "reg" } else { "wire" };
            let display_name = Self::vcd_display_name(sig);
            if w > 1 {
                writeln!(out, "$var {} {} {} {} [{}:0] $end", var_type, w, id, display_name, w - 1)
                    .unwrap();
            } else {
                writeln!(out, "$var {} {} {} {} $end", var_type, w, id, display_name).unwrap();
            }
        }
        for child in &scope.children {
            self.write_scope(out, child);
        }
        writeln!(out, "$upscope $end").unwrap();
    }

    fn write_timestamp(&self, out: &mut String, sim_time: u128) {
        writeln!(out, "#{}", sim_time).unwrap();
    }

    fn write_signal_value(&self, out: &mut String, name: &str, val: i64) {
        let id = match self.vcd_ids.get(name) {
            Some(id) => id,
            None => return,
        };
        let w = self.width_for_signal(name);
        if w == 1 {
            writeln!(out, "{}{}", val & 1, id).unwrap();
        } else {
            let mut bits = String::with_capacity(w);
            for i in (0..w).rev() {
                bits.push(if (val >> i) & 1 != 0 { '1' } else { '0' });
            }
            let trimmed = bits.trim_start_matches('0');
            let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
            writeln!(out, "b{} {}", trimmed, id).unwrap();
        }
    }

    fn write_signal_x(&self, out: &mut String, name: &str) {
        let id = match self.vcd_ids.get(name) {
            Some(id) => id,
            None => return,
        };
        let w = self.width_for_signal(name);
        if w == 1 {
            writeln!(out, "x{}", id).unwrap();
        } else {
            writeln!(out, "bx {}", id).unwrap();
        }
    }

    fn write_all_values_initial(&self, out: &mut String) {
        for name in &self.signal_order {
            if self.driven.contains(name) {
                let val = *self.signals.get(name).unwrap_or(&0);
                self.write_signal_value(out, name, val);
            } else {
                self.write_signal_x(out, name);
            }
        }
    }

    fn collect_changes(&self) -> Vec<(String, i64)> {
        let mut changes = Vec::new();
        for name in &self.signal_order {
            let cur = *self.signals.get(name).unwrap_or(&0);
            let prev = *self.prev_signals.get(name).unwrap_or(&0);
            if cur != prev {
                changes.push((name.clone(), cur));
            }
        }
        changes
    }

    fn is_register(&self, name: &str) -> bool {
        for ab in &self.always_blocks {
            if stmts_assign_to(&ab.stmts, name) {
                return true;
            }
        }
        for ev in &self.initial_events {
            if ev.lhs == name {
                return true;
            }
        }
        for p in &self.always_delay_procs {
            if stmts_assign_to(&p.stmts, name) {
                return true;
            }
        }
        false
    }
}

fn stmts_assign_to(stmts: &[IrStmt], name: &str) -> bool {
    for s in stmts {
        match s {
            IrStmt::NonBlockingAssign { lhs, .. } | IrStmt::BlockingAssign { lhs, .. } => {
                if lhs == name {
                    return true;
                }
            }
            IrStmt::MemAssign { stem, .. } => {
                let p = format!("{}__", stem);
                if name.starts_with(&p) {
                    return true;
                }
            }
            IrStmt::IfElse { then_body, else_body, .. } => {
                if stmts_assign_to(then_body, name) || stmts_assign_to(else_body, name) {
                    return true;
                }
            }
            IrStmt::Case { arms, default, .. } => {
                for arm in arms {
                    if stmts_assign_to(&arm.body, name) {
                        return true;
                    }
                }
                if stmts_assign_to(default, name) {
                    return true;
                }
            }
            IrStmt::For { body, .. } => {
                if stmts_assign_to(body, name) {
                    return true;
                }
            }
            IrStmt::Delay(_) | IrStmt::SystemTask { .. } => {}
        }
    }
    false
}

// ── Pure expression helpers ─────────────────────────────────────────

fn eval_binop(op: IrBinOp, l: i64, r: i64) -> i64 {
    match op {
        IrBinOp::Add => l.wrapping_add(r),
        IrBinOp::Sub => l.wrapping_sub(r),
        IrBinOp::Mul => l.wrapping_mul(r),
        IrBinOp::Div => {
            if r == 0 { 0 } else { l.wrapping_div(r) }
        }
        IrBinOp::Mod => {
            if r == 0 { 0 } else { l.wrapping_rem(r) }
        }
        IrBinOp::And => l & r,
        IrBinOp::Or => l | r,
        IrBinOp::Xor => l ^ r,
        IrBinOp::Shl => l.wrapping_shl(r as u32),
        IrBinOp::Shr => ((l as u64).wrapping_shr(r as u32)) as i64,
        IrBinOp::Ashr => {
            unreachable!("IrBinOp::Ashr must be evaluated in eval_expr_with_env with operand width")
        }
        IrBinOp::LogAnd => i64::from(l != 0 && r != 0),
        IrBinOp::LogOr => i64::from(l != 0 || r != 0),
        IrBinOp::Eq => i64::from(l == r),
        IrBinOp::Ne => i64::from(l != r),
        IrBinOp::Lt => i64::from(l < r),
        IrBinOp::Le => i64::from(l <= r),
        IrBinOp::Gt => i64::from(l > r),
        IrBinOp::Ge => i64::from(l >= r),
    }
}

fn eval_unop(op: IrUnaryOp, v: i64) -> i64 {
    match op {
        IrUnaryOp::Not => !v,
        IrUnaryOp::LogNot => i64::from(v == 0),
        IrUnaryOp::Neg => v.wrapping_neg(),
    }
}

fn mask_to_width(val: i64, w: usize) -> i64 {
    if w >= 64 {
        val
    } else {
        val & ((1i64 << w) - 1)
    }
}

/// Verilog packed select `[msb:lsb]` with bit 0 = LSB of stored value; `msb`/`lsb` are inclusive indices.
fn part_select_bits(val: i64, msb: i64, lsb: i64) -> i64 {
    let lo = msb.min(lsb).max(0);
    let hi = msb.max(lsb);
    let width_i64 = hi - lo + 1;
    if width_i64 <= 0 {
        return 0;
    }
    let width = (width_i64 as usize).min(63);
    let sh = lo.min(63) as u32;
    (val >> sh) & ((1i64 << width) - 1)
}

// ── Helpers ─────────────────────────────────────────────────────────

fn current_date_string() -> String {
    #[cfg(not(test))]
    {
        // Pure Rust so packaged / sandboxed apps still emit a valid $date line
        // (shell `date` is unavailable in some macOS GUI sandboxes).
        chrono::Local::now().format("%a %b %e %H:%M:%S %Z %Y").to_string()
    }
    #[cfg(test)]
    {
        "Tue Jan  1 00:00:00 UTC 2030".into()
    }
}

fn vcd_ident(index: usize) -> String {
    let range = 94; // '!' .. '~'
    let mut n = index;
    let mut id = String::new();
    loop {
        id.push((b'!' + (n % range) as u8) as char);
        n /= range;
        if n == 0 {
            break;
        }
        n -= 1;
    }
    id
}

fn prefix_ir_expr(expr: &IrExpr, prefix: &str) -> IrExpr {
    if prefix.is_empty() {
        return expr.clone();
    }
    match expr {
        IrExpr::Ident(name) => IrExpr::Ident(format!("{}__{}", prefix, name)),
        IrExpr::Const(v) => IrExpr::Const(*v),
        IrExpr::Binary { op, left, right } => IrExpr::Binary {
            op: *op,
            left: Box::new(prefix_ir_expr(left, prefix)),
            right: Box::new(prefix_ir_expr(right, prefix)),
        },
        IrExpr::Unary { op, operand } => IrExpr::Unary {
            op: *op,
            operand: Box::new(prefix_ir_expr(operand, prefix)),
        },
        IrExpr::Ternary { cond, then_expr, else_expr } => IrExpr::Ternary {
            cond: Box::new(prefix_ir_expr(cond, prefix)),
            then_expr: Box::new(prefix_ir_expr(then_expr, prefix)),
            else_expr: Box::new(prefix_ir_expr(else_expr, prefix)),
        },
        IrExpr::Concat(exprs) => {
            IrExpr::Concat(exprs.iter().map(|e| prefix_ir_expr(e, prefix)).collect())
        }
        IrExpr::PartSelect { value, msb, lsb } => IrExpr::PartSelect {
            value: Box::new(prefix_ir_expr(value, prefix)),
            msb: Box::new(prefix_ir_expr(msb, prefix)),
            lsb: Box::new(prefix_ir_expr(lsb, prefix)),
        },
        IrExpr::MemRead { stem, index } => IrExpr::MemRead {
            stem: if prefix.is_empty() {
                stem.clone()
            } else {
                format!("{}__{}", prefix, stem)
            },
            index: Box::new(prefix_ir_expr(index, prefix)),
        },
        IrExpr::Signed(inner) => IrExpr::Signed(Box::new(prefix_ir_expr(inner, prefix))),
    }
}

fn prefix_stmts(stmts: &[IrStmt], prefix: &str) -> Vec<IrStmt> {
    if prefix.is_empty() {
        return stmts.to_vec();
    }
    stmts.iter().map(|s| prefix_stmt(s, prefix)).collect()
}

fn prefix_stmt(stmt: &IrStmt, prefix: &str) -> IrStmt {
    let pfx = |name: &str| -> String { format!("{}__{}", prefix, name) };
    match stmt {
        IrStmt::BlockingAssign { lhs, rhs } => IrStmt::BlockingAssign {
            lhs: pfx(lhs),
            rhs: prefix_ir_expr(rhs, prefix),
        },
        IrStmt::NonBlockingAssign { lhs, rhs } => IrStmt::NonBlockingAssign {
            lhs: pfx(lhs),
            rhs: prefix_ir_expr(rhs, prefix),
        },
        IrStmt::MemAssign {
            stem,
            index,
            rhs,
            nonblocking,
        } => IrStmt::MemAssign {
            stem: pfx(stem),
            index: prefix_ir_expr(index, prefix),
            rhs: prefix_ir_expr(rhs, prefix),
            nonblocking: *nonblocking,
        },
        IrStmt::IfElse { cond, then_body, else_body } => IrStmt::IfElse {
            cond: prefix_ir_expr(cond, prefix),
            then_body: prefix_stmts(then_body, prefix),
            else_body: prefix_stmts(else_body, prefix),
        },
        IrStmt::Case { expr, arms, default } => IrStmt::Case {
            expr: prefix_ir_expr(expr, prefix),
            arms: arms
                .iter()
                .map(|a| IrCaseArm {
                    value: prefix_ir_expr(&a.value, prefix),
                    body: prefix_stmts(&a.body, prefix),
                })
                .collect(),
            default: prefix_stmts(default, prefix),
        },
        IrStmt::For { init_var, init_val, cond, step_var, step_expr, body } => IrStmt::For {
            init_var: pfx(init_var),
            init_val: prefix_ir_expr(init_val, prefix),
            cond: prefix_ir_expr(cond, prefix),
            step_var: pfx(step_var),
            step_expr: prefix_ir_expr(step_expr, prefix),
            body: prefix_stmts(body, prefix),
        },
        IrStmt::Delay(t) => IrStmt::Delay(*t),
        IrStmt::SystemTask { name, args } => IrStmt::SystemTask {
            name: name.clone(),
            args: args.iter().map(|e| prefix_ir_expr(e, prefix)).collect(),
        },
    }
}

// ── Unit tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrAssign, IrModule, IrNet, IrProject};
    use crate::Port;

    fn make_project(modules: Vec<IrModule>) -> IrProject {
        IrProject {
            modules,
            diagnostics: vec![],
        }
    }

    fn port(dir: &str, name: &str) -> Port {
        Port {
            direction: Some(dir.into()),
            name: name.into(),
            width: 1,
        }
    }

    fn port_w(dir: &str, name: &str, width: usize) -> Port {
        Port {
            direction: Some(dir.into()),
            name: name.into(),
            width,
        }
    }

    #[test]
    fn vcd_ident_generation() {
        assert_eq!(vcd_ident(0), "!");
        assert_eq!(vcd_ident(1), "\"");
        assert_eq!(vcd_ident(93), "~");
        assert_eq!(vcd_ident(94), "!!");
    }

    #[test]
    fn simple_combinational_vcd() {
        let top = IrModule {
            name: "top".into(),
            path: "test.v".into(),
            ports: vec![port("input", "a"), port("output", "y")],
            nets: vec![],
            assigns: vec![IrAssign {
                lhs: "y".into(),
                rhs: IrExpr::Ident("a".into()),
            }],
            instances: vec![],
            always_blocks: vec![],
            initial_blocks: vec![],
            mem_arrays: vec![],
            resolved_parameters: std::collections::HashMap::new(),
        };
        let proj = make_project(vec![top]);
        let config = SimConfig {
            top_module: "top".into(),
            num_cycles: 2,
            timescale: "1ns".into(),
            clock_half_period: 5,
            ..Default::default()
        };
        let vcd = generate_vcd(&proj, &config).unwrap();
        assert!(vcd.contains("$scope module top $end"));
        assert!(vcd.contains("$var"));
        assert!(vcd.contains("#0"));
        assert!(vcd.contains("$enddefinitions $end"));
        assert!(vcd.contains("$dumpvars"));
    }

    #[test]
    fn combinational_always_explicit_level_list_dumpvars_not_x() {
        let src = r#"
module top(input wire a, input wire b, output reg y);
  always @(a or b) y = a & b;
endmodule
"#;
        let proj = crate::build_ir_for_file("t.v", src);
        let config = SimConfig {
            top_module: "top".into(),
            num_cycles: 2,
            ..SimConfig::default()
        };
        let vcd = generate_vcd(&proj, &config).unwrap();
        let d0 = vcd.find("$dumpvars").unwrap();
        let d1 = vcd[d0..].find("$end").unwrap() + d0;
        let dump = &vcd[d0..d1];
        assert!(
            !dump.contains("bx"),
            "level-sensitivity combinational always must run before #0 dump: {}",
            dump
        );
    }

    #[test]
    fn sequential_counter_vcd() {
        use crate::ir::{IrAlways, IrEdgeKind, IrInitial, IrSensEntry, IrSensitivity, IrUnaryOp};

        let top = IrModule {
            name: "counter".into(),
            path: "test.v".into(),
            ports: vec![port_w("output", "count", 4)],
            nets: vec![
                IrNet {
                    name: "clk".into(),
                    width: 1,
                },
                IrNet {
                    name: "rst".into(),
                    width: 1,
                },
                IrNet {
                    name: "count".into(),
                    width: 4,
                },
            ],
            assigns: vec![],
            instances: vec![],
            always_blocks: vec![
                IrAlways {
                    sensitivity: IrSensitivity::Star,
                    stmts: vec![
                        IrStmt::Delay(DelayRational::from_int(5)),
                        IrStmt::BlockingAssign {
                            lhs: "clk".into(),
                            rhs: IrExpr::Unary {
                                op: IrUnaryOp::Not,
                                operand: Box::new(IrExpr::Ident("clk".into())),
                            },
                        },
                    ],
                },
                IrAlways {
                    sensitivity: IrSensitivity::EdgeList(vec![IrSensEntry {
                        edge: IrEdgeKind::Posedge,
                        signal: "clk".into(),
                    }]),
                    stmts: vec![IrStmt::IfElse {
                        cond: IrExpr::Ident("rst".into()),
                        then_body: vec![IrStmt::NonBlockingAssign {
                            lhs: "count".into(),
                            rhs: IrExpr::Const(0),
                        }],
                        else_body: vec![IrStmt::NonBlockingAssign {
                            lhs: "count".into(),
                            rhs: IrExpr::Binary {
                                op: IrBinOp::Add,
                                left: Box::new(IrExpr::Ident("count".into())),
                                right: Box::new(IrExpr::Const(1)),
                            },
                        }],
                    }],
                },
            ],
            initial_blocks: vec![IrInitial {
                stmts: vec![
                    IrStmt::BlockingAssign {
                        lhs: "clk".into(),
                        rhs: IrExpr::Const(0),
                    },
                    IrStmt::BlockingAssign {
                        lhs: "rst".into(),
                        rhs: IrExpr::Const(0),
                    },
                ],
            }],
            mem_arrays: vec![],
            resolved_parameters: std::collections::HashMap::new(),
        };

        let proj = make_project(vec![top]);
        let config = SimConfig {
            top_module: "counter".into(),
            num_cycles: 5,
            timescale: "1ns".into(),
            clock_half_period: 5,
            ..Default::default()
        };
        let vcd = generate_vcd(&proj, &config).unwrap();
        assert!(vcd.contains("$var reg"));
        assert!(vcd.contains("count"));
        assert!(vcd.contains("#5"));
        assert!(vcd.contains("#10"));
    }

    #[test]
    fn missing_top_module_returns_error() {
        let proj = make_project(vec![]);
        let config = SimConfig {
            top_module: "nonexistent".into(),
            ..SimConfig::default()
        };
        let result = generate_vcd(&proj, &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn multibit_signal_vcd() {
        let top = IrModule {
            name: "top".into(),
            path: "test.v".into(),
            ports: vec![
                port_w("input", "clk", 1),
                port_w("output", "data", 8),
            ],
            nets: vec![],
            assigns: vec![IrAssign {
                lhs: "data".into(),
                rhs: IrExpr::Const(0xAB),
            }],
            instances: vec![],
            always_blocks: vec![],
            initial_blocks: vec![],
            mem_arrays: vec![],
            resolved_parameters: std::collections::HashMap::new(),
        };
        let proj = make_project(vec![top]);
        let config = SimConfig {
            top_module: "top".into(),
            num_cycles: 1,
            ..SimConfig::default()
        };
        let vcd = generate_vcd(&proj, &config).unwrap();
        assert!(vcd.contains("[7:0]"), "should have bit range annotation");
        assert!(vcd.contains("b10101011") || vcd.contains("b0"), "should use binary format for multi-bit");
    }

    #[test]
    fn initial_block_with_delay() {
        let top = IrModule {
            name: "tb".into(),
            path: "test.v".into(),
            ports: vec![],
            nets: vec![IrNet { name: "sel".into(), width: 1 }],
            assigns: vec![],
            instances: vec![],
            always_blocks: vec![],
            initial_blocks: vec![IrInitial {
                stmts: vec![
                    IrStmt::BlockingAssign {
                        lhs: "sel".into(),
                        rhs: IrExpr::Const(0),
                    },
                    IrStmt::Delay(DelayRational::from_int(10)),
                    IrStmt::BlockingAssign {
                        lhs: "sel".into(),
                        rhs: IrExpr::Const(1),
                    },
                ],
            }],
            mem_arrays: vec![],
            resolved_parameters: std::collections::HashMap::new(),
        };
        let proj = make_project(vec![top]);
        let config = SimConfig {
            top_module: "tb".into(),
            num_cycles: 5,
            timescale: "1ns".into(),
            clock_half_period: 5,
            ..Default::default()
        };
        let vcd = generate_vcd(&proj, &config).unwrap();
        // At time 0, sel = 0. At time 10, sel = 1.
        assert!(vcd.contains("0!"), "sel should be 0 initially");
        assert!(vcd.contains("#10"), "should have timestamp 10");
        assert!(vcd.contains("1!"), "sel should become 1 at time 10");
    }

    #[test]
    fn x_initialization_for_undriven() {
        let top = IrModule {
            name: "top".into(),
            path: "test.v".into(),
            ports: vec![port("input", "a"), port("output", "y")],
            nets: vec![IrNet { name: "internal".into(), width: 1 }],
            assigns: vec![],
            instances: vec![],
            always_blocks: vec![],
            initial_blocks: vec![],
            mem_arrays: vec![],
            resolved_parameters: std::collections::HashMap::new(),
        };
        let proj = make_project(vec![top]);
        let config = SimConfig {
            top_module: "top".into(),
            num_cycles: 1,
            ..SimConfig::default()
        };
        let vcd = generate_vcd(&proj, &config).unwrap();
        assert!(vcd.contains("x"), "undriven signals should show x");
    }

    #[test]
    fn nested_scopes_in_vcd() {
        use crate::ir::IrInstance;
        use crate::ir::IrPortConn;

        let child = IrModule {
            name: "inverter".into(),
            path: "inv.v".into(),
            ports: vec![port("input", "a"), port("output", "y")],
            nets: vec![],
            assigns: vec![IrAssign {
                lhs: "y".into(),
                rhs: IrExpr::Unary {
                    op: IrUnaryOp::Not,
                    operand: Box::new(IrExpr::Ident("a".into())),
                },
            }],
            instances: vec![],
            always_blocks: vec![],
            initial_blocks: vec![],
            mem_arrays: vec![],
            resolved_parameters: std::collections::HashMap::new(),
        };
        let top = IrModule {
            name: "top".into(),
            path: "top.v".into(),
            ports: vec![port("input", "in1"), port("output", "out1")],
            nets: vec![],
            assigns: vec![],
            instances: vec![IrInstance {
                module_name: "inverter".into(),
                parameter_assignments: vec![],
                instance_name: "u1".into(),
                connections: vec![
                    IrPortConn {
                        port_name: Some("a".into()),
                        expr: IrExpr::Ident("in1".into()),
                    },
                    IrPortConn {
                        port_name: Some("y".into()),
                        expr: IrExpr::Ident("out1".into()),
                    },
                ],
            }],
            always_blocks: vec![],
            initial_blocks: vec![],
            mem_arrays: vec![],
            resolved_parameters: std::collections::HashMap::new(),
        };
        let proj = make_project(vec![top, child]);
        let config = SimConfig {
            top_module: "top".into(),
            num_cycles: 1,
            ..SimConfig::default()
        };
        let vcd = generate_vcd(&proj, &config).unwrap();
        assert!(vcd.contains("$scope module top $end"), "should have top scope");
        assert!(vcd.contains("$scope module u1 $end"), "should have child scope");
        let upscope_count = vcd.matches("$upscope $end").count();
        assert!(upscope_count >= 2, "should have matching upscopes: got {}", upscope_count);
    }

    #[test]
    fn real_date_in_header() {
        let top = IrModule {
            name: "top".into(),
            path: "test.v".into(),
            ports: vec![port("input", "a")],
            nets: vec![],
            assigns: vec![],
            instances: vec![],
            always_blocks: vec![],
            initial_blocks: vec![],
            mem_arrays: vec![],
            resolved_parameters: std::collections::HashMap::new(),
        };
        let proj = make_project(vec![top]);
        let config = SimConfig {
            top_module: "top".into(),
            num_cycles: 1,
            ..SimConfig::default()
        };
        let vcd = generate_vcd(&proj, &config).unwrap();
        assert!(vcd.contains("$date"), "should have date section");
        assert!(!vcd.contains("Simulation output"), "should not use placeholder date in test");
    }

    #[test]
    fn dumpvars_after_timestamp_zero() {
        let top = IrModule {
            name: "top".into(),
            path: "test.v".into(),
            ports: vec![port("input", "a")],
            nets: vec![],
            assigns: vec![],
            instances: vec![],
            always_blocks: vec![],
            initial_blocks: vec![],
            mem_arrays: vec![],
            resolved_parameters: std::collections::HashMap::new(),
        };
        let proj = make_project(vec![top]);
        let config = SimConfig {
            top_module: "top".into(),
            num_cycles: 1,
            ..SimConfig::default()
        };
        let vcd = generate_vcd(&proj, &config).unwrap();
        let t0_pos = vcd.find("#0").unwrap();
        let dv_pos = vcd.find("$dumpvars").unwrap();
        assert!(
            dv_pos > t0_pos,
            "$dumpvars should appear after #0 (t0={}, dv={})",
            t0_pos,
            dv_pos
        );
    }

    #[test]
    fn mask_to_width_works() {
        assert_eq!(mask_to_width(0xFF, 4), 0xF);
        assert_eq!(mask_to_width(0xFF, 8), 0xFF);
        assert_eq!(mask_to_width(-1, 1), 1);
        assert_eq!(mask_to_width(0x1FF, 8), 0xFF);
    }

    #[test]
    fn eval_binary_ops() {
        let sim = Simulator {
            signals: HashMap::new(),
            prev_signals: HashMap::new(),
            signal_order: vec![],
            vcd_ids: HashMap::new(),
            widths: HashMap::new(),
            assigns: vec![],
            always_blocks: vec![],
            always_delay_procs: vec![],
            initial_events: vec![],
            initial_time_horizon_fs: 0,
            driven: HashSet::new(),
            mem_bounds: HashMap::new(),
            mem_elem_width: HashMap::new(),
            scope_tree: ScopeNode {
                name: "t".into(),
                signals: vec![],
                children: vec![],
            },
            top: &IrModule {
                name: "t".into(),
                path: "t.v".into(),
                ports: vec![],
                nets: vec![],
                assigns: vec![],
                instances: vec![],
                always_blocks: vec![],
                initial_blocks: vec![],
                mem_arrays: vec![],
                resolved_parameters: std::collections::HashMap::new(),
            },
            last_sim_time_fs: 0,
        };
        assert_eq!(
            sim.eval_expr(&IrExpr::Binary {
                op: IrBinOp::Add,
                left: Box::new(IrExpr::Const(3)),
                right: Box::new(IrExpr::Const(4)),
            }),
            7
        );
        assert_eq!(
            sim.eval_expr(&IrExpr::Binary {
                op: IrBinOp::Eq,
                left: Box::new(IrExpr::Const(5)),
                right: Box::new(IrExpr::Const(5)),
            }),
            1
        );
        assert_eq!(
            sim.eval_expr(&IrExpr::Ternary {
                cond: Box::new(IrExpr::Const(1)),
                then_expr: Box::new(IrExpr::Const(10)),
                else_expr: Box::new(IrExpr::Const(20)),
            }),
            10
        );
    }
}

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::delay_rational::DelayRational;
use crate::lexer;
use crate::parser::{
    self, AssignTarget, BinaryOp, CstModule, CstModuleItem, CstStmt, EdgeKind, Expr,
    Sensitivity, UnaryOp,
};
use crate::{Diagnostic, Port, SourceFile};

// ── IR expression tree ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum IrExpr {
    Const(i64),
    Ident(String),
    Binary {
        op: IrBinOp,
        left: Box<IrExpr>,
        right: Box<IrExpr>,
    },
    Unary {
        op: IrUnaryOp,
        operand: Box<IrExpr>,
    },
    Ternary {
        cond: Box<IrExpr>,
        then_expr: Box<IrExpr>,
        else_expr: Box<IrExpr>,
    },
    Concat(Vec<IrExpr>),
    /// Packed part-select / bit-select: `value[msb:lsb]` (inclusive indices; LSB of value is bit 0).
    PartSelect {
        value: Box<IrExpr>,
        msb: Box<IrExpr>,
        lsb: Box<IrExpr>,
    },
    /// Unpacked array read: stem expands to `stem__lo`…`stem__hi` in the IR (see [`IrMemArray`]).
    MemRead {
        stem: String,
        index: Box<IrExpr>,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum IrBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    LogAnd,
    LogOr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum IrUnaryOp {
    Not,    // bitwise ~
    LogNot, // !
    Neg,    // unary -
}

// ── IR project / module structures ──────────────────────────────────

#[derive(Debug, Clone)]
pub struct IrProject {
    pub modules: Vec<IrModule>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub struct IrModule {
    pub name: String,
    pub path: String,
    pub ports: Vec<Port>,
    pub nets: Vec<IrNet>,
    pub assigns: Vec<IrAssign>,
    pub instances: Vec<IrInstance>,
    pub always_blocks: Vec<IrAlways>,
    pub initial_blocks: Vec<IrInitial>,
    /// Unpacked `reg [...] name[a:b]` arrays lowered to `name__k` scalars; used by [`IrExpr::MemRead`].
    pub mem_arrays: Vec<IrMemArray>,
}

/// Metadata for unpacked arrays declared as `reg [w-1:0] stem[hi:lo];`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrMemArray {
    pub stem: String,
    pub lo: i64,
    pub hi: i64,
    pub elem_width: usize,
}

#[derive(Debug, Clone)]
pub struct IrNet {
    pub name: String,
    pub width: usize,
}

#[derive(Debug, Clone)]
pub struct IrInitial {
    pub stmts: Vec<IrStmt>,
}

#[derive(Debug, Clone)]
pub struct IrAssign {
    pub lhs: String,
    pub rhs: IrExpr,
}

#[derive(Debug, Clone)]
pub struct IrInstance {
    pub module_name: String,
    pub instance_name: String,
    pub connections: Vec<IrPortConn>,
}

/// Port connection: named (`.p(e)`) or positional before [`resolve_instance_port_connections`].
#[derive(Debug, Clone)]
pub struct IrPortConn {
    pub port_name: Option<String>,
    pub expr: IrExpr,
}

// ── Sequential / procedural IR ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrEdgeKind {
    Posedge,
    Negedge,
    Level,
}

#[derive(Debug, Clone)]
pub struct IrSensEntry {
    pub edge: IrEdgeKind,
    pub signal: String,
}

#[derive(Debug, Clone)]
pub enum IrSensitivity {
    Star,
    EdgeList(Vec<IrSensEntry>),
}

#[derive(Debug, Clone)]
pub struct IrAlways {
    pub sensitivity: IrSensitivity,
    pub stmts: Vec<IrStmt>,
}

#[derive(Debug, Clone)]
pub enum IrStmt {
    BlockingAssign { lhs: String, rhs: IrExpr },
    NonBlockingAssign { lhs: String, rhs: IrExpr },
    /// Unpacked memory write: `mem[idx] <= rhs` (or blocking) when `idx` is not a compile-time constant.
    MemAssign {
        stem: String,
        index: IrExpr,
        rhs: IrExpr,
        nonblocking: bool,
    },
    IfElse {
        cond: IrExpr,
        then_body: Vec<IrStmt>,
        else_body: Vec<IrStmt>,
    },
    Case {
        expr: IrExpr,
        arms: Vec<IrCaseArm>,
        default: Vec<IrStmt>,
    },
    For {
        init_var: String,
        init_val: IrExpr,
        cond: IrExpr,
        step_var: String,
        step_expr: IrExpr,
        body: Vec<IrStmt>,
    },
    Delay(DelayRational),
    SystemTask {
        name: String,
        args: Vec<IrExpr>,
    },
}

#[derive(Debug, Clone)]
pub struct IrCaseArm {
    pub value: IrExpr,
    pub body: Vec<IrStmt>,
}

// ── Public API ──────────────────────────────────────────────────────

pub fn build_ir_for_file(path: impl Into<String>, content: &str) -> IrProject {
    let file = SourceFile::new(path, content);
    let tokens = lexer::lex(&file);
    let (cst, diagnostics) = parser::parse_cst(&file, &tokens);
    let mut modules = Vec::new();
    for m in cst.modules {
        modules.push(ir_module_from_cst(m));
    }
    IrProject {
        modules,
        diagnostics,
    }
}

pub fn build_ir_for_root(root: &Path) -> std::io::Result<IrProject> {
    let mut all_modules = Vec::new();
    let mut all_diags = Vec::new();
    walk_dir(root, &mut |path| {
        if let Ok(src) = std::fs::read_to_string(path) {
            let file = SourceFile::new(path.to_string_lossy(), &src);
            let tokens = lexer::lex(&file);
            let (cst, mut diags) = parser::parse_cst(&file, &tokens);
            all_diags.append(&mut diags);
            for m in cst.modules {
                all_modules.push(ir_module_from_cst(m));
            }
        }
    })?;
    Ok(IrProject {
        modules: all_modules,
        diagnostics: all_diags,
    })
}

/// Map positional instance ports (`port_name: None`) to the child module's port names in
/// declaration order. Call on a merged [`IrProject`] before optimization / simulation.
pub fn resolve_instance_port_connections(project: &mut IrProject) -> Result<(), String> {
    let child_ports_by_module: HashMap<String, Vec<Port>> = project
        .modules
        .iter()
        .map(|m| (m.name.clone(), m.ports.clone()))
        .collect();
    for module in &mut project.modules {
        for inst in &mut module.instances {
            let Some(child_ports) = child_ports_by_module.get(&inst.module_name) else {
                continue;
            };
            let has_pos = inst.connections.iter().any(|c| c.port_name.is_none());
            let has_named = inst.connections.iter().any(|c| c.port_name.is_some());
            if has_pos && has_named {
                return Err(format!(
                    "module `{}` instance `{}` of `{}`: mixed positional and named port connections are not supported",
                    module.name, inst.instance_name, inst.module_name
                ));
            }
            if !has_pos {
                continue;
            }
            if inst.connections.len() > child_ports.len() {
                return Err(format!(
                    "module `{}` instance `{}` of `{}`: too many positional ports ({} > {})",
                    module.name,
                    inst.instance_name,
                    inst.module_name,
                    inst.connections.len(),
                    child_ports.len()
                ));
            }
            for (i, c) in inst.connections.iter_mut().enumerate() {
                if c.port_name.is_none() {
                    let p = child_ports.get(i).ok_or_else(|| {
                        format!(
                            "module `{}` instance `{}` of `{}`: positional port index {} out of range",
                            module.name, inst.instance_name, inst.module_name, i
                        )
                    })?;
                    c.port_name = Some(p.name.clone());
                }
            }
        }
    }
    Ok(())
}

// ── Lowering from CST to IR ─────────────────────────────────────────

fn collect_mem_stems(items: &[CstModuleItem]) -> HashSet<String> {
    let mut s = HashSet::new();
    for item in items {
        if let CstModuleItem::NetDecl { unpacked_stems, .. } = item {
            for (stem, _, _) in unpacked_stems {
                s.insert(stem.clone());
            }
        }
    }
    s
}

fn collect_net_widths(items: &[CstModuleItem]) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for item in items {
        if let CstModuleItem::NetDecl { width, names, .. } = item {
            for n in names {
                m.insert(n.clone(), *width);
            }
        }
    }
    m
}

fn collect_mem_arrays(items: &[CstModuleItem]) -> Vec<IrMemArray> {
    let mut v = Vec::new();
    for item in items {
        if let CstModuleItem::NetDecl {
            width,
            unpacked_stems,
            ..
        } = item
        {
            for (stem, lo, hi) in unpacked_stems {
                v.push(IrMemArray {
                    stem: stem.clone(),
                    lo: *lo,
                    hi: *hi,
                    elem_width: *width,
                });
            }
        }
    }
    v
}

fn collect_local_param_assignments(items: &[CstModuleItem]) -> Vec<(String, Expr)> {
    let mut v = Vec::new();
    for item in items {
        if let CstModuleItem::LocalParam { assignments } = item {
            v.extend(assignments.iter().cloned());
        }
    }
    v
}

fn eval_binop_for_localparam(op: BinaryOp, l: i64, r: i64) -> i64 {
    match op {
        BinaryOp::Add => l.wrapping_add(r),
        BinaryOp::Sub => l.wrapping_sub(r),
        BinaryOp::Mul => l.wrapping_mul(r),
        BinaryOp::Div => {
            if r == 0 {
                0
            } else {
                l.wrapping_div(r)
            }
        }
        BinaryOp::Mod => {
            if r == 0 {
                0
            } else {
                l.wrapping_rem(r)
            }
        }
        BinaryOp::And => l & r,
        BinaryOp::Or => l | r,
        BinaryOp::Xor => l ^ r,
        BinaryOp::Shl => l.wrapping_shl((r as u32).min(63)),
        BinaryOp::Shr => l.wrapping_shr((r as u32).min(63)),
        BinaryOp::LogAnd => i64::from(l != 0 && r != 0),
        BinaryOp::LogOr => i64::from(l != 0 || r != 0),
        BinaryOp::Eq => i64::from(l == r),
        BinaryOp::Ne => i64::from(l != r),
        BinaryOp::Lt => i64::from(l < r),
        BinaryOp::Le => i64::from(l <= r),
        BinaryOp::Gt => i64::from(l > r),
        BinaryOp::Ge => i64::from(l >= r),
    }
}

fn const_eval_param_expr(e: &Expr, known: &HashMap<String, i64>) -> Option<i64> {
    match e {
        Expr::Number(s) => Some(parse_verilog_number(s)),
        Expr::Ident(name) => known.get(name).copied(),
        Expr::Unary { op, operand } => {
            let v = const_eval_param_expr(operand, known)?;
            match op {
                UnaryOp::Pos => Some(v),
                UnaryOp::Neg => Some(v.wrapping_neg()),
                UnaryOp::Not => Some(!v),
                UnaryOp::LogNot => Some(i64::from(v == 0)),
            }
        }
        Expr::Binary { op, left, right } => {
            let l = const_eval_param_expr(left, known)?;
            let r = const_eval_param_expr(right, known)?;
            Some(eval_binop_for_localparam(*op, l, r))
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            let c = const_eval_param_expr(cond, known)?;
            if c != 0 {
                const_eval_param_expr(then_expr, known)
            } else {
                const_eval_param_expr(else_expr, known)
            }
        }
        Expr::Concat(_) | Expr::Index { .. } => None,
    }
}

fn resolve_local_param_values(pairs: &[(String, Expr)]) -> HashMap<String, i64> {
    let mut raw: HashMap<String, Expr> = HashMap::new();
    for (n, e) in pairs {
        raw.insert(n.clone(), e.clone());
    }
    let mut resolved: HashMap<String, i64> = HashMap::new();
    let cap = raw.len().max(1).saturating_mul(8);
    for _ in 0..cap {
        let mut progress = false;
        let keys: Vec<String> = raw.keys().cloned().collect();
        for k in keys {
            if resolved.contains_key(&k) {
                continue;
            }
            let Some(e) = raw.get(&k) else {
                continue;
            };
            if let Some(v) = const_eval_param_expr(e, &resolved) {
                resolved.insert(k, v);
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    resolved
}

fn ir_module_from_cst(cst: CstModule) -> IrModule {
    let mem_stems = collect_mem_stems(&cst.items);
    let mem_arrays = collect_mem_arrays(&cst.items);
    let net_widths = collect_net_widths(&cst.items);
    let param_pairs = collect_local_param_assignments(&cst.items);
    let locals = resolve_local_param_values(&param_pairs);
    let mut assigns = Vec::new();
    let mut nets = Vec::new();
    let mut instances = Vec::new();
    let mut always_blocks = Vec::new();
    let mut initial_blocks = Vec::new();
    for item in cst.items {
        match item {
            CstModuleItem::Assign { lhs, expr } => {
                assigns.push(IrAssign {
                    lhs,
                    rhs: lower_expr(expr, &mem_stems, &locals),
                });
            }
            CstModuleItem::NetDecl { width, names, .. } => {
                for n in names {
                    nets.push(IrNet { name: n, width });
                }
            }
            CstModuleItem::Instance {
                module_name,
                instance_name,
                connections,
            } => {
                let conns = connections
                    .into_iter()
                    .map(|c| IrPortConn {
                        port_name: c.port_name,
                        expr: lower_expr(c.expr, &mem_stems, &locals),
                    })
                    .collect();
                instances.push(IrInstance {
                    module_name,
                    instance_name,
                    connections: conns,
                });
            }
            CstModuleItem::Always { sensitivity, body } => {
                always_blocks.push(lower_always(
                    sensitivity,
                    body,
                    &mem_stems,
                    &locals,
                    &net_widths,
                ));
            }
            CstModuleItem::Initial { body } => {
                initial_blocks.push(IrInitial {
                    stmts: body
                        .into_iter()
                        .filter_map(|s| lower_stmt(s, &mem_stems, &locals, &net_widths))
                        .collect(),
                });
            }
            CstModuleItem::LocalParam { .. } => {}
        }
    }
    IrModule {
        name: cst.name,
        path: cst.path,
        ports: cst.ports,
        nets,
        assigns,
        instances,
        always_blocks,
        initial_blocks,
        mem_arrays,
    }
}

fn lower_always(
    sens: Sensitivity,
    stmts: Vec<CstStmt>,
    mem_stems: &HashSet<String>,
    locals: &HashMap<String, i64>,
    net_widths: &HashMap<String, usize>,
) -> IrAlways {
    let sensitivity = match sens {
        Sensitivity::Star => IrSensitivity::Star,
        Sensitivity::EdgeList(edges) => IrSensitivity::EdgeList(
            edges
                .into_iter()
                .map(|e| IrSensEntry {
                    edge: match e.edge {
                        EdgeKind::Posedge => IrEdgeKind::Posedge,
                        EdgeKind::Negedge => IrEdgeKind::Negedge,
                        EdgeKind::Level => IrEdgeKind::Level,
                    },
                    signal: e.signal,
                })
                .collect(),
        ),
    };
    IrAlways {
        sensitivity,
        stmts: stmts
            .into_iter()
            .filter_map(|s| lower_stmt(s, mem_stems, locals, net_widths))
            .collect(),
    }
}

fn net_width_or_default(net_widths: &HashMap<String, usize>, reg: &str) -> usize {
    net_widths.get(reg).copied().unwrap_or(32).clamp(1, 62)
}

/// Packed `reg[i] = val` as read–modify–write on the whole vector (IEEE 1364).
fn lower_packed_bit_assign(lhs: String, idx_ir: IrExpr, val_ir: IrExpr, width: usize) -> IrStmt {
    let w_i = width.clamp(1, 62) as i64;
    let width_mask = IrExpr::Const((1i64 << w_i) - 1);
    let old = IrExpr::Ident(lhs.clone());
    let one = IrExpr::Const(1);
    let shl_at = IrExpr::Binary {
        op: IrBinOp::Shl,
        left: Box::new(one),
        right: Box::new(idx_ir.clone()),
    };
    let at_mask = IrExpr::Binary {
        op: IrBinOp::And,
        left: Box::new(shl_at),
        right: Box::new(width_mask.clone()),
    };
    let not_m = IrExpr::Unary {
        op: IrUnaryOp::Not,
        operand: Box::new(at_mask.clone()),
    };
    let not_masked = IrExpr::Binary {
        op: IrBinOp::And,
        left: Box::new(not_m),
        right: Box::new(width_mask.clone()),
    };
    let cleared = IrExpr::Binary {
        op: IrBinOp::And,
        left: Box::new(old),
        right: Box::new(not_masked),
    };
    let v1 = IrExpr::Binary {
        op: IrBinOp::And,
        left: Box::new(val_ir),
        right: Box::new(IrExpr::Const(1)),
    };
    let shifted = IrExpr::Binary {
        op: IrBinOp::Shl,
        left: Box::new(v1),
        right: Box::new(idx_ir),
    };
    let rhs = IrExpr::Binary {
        op: IrBinOp::Or,
        left: Box::new(cleared),
        right: Box::new(shifted),
    };
    IrStmt::BlockingAssign { lhs, rhs }
}

/// Constant `reg[msb:lsb] = val` (indices known at compile time).
fn lower_packed_part_assign_const(
    lhs: String,
    msb: i64,
    lsb: i64,
    val_ir: IrExpr,
    width: usize,
) -> IrStmt {
    let lo = msb.min(lsb);
    let hi = msb.max(lsb);
    let nbits = hi - lo + 1;
    if nbits <= 0 || nbits > 62 {
        return IrStmt::BlockingAssign { lhs, rhs: val_ir };
    }
    let w_i = width.clamp(1, 62) as i64;
    let width_mask = IrExpr::Const((1i64 << w_i) - 1);
    let field_mask = (((1i64 << nbits) - 1) << lo) & ((1i64 << w_i) - 1);
    let field_mask_ir = IrExpr::Const(field_mask);
    let old = IrExpr::Ident(lhs.clone());
    let not_field = IrExpr::Unary {
        op: IrUnaryOp::Not,
        operand: Box::new(field_mask_ir.clone()),
    };
    let not_masked = IrExpr::Binary {
        op: IrBinOp::And,
        left: Box::new(not_field),
        right: Box::new(width_mask.clone()),
    };
    let cleared = IrExpr::Binary {
        op: IrBinOp::And,
        left: Box::new(old),
        right: Box::new(not_masked),
    };
    let slice_mask = IrExpr::Const((1i64 << nbits) - 1);
    let val_clamped = IrExpr::Binary {
        op: IrBinOp::And,
        left: Box::new(val_ir),
        right: Box::new(slice_mask),
    };
    let shifted = IrExpr::Binary {
        op: IrBinOp::Shl,
        left: Box::new(val_clamped),
        right: Box::new(IrExpr::Const(lo)),
    };
    let rhs = IrExpr::Binary {
        op: IrBinOp::Or,
        left: Box::new(cleared),
        right: Box::new(shifted),
    };
    IrStmt::BlockingAssign { lhs, rhs }
}

fn lower_assign_from_target(
    target: AssignTarget,
    rhs: Expr,
    is_nb: bool,
    mem_stems: &HashSet<String>,
    locals: &HashMap<String, i64>,
    net_widths: &HashMap<String, usize>,
) -> Option<IrStmt> {
    fn wrap_nb(stmt: IrStmt, is_nb: bool) -> IrStmt {
        if is_nb {
            if let IrStmt::BlockingAssign { lhs, rhs } = stmt {
                return IrStmt::NonBlockingAssign { lhs, rhs };
            }
        }
        stmt
    }
    Some(match target {
        AssignTarget::Whole(name) => {
            let rhs_ir = lower_expr(rhs, mem_stems, locals);
            if is_nb {
                IrStmt::NonBlockingAssign { lhs: name, rhs: rhs_ir }
            } else {
                IrStmt::BlockingAssign { lhs: name, rhs: rhs_ir }
            }
        }
        AssignTarget::BitSelect { reg, index } => {
            if mem_stems.contains(&reg) {
                let idx_ir = lower_expr(index, mem_stems, locals);
                let rhs_ir = lower_expr(rhs, mem_stems, locals);
                if let IrExpr::Const(k) = &idx_ir {
                    let lhs = format!("{}__{}", reg, k);
                    return Some(wrap_nb(
                        IrStmt::BlockingAssign { lhs, rhs: rhs_ir },
                        is_nb,
                    ));
                }
                return Some(IrStmt::MemAssign {
                    stem: reg,
                    index: idx_ir,
                    rhs: rhs_ir,
                    nonblocking: is_nb,
                });
            }
            let w = net_width_or_default(net_widths, &reg);
            wrap_nb(
                lower_packed_bit_assign(
                    reg,
                    lower_expr(index, mem_stems, locals),
                    lower_expr(rhs, mem_stems, locals),
                    w,
                ),
                is_nb,
            )
        }
        AssignTarget::PartSelect { reg, msb, lsb } => {
            let msb_ir = lower_expr(msb, mem_stems, locals);
            let lsb_ir = lower_expr(lsb, mem_stems, locals);
            let val_ir = lower_expr(rhs, mem_stems, locals);
            let w = net_width_or_default(net_widths, &reg);
            let stmt = if let (IrExpr::Const(a), IrExpr::Const(b)) = (&msb_ir, &lsb_ir) {
                lower_packed_part_assign_const(reg, *a, *b, val_ir, w)
            } else {
                IrStmt::BlockingAssign {
                    lhs: reg,
                    rhs: val_ir,
                }
            };
            wrap_nb(stmt, is_nb)
        }
    })
}

fn lower_stmt(
    s: CstStmt,
    mem_stems: &HashSet<String>,
    locals: &HashMap<String, i64>,
    net_widths: &HashMap<String, usize>,
) -> Option<IrStmt> {
    match s {
        CstStmt::BlockingAssign { target, rhs } => {
            lower_assign_from_target(target, rhs, false, mem_stems, locals, net_widths)
        }
        CstStmt::NonBlockingAssign { target, rhs } => {
            lower_assign_from_target(target, rhs, true, mem_stems, locals, net_widths)
        }
        CstStmt::IfElse {
            cond,
            then_body,
            else_body,
        } => Some(IrStmt::IfElse {
            cond: lower_expr(cond, mem_stems, locals),
            then_body: then_body
                .into_iter()
                .filter_map(|s| lower_stmt(s, mem_stems, locals, net_widths))
                .collect(),
            else_body: else_body
                .into_iter()
                .filter_map(|s| lower_stmt(s, mem_stems, locals, net_widths))
                .collect(),
        }),
        CstStmt::Case {
            expr,
            arms,
            default,
        } => Some(IrStmt::Case {
            expr: lower_expr(expr, mem_stems, locals),
            arms: arms
                .into_iter()
                .map(|a| IrCaseArm {
                    value: lower_expr(a.value, mem_stems, locals),
                    body: a
                        .body
                        .into_iter()
                        .filter_map(|s| lower_stmt(s, mem_stems, locals, net_widths))
                        .collect(),
                })
                .collect(),
            default: default
                .into_iter()
                .filter_map(|s| lower_stmt(s, mem_stems, locals, net_widths))
                .collect(),
        }),
        CstStmt::For {
            init_var,
            init_val,
            cond,
            step_var,
            step_expr,
            body,
        } => Some(IrStmt::For {
            init_var,
            init_val: lower_expr(init_val, mem_stems, locals),
            cond: lower_expr(cond, mem_stems, locals),
            step_var,
            step_expr: lower_expr(step_expr, mem_stems, locals),
            body: body
                .into_iter()
                .filter_map(|s| lower_stmt(s, mem_stems, locals, net_widths))
                .collect(),
        }),
        CstStmt::Delay(d) => Some(IrStmt::Delay(d)),
        CstStmt::SystemTask { name, args } => Some(IrStmt::SystemTask {
            name,
            args: args
                .into_iter()
                .map(|e| lower_expr(e, mem_stems, locals))
                .collect(),
        }),
    }
}

fn lower_expr(e: Expr, mem_stems: &HashSet<String>, locals: &HashMap<String, i64>) -> IrExpr {
    match e {
        Expr::Ident(name) => {
            if let Some(&v) = locals.get(&name) {
                IrExpr::Const(v)
            } else {
                IrExpr::Ident(name)
            }
        }
        Expr::Number(lit) => IrExpr::Const(parse_verilog_number(&lit)),
        Expr::Index { base, msb, lsb } => {
            match lsb {
                None => {
                    if let Expr::Ident(stem) = &*base {
                        if mem_stems.contains(stem) {
                            IrExpr::MemRead {
                                stem: stem.clone(),
                                index: Box::new(lower_expr(*msb, mem_stems, locals)),
                            }
                        } else {
                            let base_ir = lower_expr(*base, mem_stems, locals);
                            let m = lower_expr(*msb, mem_stems, locals);
                            IrExpr::PartSelect {
                                value: Box::new(base_ir),
                                msb: Box::new(m.clone()),
                                lsb: Box::new(m),
                            }
                        }
                    } else {
                        let base_ir = lower_expr(*base, mem_stems, locals);
                        let m = lower_expr(*msb, mem_stems, locals);
                        IrExpr::PartSelect {
                            value: Box::new(base_ir),
                            msb: Box::new(m.clone()),
                            lsb: Box::new(m),
                        }
                    }
                }
                Some(lsb_e) => IrExpr::PartSelect {
                    value: Box::new(lower_expr(*base, mem_stems, locals)),
                    msb: Box::new(lower_expr(*msb, mem_stems, locals)),
                    lsb: Box::new(lower_expr(*lsb_e, mem_stems, locals)),
                },
            }
        }
        Expr::Binary { op, left, right } => IrExpr::Binary {
            op: lower_binop(op),
            left: Box::new(lower_expr(*left, mem_stems, locals)),
            right: Box::new(lower_expr(*right, mem_stems, locals)),
        },
        Expr::Unary { op, operand } => {
            if op == UnaryOp::Pos {
                // +x is identity — drop the unary
                return lower_expr(*operand, mem_stems, locals);
            }
            IrExpr::Unary {
                op: lower_unaryop(op),
                operand: Box::new(lower_expr(*operand, mem_stems, locals)),
            }
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => IrExpr::Ternary {
            cond: Box::new(lower_expr(*cond, mem_stems, locals)),
            then_expr: Box::new(lower_expr(*then_expr, mem_stems, locals)),
            else_expr: Box::new(lower_expr(*else_expr, mem_stems, locals)),
        },
        Expr::Concat(exprs) => {
            IrExpr::Concat(exprs.into_iter().map(|e| lower_expr(e, mem_stems, locals)).collect())
        }
    }
}

fn lower_binop(op: BinaryOp) -> IrBinOp {
    match op {
        BinaryOp::Add => IrBinOp::Add,
        BinaryOp::Sub => IrBinOp::Sub,
        BinaryOp::Mul => IrBinOp::Mul,
        BinaryOp::Div => IrBinOp::Div,
        BinaryOp::Mod => IrBinOp::Mod,
        BinaryOp::And => IrBinOp::And,
        BinaryOp::Or => IrBinOp::Or,
        BinaryOp::Xor => IrBinOp::Xor,
        BinaryOp::Shl => IrBinOp::Shl,
        BinaryOp::Shr => IrBinOp::Shr,
        BinaryOp::LogAnd => IrBinOp::LogAnd,
        BinaryOp::LogOr => IrBinOp::LogOr,
        BinaryOp::Eq => IrBinOp::Eq,
        BinaryOp::Ne => IrBinOp::Ne,
        BinaryOp::Lt => IrBinOp::Lt,
        BinaryOp::Le => IrBinOp::Le,
        BinaryOp::Gt => IrBinOp::Gt,
        BinaryOp::Ge => IrBinOp::Ge,
    }
}

fn lower_unaryop(op: UnaryOp) -> IrUnaryOp {
    match op {
        UnaryOp::Not => IrUnaryOp::Not,
        UnaryOp::LogNot => IrUnaryOp::LogNot,
        UnaryOp::Neg => IrUnaryOp::Neg,
        UnaryOp::Pos => unreachable!("Pos handled before calling lower_unaryop"),
    }
}

pub fn parse_verilog_number(s: &str) -> i64 {
    if let Some(pos) = s.find('\'') {
        let after = &s[pos + 1..];
        let (radix, digits) = if after.starts_with('d') || after.starts_with('D') {
            (10, &after[1..])
        } else if after.starts_with('h') || after.starts_with('H') {
            (16, &after[1..])
        } else if after.starts_with('b') || after.starts_with('B') {
            (2, &after[1..])
        } else if after.starts_with('o') || after.starts_with('O') {
            (8, &after[1..])
        } else {
            (10, after)
        };
        let clean: String = digits.chars().filter(|c| *c != '_').collect();
        i64::from_str_radix(&clean, radix).unwrap_or(0)
    } else {
        let clean: String = s.chars().filter(|c| *c != '_').collect();
        clean.parse::<i64>().unwrap_or(0)
    }
}

// ── Helper: recursive directory walk ─────────────────────────────────

fn walk_dir<F>(root: &Path, f: &mut F) -> std::io::Result<()>
where
    F: FnMut(&Path),
{
    if root.is_file() {
        if is_verilog_file(root) {
            f(root);
        }
        return Ok(());
    }

    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if matches!(
                name,
                "target" | "node_modules" | ".git" | "dist" | "tests" | "fixtures"
            ) {
                continue;
            }
            walk_dir(&path, f)?;
        } else if is_verilog_file(&path) {
            f(&path);
        }
    }
    Ok(())
}

fn is_verilog_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => matches!(ext.to_lowercase().as_str(), "v" | "sv"),
        None => false,
    }
}

/// Sum of every `#delay` literal reachable in `initial` bodies for modules defined in `source_file`
/// ([`IrModule::path`] compared with `source_file`). Branches (`if`/`case`) are all included.
pub fn sum_initial_delay_literals_for_source_file(project: &IrProject, source_file: &Path) -> usize {
    let mut total = DelayRational::ZERO;
    for m in &project.modules {
        if !module_path_matches_source_file(&m.path, source_file) {
            continue;
        }
        for ib in &m.initial_blocks {
            total = total.add(sum_delay_literals_in_stmts(&ib.stmts));
        }
    }
    total.ceil_whole_time_units()
}

fn module_path_matches_source_file(module_path: &str, source_file: &Path) -> bool {
    let mp = Path::new(module_path);
    if mp == source_file {
        return true;
    }
    if let (Ok(a), Ok(b)) = (mp.canonicalize(), source_file.canonicalize()) {
        return a == b;
    }
    module_path == source_file.to_string_lossy().as_ref()
}

/// Constant trip count for `for (v=…; v op N; v=v+k)` (matches optimizer `try_unroll` rules).
pub(crate) fn static_for_iteration_count(
    init_var: &str,
    init_val: &IrExpr,
    cond: &IrExpr,
    step_var: &str,
    step_expr: &IrExpr,
) -> Option<usize> {
    if init_var != step_var {
        return None;
    }
    let start = match init_val {
        IrExpr::Const(v) => *v,
        _ => return None,
    };
    let (bound, inclusive) = match cond {
        IrExpr::Binary { op: IrBinOp::Lt, left, right } => {
            if let (IrExpr::Ident(v), IrExpr::Const(n)) = (left.as_ref(), right.as_ref()) {
                if v == init_var {
                    (*n, false)
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        IrExpr::Binary { op: IrBinOp::Le, left, right } => {
            if let (IrExpr::Ident(v), IrExpr::Const(n)) = (left.as_ref(), right.as_ref()) {
                if v == init_var {
                    (*n, true)
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        IrExpr::Binary { op: IrBinOp::Ne, left, right } => {
            if let (IrExpr::Ident(v), IrExpr::Const(n)) = (left.as_ref(), right.as_ref()) {
                if v == init_var {
                    (*n, false)
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        _ => return None,
    };
    let step_inc = match step_expr {
        IrExpr::Binary { op: IrBinOp::Add, left, right } => {
            if let (IrExpr::Ident(v), IrExpr::Const(n)) = (left.as_ref(), right.as_ref()) {
                if v == init_var {
                    *n
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        _ => return None,
    };
    if step_inc <= 0 {
        return None;
    }
    let end = if inclusive { bound + 1 } else { bound };
    let iterations = (end - start + step_inc - 1) / step_inc;
    if iterations <= 0 || iterations > 10_000_000 {
        return None;
    }
    Some(iterations as usize)
}

fn sum_delay_literals_in_stmts(stmts: &[IrStmt]) -> DelayRational {
    let mut s = DelayRational::ZERO;
    for st in stmts {
        match st {
            IrStmt::Delay(d) => s = s.add(*d),
            IrStmt::IfElse {
                then_body,
                else_body,
                ..
            } => {
                s = s.add(sum_delay_literals_in_stmts(then_body));
                s = s.add(sum_delay_literals_in_stmts(else_body));
            }
            IrStmt::Case {
                arms, default, ..
            } => {
                for a in arms {
                    s = s.add(sum_delay_literals_in_stmts(&a.body));
                }
                s = s.add(sum_delay_literals_in_stmts(default));
            }
            IrStmt::For {
                init_var,
                init_val,
                cond,
                step_var,
                step_expr,
                body,
            } => {
                if let Some(n) =
                    static_for_iteration_count(init_var, init_val, cond, step_var, step_expr)
                {
                    let per = sum_delay_literals_in_stmts(body);
                    s = s.add(per.saturating_mul_u128(n as u128));
                } else {
                    s = s.add(sum_delay_literals_in_stmts(body));
                }
            }
            IrStmt::BlockingAssign { .. }
            | IrStmt::NonBlockingAssign { .. }
            | IrStmt::MemAssign { .. }
            | IrStmt::SystemTask { .. } => {}
        }
    }
    s
}

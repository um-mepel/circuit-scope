use std::path::Path;

use crate::delay_rational::DelayRational;
use crate::lexer;
use crate::parser::{
    self, BinaryOp, CaseArm, CstModule, CstModuleItem, CstStmt, EdgeKind, Expr, Sensitivity,
    UnaryOp,
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

/// Port connection: `.port_name(signal_expr)`.
#[derive(Debug, Clone)]
pub struct IrPortConn {
    pub port_name: String,
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

// ── Lowering from CST to IR ─────────────────────────────────────────

fn ir_module_from_cst(cst: CstModule) -> IrModule {
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
                    rhs: lower_expr(expr),
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
                        expr: lower_expr(c.expr),
                    })
                    .collect();
                instances.push(IrInstance {
                    module_name,
                    instance_name,
                    connections: conns,
                });
            }
            CstModuleItem::Always { sensitivity, body } => {
                always_blocks.push(lower_always(sensitivity, body));
            }
            CstModuleItem::Initial { body } => {
                initial_blocks.push(IrInitial {
                    stmts: body.into_iter().filter_map(lower_stmt).collect(),
                });
            }
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
    }
}

fn lower_always(sens: Sensitivity, stmts: Vec<CstStmt>) -> IrAlways {
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
        stmts: stmts.into_iter().filter_map(|s| lower_stmt(s)).collect(),
    }
}

fn lower_stmt(s: CstStmt) -> Option<IrStmt> {
    match s {
        CstStmt::BlockingAssign { lhs, rhs } => Some(IrStmt::BlockingAssign {
            lhs,
            rhs: lower_expr(rhs),
        }),
        CstStmt::NonBlockingAssign { lhs, rhs } => Some(IrStmt::NonBlockingAssign {
            lhs,
            rhs: lower_expr(rhs),
        }),
        CstStmt::IfElse {
            cond,
            then_body,
            else_body,
        } => Some(IrStmt::IfElse {
            cond: lower_expr(cond),
            then_body: then_body.into_iter().filter_map(lower_stmt).collect(),
            else_body: else_body.into_iter().filter_map(lower_stmt).collect(),
        }),
        CstStmt::Case {
            expr,
            arms,
            default,
        } => Some(IrStmt::Case {
            expr: lower_expr(expr),
            arms: arms
                .into_iter()
                .map(|a| IrCaseArm {
                    value: lower_expr(a.value),
                    body: a.body.into_iter().filter_map(lower_stmt).collect(),
                })
                .collect(),
            default: default.into_iter().filter_map(lower_stmt).collect(),
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
            init_val: lower_expr(init_val),
            cond: lower_expr(cond),
            step_var,
            step_expr: lower_expr(step_expr),
            body: body.into_iter().filter_map(lower_stmt).collect(),
        }),
        CstStmt::Delay(d) => Some(IrStmt::Delay(d)),
        CstStmt::SystemTask { name, args } => Some(IrStmt::SystemTask {
            name,
            args: args.into_iter().map(lower_expr).collect(),
        }),
    }
}

fn lower_expr(e: Expr) -> IrExpr {
    match e {
        Expr::Ident(name) => IrExpr::Ident(name),
        Expr::Number(lit) => IrExpr::Const(parse_verilog_number(&lit)),
        Expr::Binary { op, left, right } => IrExpr::Binary {
            op: lower_binop(op),
            left: Box::new(lower_expr(*left)),
            right: Box::new(lower_expr(*right)),
        },
        Expr::Unary { op, operand } => {
            if op == UnaryOp::Pos {
                // +x is identity — drop the unary
                return lower_expr(*operand);
            }
            IrExpr::Unary {
                op: lower_unaryop(op),
                operand: Box::new(lower_expr(*operand)),
            }
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => IrExpr::Ternary {
            cond: Box::new(lower_expr(*cond)),
            then_expr: Box::new(lower_expr(*then_expr)),
            else_expr: Box::new(lower_expr(*else_expr)),
        },
        Expr::Concat(exprs) => IrExpr::Concat(exprs.into_iter().map(lower_expr).collect()),
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
            | IrStmt::SystemTask { .. } => {}
        }
    }
    s
}

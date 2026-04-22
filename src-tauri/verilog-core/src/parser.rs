use crate::delay_rational::DelayRational;
use crate::lexer::{Token, TokenKind};
use crate::{Diagnostic, Module, ParseResult, Port, Severity, SourceFile};

/// Concrete syntax for a parsed Verilog (IEEE 1364) file. Intentionally minimal: records modules
/// and body items for lowering to IR — not a full SystemVerilog front end.
#[derive(Debug, Clone)]
pub struct CstFile {
    pub modules: Vec<CstModule>,
}

/// Concrete syntax node for a `module` declaration.
#[derive(Debug, Clone)]
pub struct CstModule {
    pub name: String,
    pub ports: Vec<Port>,
    /// `#(parameter W = 16, ...)` from the module header.
    pub module_parameters: Vec<(String, Expr)>,
    pub path: String,
    pub items: Vec<CstModuleItem>,
}

/// Module body item.
#[derive(Debug, Clone)]
pub enum CstModuleItem {
    NetDecl {
        kind: NetKind,
        /// Packed `[msb:lsb]` when present; scalar width uses `width` when this is `None`.
        packed_dim: Option<(Expr, Expr)>,
        width: usize,
        names: Vec<String>,
        /// Unpacked `[hi:lo]` per stem (`x[0:9]`); bounds may be expressions (e.g. `[BoothIter-1:0]`).
        unpacked_stems: Vec<(String, Expr, Expr)>,
        /// `input` / `output` / `inout` from a directional port declaration in the module body.
        decl_dir: Option<String>,
    },
    Assign {
        target: AssignTarget,
        expr: Expr,
    },
    Instance {
        module_name: String,
        /// `#(.param(expr), …)` — empty if no parameter override list.
        parameter_assignments: Vec<(String, Expr)>,
        instance_name: String,
        connections: Vec<PortConnection>,
    },
    Always {
        sensitivity: Sensitivity,
        body: Vec<CstStmt>,
    },
    Initial {
        body: Vec<CstStmt>,
    },
    /// `localparam` / `parameter` assignments (`localparam a = 1, b = 2;`).
    LocalParam {
        assignments: Vec<(String, Expr)>,
    },
    /// `generate for (i=0; i<N; i=i+1) begin … <one instance> end` — expanded during IR lowering
    /// so `N` uses the module's **current** parameters (specialized `#(.W(11))`, etc.).
    GenerateFor {
        loop_var: String,
        upper_expr: Expr,
        module_name: String,
        parameter_assignments: Vec<(String, Expr)>,
        instance_stem: String,
        connections: Vec<PortConnection>,
    },
}

/// Port connection: `.port_name(signal_expr)` or **positional** (`expr` only, mapped to child ports by order).
#[derive(Debug, Clone)]
pub struct PortConnection {
    pub port_name: Option<String>,
    pub expr: Expr,
}

/// Sensitivity list for always blocks.
#[derive(Debug, Clone)]
pub enum Sensitivity {
    Star,
    EdgeList(Vec<SensEdge>),
}

#[derive(Debug, Clone)]
pub struct SensEdge {
    pub edge: EdgeKind,
    pub signal: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Posedge,
    Negedge,
    Level,
}

/// Left-hand side of a procedural assignment (`reg`, `reg[i]`, `reg[msb:lsb]`).
#[derive(Debug, Clone)]
pub enum AssignTarget {
    Whole(String),
    BitSelect { reg: String, index: Expr },
    PartSelect {
        reg: String,
        msb: Expr,
        lsb: Expr,
    },
}

/// Procedural statement inside an always/initial block.
#[derive(Debug, Clone)]
pub enum CstStmt {
    BlockingAssign { target: AssignTarget, rhs: Expr },
    NonBlockingAssign { target: AssignTarget, rhs: Expr },
    IfElse {
        cond: Expr,
        then_body: Vec<CstStmt>,
        else_body: Vec<CstStmt>,
    },
    Case {
        expr: Expr,
        arms: Vec<CaseArm>,
        default: Vec<CstStmt>,
    },
    For {
        init_var: String,
        init_val: Expr,
        cond: Expr,
        step_var: String,
        step_expr: Expr,
        body: Vec<CstStmt>,
    },
    Delay(DelayRational),
    SystemTask { name: String, args: Vec<Expr> },
}

#[derive(Debug, Clone)]
pub struct CaseArm {
    pub value: Expr,
    pub body: Vec<CstStmt>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NetKind {
    Wire,
    Reg,
}

/// Expression tree used for assignments and optimisation.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Ident(String),
    Number(String),
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Ternary {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    Concat(Vec<Expr>),
    /// Index or part-select postfix: `base[msb]` or `base[msb:lsb]`.
    Index {
        base: Box<Expr>,
        msb: Box<Expr>,
        lsb: Option<Box<Expr>>,
    },
    /// `$clog2(expr)` — ceiling log2; evaluated for parameters/localparams.
    Clog2(Box<Expr>),
    /// `$signed(expr)` — interpret packed value as signed (IEEE 1364).
    Signed(Box<Expr>),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,      // &
    Or,       // |
    Xor,      // ^
    Shl,      // <<
    Shr,      // >>
    Ashr,     // >>>
    LogAnd,   // &&
    LogOr,    // ||
    Eq,       // ==
    Ne,       // !=
    Lt,       // <
    Le,       // <=
    Gt,       // >
    Ge,       // >=
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum UnaryOp {
    Not,    // ~
    LogNot, // !
    Neg,    // -
    Pos,    // +
}

pub(crate) fn parse_cst<'a>(
    file: &'a SourceFile,
    tokens: &'a [Token],
) -> (CstFile, Vec<Diagnostic>) {
    let mut parser = Parser::new(file, tokens);
    parser.parse()
}

pub(crate) fn parse_file(file: &SourceFile, tokens: &[Token]) -> ParseResult {
    let (cst, diagnostics) = parse_cst(file, tokens);

    let modules = cst
        .modules
        .into_iter()
        .map(|m| Module {
            name: m.name,
            ports: m.ports,
            path: m.path,
        })
        .collect();

    ParseResult { modules, diagnostics }
}

struct Parser<'a> {
    file: &'a SourceFile,
    tokens: &'a [Token],
    pos: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    fn new(file: &'a SourceFile, tokens: &'a [Token]) -> Self {
        Self {
            file,
            tokens,
            pos: 0,
            diagnostics: Vec::new(),
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn bump(&mut self) {
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
    }

    fn match_kind(&mut self, kind: TokenKind) -> bool {
        if self.current().kind == kind {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_identifier(&mut self, message: &str) -> Option<String> {
        if self.current().kind == TokenKind::Identifier {
            let name = self.current().lexeme.clone();
            self.bump();
            Some(name)
        } else {
            self.error_at_current(message);
            None
        }
    }

    fn error_at_current(&mut self, message: &str) {
        let tok = self.current();
        let (line, col) = offset_to_line_col(&self.file.content, tok.offset);
        self.diagnostics.push(Diagnostic {
            message: message.to_string(),
            severity: Severity::Error,
            line,
            column: col,
            path: self.file.path.clone(),
        });
        self.bump();
    }

    fn parse(&mut self) -> (CstFile, Vec<Diagnostic>) {
        let mut modules = Vec::new();
        while self.current().kind != TokenKind::Eof {
            if self.current().kind == TokenKind::Module {
                if let Some(m) = self.parse_module() {
                    modules.push(m);
                }
            } else {
                self.bump();
            }
        }
        (
            CstFile { modules },
            std::mem::take(&mut self.diagnostics),
        )
    }

    fn parse_module(&mut self) -> Option<CstModule> {
        self.bump(); // consume 'module'
        let name = self.expect_identifier("expected module name")?;
        let mut ports = Vec::new();

        let module_parameters = self.parse_module_parameter_list();

        if self.match_kind(TokenKind::LParen) {
            // ANSI lists: `output [6:0] a, b` repeats direction and vector width for comma-separated names.
            // Parametric ranges like `[W-1:0]` must be parsed as expressions and evaluated (see eval).
            let mut last_port_direction: Option<String> = None;
            let mut last_port_width: usize = 1;
            'ansi_ports: loop {
                if matches!(self.current().kind, TokenKind::RParen | TokenKind::Eof) {
                    break 'ansi_ports;
                }

                let direction = if matches!(
                    self.current().kind,
                    TokenKind::Input | TokenKind::Output | TokenKind::Inout
                ) {
                    let d = self.current().lexeme.clone();
                    self.bump();
                    last_port_direction = Some(d.clone());
                    // Optional net type: `wire` / `reg` / `logic` (e.g. `input wire clk`).
                    if matches!(
                        self.current().kind,
                        TokenKind::Wire | TokenKind::Reg | TokenKind::Logic
                    ) {
                        self.bump();
                    }
                    if self.current().kind == TokenKind::Signed {
                        self.bump();
                    }
                    if self.current().kind == TokenKind::LBracket {
                        self.bump();
                        let msb = self.parse_expression(0);
                        let lsb = if self.match_kind(TokenKind::Colon) {
                            self.parse_expression(0)
                        } else {
                            msb.clone()
                        };
                        let _ = self.match_kind(TokenKind::RBracket);
                        last_port_width =
                            Self::eval_port_packed_width(&msb, &lsb, &module_parameters);
                    } else {
                        last_port_width = 1;
                    }
                    Some(d)
                } else if self.current().kind == TokenKind::Identifier {
                    // Continuation after `input [7:0] a,` **or** legacy `(a, b, c)` with no directions.
                    if last_port_direction.is_none() {
                        last_port_width = 1;
                    }
                    last_port_direction.clone()
                } else {
                    self.error_at_current("expected port direction or name");
                    break 'ansi_ports;
                };

                let Some(port_name) = self.expect_identifier("expected port name") else {
                    break 'ansi_ports;
                };
                ports.push(Port {
                    direction,
                    name: port_name,
                    width: last_port_width,
                });

                if !self.match_kind(TokenKind::Comma) {
                    break 'ansi_ports;
                }
            }

            let _ = self.match_kind(TokenKind::RParen);
        }

        // skip tokens until ';' to get past header, but flag obviously bad cases
        let mut saw_semicolon = false;
        while self.current().kind != TokenKind::Semicolon
            && self.current().kind != TokenKind::Eof
        {
            if matches!(
                self.current().kind,
                TokenKind::Assign
                    | TokenKind::Wire
                    | TokenKind::Reg
                    | TokenKind::Logic
                    | TokenKind::Module
                    | TokenKind::Endmodule
            ) {
                self.error_at_current("expected ';' after module header");
                break;
            }
            self.bump();
        }
        if self.match_kind(TokenKind::Semicolon) {
            saw_semicolon = true;
        }
        if !saw_semicolon {
            // already reported above
        }

        let mut items = Vec::new();
        while self.current().kind != TokenKind::Endmodule
            && self.current().kind != TokenKind::Eof
        {
            if self.current().kind == TokenKind::Genvar {
                self.skip_genvar_statement();
            } else if self.current().kind == TokenKind::Generate {
                let mut inner = self.parse_generate_construct();
                items.append(&mut inner);
            } else if matches!(
                self.current().kind,
                TokenKind::Input | TokenKind::Output | TokenKind::Inout
            ) {
                if let Some(item) = self.parse_directional_net_decl() {
                    items.push(item);
                }
            } else if self.current().kind == TokenKind::Wire
                || self.current().kind == TokenKind::Reg
                || self.current().kind == TokenKind::Integer
                || self.current().kind == TokenKind::Logic
            {
                if let Some(item) = self.parse_net_decl() {
                    items.push(item);
                }
            } else if self.current().kind == TokenKind::Assign {
                if let Some(item) = self.parse_assign() {
                    items.push(item);
                }
            } else if self.current().kind == TokenKind::Always
                || self.current().kind == TokenKind::Initial
            {
                if let Some(item) = self.parse_always() {
                    items.push(item);
                }
            } else if self.current().kind == TokenKind::Parameter
                || self.current().kind == TokenKind::Localparam
            {
                self.bump();
                if let Some(assigns) = self.parse_param_assign_list_after_keyword() {
                    items.push(CstModuleItem::LocalParam { assignments: assigns });
                } else {
                    self.skip_to_semicolon();
                }
            } else if self.current().kind == TokenKind::Identifier {
                if let Some(item) = self.parse_instance_like() {
                    items.push(item);
                } else {
                    self.skip_to_semicolon();
                }
            } else {
                self.bump();
            }
        }
        let _ = self.match_kind(TokenKind::Endmodule);

        Some(CstModule {
            name,
            ports,
            module_parameters,
            path: self.file.path.clone(),
            items,
        })
    }

    fn skip_to_semicolon(&mut self) {
        while self.current().kind != TokenKind::Semicolon
            && self.current().kind != TokenKind::Eof
        {
            self.bump();
        }
        let _ = self.match_kind(TokenKind::Semicolon);
    }

    fn skip_genvar_statement(&mut self) {
        self.bump(); // genvar
        let _ = self.skip_to_semicolon();
    }

    /// Bit-width of `[msb:lsb]` for ANSI ports using module-parameter environment only.
    fn eval_port_packed_width(
        msb: &Expr,
        lsb: &Expr,
        module_parameters: &[(String, Expr)],
    ) -> usize {
        let known = crate::expr_const::resolve_local_param_values(module_parameters);
        let m = crate::expr_const::const_eval_param_expr(msb, &known).unwrap_or(1);
        let l = crate::expr_const::const_eval_param_expr(lsb, &known).unwrap_or(1);
        ((m - l).abs() as usize).saturating_add(1).max(1)
    }

    fn skip_to_endgenerate(&mut self) {
        while self.current().kind != TokenKind::Endgenerate
            && self.current().kind != TokenKind::Eof
        {
            self.bump();
        }
        let _ = self.match_kind(TokenKind::Endgenerate);
    }

    fn parse_module_parameter_list(&mut self) -> Vec<(String, Expr)> {
        let mut v = Vec::new();
        if !self.match_kind(TokenKind::Hash) {
            return v;
        }
        if !self.match_kind(TokenKind::LParen) {
            return v;
        }
        while self.current().kind != TokenKind::RParen && self.current().kind != TokenKind::Eof {
            if self.current().kind == TokenKind::Parameter {
                self.bump();
            }
            let Some(pname) = self.expect_identifier("expected parameter name") else {
                break;
            };
            if !self.match_kind(TokenKind::Eq) {
                self.error_at_current("expected `=` in module parameter list");
                while self.current().kind != TokenKind::RParen && self.current().kind != TokenKind::Eof {
                    self.bump();
                }
                break;
            }
            let rhs = self.parse_expression(0);
            v.push((pname, rhs));
            if self.match_kind(TokenKind::Comma) {
                continue;
            }
            break;
        }
        let _ = self.match_kind(TokenKind::RParen);
        v
    }

    fn parse_directional_net_decl(&mut self) -> Option<CstModuleItem> {
        if !matches!(
            self.current().kind,
            TokenKind::Input | TokenKind::Output | TokenKind::Inout
        ) {
            return None;
        }
        let decl_dir = self.current().lexeme.clone();
        self.bump();
        let mut item = self.parse_net_decl_core(NetKind::Wire)?;
        if let CstModuleItem::NetDecl { decl_dir: d, .. } = &mut item {
            *d = Some(decl_dir);
        }
        Some(item)
    }

    /// `generate … endgenerate` with `for (i=0; i<W; i=i+1) begin : … <one instance>; end`.
    fn parse_generate_construct(&mut self) -> Vec<CstModuleItem> {
        self.bump(); // generate
        if self.current().kind != TokenKind::For {
            self.skip_to_endgenerate();
            return Vec::new();
        }
        self.bump(); // for
        if !self.match_kind(TokenKind::LParen) {
            self.skip_to_endgenerate();
            return Vec::new();
        }
        let loop_var = match self.expect_identifier("expected loop variable") {
            Some(v) => v,
            None => {
                self.skip_to_endgenerate();
                return Vec::new();
            }
        };
        if !self.match_kind(TokenKind::Eq) {
            self.skip_to_endgenerate();
            return Vec::new();
        }
        let _init = self.parse_expression(0);
        if !self.match_kind(TokenKind::Semicolon) {
            self.skip_to_endgenerate();
            return Vec::new();
        }
        let _iter = match self.expect_identifier("expected loop variable") {
            Some(v) => v,
            None => {
                self.skip_to_endgenerate();
                return Vec::new();
            }
        };
        if !self.match_kind(TokenKind::Lt) {
            self.skip_to_endgenerate();
            return Vec::new();
        }
        let upper_expr = self.parse_expression(0);
        if !self.match_kind(TokenKind::Semicolon) {
            self.skip_to_endgenerate();
            return Vec::new();
        }
        let _lhs = match self.expect_identifier("expected loop variable") {
            Some(v) => v,
            None => {
                self.skip_to_endgenerate();
                return Vec::new();
            }
        };
        if !self.match_kind(TokenKind::Eq) {
            self.skip_to_endgenerate();
            return Vec::new();
        }
        let _step = self.parse_expression(0);
        if !self.match_kind(TokenKind::RParen) {
            self.skip_to_endgenerate();
            return Vec::new();
        }
        if !self.match_kind(TokenKind::Begin) {
            self.skip_to_endgenerate();
            return Vec::new();
        }
        if self.current().kind == TokenKind::Colon {
            self.bump();
            let _ = self.expect_identifier("expected block name after begin:");
        }
        let inst = match self.parse_instance_like() {
            Some(CstModuleItem::Instance {
                module_name,
                parameter_assignments,
                instance_name,
                connections,
            }) => (module_name, parameter_assignments, instance_name, connections),
            _ => {
                self.skip_to_endgenerate();
                return Vec::new();
            }
        };
        if !self.match_kind(TokenKind::End) {
            self.skip_to_endgenerate();
            return Vec::new();
        }
        if !self.match_kind(TokenKind::Endgenerate) {
            self.skip_to_endgenerate();
            return Vec::new();
        }
        vec![CstModuleItem::GenerateFor {
            loop_var,
            upper_expr,
            module_name: inst.0.clone(),
            parameter_assignments: inst.1.clone(),
            instance_stem: inst.2.clone(),
            connections: inst.3.clone(),
        }]
    }

    /// After consuming the `parameter` / `localparam` keyword: optional `[high:low]`, then
    /// `name = expr` comma-lists (IEEE 1364).
    fn parse_param_assign_list_after_keyword(&mut self) -> Option<Vec<(String, Expr)>> {
        if self.current().kind == TokenKind::LBracket {
            while self.current().kind != TokenKind::RBracket && self.current().kind != TokenKind::Eof {
                self.bump();
            }
            let _ = self.match_kind(TokenKind::RBracket);
        }
        let mut pairs = Vec::new();
        loop {
            let name = self.expect_identifier("expected parameter name")?;
            if !self.match_kind(TokenKind::Eq) {
                self.error_at_current("expected `=` in parameter/localparam declaration");
                return None;
            }
            let rhs = self.parse_expression(0);
            pairs.push((name, rhs));
            if self.match_kind(TokenKind::Comma) {
                continue;
            }
            break;
        }
        let _ = self.match_kind(TokenKind::Semicolon);
        Some(pairs)
    }

    fn parse_net_decl(&mut self) -> Option<CstModuleItem> {
        let kind = match self.current().kind {
            TokenKind::Wire => NetKind::Wire,
            TokenKind::Integer => NetKind::Reg,
            TokenKind::Logic => NetKind::Reg,
            _ => NetKind::Reg,
        };
        self.bump();
        self.parse_net_decl_core(kind)
    }

    fn parse_net_decl_core(&mut self, kind: NetKind) -> Option<CstModuleItem> {
        let mut width = if kind == NetKind::Reg && self.tokens[self.pos - 1].lexeme == "integer" {
            32
        } else {
            1
        };

        if self.current().kind == TokenKind::Signed {
            self.bump();
        }

        let mut packed_dim: Option<(Expr, Expr)> = None;
        if self.current().kind == TokenKind::LBracket {
            self.bump(); // [
            let msb = self.parse_expression(0);
            let lsb = if self.match_kind(TokenKind::Colon) {
                self.parse_expression(0)
            } else {
                msb.clone()
            };
            let _ = self.match_kind(TokenKind::RBracket);
            packed_dim = Some((msb, lsb));
            width = 1;
        }

        let mut names = Vec::new();
        let mut unpacked_stems: Vec<(String, Expr, Expr)> = Vec::new();
        loop {
            let stem = match self.expect_identifier("expected signal name") {
                Some(n) => n,
                None => break,
            };
            if self.current().kind == TokenKind::LBracket {
                self.bump();
                let hi = self.parse_expression(0);
                let lo = if self.match_kind(TokenKind::Colon) {
                    self.parse_expression(0)
                } else {
                    hi.clone()
                };
                let _ = self.match_kind(TokenKind::RBracket);
                unpacked_stems.push((stem.clone(), hi, lo));
                names.push(stem);
            } else {
                names.push(stem);
            }
            if !self.match_kind(TokenKind::Comma) {
                break;
            }
        }
        self.skip_to_semicolon();
        if names.is_empty() {
            None
        } else {
            Some(CstModuleItem::NetDecl {
                kind,
                packed_dim,
                width,
                names,
                unpacked_stems,
                decl_dir: None,
            })
        }
    }

    fn parse_assign(&mut self) -> Option<CstModuleItem> {
        self.bump(); // consume 'assign'
        let reg = match self.expect_identifier("expected left-hand side of assign") {
            Some(name) => name,
            None => {
                self.skip_to_semicolon();
                return None;
            }
        };
        let target = self.parse_assign_target_suffix(reg);
        let _ = self.match_kind(TokenKind::Eq);
        let expr = self.parse_expression(0);
        self.skip_to_semicolon();
        Some(CstModuleItem::Assign { target, expr })
    }

    fn parse_instance_like(&mut self) -> Option<CstModuleItem> {
        let module_name = self.current().lexeme.clone();
        self.bump();
        let mut parameter_assignments = Vec::new();
        if self.match_kind(TokenKind::Hash) {
            if self.match_kind(TokenKind::LParen) {
                while self.current().kind != TokenKind::RParen
                    && self.current().kind != TokenKind::Eof
                {
                    if !self.match_kind(TokenKind::Dot) {
                        break;
                    }
                    let pname = match self.expect_identifier("expected parameter name after `.`") {
                        Some(n) => n,
                        None => break,
                    };
                    if !self.match_kind(TokenKind::LParen) {
                        break;
                    }
                    let rhs = self.parse_expression(0);
                    let _ = self.match_kind(TokenKind::RParen);
                    parameter_assignments.push((pname, rhs));
                    let _ = self.match_kind(TokenKind::Comma);
                }
                let _ = self.match_kind(TokenKind::RParen);
            }
        }
        let instance_name =
            match self.expect_identifier("expected instance name after module name") {
                Some(n) => n,
                None => return None,
            };

        let mut connections = Vec::new();
        if self.match_kind(TokenKind::LParen) {
            while self.current().kind != TokenKind::RParen
                && self.current().kind != TokenKind::Eof
            {
                if self.current().kind == TokenKind::Dot {
                    self.bump(); // consume '.'
                    if let Some(port_name) = self.expect_identifier("expected port name") {
                        let _ = self.match_kind(TokenKind::LParen);
                        let expr = self.parse_expression(0);
                        let _ = self.match_kind(TokenKind::RParen);
                        connections.push(PortConnection {
                            port_name: Some(port_name),
                            expr,
                        });
                    }
                } else {
                    let expr = self.parse_expression(0);
                    connections.push(PortConnection {
                        port_name: None,
                        expr,
                    });
                }
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
            }
            let _ = self.match_kind(TokenKind::RParen);
        }
        self.skip_to_semicolon();
        Some(CstModuleItem::Instance {
            module_name,
            parameter_assignments,
            instance_name,
            connections,
        })
    }

    fn parse_always(&mut self) -> Option<CstModuleItem> {
        let is_initial = self.current().kind == TokenKind::Initial;
        self.bump(); // consume 'always' or 'initial'
        if is_initial {
            let body = self.parse_stmt_block();
            return Some(CstModuleItem::Initial { body });
        }
        // `always #delay stmt` — procedural delay before the statement (e.g. clock generators).
        if self.current().kind == TokenKind::Hash {
            let ticks = self.parse_delay_numeric_after_hash();
            let mut body = vec![CstStmt::Delay(ticks)];
            if let Some(s) = self.parse_stmt() {
                body.push(s);
            }
            return Some(CstModuleItem::Always {
                sensitivity: Sensitivity::Star,
                body,
            });
        }
        let sensitivity = self.parse_sensitivity();
        let body = self.parse_stmt_block();
        Some(CstModuleItem::Always { sensitivity, body })
    }

    /// `#` then integer or real delay (`#5`, `#0.5`); does not consume trailing `;`.
    fn parse_delay_numeric_after_hash(&mut self) -> DelayRational {
        if self.current().kind != TokenKind::Hash {
            return DelayRational::ZERO;
        }
        self.bump();
        Self::delay_from_delay_lexeme(&self.parse_delay_lexeme_tokens())
    }

    /// Reads delay literal after `#` already consumed: one number, optional `.` fraction.
    fn parse_delay_lexeme_tokens(&mut self) -> String {
        if self.current().kind != TokenKind::Number {
            return String::new();
        }
        let mut s = self.current().lexeme.clone();
        self.bump();
        if self.current().kind == TokenKind::Dot {
            self.bump();
            if self.current().kind == TokenKind::Number {
                s.push('.');
                s.push_str(&self.current().lexeme);
                self.bump();
            }
        }
        s
    }

    fn delay_from_delay_lexeme(s: &str) -> DelayRational {
        DelayRational::from_delay_lexeme(s)
    }

    fn parse_sensitivity(&mut self) -> Sensitivity {
        if !self.match_kind(TokenKind::At) {
            return Sensitivity::Star;
        }
        // IEEE 1364: `always @*` (implicit event list) without parentheses.
        if self.current().kind == TokenKind::Star {
            self.bump();
            return Sensitivity::Star;
        }
        if !self.match_kind(TokenKind::LParen) {
            return Sensitivity::Star;
        }
        // Check for @(*)
        if self.current().kind == TokenKind::Star {
            self.bump();
            let _ = self.match_kind(TokenKind::RParen);
            return Sensitivity::Star;
        }
        let mut edges = Vec::new();
        loop {
            let edge = if self.current().kind == TokenKind::Posedge {
                self.bump();
                EdgeKind::Posedge
            } else if self.current().kind == TokenKind::Negedge {
                self.bump();
                EdgeKind::Negedge
            } else {
                EdgeKind::Level
            };
            if let Some(sig) = self.expect_identifier("expected signal in sensitivity list") {
                edges.push(SensEdge { edge, signal: sig });
            } else {
                break;
            }
            // 'or' or ','  separates entries
            if self.current().kind == TokenKind::Identifier && self.current().lexeme == "or" {
                self.bump();
            } else if self.current().kind == TokenKind::Comma {
                self.bump();
            } else {
                break;
            }
        }
        let _ = self.match_kind(TokenKind::RParen);
        Sensitivity::EdgeList(edges)
    }

    fn parse_stmt_block(&mut self) -> Vec<CstStmt> {
        if self.match_kind(TokenKind::Begin) {
            let mut stmts = Vec::new();
            while self.current().kind != TokenKind::End
                && self.current().kind != TokenKind::Eof
            {
                if let Some(s) = self.parse_stmt() {
                    stmts.push(s);
                }
            }
            let _ = self.match_kind(TokenKind::End);
            stmts
        } else if let Some(s) = self.parse_stmt() {
            vec![s]
        } else {
            vec![]
        }
    }

    /// After the register/net identifier: optional `[bit]` or `[msb:lsb]`.
    fn parse_assign_target_suffix(&mut self, reg: String) -> AssignTarget {
        if self.current().kind == TokenKind::LBracket {
            self.bump();
            let msb = self.parse_expression(0);
            if self.match_kind(TokenKind::Colon) {
                let lsb = self.parse_expression(0);
                let _ = self.match_kind(TokenKind::RBracket);
                AssignTarget::PartSelect { reg, msb, lsb }
            } else {
                let _ = self.match_kind(TokenKind::RBracket);
                AssignTarget::BitSelect { reg, index: msb }
            }
        } else {
            AssignTarget::Whole(reg)
        }
    }

    fn parse_stmt(&mut self) -> Option<CstStmt> {
        match self.current().kind {
            TokenKind::Hash => {
                let ticks = self.parse_delay_numeric_after_hash();
                let _ = self.match_kind(TokenKind::Semicolon);
                return Some(CstStmt::Delay(ticks));
            }
            _ if self.current().kind == TokenKind::Identifier
                && self.current().lexeme.starts_with('$') =>
            {
                let name = self.current().lexeme.clone();
                self.bump();
                let mut args = Vec::new();
                if self.match_kind(TokenKind::LParen) {
                    if self.current().kind != TokenKind::RParen {
                        args.push(self.parse_expression(0));
                        while self.match_kind(TokenKind::Comma) {
                            args.push(self.parse_expression(0));
                        }
                    }
                    let _ = self.match_kind(TokenKind::RParen);
                }
                let _ = self.match_kind(TokenKind::Semicolon);
                return Some(CstStmt::SystemTask { name, args });
            }
            _ => {}
        }
        match self.current().kind {
            TokenKind::If => {
                self.bump();
                let _ = self.match_kind(TokenKind::LParen);
                let cond = self.parse_expression(0);
                let _ = self.match_kind(TokenKind::RParen);
                let then_body = self.parse_stmt_block();
                let else_body = if self.match_kind(TokenKind::Else) {
                    self.parse_stmt_block()
                } else {
                    vec![]
                };
                Some(CstStmt::IfElse { cond, then_body, else_body })
            }
            TokenKind::Case => {
                self.bump();
                let _ = self.match_kind(TokenKind::LParen);
                let expr = self.parse_expression(0);
                let _ = self.match_kind(TokenKind::RParen);
                let mut arms = Vec::new();
                let mut default = Vec::new();
                while self.current().kind != TokenKind::Endcase
                    && self.current().kind != TokenKind::Eof
                {
                    if self.current().kind == TokenKind::Default {
                        self.bump();
                        let _ = self.match_kind(TokenKind::Colon);
                        default = self.parse_stmt_block();
                    } else {
                        let value = self.parse_expression(0);
                        let _ = self.match_kind(TokenKind::Colon);
                        let body = self.parse_stmt_block();
                        arms.push(CaseArm { value, body });
                    }
                }
                let _ = self.match_kind(TokenKind::Endcase);
                Some(CstStmt::Case { expr, arms, default })
            }
            TokenKind::For => {
                self.bump();
                let _ = self.match_kind(TokenKind::LParen);
                // init: var = expr
                let init_var = self.expect_identifier("expected loop variable")?;
                let _ = self.match_kind(TokenKind::Eq);
                let init_val = self.parse_expression(0);
                let _ = self.match_kind(TokenKind::Semicolon);
                // cond
                let cond = self.parse_expression(0);
                let _ = self.match_kind(TokenKind::Semicolon);
                // step: var = expr
                let step_var = self.expect_identifier("expected step variable")?;
                let _ = self.match_kind(TokenKind::Eq);
                let step_expr = self.parse_expression(0);
                let _ = self.match_kind(TokenKind::RParen);
                let body = self.parse_stmt_block();
                Some(CstStmt::For { init_var, init_val, cond, step_var, step_expr, body })
            }
            TokenKind::Identifier => {
                let reg = self.current().lexeme.clone();
                self.bump();
                let target = self.parse_assign_target_suffix(reg);
                if self.current().kind == TokenKind::Le {
                    // Non-blocking assignment: lhs <= rhs
                    self.bump();
                    let rhs = self.parse_expression(0);
                    let _ = self.match_kind(TokenKind::Semicolon);
                    Some(CstStmt::NonBlockingAssign { target, rhs })
                } else if self.match_kind(TokenKind::Eq) {
                    // Blocking assignment: lhs = rhs
                    let rhs = self.parse_expression(0);
                    let _ = self.match_kind(TokenKind::Semicolon);
                    Some(CstStmt::BlockingAssign { target, rhs })
                } else {
                    self.skip_to_semicolon();
                    None
                }
            }
            _ => {
                self.bump();
                None
            }
        }
    }

    // ── Expression parsing with full Verilog precedence ──────────────

    /// Pratt-style expression parser. Precedence table (low → high):
    ///  1: ||         (LogOr)
    ///  2: &&         (LogAnd)
    ///  3: |          (Or)
    ///  4: ^          (Xor)
    ///  5: &          (And)
    ///  6: == !=      (Eq / Ne)
    ///  7: < <= > >=  (Comparison)
    ///  8: << >>      (Shift)
    ///  9: + -        (Add / Sub)
    /// 10: * / %      (Mul / Div / Mod)
    fn parse_expression(&mut self, min_prec: u8) -> Expr {
        let mut left = self.parse_unary();
        loop {
            let (op, prec) = match self.current().kind {
                TokenKind::LogOr  => (BinaryOp::LogOr, 1),
                TokenKind::LogAnd => (BinaryOp::LogAnd, 2),
                TokenKind::Pipe   => (BinaryOp::Or, 3),
                TokenKind::Caret  => (BinaryOp::Xor, 4),
                TokenKind::Amp    => (BinaryOp::And, 5),
                TokenKind::EqEq   => (BinaryOp::Eq, 6),
                TokenKind::Ne     => (BinaryOp::Ne, 6),
                TokenKind::Lt     => (BinaryOp::Lt, 7),
                TokenKind::Le     => (BinaryOp::Le, 7),
                TokenKind::Gt     => (BinaryOp::Gt, 7),
                TokenKind::Ge     => (BinaryOp::Ge, 7),
                TokenKind::Shl    => (BinaryOp::Shl, 8),
                TokenKind::Shr    => (BinaryOp::Shr, 8),
                TokenKind::Ashr   => (BinaryOp::Ashr, 8),
                TokenKind::Plus   => (BinaryOp::Add, 9),
                TokenKind::Minus  => (BinaryOp::Sub, 9),
                TokenKind::Star   => (BinaryOp::Mul, 10),
                TokenKind::Slash  => (BinaryOp::Div, 10),
                TokenKind::Percent => (BinaryOp::Mod, 10),
                // Ternary handled inline
                TokenKind::Question => {
                    if min_prec > 0 {
                        break;
                    }
                    self.bump();
                    let then_expr = self.parse_expression(0);
                    let _ = self.match_kind(TokenKind::Colon);
                    let else_expr = self.parse_expression(0);
                    left = Expr::Ternary {
                        cond: Box::new(left),
                        then_expr: Box::new(then_expr),
                        else_expr: Box::new(else_expr),
                    };
                    continue;
                }
                _ => break,
            };
            if prec < min_prec {
                break;
            }
            self.bump();
            let right = self.parse_expression(prec + 1);
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_unary(&mut self) -> Expr {
        match self.current().kind {
            TokenKind::Tilde => {
                self.bump();
                let operand = self.parse_unary();
                Expr::Unary { op: UnaryOp::Not, operand: Box::new(operand) }
            }
            TokenKind::Bang => {
                self.bump();
                let operand = self.parse_unary();
                Expr::Unary { op: UnaryOp::LogNot, operand: Box::new(operand) }
            }
            TokenKind::Minus => {
                self.bump();
                let operand = self.parse_unary();
                Expr::Unary { op: UnaryOp::Neg, operand: Box::new(operand) }
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Expr {
        let expr = match self.current().kind {
            TokenKind::Identifier => {
                let name = self.current().lexeme.clone();
                self.bump();
                if name == "$signed" && self.match_kind(TokenKind::LParen) {
                    let arg = self.parse_expression(0);
                    let _ = self.match_kind(TokenKind::RParen);
                    Expr::Signed(Box::new(arg))
                } else if name == "$clog2" && self.match_kind(TokenKind::LParen) {
                    let arg = self.parse_expression(0);
                    let _ = self.match_kind(TokenKind::RParen);
                    Expr::Clog2(Box::new(arg))
                } else {
                    Expr::Ident(name)
                }
            }
            TokenKind::Number => {
                let lit = self.current().lexeme.clone();
                self.bump();
                Expr::Number(lit)
            }
            TokenKind::LParen => {
                self.bump();
                let expr = self.parse_expression(0);
                let _ = self.match_kind(TokenKind::RParen);
                expr
            }
            TokenKind::LBrace => {
                self.bump();
                // Detect replication: {N{expr}}
                // Pattern: number followed by LBrace
                if self.current().kind == TokenKind::Number
                    && self.pos + 1 < self.tokens.len()
                    && self.tokens[self.pos + 1].kind == TokenKind::LBrace
                {
                    let count_str = self.current().lexeme.clone();
                    let count = count_str.parse::<usize>().unwrap_or(1);
                    self.bump(); // consume the number
                    self.bump(); // consume the inner '{'
                    let inner = self.parse_expression(0);
                    let _ = self.match_kind(TokenKind::RBrace); // inner '}'
                    let _ = self.match_kind(TokenKind::RBrace); // outer '}'
                    let exprs = vec![inner; count];
                    Expr::Concat(exprs)
                } else {
                    let mut exprs = Vec::new();
                    if self.current().kind != TokenKind::RBrace {
                        exprs.push(self.parse_expression(0));
                        while self.match_kind(TokenKind::Comma) {
                            exprs.push(self.parse_expression(0));
                        }
                    }
                    let _ = self.match_kind(TokenKind::RBrace);
                    Expr::Concat(exprs)
                }
            }
            _ => {
                self.bump();
                Expr::Ident("<unsupported>".to_string())
            }
        };
        self.parse_index_suffixes(expr)
    }

    fn parse_index_suffixes(&mut self, mut expr: Expr) -> Expr {
        while self.current().kind == TokenKind::LBracket {
            self.bump();
            let msb = self.parse_expression(0);
            let lsb = if self.match_kind(TokenKind::Colon) {
                Some(Box::new(self.parse_expression(0)))
            } else {
                None
            };
            let _ = self.match_kind(TokenKind::RBracket);
            expr = Expr::Index {
                base: Box::new(expr),
                msb: Box::new(msb),
                lsb,
            };
        }
        expr
    }
}

fn subst_expr_loop_var(e: Expr, loop_var: &str, k: i64) -> Expr {
    match e {
        Expr::Ident(s) if s == loop_var => Expr::Number(format!("{k}")),
        Expr::Ident(s) => Expr::Ident(s),
        Expr::Number(n) => Expr::Number(n),
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: Box::new(subst_expr_loop_var(*left, loop_var, k)),
            right: Box::new(subst_expr_loop_var(*right, loop_var, k)),
        },
        Expr::Unary { op, operand } => Expr::Unary {
            op,
            operand: Box::new(subst_expr_loop_var(*operand, loop_var, k)),
        },
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => Expr::Ternary {
            cond: Box::new(subst_expr_loop_var(*cond, loop_var, k)),
            then_expr: Box::new(subst_expr_loop_var(*then_expr, loop_var, k)),
            else_expr: Box::new(subst_expr_loop_var(*else_expr, loop_var, k)),
        },
        Expr::Concat(exprs) => Expr::Concat(
            exprs
                .into_iter()
                .map(|e| subst_expr_loop_var(e, loop_var, k))
                .collect(),
        ),
        Expr::Index { base, msb, lsb } => Expr::Index {
            base: Box::new(subst_expr_loop_var(*base, loop_var, k)),
            msb: Box::new(subst_expr_loop_var(*msb, loop_var, k)),
            lsb: lsb.map(|x| Box::new(subst_expr_loop_var(*x, loop_var, k))),
        },
        Expr::Clog2(a) => Expr::Clog2(Box::new(subst_expr_loop_var(*a, loop_var, k))),
        Expr::Signed(a) => Expr::Signed(Box::new(subst_expr_loop_var(*a, loop_var, k))),
    }
}

pub(crate) fn subst_port_connections(conns: &[PortConnection], loop_var: &str, k: i64) -> Vec<PortConnection> {
    conns
        .iter()
        .map(|c| PortConnection {
            port_name: c.port_name.clone(),
            expr: subst_expr_loop_var(c.expr.clone(), loop_var, k),
        })
        .collect()
}

fn offset_to_line_col(text: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in text.chars().enumerate() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

use crate::delay_rational::DelayRational;
use crate::lexer::{Token, TokenKind};
use crate::{Diagnostic, Module, ParseResult, Port, Severity, SourceFile};

/// A single parsed file in concrete form. This is intentionally minimal for now:
/// it only records modules so we can grow it later without breaking callers.
#[derive(Debug, Clone)]
pub struct CstFile {
    pub modules: Vec<CstModule>,
}

/// Concrete syntax node for a `module` declaration.
#[derive(Debug, Clone)]
pub struct CstModule {
    pub name: String,
    pub ports: Vec<Port>,
    pub path: String,
    pub items: Vec<CstModuleItem>,
}

/// Module body item.
#[derive(Debug, Clone)]
pub enum CstModuleItem {
    NetDecl {
        kind: NetKind,
        width: usize,
        names: Vec<String>,
    },
    Assign {
        lhs: String,
        expr: Expr,
    },
    Instance {
        module_name: String,
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
}

/// Port connection in a module instance: `.port_name(signal_expr)`.
#[derive(Debug, Clone)]
pub struct PortConnection {
    pub port_name: String,
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

/// Procedural statement inside an always/initial block.
#[derive(Debug, Clone)]
pub enum CstStmt {
    BlockingAssign { lhs: String, rhs: Expr },
    NonBlockingAssign { lhs: String, rhs: Expr },
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

/// Extract bit-width from a `[high:low]` token slice. Returns 1 if no valid
/// range is found.
fn extract_range_width(tokens: &[Token]) -> usize {
    // Look for pattern: `[` number `:` number `]`
    let nums: Vec<i64> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Number)
        .filter_map(|t| t.lexeme.parse::<i64>().ok())
        .collect();
    if nums.len() >= 2 {
        let high = nums[0];
        let low = nums[1];
        ((high - low).abs() + 1) as usize
    } else {
        1
    }
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

        // Optional parameter list: #(parameter ...) — skip it.
        if self.match_kind(TokenKind::Hash) {
            let _ = self.match_kind(TokenKind::LParen);
            while self.current().kind != TokenKind::RParen
                && self.current().kind != TokenKind::Eof
            {
                self.bump();
            }
            let _ = self.match_kind(TokenKind::RParen);
        }

        if self.match_kind(TokenKind::LParen) {
            loop {
                while !matches!(
                    self.current().kind,
                    TokenKind::Input
                        | TokenKind::Output
                        | TokenKind::Inout
                        | TokenKind::Identifier
                        | TokenKind::RParen
                        | TokenKind::Eof
                ) {
                    self.bump();
                }
                if matches!(
                    self.current().kind,
                    TokenKind::RParen | TokenKind::Eof
                ) {
                    break;
                }

                let mut direction = None;
                if matches!(
                    self.current().kind,
                    TokenKind::Input | TokenKind::Output | TokenKind::Inout
                ) {
                    direction = Some(self.current().lexeme.clone());
                    self.bump();
                }

                let (name, width) = {
                    let start_idx = self.pos;
                    while !matches!(
                        self.current().kind,
                        TokenKind::Comma
                            | TokenKind::RParen
                            | TokenKind::Eof
                            | TokenKind::Assign
                            | TokenKind::Module
                            | TokenKind::Endmodule
                    ) {
                        self.bump();
                    }
                    let end_idx = self.pos;
                    let mut found_name: Option<(usize, String)> = None;
                    for idx in (start_idx..end_idx).rev() {
                        if self.tokens[idx].kind == TokenKind::Identifier {
                            found_name =
                                Some((idx + 1, self.tokens[idx].lexeme.clone()));
                            break;
                        }
                    }
                    let w = extract_range_width(&self.tokens[start_idx..end_idx]);
                    if let Some((next_pos, name)) = found_name {
                        self.pos = next_pos;
                        (Some(name), w)
                    } else {
                        (None, w)
                    }
                };

                if let Some(port_name) = name {
                    ports.push(Port {
                        direction,
                        name: port_name,
                        width,
                    });
                } else {
                    self.error_at_current("expected port name");
                    break;
                }

                if !self.match_kind(TokenKind::Comma) {
                    break;
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
            if self.current().kind == TokenKind::Wire
                || self.current().kind == TokenKind::Reg
                || self.current().kind == TokenKind::Integer
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
                self.skip_to_semicolon();
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

    fn parse_net_decl(&mut self) -> Option<CstModuleItem> {
        let kind = match self.current().kind {
            TokenKind::Wire => NetKind::Wire,
            TokenKind::Integer => NetKind::Reg,
            _ => NetKind::Reg,
        };
        self.bump();

        let mut width = if kind == NetKind::Reg && self.tokens[self.pos - 1].lexeme == "integer" {
            32
        } else {
            1
        };

        if self.current().kind == TokenKind::LBracket {
            let range_start = self.pos;
            while self.current().kind != TokenKind::RBracket
                && self.current().kind != TokenKind::Eof
            {
                self.bump();
            }
            let range_end = self.pos;
            let _ = self.match_kind(TokenKind::RBracket);
            width = extract_range_width(&self.tokens[range_start..range_end]);
        }

        let mut names = Vec::new();
        loop {
            if let Some(name) = self.expect_identifier("expected signal name") {
                names.push(name);
            } else {
                break;
            }
            if !self.match_kind(TokenKind::Comma) {
                break;
            }
        }
        self.skip_to_semicolon();
        if names.is_empty() {
            None
        } else {
            Some(CstModuleItem::NetDecl { kind, width, names })
        }
    }

    fn parse_assign(&mut self) -> Option<CstModuleItem> {
        self.bump(); // consume 'assign'
        let lhs = match self.expect_identifier("expected left-hand side of assign") {
            Some(name) => name,
            None => {
                self.skip_to_semicolon();
                return None;
            }
        };
        let _ = self.match_kind(TokenKind::Eq);
        let expr = self.parse_expression(0);
        self.skip_to_semicolon();
        Some(CstModuleItem::Assign { lhs, expr })
    }

    fn parse_instance_like(&mut self) -> Option<CstModuleItem> {
        let module_name = self.current().lexeme.clone();
        self.bump();
        let instance_name =
            match self.expect_identifier("expected instance name after module name") {
                Some(n) => n,
                None => return None,
            };

        // Optional parameter override: #(...)
        if self.match_kind(TokenKind::Hash) {
            let _ = self.match_kind(TokenKind::LParen);
            while self.current().kind != TokenKind::RParen
                && self.current().kind != TokenKind::Eof
            {
                self.bump();
            }
            let _ = self.match_kind(TokenKind::RParen);
        }

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
                        connections.push(PortConnection { port_name, expr });
                    }
                } else {
                    // Positional connection — skip
                    self.bump();
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
                let lhs = self.current().lexeme.clone();
                self.bump();
                if self.current().kind == TokenKind::Le {
                    // Non-blocking assignment: lhs <= rhs
                    self.bump();
                    let rhs = self.parse_expression(0);
                    let _ = self.match_kind(TokenKind::Semicolon);
                    Some(CstStmt::NonBlockingAssign { lhs, rhs })
                } else if self.match_kind(TokenKind::Eq) {
                    // Blocking assignment: lhs = rhs
                    let rhs = self.parse_expression(0);
                    let _ = self.match_kind(TokenKind::Semicolon);
                    Some(CstStmt::BlockingAssign { lhs, rhs })
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
        match self.current().kind {
            TokenKind::Identifier => {
                let name = self.current().lexeme.clone();
                self.bump();
                Expr::Ident(name)
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
        }
    }
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

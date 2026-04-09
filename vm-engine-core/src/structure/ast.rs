//! Structured AST produced by control flow recovery.
//!
//! Two types: [`Stmt`] for statements and [`Expr`] for expressions.
//! Together they represent a readable, nested program — no gotos.

// ============================================================================
// Imports
// ============================================================================

use std::fmt;

use crate::value::Value;
use crate::value::ops::{BinaryOp, UnaryOp};

// ============================================================================
// Stmt
// ============================================================================

/// A structured statement. Produced by recovering control flow from a CFG.
#[derive(Debug, Clone)]
pub enum Stmt {
    /// `name = expr`
    VarAssign { name: String, expr: Expr },
    /// `obj[key] = val`
    PropSet { obj: Expr, key: Expr, val: Expr },
    /// `if (cond) { then } else { else }`
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
    },
    /// `while (cond) { body }`
    While { cond: Expr, body: Vec<Stmt> },
    /// `do { body } while (cond)`
    DoWhile { body: Vec<Stmt>, cond: Expr },
    /// `loop { body }` — infinite loop, exit via break
    Loop { body: Vec<Stmt> },
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `return expr` or `return`
    Return(Option<Expr>),
    /// `throw expr`
    Throw(Expr),
    /// Expression used as a statement (e.g. function call with discarded result).
    ExprStmt(Expr),
    /// Block comment — inserted by recovery for context.
    Comment(String),
}

// ============================================================================
// Expr
// ============================================================================

/// An expression. Composed recursively from sub-expressions.
#[derive(Debug, Clone)]
pub enum Expr {
    /// Named variable reference.
    Var(String),
    /// Literal constant.
    Const(Value),
    /// `left op right`
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `op operand`
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    /// `func(args...)`
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `obj.method(args...)`
    MethodCall {
        obj: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    /// `obj[key]` — property access
    PropAccess {
        obj: Box<Expr>,
        key: Box<Expr>,
    },
    /// `arr[index]` — indexed access
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
    },
    /// `[a, b, c]`
    ArrayLit(Vec<Expr>),
    /// `{ key: val, ... }`
    ObjectLit(Vec<(String, Expr)>),
    /// Unknown / unrecoverable — preserves the IR variable for debugging.
    Unknown(String),
}

impl Expr {
    /// Convenience: wrap a string variable name.
    pub fn var(name: impl Into<String>) -> Self {
        Self::Var(name.into())
    }

    /// Convenience: wrap a constant value.
    pub fn constant(val: Value) -> Self {
        Self::Const(val)
    }

    /// Convenience: binary operation.
    pub fn binary(op: BinaryOp, left: Expr, right: Expr) -> Self {
        Self::Binary { op, left: Box::new(left), right: Box::new(right) }
    }

    /// Convenience: unary operation.
    pub fn unary(op: UnaryOp, operand: Expr) -> Self {
        Self::Unary { op, operand: Box::new(operand) }
    }
}

// ============================================================================
// Display — readable pseudo-JS output
// ============================================================================

impl fmt::Display for Stmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_stmt(f, self, 0)
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_expr(f, self)
    }
}

fn indent(f: &mut fmt::Formatter<'_>, level: usize) -> fmt::Result {
    for _ in 0..level {
        write!(f, "    ")?;
    }
    Ok(())
}

fn write_stmts(f: &mut fmt::Formatter<'_>, stmts: &[Stmt], level: usize) -> fmt::Result {
    for stmt in stmts {
        write_stmt(f, stmt, level)?;
    }
    Ok(())
}

fn write_stmt(f: &mut fmt::Formatter<'_>, stmt: &Stmt, level: usize) -> fmt::Result {
    match stmt {
        Stmt::VarAssign { name, expr } => {
            indent(f, level)?;
            writeln!(f, "{name} = {expr}")
        }
        Stmt::PropSet { obj, key, val } => {
            indent(f, level)?;
            writeln!(f, "{obj}[{key}] = {val}")
        }
        Stmt::If { cond, then_body, else_body } => {
            indent(f, level)?;
            writeln!(f, "if ({cond}) {{")?;
            write_stmts(f, then_body, level + 1)?;
            if let Some(els) = else_body {
                indent(f, level)?;
                writeln!(f, "}} else {{")?;
                write_stmts(f, els, level + 1)?;
            }
            indent(f, level)?;
            writeln!(f, "}}")
        }
        Stmt::While { cond, body } => {
            indent(f, level)?;
            writeln!(f, "while ({cond}) {{")?;
            write_stmts(f, body, level + 1)?;
            indent(f, level)?;
            writeln!(f, "}}")
        }
        Stmt::DoWhile { body, cond } => {
            indent(f, level)?;
            writeln!(f, "do {{")?;
            write_stmts(f, body, level + 1)?;
            indent(f, level)?;
            writeln!(f, "}} while ({cond})")
        }
        Stmt::Loop { body } => {
            indent(f, level)?;
            writeln!(f, "loop {{")?;
            write_stmts(f, body, level + 1)?;
            indent(f, level)?;
            writeln!(f, "}}")
        }
        Stmt::Break => { indent(f, level)?; writeln!(f, "break") }
        Stmt::Continue => { indent(f, level)?; writeln!(f, "continue") }
        Stmt::Return(Some(expr)) => { indent(f, level)?; writeln!(f, "return {expr}") }
        Stmt::Return(None) => { indent(f, level)?; writeln!(f, "return") }
        Stmt::Throw(expr) => { indent(f, level)?; writeln!(f, "throw {expr}") }
        Stmt::ExprStmt(expr) => { indent(f, level)?; writeln!(f, "{expr}") }
        Stmt::Comment(text) => { indent(f, level)?; writeln!(f, "// {text}") }
    }
}

fn write_expr(f: &mut fmt::Formatter<'_>, expr: &Expr) -> fmt::Result {
    match expr {
        Expr::Var(name) => write!(f, "{name}"),
        Expr::Const(val) => match val {
            Value::String(s) => write!(f, "\"{s}\""),
            other => write!(f, "{other}"),
        },
        Expr::Binary { op, left, right } => {
            let need_parens_l = matches!(**left, Expr::Binary { .. });
            let need_parens_r = matches!(**right, Expr::Binary { .. });
            if need_parens_l { write!(f, "(")?; }
            write_expr(f, left)?;
            if need_parens_l { write!(f, ")")?; }
            write!(f, " {op} ")?;
            if need_parens_r { write!(f, "(")?; }
            write_expr(f, right)?;
            if need_parens_r { write!(f, ")")?; }
            Ok(())
        }
        Expr::Unary { op, operand } => {
            write!(f, "{op}")?;
            let need_parens = matches!(**operand, Expr::Binary { .. });
            if need_parens { write!(f, "(")?; }
            write_expr(f, operand)?;
            if need_parens { write!(f, ")")?; }
            Ok(())
        }
        Expr::Call { func, args } => {
            write_expr(f, func)?;
            write!(f, "(")?;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 { write!(f, ", ")?; }
                write_expr(f, arg)?;
            }
            write!(f, ")")
        }
        Expr::MethodCall { obj, method, args } => {
            write_expr(f, obj)?;
            write!(f, ".{method}(")?;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 { write!(f, ", ")?; }
                write_expr(f, arg)?;
            }
            write!(f, ")")
        }
        Expr::PropAccess { obj, key } => {
            write_expr(f, obj)?;
            // Use dot notation for string keys that are identifiers
            if let Expr::Const(Value::String(s)) = key.as_ref()
                && is_identifier(s)
            {
                return write!(f, ".{s}");
            }
            write!(f, "[")?;
            write_expr(f, key)?;
            write!(f, "]")
        }
        Expr::Index { array, index } => {
            write_expr(f, array)?;
            write!(f, "[")?;
            write_expr(f, index)?;
            write!(f, "]")
        }
        Expr::ArrayLit(items) => {
            write!(f, "[")?;
            for (i, item) in items.iter().enumerate() {
                if i > 0 { write!(f, ", ")?; }
                write_expr(f, item)?;
            }
            write!(f, "]")
        }
        Expr::ObjectLit(pairs) => {
            write!(f, "{{")?;
            for (i, (key, val)) in pairs.iter().enumerate() {
                if i > 0 { write!(f, ", ")?; }
                write!(f, "{key}: ")?;
                write_expr(f, val)?;
            }
            write!(f, "}}")
        }
        Expr::Unknown(desc) => write!(f, "/* {desc} */"),
    }
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_var_assign() {
        let stmt = Stmt::VarAssign {
            name: "x".into(),
            expr: Expr::binary(BinaryOp::Add, Expr::var("a"), Expr::var("b")),
        };
        assert_eq!(stmt.to_string(), "x = a + b\n");
    }

    #[test]
    fn display_prop_set() {
        let stmt = Stmt::PropSet {
            obj: Expr::var("sbox"),
            key: Expr::var("i"),
            val: Expr::binary(BinaryOp::BitXor, Expr::var("v"), Expr::var("key")),
        };
        assert_eq!(stmt.to_string(), "sbox[i] = v ^ key\n");
    }

    #[test]
    fn display_while_loop() {
        let stmt = Stmt::While {
            cond: Expr::binary(BinaryOp::Lt, Expr::var("i"), Expr::constant(Value::number(256.0))),
            body: vec![
                Stmt::VarAssign {
                    name: "i".into(),
                    expr: Expr::binary(BinaryOp::Add, Expr::var("i"), Expr::constant(Value::number(1.0))),
                },
            ],
        };
        let text = stmt.to_string();
        assert!(text.contains("while (i < 256)"));
        assert!(text.contains("    i = i + 1"));
    }

    #[test]
    fn display_if_else() {
        let stmt = Stmt::If {
            cond: Expr::var("flag"),
            then_body: vec![Stmt::Return(Some(Expr::constant(Value::number(1.0))))],
            else_body: Some(vec![Stmt::Return(Some(Expr::constant(Value::number(0.0))))]),
        };
        let text = stmt.to_string();
        assert!(text.contains("if (flag)"));
        assert!(text.contains("return 1"));
        assert!(text.contains("} else {"));
        assert!(text.contains("return 0"));
    }

    #[test]
    fn display_nested_exprs_with_parens() {
        let expr = Expr::binary(
            BinaryOp::BitXor,
            Expr::binary(BinaryOp::BitAnd, Expr::var("x"), Expr::constant(Value::number(255.0))),
            Expr::var("key"),
        );
        assert_eq!(expr.to_string(), "(x & 255) ^ key");
    }

    #[test]
    fn display_method_call() {
        let expr = Expr::MethodCall {
            obj: Box::new(Expr::var("arr")),
            method: "push".into(),
            args: vec![Expr::constant(Value::number(42.0))],
        };
        assert_eq!(expr.to_string(), "arr.push(42)");
    }

    #[test]
    fn display_prop_access_dot_notation() {
        let expr = Expr::PropAccess {
            obj: Box::new(Expr::var("navigator")),
            key: Box::new(Expr::constant(Value::string("userAgent"))),
        };
        assert_eq!(expr.to_string(), "navigator.userAgent");
    }

    #[test]
    fn display_prop_access_bracket_notation() {
        let expr = Expr::PropAccess {
            obj: Box::new(Expr::var("obj")),
            key: Box::new(Expr::var("key")),
        };
        assert_eq!(expr.to_string(), "obj[key]");
    }

    #[test]
    fn display_comment() {
        let stmt = Stmt::Comment("loop header".into());
        assert_eq!(stmt.to_string(), "// loop header\n");
    }
}

//! AST simplification passes.
//!
//! Constant folding, identity removal, dead branch elimination.
//! Applied after structure recovery to clean up the output.

// ============================================================================
// Imports
// ============================================================================

use crate::value::Value;
use crate::value::ops::{self, BinaryOp, UnaryOp};

use super::ast::{Expr, Stmt};

// ============================================================================
// Simplify
// ============================================================================

/// Apply all simplification passes to a statement list.
pub fn simplify(stmts: &mut Vec<Stmt>) {
    for stmt in stmts.iter_mut() {
        simplify_stmt(stmt);
    }
    // Remove empty comments and no-op statements
    stmts.retain(|s| !is_noop(s));
}

fn simplify_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::VarAssign { expr, .. } => {
            take_and_simplify(expr);
        }
        Stmt::PropSet { obj, key, val } => {
            take_and_simplify(obj);
            take_and_simplify(key);
            take_and_simplify(val);
        }
        Stmt::If { cond, then_body, else_body } => {
            take_and_simplify(cond);
            simplify(then_body);
            if let Some(els) = else_body {
                simplify(els);
                if els.is_empty() {
                    *else_body = None;
                }
            }
        }
        Stmt::While { cond, body } => {
            take_and_simplify(cond);
            simplify(body);
        }
        Stmt::DoWhile { body, cond } => {
            simplify(body);
            take_and_simplify(cond);
        }
        Stmt::Loop { body } => {
            simplify(body);
        }
        Stmt::Return(Some(expr)) => {
            take_and_simplify(expr);
        }
        Stmt::Throw(expr) => {
            take_and_simplify(expr);
        }
        Stmt::ExprStmt(expr) => {
            take_and_simplify(expr);
        }
        _ => {}
    }
}

/// Take ownership of an expression, simplify it, and put it back — zero clones.
fn take_and_simplify(expr: &mut Expr) {
    let taken = std::mem::replace(expr, Expr::Unknown(String::new()));
    *expr = simplify_expr(taken);
}

/// Simplify an expression. Returns a new (possibly simpler) expression.
pub fn simplify_expr(expr: Expr) -> Expr {
    match expr {
        Expr::Binary { op, left, right } => {
            let left = simplify_expr(*left);
            let right = simplify_expr(*right);

            // Constant folding: both sides are constants
            if let (Expr::Const(lv), Expr::Const(rv)) = (&left, &right) {
                let result = ops::binary(op, lv, rv);
                return Expr::Const(result);
            }

            // Identity: x + 0 = x, x * 1 = x, x ^ 0 = x, x | 0 = x
            if let Expr::Const(Value::Number(n)) = &right {
                match op {
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::BitOr
                    | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr
                    | BinaryOp::UShr if *n == 0.0 => return left,
                    BinaryOp::Mul | BinaryOp::Div if *n == 1.0 => return left,
                    _ => {}
                }
            }
            if let Expr::Const(Value::Number(n)) = &left {
                match op {
                    BinaryOp::Add | BinaryOp::BitOr | BinaryOp::BitXor
                        if *n == 0.0 => return right,
                    BinaryOp::Mul if *n == 1.0 => return right,
                    _ => {}
                }
            }

            Expr::Binary { op, left: Box::new(left), right: Box::new(right) }
        }
        Expr::Unary { op, operand } => {
            let operand = simplify_expr(*operand);

            // Constant folding
            if let Expr::Const(val) = &operand {
                let result = ops::unary(op, val);
                return Expr::Const(result);
            }

            // Double negation: !!x = x (for boolean context)
            if op == UnaryOp::LogicalNot
                && let Expr::Unary { op: UnaryOp::LogicalNot, .. } = &operand
            {
                // Destructure to take ownership without cloning
                if let Expr::Unary { operand: inner, .. } = operand {
                    return *inner;
                }
            }

            Expr::Unary { op, operand: Box::new(operand) }
        }
        Expr::Call { func, args } => {
            let func = simplify_expr(*func);
            let args = args.into_iter().map(simplify_expr).collect();
            Expr::Call { func: Box::new(func), args }
        }
        Expr::MethodCall { obj, method, args } => {
            let obj = simplify_expr(*obj);
            let args = args.into_iter().map(simplify_expr).collect();
            Expr::MethodCall { obj: Box::new(obj), method, args }
        }
        Expr::PropAccess { obj, key } => {
            let obj = simplify_expr(*obj);
            let key = simplify_expr(*key);
            Expr::PropAccess { obj: Box::new(obj), key: Box::new(key) }
        }
        Expr::Index { array, index } => {
            let array = simplify_expr(*array);
            let index = simplify_expr(*index);
            Expr::Index { array: Box::new(array), index: Box::new(index) }
        }
        other => other,
    }
}

fn is_noop(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Comment(s) if s.is_empty())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_constant_addition() {
        let expr = Expr::binary(
            BinaryOp::Add,
            Expr::Const(Value::number(10.0)),
            Expr::Const(Value::number(20.0)),
        );
        let result = simplify_expr(expr);
        assert!(matches!(result, Expr::Const(Value::Number(n)) if n == 30.0));
    }

    #[test]
    fn fold_constant_bitxor() {
        // 185 ^ 171 = 18 (Cloudflare dispatch example)
        let expr = Expr::binary(
            BinaryOp::BitXor,
            Expr::Const(Value::number(185.0)),
            Expr::Const(Value::number(171.0)),
        );
        let result = simplify_expr(expr);
        assert!(matches!(result, Expr::Const(Value::Number(n)) if n == 18.0));
    }

    #[test]
    fn identity_add_zero() {
        let expr = Expr::binary(BinaryOp::Add, Expr::var("x"), Expr::Const(Value::number(0.0)));
        let result = simplify_expr(expr);
        assert!(matches!(result, Expr::Var(name) if name == "x"));
    }

    #[test]
    fn identity_mul_one() {
        let expr = Expr::binary(BinaryOp::Mul, Expr::var("x"), Expr::Const(Value::number(1.0)));
        let result = simplify_expr(expr);
        assert!(matches!(result, Expr::Var(name) if name == "x"));
    }

    #[test]
    fn identity_xor_zero() {
        let expr = Expr::binary(BinaryOp::BitXor, Expr::var("x"), Expr::Const(Value::number(0.0)));
        let result = simplify_expr(expr);
        assert!(matches!(result, Expr::Var(name) if name == "x"));
    }

    #[test]
    fn double_negation_removed() {
        let expr = Expr::unary(
            UnaryOp::LogicalNot,
            Expr::unary(UnaryOp::LogicalNot, Expr::var("flag")),
        );
        let result = simplify_expr(expr);
        assert!(matches!(result, Expr::Var(name) if name == "flag"));
    }

    #[test]
    fn fold_nested_constants() {
        // (10 + 20) * 3 = 90
        let expr = Expr::binary(
            BinaryOp::Mul,
            Expr::binary(BinaryOp::Add, Expr::Const(Value::number(10.0)), Expr::Const(Value::number(20.0))),
            Expr::Const(Value::number(3.0)),
        );
        let result = simplify_expr(expr);
        assert!(matches!(result, Expr::Const(Value::Number(n)) if n == 90.0));
    }

    #[test]
    fn simplify_stmt_list() {
        let mut stmts = vec![
            Stmt::VarAssign {
                name: "x".into(),
                expr: Expr::binary(BinaryOp::Add, Expr::var("y"), Expr::Const(Value::number(0.0))),
            },
        ];
        simplify(&mut stmts);

        // x = y + 0 → x = y
        let text = stmts[0].to_string();
        assert!(text.contains("x = y"), "got: {text}");
    }
}

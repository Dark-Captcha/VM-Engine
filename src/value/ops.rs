//! Binary and unary operators with ECMAScript semantics.
//!
//! One match per category. Not one file per operator.

// ============================================================================
// Imports
// ============================================================================

use super::Value;
use super::coerce;

// ============================================================================
// Operator enums
// ============================================================================

/// Binary operators. 22 variants covering arithmetic, bitwise, and comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    // Arithmetic
    Add, Sub, Mul, Div, Mod, Pow,
    // Bitwise
    BitAnd, BitOr, BitXor, Shl, Shr, UShr,
    // Comparison
    Eq, Neq, StrictEq, StrictNeq,
    Lt, Lte, Gt, Gte,
    // Relational
    In, InstanceOf,
}

/// Unary operators. 6 variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Neg, Pos, LogicalNot, BitNot, TypeOf, Void,
}

// ============================================================================
// Evaluation
// ============================================================================

/// Evaluate a binary operation on two values.
pub fn binary(op: BinaryOp, left: &Value, right: &Value) -> Value {
    match op {
        BinaryOp::Add => js_add(left, right),
        BinaryOp::Sub => float_op(left, right, |a, b| a - b),
        BinaryOp::Mul => float_op(left, right, |a, b| a * b),
        BinaryOp::Div => float_op(left, right, |a, b| a / b),
        BinaryOp::Mod => float_op(left, right, |a, b| a % b),
        BinaryOp::Pow => float_op(left, right, f64::powf),
        BinaryOp::BitAnd => int32_op(left, right, |a, b| a & b),
        BinaryOp::BitOr => int32_op(left, right, |a, b| a | b),
        BinaryOp::BitXor => int32_op(left, right, |a, b| a ^ b),
        BinaryOp::Shl => int32_op(left, right, |a, b| a << (b & 31)),
        BinaryOp::Shr => int32_op(left, right, |a, b| a >> (b & 31)),
        BinaryOp::UShr => {
            let a = coerce::to_uint32(left);
            let b = coerce::to_uint32(right);
            Value::number((a >> (b & 31)) as f64)
        }
        BinaryOp::StrictEq => Value::bool(strict_eq(left, right)),
        BinaryOp::StrictNeq => Value::bool(!strict_eq(left, right)),
        BinaryOp::Eq => Value::bool(abstract_eq(left, right)),
        BinaryOp::Neq => Value::bool(!abstract_eq(left, right)),
        BinaryOp::Lt => Value::bool(abstract_rel(left, right) == Some(true)),
        BinaryOp::Gt => Value::bool(abstract_rel(right, left) == Some(true)),
        BinaryOp::Lte => Value::bool(abstract_rel(right, left) == Some(false)),
        BinaryOp::Gte => Value::bool(abstract_rel(left, right) == Some(false)),
        BinaryOp::In | BinaryOp::InstanceOf => Value::bool(false),
    }
}

/// Evaluate a unary operation on a value.
pub fn unary(op: UnaryOp, val: &Value) -> Value {
    match op {
        UnaryOp::LogicalNot => Value::bool(!coerce::to_boolean(val)),
        UnaryOp::BitNot => Value::number((!coerce::to_int32(val)) as f64),
        UnaryOp::Neg => Value::number(-coerce::to_number(val)),
        UnaryOp::Pos => Value::number(coerce::to_number(val)),
        UnaryOp::TypeOf => Value::string(val.type_name()),
        UnaryOp::Void => Value::Undefined,
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// JS `+` operator: string concat if either side is a string, numeric add otherwise.
fn js_add(left: &Value, right: &Value) -> Value {
    if matches!(left, Value::String(_)) || matches!(right, Value::String(_)) {
        return Value::string(coerce::to_string(left) + &coerce::to_string(right));
    }
    Value::number(coerce::to_number(left) + coerce::to_number(right))
}

fn float_op(left: &Value, right: &Value, operation: fn(f64, f64) -> f64) -> Value {
    Value::number(operation(coerce::to_number(left), coerce::to_number(right)))
}

fn int32_op(left: &Value, right: &Value, operation: fn(i32, i32) -> i32) -> Value {
    Value::number(operation(coerce::to_int32(left), coerce::to_int32(right)) as f64)
}

fn strict_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
        (Value::Object(a), Value::Object(b)) => a == b,
        (Value::Closure(a), Value::Closure(b)) => a == b,
        _ => false,
    }
}

fn abstract_eq(a: &Value, b: &Value) -> bool {
    if std::mem::discriminant(a) == std::mem::discriminant(b) {
        return strict_eq(a, b);
    }
    match (a, b) {
        (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null) => true,
        (Value::Number(_), Value::String(_)) => {
            abstract_eq(a, &Value::number(coerce::to_number(b)))
        }
        (Value::String(_), Value::Number(_)) => {
            abstract_eq(&Value::number(coerce::to_number(a)), b)
        }
        (Value::Bool(_), _) => abstract_eq(&Value::number(coerce::to_number(a)), b),
        (_, Value::Bool(_)) => abstract_eq(a, &Value::number(coerce::to_number(b))),
        // ECMAScript: ToPrimitive for arrays → toString then compare
        (Value::Array(_), Value::Number(_) | Value::String(_)) => {
            abstract_eq(&Value::string(coerce::to_string(a)), b)
        }
        (Value::Number(_) | Value::String(_), Value::Array(_)) => {
            abstract_eq(a, &Value::string(coerce::to_string(b)))
        }
        // Object comparison with primitives
        (Value::Object(_), Value::Number(_) | Value::String(_)) => {
            // Objects coerce to "[object Object]" which rarely equals primitives
            abstract_eq(&Value::string(coerce::to_string(a)), b)
        }
        (Value::Number(_) | Value::String(_), Value::Object(_)) => {
            abstract_eq(a, &Value::string(coerce::to_string(b)))
        }
        _ => false,
    }
}

fn abstract_rel(left: &Value, right: &Value) -> Option<bool> {
    if let (Value::String(a), Value::String(b)) = (left, right) {
        return Some(a < b);
    }
    let (ln, rn) = (coerce::to_number(left), coerce::to_number(right));
    if ln.is_nan() || rn.is_nan() {
        return None;
    }
    Some(ln < rn)
}

// ============================================================================
// Display
// ============================================================================

impl std::fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Add => "+", Self::Sub => "-", Self::Mul => "*",
            Self::Div => "/", Self::Mod => "%", Self::Pow => "**",
            Self::BitAnd => "&", Self::BitOr => "|", Self::BitXor => "^",
            Self::Shl => "<<", Self::Shr => ">>", Self::UShr => ">>>",
            Self::Eq => "==", Self::Neq => "!=",
            Self::StrictEq => "===", Self::StrictNeq => "!==",
            Self::Lt => "<", Self::Lte => "<=",
            Self::Gt => ">", Self::Gte => ">=",
            Self::In => "in", Self::InstanceOf => "instanceof",
        };
        write!(f, "{s}")
    }
}

impl std::fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Neg => "-", Self::Pos => "+",
            Self::LogicalNot => "!", Self::BitNot => "~",
            Self::TypeOf => "typeof ", Self::Void => "void ",
        };
        write!(f, "{s}")
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_numbers() {
        let r = binary(BinaryOp::Add, &Value::number(10.0), &Value::number(20.0));
        assert_eq!(r, Value::number(30.0));
    }

    #[test]
    fn add_string_concat() {
        let r = binary(BinaryOp::Add, &Value::string("3"), &Value::number(4.0));
        assert_eq!(r, Value::string("34"));
    }

    #[test]
    fn bitxor_cloudflare_dispatch() {
        // From Cloudflare walkthrough: 185 ^ 171 = 18
        let r = binary(BinaryOp::BitXor, &Value::number(185.0), &Value::number(171.0));
        assert_eq!(r, Value::number(18.0));
    }

    #[test]
    fn ushr_minus_one() {
        // -1 >>> 0 = 4294967295
        let r = binary(BinaryOp::UShr, &Value::number(-1.0), &Value::number(0.0));
        assert_eq!(r, Value::number(4_294_967_295.0));
    }

    #[test]
    fn null_eq_undefined() {
        assert_eq!(
            binary(BinaryOp::Eq, &Value::Null, &Value::Undefined),
            Value::bool(true)
        );
        assert_eq!(
            binary(BinaryOp::StrictEq, &Value::Null, &Value::Undefined),
            Value::bool(false)
        );
    }

    #[test]
    fn logical_not_zero_is_true() {
        assert_eq!(unary(UnaryOp::LogicalNot, &Value::number(0.0)), Value::bool(true));
    }

    #[test]
    fn bitnot_five_is_minus_six() {
        assert_eq!(unary(UnaryOp::BitNot, &Value::number(5.0)), Value::number(-6.0));
    }

    #[test]
    fn typeof_returns_correct_strings() {
        assert_eq!(unary(UnaryOp::TypeOf, &Value::number(0.0)), Value::string("number"));
        assert_eq!(unary(UnaryOp::TypeOf, &Value::Null), Value::string("object"));
    }

    #[test]
    fn void_returns_undefined() {
        assert_eq!(unary(UnaryOp::Void, &Value::number(42.0)), Value::Undefined);
    }

    #[test]
    fn comparison_operators() {
        assert_eq!(binary(BinaryOp::Lt, &Value::number(1.0), &Value::number(2.0)), Value::bool(true));
        assert_eq!(binary(BinaryOp::Gte, &Value::number(2.0), &Value::number(2.0)), Value::bool(true));
        assert_eq!(binary(BinaryOp::Gt, &Value::string("b"), &Value::string("a")), Value::bool(true));
    }

    #[test]
    fn shift_operators() {
        assert_eq!(binary(BinaryOp::Shl, &Value::number(1.0), &Value::number(8.0)), Value::number(256.0));
        assert_eq!(binary(BinaryOp::Shr, &Value::number(-256.0), &Value::number(4.0)), Value::number(-16.0));
    }

    #[test]
    fn array_loose_equality() {
        // [] == 0 ([] → "" → 0)
        assert_eq!(
            binary(BinaryOp::Eq, &Value::Array(vec![]), &Value::number(0.0)),
            Value::bool(true)
        );
        // [0] == 0
        assert_eq!(
            binary(BinaryOp::Eq, &Value::Array(vec![Value::number(0.0)]), &Value::number(0.0)),
            Value::bool(true)
        );
        // [] == ""
        assert_eq!(
            binary(BinaryOp::Eq, &Value::Array(vec![]), &Value::string("")),
            Value::bool(true)
        );
        // [1,2] == "1,2"
        assert_eq!(
            binary(BinaryOp::Eq,
                &Value::Array(vec![Value::number(1.0), Value::number(2.0)]),
                &Value::string("1,2")
            ),
            Value::bool(true)
        );
    }

    #[test]
    fn array_strict_equality() {
        // [] !== 0 (strict equality, no coercion)
        assert_eq!(
            binary(BinaryOp::StrictEq, &Value::Array(vec![]), &Value::number(0.0)),
            Value::bool(false)
        );
    }
}

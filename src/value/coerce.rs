//! ECMAScript type coercion rules.
//!
//! Each function implements the corresponding ECMAScript specification section.
//! These are used by the operator evaluation in `ops.rs` and by the IR executor.

// ============================================================================
// Imports
// ============================================================================

use super::Value;

// ============================================================================
// Coercion functions
// ============================================================================

/// ECMAScript 7.1.2 — ToBoolean.
pub fn to_boolean(val: &Value) -> bool {
    match val {
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Null | Value::Undefined => false,
        Value::Object(_) | Value::Array(_) | Value::Bytes(_) | Value::Closure(_) => true,
    }
}

/// ECMAScript 7.1.3 — ToNumber.
pub fn to_number(val: &Value) -> f64 {
    match val {
        Value::Number(n) => *n,
        Value::Bool(true) => 1.0,
        Value::Bool(false) | Value::Null => 0.0,
        Value::Undefined => f64::NAN,
        Value::String(s) => string_to_number(s),
        // ECMAScript: ToPrimitive(array) → array.toString() → ToNumber(string)
        Value::Array(arr) => {
            let s = array_to_string(arr);
            string_to_number(&s)
        }
        Value::Object(_) | Value::Bytes(_) | Value::Closure(_) => f64::NAN,
    }
}

/// ECMAScript 7.1.5 — ToInt32.
pub fn to_int32(val: &Value) -> i32 {
    let n = to_number(val);
    if n.is_nan() || n.is_infinite() || n == 0.0 {
        return 0;
    }
    let int = n.trunc() as i64;
    let modulo = 1i64 << 32;
    let wrapped = ((int % modulo) + modulo) % modulo;
    if wrapped >= (1i64 << 31) {
        (wrapped - modulo) as i32
    } else {
        wrapped as i32
    }
}

/// ECMAScript 7.1.6 — ToUint32.
pub fn to_uint32(val: &Value) -> u32 {
    to_int32(val) as u32
}

/// ECMAScript 7.1.12 — ToString.
pub fn to_string(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => number_to_string(*n),
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) => "[object Object]".into(),
        // ECMAScript: Array.prototype.toString() joins elements with comma
        Value::Array(arr) => array_to_string(arr),
        Value::Bytes(_) => "[object Uint8Array]".into(),
        Value::Closure(_) => "function() { [native code] }".into(),
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// ECMAScript Array.prototype.toString() — join elements with comma.
fn array_to_string(arr: &[super::Value]) -> String {
    arr.iter()
        .map(|v| match v {
            super::Value::Null | super::Value::Undefined => String::new(),
            other => to_string(other),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn string_to_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).map(|v| v as f64).unwrap_or(f64::NAN);
    }
    if let Some(oct) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")) {
        return i64::from_str_radix(oct, 8).map(|v| v as f64).unwrap_or(f64::NAN);
    }
    if let Some(bin) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        return i64::from_str_radix(bin, 2).map(|v| v as f64).unwrap_or(f64::NAN);
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if n == 0.0 {
        return "0".into();
    }
    if n.fract() == 0.0 && n.abs() < 1e20 {
        return format!("{}", n as i64);
    }
    format!("{n}")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_falsy_values() {
        assert!(!to_boolean(&Value::Bool(false)));
        assert!(!to_boolean(&Value::number(0.0)));
        assert!(!to_boolean(&Value::number(-0.0)));
        assert!(!to_boolean(&Value::number(f64::NAN)));
        assert!(!to_boolean(&Value::string("")));
        assert!(!to_boolean(&Value::Null));
        assert!(!to_boolean(&Value::Undefined));
    }

    #[test]
    fn boolean_truthy_values() {
        assert!(to_boolean(&Value::string("0")));
        assert!(to_boolean(&Value::string("false")));
        assert!(to_boolean(&Value::number(1.0)));
        assert!(to_boolean(&Value::Array(vec![])));
        assert!(to_boolean(&Value::Bytes(vec![])));
    }

    #[test]
    fn number_from_strings() {
        assert_eq!(to_number(&Value::string("42")), 42.0);
        assert_eq!(to_number(&Value::string("  3.14  ")), 3.14);
        assert_eq!(to_number(&Value::string("0xFF")), 255.0);
        assert_eq!(to_number(&Value::string("0b1010")), 10.0);
        assert_eq!(to_number(&Value::string("0o17")), 15.0);
        assert_eq!(to_number(&Value::string("")), 0.0);
        assert!(to_number(&Value::string("abc")).is_nan());
        assert!(to_number(&Value::Undefined).is_nan());
        assert_eq!(to_number(&Value::Null), 0.0);
    }

    #[test]
    fn int32_wrapping() {
        assert_eq!(to_int32(&Value::number(2_147_483_648.0)), -2_147_483_648);
        assert_eq!(to_int32(&Value::number(0.0)), 0);
        assert_eq!(to_int32(&Value::number(f64::NAN)), 0);
        assert_eq!(to_int32(&Value::number(f64::INFINITY)), 0);
    }

    #[test]
    fn uint32_from_negative() {
        assert_eq!(to_uint32(&Value::number(-1.0)), 4_294_967_295);
    }

    #[test]
    fn string_from_values() {
        assert_eq!(to_string(&Value::number(-0.0)), "0");
        assert_eq!(to_string(&Value::number(f64::NAN)), "NaN");
        assert_eq!(to_string(&Value::Null), "null");
        assert_eq!(to_string(&Value::Undefined), "undefined");
        assert_eq!(to_string(&Value::Bool(true)), "true");
    }

    #[test]
    fn array_to_string_joins_elements() {
        // Empty array → ""
        assert_eq!(to_string(&Value::Array(vec![])), "");
        // Single element → that element's string
        assert_eq!(to_string(&Value::Array(vec![Value::number(42.0)])), "42");
        // Multiple elements → comma-joined
        assert_eq!(
            to_string(&Value::Array(vec![
                Value::number(1.0),
                Value::number(2.0),
                Value::number(3.0),
            ])),
            "1,2,3"
        );
        // Null/undefined elements → empty string
        assert_eq!(
            to_string(&Value::Array(vec![
                Value::number(1.0),
                Value::Null,
                Value::number(3.0),
            ])),
            "1,,3"
        );
    }

    #[test]
    fn array_to_number_via_tostring() {
        // Empty array → "" → 0
        assert_eq!(to_number(&Value::Array(vec![])), 0.0);
        // Single numeric element → that number
        assert_eq!(to_number(&Value::Array(vec![Value::number(42.0)])), 42.0);
        // Multiple elements → NaN (comma-separated string is not a number)
        assert!(to_number(&Value::Array(vec![
            Value::number(1.0),
            Value::number(2.0),
        ])).is_nan());
    }
}

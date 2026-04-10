//! JavaScript-compatible value types for VM execution and analysis.
//!
//! Every anti-bot VM targets JavaScript, so every runtime value is one of these
//! types and every operator follows ECMAScript semantics.

pub mod coerce;
pub mod ops;

// ============================================================================
// Imports
// ============================================================================

use std::fmt;

// ============================================================================
// IDs
// ============================================================================

/// Handle to a heap-allocated object in the executor.
///
/// Created by [`crate::exec::heap::Heap::alloc`] — not constructible by users.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectId(pub(crate) u32);

impl ObjectId {
    /// Raw numeric index (for serialization/debugging).
    pub fn index(self) -> u32 { self.0 }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "obj#{}", self.0)
    }
}

/// Handle to a closure stored in the executor runtime.
///
/// Created internally — not constructible by users.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClosureId(pub(crate) u32);

impl ClosureId {
    /// Raw numeric index (for serialization/debugging).
    pub fn index(self) -> u32 { self.0 }
}

impl fmt::Display for ClosureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "closure#{}", self.0)
    }
}

// ============================================================================
// Value
// ============================================================================

/// A runtime value. Every VM operation produces and consumes these.
///
/// Variants cover all types encountered across the 6 known anti-bot VMs:
/// stack-based (FunCaptcha, Shape F5, DataDome), register-based (Cloudflare,
/// Kasada), and combinator (Incapsula).
#[derive(Debug, Clone, Default)]
pub enum Value {
    /// IEEE 754 f64 — all JS numbers.
    Number(f64),
    /// UTF-8 string.
    String(String),
    /// Boolean.
    Bool(bool),
    /// JS null.
    Null,
    /// JS undefined (default).
    #[default]
    Undefined,
    /// Reference to a heap object with properties.
    Object(ObjectId),
    /// Indexed collection. S-boxes (256 entries), Kasada E[] (185k elements).
    Array(Vec<Value>),
    /// Raw binary data. Cipher buffers, bytecode chunks.
    Bytes(Vec<u8>),
    /// First-class callable. Incapsula combinators are functions that return functions.
    Closure(ClosureId),
}

impl Value {
    #[inline]
    pub fn number(n: f64) -> Self {
        Self::Number(n)
    }

    #[inline]
    pub fn string(s: impl Into<String>) -> Self {
        Self::String(s.into())
    }

    #[inline]
    pub fn bool(b: bool) -> Self {
        Self::Bool(b)
    }

    #[inline]
    pub fn as_number(&self) -> Option<f64> {
        if let Self::Number(n) = self { Some(*n) } else { None }
    }

    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(s) = self { Some(s) } else { None }
    }

    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(b) = self { Some(*b) } else { None }
    }

    #[inline]
    pub fn as_object(&self) -> Option<ObjectId> {
        if let Self::Object(id) = self { Some(*id) } else { None }
    }

    #[inline]
    pub fn as_array(&self) -> Option<&[Value]> {
        if let Self::Array(a) = self { Some(a) } else { None }
    }

    #[inline]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        if let Self::Bytes(b) = self { Some(b) } else { None }
    }

    #[inline]
    pub fn as_closure(&self) -> Option<ClosureId> {
        if let Self::Closure(id) = self { Some(*id) } else { None }
    }

    #[inline]
    pub fn is_nullish(&self) -> bool {
        matches!(self, Self::Null | Self::Undefined)
    }

    /// ECMAScript typeof result.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::Bool(_) => "boolean",
            Self::Null => "object", // typeof null === "object" per spec
            Self::Undefined => "undefined",
            Self::Object(_) => "object",
            Self::Array(_) => "object",
            Self::Bytes(_) => "object",
            Self::Closure(_) => "function",
        }
    }
}

// ============================================================================
// PartialEq — IEEE 754 semantics
// ============================================================================

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a == b, // NaN != NaN, -0 == 0
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Null, Self::Null) | (Self::Undefined, Self::Undefined) => true,
            (Self::Object(a), Self::Object(b)) => a == b,
            (Self::Closure(a), Self::Closure(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => std::ptr::eq(a.as_ptr(), b.as_ptr()),
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            _ => false,
        }
    }
}

// ============================================================================
// Display
// ============================================================================

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{}", format_number(*n)),
            Self::String(s) => write!(f, "{s}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Null => write!(f, "null"),
            Self::Undefined => write!(f, "undefined"),
            Self::Object(id) => write!(f, "[object {id}]"),
            Self::Array(a) => write!(f, "[Array({})]", a.len()),
            Self::Bytes(b) => write!(f, "[Bytes({})]", b.len()),
            Self::Closure(id) => write!(f, "[closure {id}]"),
        }
    }
}

/// Format a number following ECMAScript conventions.
fn format_number(n: f64) -> String {
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
    fn default_is_undefined() {
        assert_eq!(Value::default(), Value::Undefined);
    }

    #[test]
    fn display_numbers() {
        assert_eq!(Value::number(42.0).to_string(), "42");
        assert_eq!(Value::number(-0.0).to_string(), "0");
        assert_eq!(Value::number(3.14).to_string(), "3.14");
        assert_eq!(Value::number(f64::NAN).to_string(), "NaN");
        assert_eq!(Value::number(f64::INFINITY).to_string(), "Infinity");
        assert_eq!(Value::number(f64::NEG_INFINITY).to_string(), "-Infinity");
    }

    #[test]
    fn nan_not_equal_to_itself() {
        assert_ne!(Value::number(f64::NAN), Value::number(f64::NAN));
    }

    #[test]
    fn neg_zero_equals_pos_zero() {
        assert_eq!(Value::number(-0.0), Value::number(0.0));
    }

    #[test]
    fn typeof_matches_ecmascript() {
        assert_eq!(Value::Null.type_name(), "object");
        assert_eq!(Value::Undefined.type_name(), "undefined");
        assert_eq!(Value::number(0.0).type_name(), "number");
        assert_eq!(Value::string("").type_name(), "string");
        assert_eq!(Value::bool(true).type_name(), "boolean");
        assert_eq!(Value::Closure(ClosureId(0)).type_name(), "function");
        assert_eq!(Value::Array(vec![]).type_name(), "object");
    }

    #[test]
    fn nullish_check() {
        assert!(Value::Null.is_nullish());
        assert!(Value::Undefined.is_nullish());
        assert!(!Value::number(0.0).is_nullish());
        assert!(!Value::bool(false).is_nullish());
    }

    #[test]
    fn array_display_shows_length() {
        let arr = Value::Array(vec![Value::number(1.0), Value::number(2.0)]);
        assert_eq!(arr.to_string(), "[Array(2)]");
    }

    #[test]
    fn bytes_display_shows_length() {
        let b = Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(b.to_string(), "[Bytes(4)]");
    }
}

//! Global object setup: window, globalThis, self, and global constants.

// ============================================================================
// Imports
// ============================================================================

use vm_engine_core::exec::heap::Heap;
use vm_engine_core::value::{ObjectId, Value};

// ============================================================================
// Install
// ============================================================================

/// Install global object references and constants.
///
/// Sets `window`, `globalThis`, and `self` to point to the global object.
/// Sets `undefined`, `NaN`, `Infinity`.
pub fn install_globals(heap: &mut Heap, global: ObjectId) {
    // Self-references
    heap.set_property(global, "window", Value::Object(global));
    heap.set_property(global, "globalThis", Value::Object(global));
    heap.set_property(global, "self", Value::Object(global));

    // Global constants
    heap.set_property(global, "undefined", Value::Undefined);
    heap.set_property(global, "NaN", Value::number(f64::NAN));
    heap.set_property(global, "Infinity", Value::number(f64::INFINITY));

    // Global functions
    let is_nan = heap.alloc_native(|args, _heap| {
        let number = args.first().map(vm_engine_core::value::coerce::to_number).unwrap_or(f64::NAN);
        Value::bool(number.is_nan())
    });
    heap.set_property(global, "isNaN", Value::Object(is_nan));

    let is_finite = heap.alloc_native(|args, _heap| {
        let number = args.first().map(vm_engine_core::value::coerce::to_number).unwrap_or(f64::NAN);
        Value::bool(number.is_finite())
    });
    heap.set_property(global, "isFinite", Value::Object(is_finite));

    let parse_int = heap.alloc_native(|args, _heap| {
        let string = args.first().map(vm_engine_core::value::coerce::to_string).unwrap_or_default();
        let explicit_radix = args.get(1)
            .and_then(|v| v.as_number())
            .map(|n| n as u32)
            .filter(|r| (2..=36).contains(r));
        let trimmed = string.trim();

        // Auto-detect radix from prefix when not explicitly provided
        let (cleaned, radix) = if let Some(radix) = explicit_radix {
            let stripped = if radix == 16 {
                trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")).unwrap_or(trimmed)
            } else {
                trimmed
            };
            (stripped, radix)
        } else if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
            (hex, 16)
        } else if let Some(oct) = trimmed.strip_prefix("0o").or_else(|| trimmed.strip_prefix("0O")) {
            (oct, 8)
        } else if let Some(bin) = trimmed.strip_prefix("0b").or_else(|| trimmed.strip_prefix("0B")) {
            (bin, 2)
        } else {
            (trimmed, 10)
        };

        match i64::from_str_radix(cleaned, radix) {
            Ok(value) => Value::number(value as f64),
            Err(_) => Value::number(f64::NAN),
        }
    });
    heap.set_property(global, "parseInt", Value::Object(parse_int));

    let parse_float = heap.alloc_native(|args, _heap| {
        let string = args.first().map(vm_engine_core::value::coerce::to_string).unwrap_or_default();
        match string.trim().parse::<f64>() {
            Ok(val) => Value::number(val),
            Err(_) => Value::number(f64::NAN),
        }
    });
    heap.set_property(global, "parseFloat", Value::Object(parse_float));
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_points_to_global() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_globals(&mut heap, global);

        assert_eq!(heap.get_property(global, "window").as_object(), Some(global));
        assert_eq!(heap.get_property(global, "globalThis").as_object(), Some(global));
    }

    #[test]
    fn global_constants() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_globals(&mut heap, global);

        assert!(heap.get_property(global, "NaN").as_number().unwrap().is_nan());
        assert_eq!(heap.get_property(global, "Infinity"), Value::number(f64::INFINITY));
        assert_eq!(heap.get_property(global, "undefined"), Value::Undefined);
    }

    #[test]
    fn parse_int_with_explicit_radix() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_globals(&mut heap, global);

        let func = heap.get_property(global, "parseInt").as_object().unwrap();
        let result = heap.call(func, &[Value::string("0xFF"), Value::number(16.0)]).unwrap();
        assert_eq!(result, Value::number(255.0));
    }

    #[test]
    fn parse_int_auto_detect_hex() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_globals(&mut heap, global);

        let func = heap.get_property(global, "parseInt").as_object().unwrap();
        // No explicit radix — should auto-detect 0x prefix
        let result = heap.call(func, &[Value::string("0xFF")]).unwrap();
        assert_eq!(result, Value::number(255.0));
    }

    #[test]
    fn parse_int_decimal_default() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_globals(&mut heap, global);

        let func = heap.get_property(global, "parseInt").as_object().unwrap();
        let result = heap.call(func, &[Value::string("42")]).unwrap();
        assert_eq!(result, Value::number(42.0));
    }

    #[test]
    fn parse_float_works() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_globals(&mut heap, global);

        let func = heap.get_property(global, "parseFloat").as_object().unwrap();
        let result = heap.call(func, &[Value::string("3.14")]).unwrap();
        assert_eq!(result, Value::number(3.14));
    }

    #[test]
    fn is_nan_works() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_globals(&mut heap, global);

        let func = heap.get_property(global, "isNaN").as_object().unwrap();
        assert_eq!(heap.call(func, &[Value::number(f64::NAN)]).unwrap(), Value::bool(true));
        assert_eq!(heap.call(func, &[Value::number(42.0)]).unwrap(), Value::bool(false));
    }

    #[test]
    fn is_finite_works() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_globals(&mut heap, global);

        let func = heap.get_property(global, "isFinite").as_object().unwrap();
        assert_eq!(heap.call(func, &[Value::number(42.0)]).unwrap(), Value::bool(true));
        assert_eq!(heap.call(func, &[Value::number(f64::INFINITY)]).unwrap(), Value::bool(false));
    }
}

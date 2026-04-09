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
        let radix = args.get(1)
            .and_then(|v| v.as_number())
            .map(|n| n as u32)
            .unwrap_or(10);
        let trimmed = string.trim();
        // Strip 0x/0X prefix for hex
        let cleaned = if radix == 16 {
            trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")).unwrap_or(trimmed)
        } else {
            trimmed
        };
        match i64::from_str_radix(cleaned, radix) {
            Ok(val) => Value::number(val as f64),
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
    fn parse_int_works() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_globals(&mut heap, global);

        let parse_int_id = heap.get_property(global, "parseInt").as_object().unwrap();
        let result = heap.call(parse_int_id, &[Value::string("0xFF"), Value::number(16.0)]).unwrap();
        assert_eq!(result, Value::number(255.0));
    }
}

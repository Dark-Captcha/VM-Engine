//! Global object setup: window, globalThis, self, and global constants.

// ============================================================================
// Imports
// ============================================================================

use crate::exec::heap::Heap;
use crate::value::{ObjectId, Value};

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
        let number = args.first().map(crate::value::coerce::to_number).unwrap_or(f64::NAN);
        Value::bool(number.is_nan())
    });
    heap.set_property(global, "isNaN", Value::Object(is_nan));

    let is_finite = heap.alloc_native(|args, _heap| {
        let number = args.first().map(crate::value::coerce::to_number).unwrap_or(f64::NAN);
        Value::bool(number.is_finite())
    });
    heap.set_property(global, "isFinite", Value::Object(is_finite));

    let parse_int = heap.alloc_native(|args, _heap| {
        let string = args.first().map(crate::value::coerce::to_string).unwrap_or_default();
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
        let string = args.first().map(crate::value::coerce::to_string).unwrap_or_default();
        match string.trim().parse::<f64>() {
            Ok(val) => Value::number(val),
            Err(_) => Value::number(f64::NAN),
        }
    });
    heap.set_property(global, "parseFloat", Value::Object(parse_float));

    // Uint8Array constructor: new Uint8Array(array_or_length) → Uint8Array-like object
    // Handles both Value::Array and heap-allocated Object arrays (from COLLECT opcode).
    let uint8array_ctor = heap.alloc_native(|args, heap| {
        match args.first() {
            Some(Value::Array(elements)) => {
                let result = heap.alloc();
                for (i, v) in elements.iter().enumerate() {
                    let n = crate::value::coerce::to_number(v) as u8;
                    heap.set_property(result, &i.to_string(), Value::number(n as f64));
                }
                heap.set_property(result, "length", Value::number(elements.len() as f64));
                Value::Object(result)
            }
            Some(Value::Object(src_oid)) => {
                // Read from heap object (array created by COLLECT → NewArray on heap)
                let len = crate::value::coerce::to_number(
                    &heap.get_property(*src_oid, "length")
                ) as usize;
                let result = heap.alloc();
                for i in 0..len {
                    let v = heap.get_property(*src_oid, &i.to_string());
                    let n = crate::value::coerce::to_number(&v) as u8;
                    heap.set_property(result, &i.to_string(), Value::number(n as f64));
                }
                heap.set_property(result, "length", Value::number(len as f64));
                Value::Object(result)
            }
            Some(Value::Number(n)) => {
                let len = *n as usize;
                let result = heap.alloc();
                for i in 0..len {
                    heap.set_property(result, &i.to_string(), Value::number(0.0));
                }
                heap.set_property(result, "length", Value::number(len as f64));
                Value::Object(result)
            }
            _ => {
                let result = heap.alloc();
                heap.set_property(result, "length", Value::number(0.0));
                Value::Object(result)
            }
        }
    });
    heap.set_property(global, "Uint8Array", Value::Object(uint8array_ctor));

    // Event listener stubs (no-op but returns correctly typed values)
    let add_event_listener = heap.alloc_native(|_args, _heap| {
        // addEventListener(type, listener, options?) — no-op in VM
        Value::Undefined
    });
    heap.set_property(global, "addEventListener", Value::Object(add_event_listener));

    let remove_event_listener = heap.alloc_native(|_args, _heap| {
        // removeEventListener(type, listener, options?) — no-op
        Value::Undefined
    });
    heap.set_property(global, "removeEventListener", Value::Object(remove_event_listener));

    let dispatch_event = heap.alloc_native(|_args, _heap| {
        // dispatchEvent(event) — always returns true (event not cancelled)
        Value::bool(true)
    });
    heap.set_property(global, "dispatchEvent", Value::Object(dispatch_event));

    // Timer stubs — execute immediately (no async in VM)
    let set_timeout = heap.alloc_native(|args, heap| {
        // setTimeout(callback, delay?, ...args) → timerId
        // We execute callback immediately since VM doesn't support async
        if let Some(Value::Object(callback_oid)) = args.first() {
            let callback_args = if args.len() > 2 { &args[2..] } else { &[] };
            let _ = heap.call(*callback_oid, callback_args);
        }
        // Return a fake timer ID
        Value::number(1.0)
    });
    heap.set_property(global, "setTimeout", Value::Object(set_timeout));

    let set_interval = heap.alloc_native(|_args, _heap| {
        // setInterval — return timer ID, but don't actually repeat
        Value::number(2.0)
    });
    heap.set_property(global, "setInterval", Value::Object(set_interval));

    let clear_timeout = heap.alloc_native(|_args, _heap| {
        // clearTimeout — no-op
        Value::Undefined
    });
    heap.set_property(global, "clearTimeout", Value::Object(clear_timeout));

    let clear_interval = heap.alloc_native(|_args, _heap| {
        // clearInterval — no-op
        Value::Undefined
    });
    heap.set_property(global, "clearInterval", Value::Object(clear_interval));

    let set_immediate = heap.alloc_native(|args, heap| {
        // setImmediate(callback, ...args) — execute immediately
        if let Some(Value::Object(callback_oid)) = args.first() {
            let callback_args = if args.len() > 1 { &args[1..] } else { &[] };
            let _ = heap.call(*callback_oid, callback_args);
        }
        Value::number(3.0)
    });
    heap.set_property(global, "setImmediate", Value::Object(set_immediate));

    // Symbol stub (basic support)
    let symbol_ctor = heap.alloc_native(|args, _heap| {
        let desc = args.first()
            .map(crate::value::coerce::to_string)
            .unwrap_or_default();
        // Return a string representation since we don't have real Symbol type
        Value::string(format!("Symbol({})", desc))
    });
    heap.set_property(global, "Symbol", Value::Object(symbol_ctor));

    // Promise stub (returns resolved-like object)
    let promise_ctor = heap.alloc_native(|args, heap| {
        let promise_obj = heap.alloc();
        // Call executor(resolve, reject) immediately with stub resolvers
        if let Some(Value::Object(executor)) = args.first() {
            let resolve_fn = heap.alloc_native(|_args, _heap| Value::Undefined);
            let reject_fn = heap.alloc_native(|_args, _heap| Value::Undefined);
            let _ = heap.call(*executor, &[Value::Object(resolve_fn), Value::Object(reject_fn)]);
        }
        // Add .then() stub
        let then_fn = heap.alloc_native(|_args, _heap| Value::Undefined);
        heap.set_property(promise_obj, "then", Value::Object(then_fn));
        let catch_fn = heap.alloc_native(|_args, _heap| Value::Undefined);
        heap.set_property(promise_obj, "catch", Value::Object(catch_fn));
        Value::Object(promise_obj)
    });
    // Promise.resolve()
    let promise_resolve = heap.alloc_native(|args, heap| {
        let promise_obj = heap.alloc();
        let then_fn = heap.alloc_native(|callback_args, heap| {
            if let Some(Value::Object(callback)) = callback_args.first() {
                let _ = heap.call(*callback, &[]);
            }
            Value::Undefined
        });
        heap.set_property(promise_obj, "then", Value::Object(then_fn));
        heap.set_property(promise_obj, "value", args.first().cloned().unwrap_or(Value::Undefined));
        Value::Object(promise_obj)
    });
    heap.set_property(promise_ctor, "resolve", Value::Object(promise_resolve));
    heap.set_property(global, "Promise", Value::Object(promise_ctor));

    // Object constructor stubs
    let object_ctor = heap.alloc_native(|_args, heap| {
        Value::Object(heap.alloc())
    });
    let object_keys = heap.alloc_native(|args, heap| {
        if let Some(Value::Object(oid)) = args.first() {
            if let Some(obj) = heap.get(*oid) {
                let keys: Vec<Value> = obj.properties.keys()
                    .map(|k| Value::string(k.clone()))
                    .collect();
                return Value::Array(keys);
            }
        }
        Value::Array(vec![])
    });
    heap.set_property(object_ctor, "keys", Value::Object(object_keys));
    let object_values = heap.alloc_native(|args, heap| {
        if let Some(Value::Object(oid)) = args.first() {
            if let Some(obj) = heap.get(*oid) {
                let values: Vec<Value> = obj.properties.values().cloned().collect();
                return Value::Array(values);
            }
        }
        Value::Array(vec![])
    });
    heap.set_property(object_ctor, "values", Value::Object(object_values));
    heap.set_property(global, "Object", Value::Object(object_ctor));

    // Array constructor stubs
    let array_ctor = heap.alloc_native(|args, _heap| {
        if args.len() == 1 {
            if let Some(n) = args[0].as_number() {
                // new Array(length)
                let len = n as usize;
                return Value::Array(vec![Value::Undefined; len]);
            }
        }
        // new Array(...items)
        Value::Array(args.to_vec())
    });
    let array_is_array = heap.alloc_native(|args, _heap| {
        Value::bool(matches!(args.first(), Some(Value::Array(_))))
    });
    heap.set_property(array_ctor, "isArray", Value::Object(array_is_array));
    heap.set_property(global, "Array", Value::Object(array_ctor));
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

//! String static methods: `String.fromCharCode`, `String.fromCodePoint`.

// ============================================================================
// Imports
// ============================================================================

use crate::exec::heap::Heap;
use crate::value::{ObjectId, Value};
use crate::value::coerce;

// ============================================================================
// Install
// ============================================================================

/// Install the `String` object with static methods on the global.
pub fn install_string_utils(heap: &mut Heap, global: ObjectId) {
    let string_obj = heap.alloc();

    // fromCharCode: truncate to u16 (UTF-16 code unit), per ECMAScript spec
    let from_char_code = heap.alloc_native(|args, _heap| {
        let result: String = args.iter()
            .map(|arg| (coerce::to_number(arg) as u32) & 0xFFFF) // Truncate to u16
            .filter_map(|n| char::from_u32(n))
            .collect();
        Value::string(result)
    });
    heap.set_property(string_obj, "fromCharCode", Value::Object(from_char_code));

    // fromCodePoint: use full u32 code point, per ECMAScript spec
    let from_code_point = heap.alloc_native(|args, _heap| {
        let result: String = args.iter()
            .map(|arg| coerce::to_number(arg) as u32)
            .filter_map(char::from_u32)
            .collect();
        Value::string(result)
    });
    heap.set_property(string_obj, "fromCodePoint", Value::Object(from_code_point));

    heap.set_property(global, "String", Value::Object(string_obj));
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_char_code_ascii() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_string_utils(&mut heap, global);

        let string_obj = heap.get_property(global, "String").as_object().unwrap();
        let from_cc = heap.get_property(string_obj, "fromCharCode").as_object().unwrap();
        let result = heap.call(from_cc, &[
            Value::number(72.0),  // H
            Value::number(101.0), // e
            Value::number(108.0), // l
            Value::number(108.0), // l
            Value::number(111.0), // o
        ]).unwrap();
        assert_eq!(result, Value::string("Hello"));
    }

    #[test]
    fn from_char_code_empty() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_string_utils(&mut heap, global);

        let string_obj = heap.get_property(global, "String").as_object().unwrap();
        let from_cc = heap.get_property(string_obj, "fromCharCode").as_object().unwrap();
        let result = heap.call(from_cc, &[]).unwrap();
        assert_eq!(result, Value::string(""));
    }
}

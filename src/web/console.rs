//! Console capture: `console.log(...)`.
//!
//! Captures output as hidden heap properties for test verification.

// ============================================================================
// Imports
// ============================================================================

use crate::exec::heap::Heap;
use crate::value::{ObjectId, Value};
use crate::value::coerce;

// ============================================================================
// Install
// ============================================================================

/// Install `console.log` that captures output on the global object.
///
/// Captured data:
/// - `__console_count`: number of calls
/// - `__console_last`: last logged message as string
pub fn install_console(heap: &mut Heap, global: ObjectId) {
    heap.set_property(global, "__console_count", Value::number(0.0));
    heap.set_property(global, "__console_last", Value::string(""));

    let log_fn = heap.alloc_closure(move |args, heap| {
        let count = heap.get_property(global, "__console_count")
            .as_number().unwrap_or(0.0);
        heap.set_property(global, "__console_count", Value::number(count + 1.0));

        let message: String = args.iter()
            .map(coerce::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        heap.set_property(global, "__console_last", Value::string(&message));
        Value::Undefined
    });

    let console_object = heap.alloc();
    heap.set_property(console_object, "log", Value::Object(log_fn));
    heap.set_property(global, "console", Value::Object(console_object));
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_log_captures() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_console(&mut heap, global);

        let console = heap.get_property(global, "console").as_object().unwrap();
        let log = heap.get_property(console, "log").as_object().unwrap();

        heap.call(log, &[Value::string("hello"), Value::number(42.0)]).unwrap();

        assert_eq!(heap.get_property(global, "__console_count"), Value::number(1.0));
        assert_eq!(heap.get_property(global, "__console_last"), Value::string("hello 42"));
    }
}

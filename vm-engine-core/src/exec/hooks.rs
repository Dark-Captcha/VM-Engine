//! Hook trait: bridge between IR interpreter and the outside world.
//!
//! When execution hits a Call or property access that doesn't resolve to
//! an IR function, it asks the hook. The `vm-engine-web` crate provides
//! a hook that implements browser globals.

// ============================================================================
// Imports
// ============================================================================

use crate::value::{ObjectId, Value};

use super::heap::Heap;

// ============================================================================
// Hook trait
// ============================================================================

/// External environment hook for the IR interpreter.
///
/// Intercepts operations that require outside-world knowledge (browser APIs,
/// mocked services, data extraction points).
pub trait Hook {
    /// Intercept a function call by name. Return `Some(value)` to provide
    /// the result, or `None` to let the interpreter handle it.
    fn on_call(&mut self, _name: &str, _args: &[Value], _heap: &mut Heap) -> Option<Value> {
        None
    }

    /// Intercept a property read. Return `Some(value)` to override.
    fn on_prop_get(
        &mut self,
        _obj: ObjectId,
        _key: &str,
        _heap: &Heap,
    ) -> Option<Value> {
        None
    }

    /// Observe a property write.
    fn on_prop_set(
        &mut self,
        _obj: ObjectId,
        _key: &str,
        _val: &Value,
        _heap: &Heap,
    ) {
    }

    /// Intercept a constructor call (`new Foo(...)`). Return `Some(value)` to override.
    fn on_new(
        &mut self,
        _constructor: &str,
        _args: &[Value],
        _heap: &mut Heap,
    ) -> Option<Value> {
        None
    }
}

/// Default hook: no interception. Used for pure IR execution.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullHook;

impl Hook for NullHook {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_hook_returns_none() {
        let mut hook = NullHook;
        let mut heap = Heap::new();
        assert!(hook.on_call("btoa", &[], &mut heap).is_none());
        assert!(hook.on_prop_get(ObjectId(0), "x", &heap).is_none());
    }
}

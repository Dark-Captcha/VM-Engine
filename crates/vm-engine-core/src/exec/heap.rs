//! Object heap with properties and prototype chains.

// ============================================================================
// Imports
// ============================================================================

use std::collections::HashMap;
use std::fmt;

use crate::value::{ObjectId, Value};
use crate::ir::FuncId;

// ============================================================================
// Types
// ============================================================================

/// A heap-allocated object.
#[derive(Debug, Clone)]
pub struct Object {
    pub properties: HashMap<String, Value>,
    pub prototype: Option<ObjectId>,
    pub callable: Option<CallableKind>,
}

/// Shared closure type for callable objects.
pub type ClosureFn = std::sync::Arc<dyn Fn(&[Value], &mut Heap) -> Value + Send + Sync>;

/// How a callable object invokes.
#[derive(Clone)]
pub enum CallableKind {
    /// Rust-side function (host APIs, mocked browser methods).
    Native(fn(&[Value], &mut Heap) -> Value),
    /// Closure that captures state.
    Closure(ClosureFn),
    /// Function defined in the IR.
    IrFunction { func_id: FuncId, captures: Vec<Value> },
}

impl fmt::Debug for CallableKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(_) => write!(f, "Native(<fn>)"),
            Self::Closure(_) => write!(f, "Closure(<fn>)"),
            Self::IrFunction { func_id, .. } => write!(f, "IrFunction({func_id})"),
        }
    }
}

// ============================================================================
// Heap
// ============================================================================

/// Arena-based object heap with prototype chains.
pub struct Heap {
    slots: Vec<Option<Object>>,
    free_list: Vec<u32>,
}

impl Heap {
    pub fn new() -> Self {
        Self { slots: Vec::new(), free_list: Vec::new() }
    }

    /// Allocate a new empty object.
    pub fn alloc(&mut self) -> ObjectId {
        if let Some(idx) = self.free_list.pop() {
            self.slots[idx as usize] = Some(Object {
                properties: HashMap::new(),
                prototype: None,
                callable: None,
            });
            ObjectId(idx)
        } else {
            let idx = self.slots.len() as u32;
            self.slots.push(Some(Object {
                properties: HashMap::new(),
                prototype: None,
                callable: None,
            }));
            ObjectId(idx)
        }
    }

    /// Allocate a native callable object.
    pub fn alloc_native(&mut self, f: fn(&[Value], &mut Heap) -> Value) -> ObjectId {
        let id = self.alloc();
        if let Some(obj) = self.get_mut(id) {
            obj.callable = Some(CallableKind::Native(f));
        }
        id
    }

    /// Allocate a closure callable object.
    pub fn alloc_closure(
        &mut self,
        f: impl Fn(&[Value], &mut Heap) -> Value + Send + Sync + 'static,
    ) -> ObjectId {
        let id = self.alloc();
        if let Some(obj) = self.get_mut(id) {
            obj.callable = Some(CallableKind::Closure(std::sync::Arc::new(f)));
        }
        id
    }

    pub fn get(&self, id: ObjectId) -> Option<&Object> {
        self.slots.get(id.0 as usize).and_then(|s| s.as_ref())
    }

    pub fn get_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        self.slots.get_mut(id.0 as usize).and_then(|s| s.as_mut())
    }

    /// Get property, walking prototype chain.
    pub fn get_property(&self, id: ObjectId, key: &str) -> Value {
        let mut current = Some(id);
        while let Some(oid) = current {
            if let Some(obj) = self.get(oid) {
                if let Some(val) = obj.properties.get(key) {
                    return val.clone();
                }
                current = obj.prototype;
            } else {
                break;
            }
        }
        Value::Undefined
    }

    /// Set own property.
    pub fn set_property(&mut self, id: ObjectId, key: &str, val: Value) {
        if let Some(obj) = self.get_mut(id) {
            obj.properties.insert(key.to_owned(), val);
        }
    }

    /// Check if key exists (own or inherited).
    pub fn has_property(&self, id: ObjectId, key: &str) -> bool {
        let mut current = Some(id);
        while let Some(oid) = current {
            if let Some(obj) = self.get(oid) {
                if obj.properties.contains_key(key) {
                    return true;
                }
                current = obj.prototype;
            } else {
                break;
            }
        }
        false
    }

    /// Delete own property.
    pub fn delete_property(&mut self, id: ObjectId, key: &str) -> bool {
        self.get_mut(id)
            .map(|obj| obj.properties.remove(key).is_some())
            .unwrap_or(false)
    }

    /// Set prototype link.
    pub fn set_prototype(&mut self, id: ObjectId, proto: ObjectId) {
        if let Some(obj) = self.get_mut(id) {
            obj.prototype = Some(proto);
        }
    }

    /// Call a callable object. Handles closure re-entrancy.
    pub fn call(&mut self, id: ObjectId, args: &[Value]) -> Option<Value> {
        let callable = self.get(id)?.callable.clone()?;
        match callable {
            CallableKind::Native(f) => Some(f(args, self)),
            CallableKind::Closure(f) => Some(f(args, self)),
            CallableKind::IrFunction { .. } => None, // handled by interpreter
        }
    }

    /// Free an object (return slot to free list).
    pub fn free(&mut self, id: ObjectId) {
        if (id.0 as usize) < self.slots.len() {
            self.slots[id.0 as usize] = None;
            self.free_list.push(id.0);
        }
    }

    /// Number of live objects.
    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

impl Default for Heap {
    fn default() -> Self { Self::new() }
}

impl fmt::Debug for Heap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Heap({} live)", self.live_count())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_and_property() {
        let mut h = Heap::new();
        let obj = h.alloc();
        h.set_property(obj, "x", Value::number(42.0));
        assert_eq!(h.get_property(obj, "x"), Value::number(42.0));
        assert_eq!(h.get_property(obj, "y"), Value::Undefined);
    }

    #[test]
    fn prototype_chain() {
        let mut h = Heap::new();
        let proto = h.alloc();
        h.set_property(proto, "inherited", Value::string("yes"));
        let child = h.alloc();
        h.set_prototype(child, proto);
        assert_eq!(h.get_property(child, "inherited"), Value::string("yes"));
    }

    #[test]
    fn call_native() {
        let mut h = Heap::new();
        let f = h.alloc_native(|args, _heap| {
            let n = args.first().and_then(|v| v.as_number()).unwrap_or(0.0);
            Value::number(n * 2.0)
        });
        let result = h.call(f, &[Value::number(21.0)]).unwrap();
        assert_eq!(result, Value::number(42.0));
    }

    #[test]
    fn free_and_reuse() {
        let mut h = Heap::new();
        let a = h.alloc();
        h.free(a);
        let b = h.alloc();
        assert_eq!(a.0, b.0); // reused slot
    }

    #[test]
    fn has_property_with_undefined_value() {
        let mut h = Heap::new();
        let obj = h.alloc();
        h.set_property(obj, "x", Value::Undefined);
        assert!(h.has_property(obj, "x")); // key exists
    }

    #[test]
    fn live_count() {
        let mut h = Heap::new();
        assert_eq!(h.live_count(), 0);
        let a = h.alloc();
        let _b = h.alloc();
        assert_eq!(h.live_count(), 2);
        h.free(a);
        assert_eq!(h.live_count(), 1);
    }
}

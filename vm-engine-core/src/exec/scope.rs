//! Lexical scope chain for variable lookup.

// ============================================================================
// Imports
// ============================================================================

use std::collections::HashMap;

use crate::value::Value;

// ============================================================================
// Types
// ============================================================================

/// A variable scope with parent link.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub bindings: HashMap<String, Value>,
    pub parent: Option<usize>,
}

/// Scope chain: vector of scopes, innermost last.
///
/// Lookup walks from innermost to root via parent links.
#[derive(Debug, Clone)]
pub struct ScopeChain {
    scopes: Vec<Scope>,
}

impl ScopeChain {
    pub fn new() -> Self {
        Self { scopes: vec![Scope::default()] }
    }

    /// Look up a variable by walking the scope chain.
    pub fn get(&self, name: &str) -> Option<Value> {
        let mut idx = self.scopes.len().checked_sub(1);
        while let Some(i) = idx {
            if let Some(val) = self.scopes[i].bindings.get(name) {
                return Some(val.clone());
            }
            idx = self.scopes[i].parent;
        }
        None
    }

    /// Set a variable in the innermost scope.
    pub fn set(&mut self, name: &str, val: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.bindings.insert(name.to_owned(), val);
        }
    }

    /// Set a variable in the scope where it already exists, or innermost if not found.
    pub fn set_existing(&mut self, name: &str, val: Value) {
        let mut idx = self.scopes.len().checked_sub(1);
        while let Some(i) = idx {
            if self.scopes[i].bindings.contains_key(name) {
                self.scopes[i].bindings.insert(name.to_owned(), val);
                return;
            }
            idx = self.scopes[i].parent;
        }
        // Not found — set in innermost
        self.set(name, val);
    }

    /// Push a new scope with a parent link.
    pub fn push_scope(&mut self, parent: Option<usize>) {
        self.scopes.push(Scope {
            bindings: HashMap::new(),
            parent,
        });
    }

    /// Current (innermost) scope index.
    pub fn current_index(&self) -> usize {
        self.scopes.len().saturating_sub(1)
    }

    /// Truncate to `len` scopes (for returning from function calls).
    pub fn truncate(&mut self, len: usize) {
        self.scopes.truncate(len);
    }

    /// Number of scopes.
    pub fn len(&self) -> usize {
        self.scopes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

impl Default for ScopeChain {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get() {
        let mut sc = ScopeChain::new();
        sc.set("x", Value::number(10.0));
        assert_eq!(sc.get("x"), Some(Value::number(10.0)));
    }

    #[test]
    fn not_found_returns_none() {
        let sc = ScopeChain::new();
        assert_eq!(sc.get("missing"), None);
    }

    #[test]
    fn parent_chain_lookup() {
        let mut sc = ScopeChain::new();
        sc.set("outer", Value::number(1.0));
        let parent = sc.current_index();
        sc.push_scope(Some(parent));
        sc.set("inner", Value::number(2.0));

        assert_eq!(sc.get("inner"), Some(Value::number(2.0)));
        assert_eq!(sc.get("outer"), Some(Value::number(1.0)));
    }

    #[test]
    fn inner_shadows_outer() {
        let mut sc = ScopeChain::new();
        sc.set("x", Value::number(1.0));
        let parent = sc.current_index();
        sc.push_scope(Some(parent));
        sc.set("x", Value::number(2.0));

        assert_eq!(sc.get("x"), Some(Value::number(2.0)));
    }

    #[test]
    fn set_existing_updates_outer() {
        let mut sc = ScopeChain::new();
        sc.set("x", Value::number(1.0));
        let parent = sc.current_index();
        sc.push_scope(Some(parent));
        sc.set_existing("x", Value::number(99.0));

        // Inner scope doesn't have "x", but outer does — should update outer
        sc.truncate(1); // pop inner
        assert_eq!(sc.get("x"), Some(Value::number(99.0)));
    }
}

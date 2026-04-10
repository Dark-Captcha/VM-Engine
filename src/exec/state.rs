//! Interpreter runtime state.

// ============================================================================
// Imports
// ============================================================================

use std::collections::HashMap;
use std::fmt;

use crate::ir::{BlockId, FuncId, Var};
use crate::value::{ObjectId, Value};

use super::heap::Heap;
use super::scope::ScopeChain;

// ============================================================================
// Cursor
// ============================================================================

/// Points to a specific instruction in the IR program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Which function is executing.
    pub function: FuncId,
    /// Which block within that function.
    pub block: BlockId,
    /// Index of the current instruction within the block body.
    pub instruction: usize,
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}.{}", self.function, self.block, self.instruction)
    }
}

// ============================================================================
// CallFrame
// ============================================================================

/// Saved state for function call/return.
#[derive(Debug, Clone)]
pub struct CallFrame {
    /// Where to resume after return.
    pub return_cursor: Cursor,
    /// Scope chain depth to restore.
    pub scope_depth: usize,
    /// Saved local variable bindings.
    pub locals: HashMap<Var, Value>,
}

// ============================================================================
// State
// ============================================================================

/// Full runtime state of the IR interpreter.
#[derive(Debug)]
pub struct State {
    /// IR variable bindings (Var → Value).
    pub vars: HashMap<Var, Value>,
    /// Object heap.
    pub heap: Heap,
    /// Lexical scope chain.
    pub scopes: ScopeChain,
    /// Function call stack.
    pub call_stack: Vec<CallFrame>,
    /// Current position in the IR.
    pub cursor: Cursor,
    /// The block we came from (for Phi node resolution).
    pub previous_block: Option<BlockId>,
    /// The global object — `LoadScope` falls back here when scope lookup fails.
    pub global_object: Option<ObjectId>,
    /// Whether execution has stopped.
    pub halted: bool,
    /// Total instructions executed.
    pub instruction_count: u64,
}

impl State {
    /// Create a new state starting at the entry of a function.
    pub fn new(entry_func: FuncId, entry_block: BlockId) -> Self {
        Self {
            vars: HashMap::new(),
            heap: Heap::new(),
            scopes: ScopeChain::new(),
            call_stack: Vec::new(),
            cursor: Cursor {
                function: entry_func,
                block: entry_block,
                instruction: 0,
            },
            previous_block: None,
            global_object: None,
            halted: false,
            instruction_count: 0,
        }
    }

    /// Get the value of a Var.
    pub fn get_var(&self, var: Var) -> Value {
        self.vars.get(&var).cloned().unwrap_or(Value::Undefined)
    }

    /// Set the value of a Var.
    pub fn set_var(&mut self, var: Var, val: Value) {
        self.vars.insert(var, val);
    }

    /// Current call depth.
    pub fn call_depth(&self) -> usize {
        self.call_stack.len()
    }
}

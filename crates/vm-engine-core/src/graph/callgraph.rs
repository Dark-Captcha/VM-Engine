//! Inter-procedural call graph construction.
//!
//! Scans all `Call` and `CallMethod` instructions across a module to build
//! a map of which functions call which.

// ============================================================================
// Imports
// ============================================================================

use std::collections::BTreeMap;

use crate::ir::{FuncId, Module};
use crate::ir::opcode::OpCode;
use crate::ir::operand::Operand;

// ============================================================================
// Types
// ============================================================================

/// Call graph for a module.
#[derive(Debug, Clone)]
pub struct CallGraph {
    /// For each function, which functions it calls.
    pub callees: BTreeMap<FuncId, Vec<FuncId>>,
    /// For each function, which functions call it.
    pub callers: BTreeMap<FuncId, Vec<FuncId>>,
    /// Functions with no callers (entry points).
    pub roots: Vec<FuncId>,
    /// Functions that call nothing.
    pub leaves: Vec<FuncId>,
}

// ============================================================================
// Build
// ============================================================================

/// Build a call graph from an IR module.
pub fn build_call_graph(module: &Module) -> CallGraph {
    let mut callees: BTreeMap<FuncId, Vec<FuncId>> = BTreeMap::new();
    let mut callers: BTreeMap<FuncId, Vec<FuncId>> = BTreeMap::new();

    // Initialize entries for all functions
    for func in &module.functions {
        callees.entry(func.id).or_default();
        callers.entry(func.id).or_default();
    }

    // Scan all instructions for Call/CallMethod with FuncRef operands
    for func in &module.functions {
        for block in &func.blocks {
            for instr in &block.body {
                if !matches!(instr.op, OpCode::Call | OpCode::CallMethod) {
                    continue;
                }
                for operand in &instr.operands {
                    if let Operand::Func(callee_id) = operand {
                        callees.entry(func.id).or_default().push(*callee_id);
                        callers.entry(*callee_id).or_default().push(func.id);
                    }
                }
            }
        }
    }

    // Deduplicate
    for targets in callees.values_mut() {
        targets.sort();
        targets.dedup();
    }
    for sources in callers.values_mut() {
        sources.sort();
        sources.dedup();
    }

    let roots: Vec<FuncId> = callers.iter()
        .filter(|(_, v)| v.is_empty())
        .map(|(&k, _)| k)
        .collect();

    let leaves: Vec<FuncId> = callees.iter()
        .filter(|(_, v)| v.is_empty())
        .map(|(&k, _)| k)
        .collect();

    CallGraph { callees, callers, roots, leaves }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::IrBuilder;

    #[test]
    fn single_function_is_root_and_leaf() {
        let mut b = IrBuilder::new();
        b.begin_function("main");
        b.create_and_switch("entry");
        b.halt();
        b.end_function();

        let module = b.build();
        let cg = build_call_graph(&module);

        assert_eq!(cg.roots, vec![FuncId(0)]);
        assert_eq!(cg.leaves, vec![FuncId(0)]);
    }

    #[test]
    fn caller_callee_relationship() {
        let mut b = IrBuilder::new();

        let helper_id = b.begin_function("helper");
        b.create_and_switch("entry");
        b.halt();
        b.end_function();

        b.begin_function("main");
        b.create_and_switch("entry");
        let _r = b.call(helper_id, &[]);
        b.halt();
        b.end_function();

        let module = b.build();
        let cg = build_call_graph(&module);

        // main calls helper
        assert!(cg.callees[&FuncId(1)].contains(&FuncId(0)));
        // helper is called by main
        assert!(cg.callers[&FuncId(0)].contains(&FuncId(1)));
        // main is root (nobody calls it)
        assert!(cg.roots.contains(&FuncId(1)));
        // helper is leaf (calls nothing)
        assert!(cg.leaves.contains(&FuncId(0)));
    }

    #[test]
    fn chain_of_calls() {
        let mut b = IrBuilder::new();

        let c_id = b.begin_function("c");
        b.create_and_switch("entry");
        b.halt();
        b.end_function();

        let b_id = b.begin_function("b");
        b.create_and_switch("entry");
        let _ = b.call(c_id, &[]);
        b.halt();
        b.end_function();

        b.begin_function("a");
        b.create_and_switch("entry");
        let _ = b.call(b_id, &[]);
        b.halt();
        b.end_function();

        let module = b.build();
        let cg = build_call_graph(&module);

        assert_eq!(cg.roots, vec![FuncId(2)]); // a is the root
        assert_eq!(cg.leaves, vec![FuncId(0)]); // c is the leaf
    }
}

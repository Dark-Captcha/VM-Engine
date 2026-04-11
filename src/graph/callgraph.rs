//! Inter-procedural call graph construction.
//!
//! Scans all `Call` and `CallMethod` instructions across a module to build
//! a map of which functions call which.

// ============================================================================
// Imports
// ============================================================================

use std::collections::BTreeMap;

use crate::ir::{BlockId, FuncId, Module};
use crate::ir::opcode::OpCode;
use crate::ir::operand::Operand;

// ============================================================================
// Types
// ============================================================================

/// Information about an indirect call (target unknown at analysis time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndirectCallSite {
    /// Function containing the call.
    pub caller: FuncId,
    /// Block containing the call instruction.
    pub block: BlockId,
    /// Index of the instruction within the block body.
    pub instruction: usize,
    /// True if the call is via CallMethod (obj.method(...)); false for Call(var).
    pub is_method: bool,
}

/// Call graph for a module.
#[derive(Debug, Clone)]
pub struct CallGraph {
    /// For each function, which functions it calls.
    pub callees: BTreeMap<FuncId, Vec<FuncId>>,
    /// For each function, which functions call it.
    pub callers: BTreeMap<FuncId, Vec<FuncId>>,
    /// Functions with no callers (entry points).
    pub roots: Vec<FuncId>,
    /// Functions that call nothing (direct or indirect).
    pub leaves: Vec<FuncId>,
    /// Indirect call sites (variable/method dispatch — target unknown at analysis time).
    pub indirect_calls: Vec<IndirectCallSite>,
}

// ============================================================================
// Build
// ============================================================================

/// Build a call graph from an IR module.
pub fn build_call_graph(module: &Module) -> CallGraph {
    let mut callees: BTreeMap<FuncId, Vec<FuncId>> = BTreeMap::new();
    let mut callers: BTreeMap<FuncId, Vec<FuncId>> = BTreeMap::new();
    let mut indirect_calls: Vec<IndirectCallSite> = Vec::new();

    // Initialize entries for all functions
    for func in &module.functions {
        callees.entry(func.id).or_default();
        callers.entry(func.id).or_default();
    }

    // Scan all instructions for Call/CallMethod, tracking both direct and indirect calls.
    for func in &module.functions {
        for block in &func.blocks {
            for (idx, instr) in block.body.iter().enumerate() {
                if !matches!(instr.op, OpCode::Call | OpCode::CallMethod) {
                    continue;
                }

                // Determine the target operand:
                // - Call:       first operand is the target (Func, Var, or Const)
                // - CallMethod: first operand is the object, second is the method name
                let target_idx = match instr.op {
                    OpCode::Call => 0,
                    OpCode::CallMethod => 0, // object operand — method lookup is dynamic
                    _ => continue,
                };

                let is_method = matches!(instr.op, OpCode::CallMethod);

                if let Some(operand) = instr.operands.get(target_idx) {
                    match operand {
                        Operand::Func(callee_id) => {
                            // Direct call — add to callees/callers graph
                            callees.entry(func.id).or_default().push(*callee_id);
                            callers.entry(*callee_id).or_default().push(func.id);
                        }
                        Operand::Var(_) | Operand::Const(_) => {
                            // Indirect call — record site (target unknown statically)
                            indirect_calls.push(IndirectCallSite {
                                caller: func.id,
                                block: block.id,
                                instruction: idx,
                                is_method,
                            });
                        }
                        _ => {}
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

    // A function is a leaf only if it has no direct callees AND no indirect calls
    let has_indirect: std::collections::HashSet<FuncId> = indirect_calls.iter()
        .map(|ic| ic.caller)
        .collect();
    let leaves: Vec<FuncId> = callees.iter()
        .filter(|(k, v)| v.is_empty() && !has_indirect.contains(k))
        .map(|(&k, _)| k)
        .collect();

    CallGraph { callees, callers, roots, leaves, indirect_calls }
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

    #[test]
    fn indirect_calls_are_tracked() {
        use crate::ir::opcode::OpCode;

        let mut b = IrBuilder::new();
        b.begin_function("main");
        b.create_and_switch("entry");
        // Create a callable via LoadScope and call it (indirect call via Var)
        let fn_var = b.load_scope("someFunction");
        let _result = b.emit(OpCode::Call, vec![Operand::Var(fn_var)]);
        b.halt();
        b.end_function();

        let module = b.build();
        let cg = build_call_graph(&module);

        // Indirect call should be recorded
        assert_eq!(cg.indirect_calls.len(), 1);
        assert_eq!(cg.indirect_calls[0].caller, FuncId(0));
        assert!(!cg.indirect_calls[0].is_method);

        // Function with indirect calls should NOT be a leaf
        assert!(!cg.leaves.contains(&FuncId(0)));
    }
}

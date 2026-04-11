//! IR well-formedness validation.
//!
//! Checks that a [`Module`] is structurally valid before analysis or execution.

// ============================================================================
// Imports
// ============================================================================

use std::collections::HashSet;

use crate::error::{Error, Result};

use super::opcode::{OpCode, Terminator};
use super::operand::Operand;
use super::{BlockId, FuncId, Module, Var};

// ============================================================================
// Validation
// ============================================================================

/// Validate an IR module. Returns `Ok(())` if well-formed, or an error describing
/// the first violation found.
pub fn validate(module: &Module) -> Result<()> {
    // Collect all valid function IDs for Call validation
    let func_ids: HashSet<FuncId> = module.functions.iter().map(|f| f.id).collect();

    for func in &module.functions {
        if func.blocks.is_empty() {
            return Err(Error::validation(format!(
                "function '{}' has no blocks",
                func.name,
            )));
        }

        // Entry block must exist
        if func.block(func.entry).is_none() {
            return Err(Error::validation(format!(
                "function '{}': entry block {} not found",
                func.name, func.entry,
            )));
        }

        // Collect all defined vars and block IDs
        let mut defined_vars: HashSet<Var> = HashSet::new();
        let mut block_ids: HashSet<BlockId> = HashSet::new();

        for param in &func.params {
            defined_vars.insert(*param);
        }

        for block in &func.blocks {
            block_ids.insert(block.id);
            for instr in &block.body {
                if let Some(var) = instr.result
                    && !defined_vars.insert(var)
                {
                    return Err(Error::validation(format!(
                        "function '{}', block '{}': variable {} defined more than once",
                        func.name, block.label, var,
                    )));
                }
            }
        }

        // Compute reachable blocks from entry
        let reachable = compute_reachable(func);

        // Check all var references resolve
        for block in &func.blocks {
            for instr in &block.body {
                for operand in &instr.operands {
                    match operand {
                        Operand::Var(v) => {
                            if !defined_vars.contains(v) {
                                return Err(Error::validation(format!(
                                    "function '{}', block '{}': reference to undefined variable {}",
                                    func.name, block.label, v,
                                )));
                            }
                        }
                        Operand::Func(fid) => {
                            if !func_ids.contains(fid) {
                                return Err(Error::validation(format!(
                                    "function '{}', block '{}': reference to undefined function {}",
                                    func.name, block.label, fid,
                                )));
                            }
                        }
                        Operand::Block(bid) => {
                            if !block_ids.contains(bid) {
                                return Err(Error::validation(format!(
                                    "function '{}', block '{}': reference to undefined block {}",
                                    func.name, block.label, bid,
                                )));
                            }
                        }
                        _ => {}
                    }
                }

                // Validate operand arity for common opcodes
                validate_opcode_arity(&func.name, &block.label, instr)?;
            }

            // Check terminator var references
            match &block.terminator {
                Terminator::BranchIf { cond, .. } => {
                    if !defined_vars.contains(cond) {
                        return Err(Error::validation(format!(
                            "function '{}', block '{}': branch condition {} is undefined",
                            func.name, block.label, cond,
                        )));
                    }
                }
                Terminator::Switch { value, .. } => {
                    if !defined_vars.contains(value) {
                        return Err(Error::validation(format!(
                            "function '{}', block '{}': switch value {} is undefined",
                            func.name, block.label, value,
                        )));
                    }
                }
                Terminator::Return { value: Some(v) } | Terminator::Throw { value: v } => {
                    if !defined_vars.contains(v) {
                        return Err(Error::validation(format!(
                            "function '{}', block '{}': terminator references undefined variable {}",
                            func.name, block.label, v,
                        )));
                    }
                }
                _ => {}
            }

            // Check terminator block targets exist
            for target in block.terminator.targets() {
                if !block_ids.contains(&target) {
                    return Err(Error::validation(format!(
                        "function '{}', block '{}': terminator targets unknown block {}",
                        func.name, block.label, target,
                    )));
                }
            }

            // Check for reachable blocks with Unreachable terminator
            if reachable.contains(&block.id)
                && matches!(block.terminator, Terminator::Unreachable)
            {
                return Err(Error::validation(format!(
                    "function '{}', block '{}': reachable block has Unreachable terminator \
                     (terminator was never set)",
                    func.name, block.label,
                )));
            }

            // Check for duplicate cases in switch
            if let Terminator::Switch { cases, .. } = &block.terminator {
                for (i, (val_a, _)) in cases.iter().enumerate() {
                    for (val_b, _) in cases.iter().skip(i + 1) {
                        if val_a == val_b {
                            return Err(Error::validation(format!(
                                "function '{}', block '{}': duplicate switch case value",
                                func.name, block.label,
                            )));
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Compute the set of blocks reachable from the entry block via terminator edges.
fn compute_reachable(func: &super::Function) -> HashSet<BlockId> {
    let mut reachable = HashSet::new();
    let mut queue = vec![func.entry];
    while let Some(block_id) = queue.pop() {
        if !reachable.insert(block_id) {
            continue;
        }
        if let Some(block) = func.block(block_id) {
            for target in block.terminator.targets() {
                if !reachable.contains(&target) {
                    queue.push(target);
                }
            }
        }
    }
    reachable
}

/// Verify that an instruction has the right number of operands for its opcode.
fn validate_opcode_arity(
    func_name: &str,
    block_label: &str,
    instr: &super::Instruction,
) -> Result<()> {
    let expected_min = match instr.op {
        // Binary arithmetic/comparison ops: exactly 2 operands
        OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Pow
        | OpCode::BitAnd | OpCode::BitOr | OpCode::BitXor
        | OpCode::Shl | OpCode::Shr | OpCode::UShr
        | OpCode::Eq | OpCode::Neq | OpCode::StrictEq | OpCode::StrictNeq
        | OpCode::Lt | OpCode::Gt | OpCode::Lte | OpCode::Gte => Some((2, 2)),
        // Unary ops: exactly 1 operand
        OpCode::Neg | OpCode::Pos | OpCode::LogicalNot | OpCode::BitNot
        | OpCode::TypeOf | OpCode::Void | OpCode::Move => Some((1, 1)),
        // Const/Param: exactly 1 operand
        OpCode::Const | OpCode::Param => Some((1, 1)),
        // Memory ops
        OpCode::LoadProp | OpCode::LoadIndex | OpCode::HasProp | OpCode::DeleteProp => Some((2, 2)),
        OpCode::StoreProp | OpCode::StoreIndex => Some((3, 3)),
        OpCode::LoadScope => Some((1, 1)),
        OpCode::StoreScope => Some((2, 2)),
        OpCode::NewObject | OpCode::NewArray => Some((0, 0)),
        // Variable-arity ops (no validation here)
        OpCode::Call | OpCode::CallMethod | OpCode::Phi => None,
    };

    if let Some((min, max)) = expected_min {
        let n = instr.operands.len();
        if n < min || n > max {
            return Err(Error::validation(format!(
                "function '{func_name}', block '{block_label}': opcode {:?} expects {min} operand(s), got {n}",
                instr.op,
            )));
        }
    }
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::IrBuilder;
    use crate::ir::opcode::OpCode;
    use crate::ir::{Block, BlockId, FuncId, Function, Instruction};
    use crate::value::Value;

    #[test]
    fn valid_module_passes() {
        let mut b = IrBuilder::new();
        b.begin_function("main");
        b.create_and_switch("entry");
        let v = b.const_number(42.0);
        b.ret(Some(v));
        b.end_function();

        let module = b.build();
        assert!(validate(&module).is_ok());
    }

    #[test]
    fn empty_function_fails() {
        let module = Module {
            functions: vec![Function {
                id: FuncId(0),
                name: "empty".into(),
                params: vec![],
                entry: BlockId(0),
                blocks: vec![],
            }],
        };
        let err = validate(&module).unwrap_err();
        assert!(err.to_string().contains("no blocks"));
    }

    #[test]
    fn missing_entry_block_fails() {
        let module = Module {
            functions: vec![Function {
                id: FuncId(0),
                name: "bad_entry".into(),
                params: vec![],
                entry: BlockId(99), // does not exist
                blocks: vec![Block {
                    id: BlockId(0),
                    label: "real".into(),
                    body: vec![],
                    terminator: Terminator::Halt,
                }],
            }],
        };
        let err = validate(&module).unwrap_err();
        assert!(err.to_string().contains("entry block"));
    }

    #[test]
    fn undefined_var_reference_fails() {
        let module = Module {
            functions: vec![Function {
                id: FuncId(0),
                name: "bad_ref".into(),
                params: vec![],
                entry: BlockId(0),
                blocks: vec![Block {
                    id: BlockId(0),
                    label: "entry".into(),
                    body: vec![Instruction {
                        result: Some(Var(0)),
                        op: OpCode::Add,
                        operands: vec![
                            Operand::Var(Var(99)), // undefined
                            Operand::Var(Var(98)), // undefined
                        ],
                        source: None,
                    }],
                    terminator: Terminator::Halt,
                }],
            }],
        };
        let err = validate(&module).unwrap_err();
        assert!(err.to_string().contains("undefined variable"));
    }

    #[test]
    fn duplicate_var_definition_fails() {
        let module = Module {
            functions: vec![Function {
                id: FuncId(0),
                name: "dup_var".into(),
                params: vec![],
                entry: BlockId(0),
                blocks: vec![Block {
                    id: BlockId(0),
                    label: "entry".into(),
                    body: vec![
                        Instruction {
                            result: Some(Var(0)),
                            op: OpCode::Const,
                            operands: vec![Operand::Const(Value::number(1.0))],
                            source: None,
                        },
                        Instruction {
                            result: Some(Var(0)), // duplicate
                            op: OpCode::Const,
                            operands: vec![Operand::Const(Value::number(2.0))],
                            source: None,
                        },
                    ],
                    terminator: Terminator::Halt,
                }],
            }],
        };
        let err = validate(&module).unwrap_err();
        assert!(err.to_string().contains("defined more than once"));
    }

    #[test]
    fn unknown_branch_target_fails() {
        let module = Module {
            functions: vec![Function {
                id: FuncId(0),
                name: "bad_target".into(),
                params: vec![],
                entry: BlockId(0),
                blocks: vec![Block {
                    id: BlockId(0),
                    label: "entry".into(),
                    body: vec![Instruction {
                        result: Some(Var(0)),
                        op: OpCode::Const,
                        operands: vec![Operand::Const(Value::bool(true))],
                        source: None,
                    }],
                    terminator: Terminator::BranchIf {
                        cond: Var(0),
                        if_true: BlockId(99),  // does not exist
                        if_false: BlockId(98), // does not exist
                    },
                }],
            }],
        };
        let err = validate(&module).unwrap_err();
        assert!(err.to_string().contains("unknown block"));
    }

    #[test]
    fn params_are_defined_vars() {
        let mut b = IrBuilder::new();
        b.begin_function("use_param");
        let p = b.add_param();
        b.create_and_switch("entry");
        b.ret(Some(p)); // param should be a valid var
        b.end_function();

        let module = b.build();
        assert!(validate(&module).is_ok());
    }

    #[test]
    fn unreachable_terminator_on_reachable_block_fails() {
        let module = Module {
            functions: vec![Function {
                id: FuncId(0),
                name: "test".into(),
                params: vec![],
                entry: BlockId(0),
                blocks: vec![Block {
                    id: BlockId(0),
                    label: "entry".into(),
                    body: vec![],
                    terminator: Terminator::Unreachable, // Never set!
                }],
            }],
        };
        let err = validate(&module).unwrap_err();
        assert!(err.to_string().contains("Unreachable terminator"));
    }

    #[test]
    fn unreachable_block_with_unreachable_terminator_passes() {
        // Unreachable block with Unreachable terminator is ok (dead code)
        let module = Module {
            functions: vec![Function {
                id: FuncId(0),
                name: "test".into(),
                params: vec![],
                entry: BlockId(0),
                blocks: vec![
                    Block {
                        id: BlockId(0),
                        label: "entry".into(),
                        body: vec![],
                        terminator: Terminator::Halt,
                    },
                    Block {
                        id: BlockId(1),
                        label: "dead".into(), // Never referenced
                        body: vec![],
                        terminator: Terminator::Unreachable,
                    },
                ],
            }],
        };
        assert!(validate(&module).is_ok());
    }

    #[test]
    fn undefined_function_reference_fails() {
        let module = Module {
            functions: vec![Function {
                id: FuncId(0),
                name: "main".into(),
                params: vec![],
                entry: BlockId(0),
                blocks: vec![Block {
                    id: BlockId(0),
                    label: "entry".into(),
                    body: vec![Instruction {
                        result: Some(Var(0)),
                        op: OpCode::Call,
                        operands: vec![Operand::Func(FuncId(999))], // undefined
                        source: None,
                    }],
                    terminator: Terminator::Halt,
                }],
            }],
        };
        let err = validate(&module).unwrap_err();
        assert!(err.to_string().contains("undefined function"));
    }

    #[test]
    fn bad_operand_arity_fails() {
        let module = Module {
            functions: vec![Function {
                id: FuncId(0),
                name: "main".into(),
                params: vec![],
                entry: BlockId(0),
                blocks: vec![Block {
                    id: BlockId(0),
                    label: "entry".into(),
                    body: vec![Instruction {
                        result: Some(Var(0)),
                        op: OpCode::Add,
                        operands: vec![Operand::Const(Value::number(1.0))], // Only 1, need 2
                        source: None,
                    }],
                    terminator: Terminator::Halt,
                }],
            }],
        };
        let err = validate(&module).unwrap_err();
        assert!(err.to_string().contains("expects 2 operand"));
    }

    #[test]
    fn duplicate_switch_case_fails() {
        let module = Module {
            functions: vec![Function {
                id: FuncId(0),
                name: "main".into(),
                params: vec![],
                entry: BlockId(0),
                blocks: vec![
                    Block {
                        id: BlockId(0),
                        label: "entry".into(),
                        body: vec![Instruction {
                            result: Some(Var(0)),
                            op: OpCode::Const,
                            operands: vec![Operand::Const(Value::number(1.0))],
                            source: None,
                        }],
                        terminator: Terminator::Switch {
                            value: Var(0),
                            cases: vec![
                                (Value::number(1.0), BlockId(1)),
                                (Value::number(1.0), BlockId(2)), // Duplicate!
                            ],
                            default: BlockId(3),
                        },
                    },
                    Block { id: BlockId(1), label: "c1".into(), body: vec![], terminator: Terminator::Halt },
                    Block { id: BlockId(2), label: "c2".into(), body: vec![], terminator: Terminator::Halt },
                    Block { id: BlockId(3), label: "d".into(), body: vec![], terminator: Terminator::Halt },
                ],
            }],
        };
        let err = validate(&module).unwrap_err();
        assert!(err.to_string().contains("duplicate switch case"));
    }
}

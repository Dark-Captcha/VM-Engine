//! IR well-formedness validation.
//!
//! Checks that a [`Module`] is structurally valid before analysis or execution.

// ============================================================================
// Imports
// ============================================================================

use std::collections::HashSet;

use crate::error::{Error, Result};

use super::opcode::Terminator;
use super::operand::Operand;
use super::{Module, Var};

// ============================================================================
// Validation
// ============================================================================

/// Validate an IR module. Returns `Ok(())` if well-formed, or an error describing
/// the first violation found.
pub fn validate(module: &Module) -> Result<()> {
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
        let mut block_ids: HashSet<super::BlockId> = HashSet::new();

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

        // Check all var references resolve
        for block in &func.blocks {
            for instr in &block.body {
                for operand in &instr.operands {
                    if let Operand::Var(v) = operand
                        && !defined_vars.contains(v)
                    {
                        return Err(Error::validation(format!(
                            "function '{}', block '{}': reference to undefined variable {}",
                            func.name, block.label, v,
                        )));
                    }
                }
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

            // Check no block has Unreachable unless intentional (warning-level, not error)
            // We skip this for now — Unreachable is a valid placeholder.
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
}

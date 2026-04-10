//! Intermediate representation: the universal format every VM converts into.
//!
//! Structured from creation — never a flat instruction list.
//! Module → Function → Block → Instruction.

pub mod builder;
pub mod display;
pub mod opcode;
pub mod operand;
pub mod validate;

// ============================================================================
// Imports
// ============================================================================

use std::fmt;

pub use opcode::{OpCode, Terminator};
pub use operand::{Operand, SourceLoc};

// ============================================================================
// IDs — Newtypes
// ============================================================================

/// SSA variable. Each instruction that produces a value defines a fresh Var.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Var(pub u32);

impl fmt::Display for Var {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

/// Basic block identifier within a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u32);

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@B{}", self.0)
    }
}

/// Function identifier within a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FuncId(pub u32);

impl fmt::Display for FuncId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fn#{}", self.0)
    }
}

// ============================================================================
// IR types
// ============================================================================

/// Top-level container: a complete IR program.
#[derive(Debug, Clone)]
pub struct Module {
    pub functions: Vec<Function>,
}

impl Module {
    /// Find a function by name.
    pub fn function_by_name(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// Find a function by ID.
    pub fn function_by_id(&self, id: FuncId) -> Option<&Function> {
        self.functions.iter().find(|f| f.id == id)
    }
}

/// A function: named, parameterized, contains blocks.
#[derive(Debug, Clone)]
pub struct Function {
    pub id: FuncId,
    pub name: String,
    pub params: Vec<Var>,
    pub entry: BlockId,
    pub blocks: Vec<Block>,
}

impl Function {
    /// Find a block by ID.
    pub fn block(&self, id: BlockId) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    /// The entry block.
    pub fn entry_block(&self) -> Option<&Block> {
        self.block(self.entry)
    }

    /// All block IDs in this function.
    pub fn block_ids(&self) -> Vec<BlockId> {
        self.blocks.iter().map(|b| b.id).collect()
    }
}

/// A basic block: sequence of instructions ending with a terminator.
#[derive(Debug, Clone)]
pub struct Block {
    pub id: BlockId,
    pub label: String,
    pub body: Vec<Instruction>,
    pub terminator: Terminator,
}

impl Block {
    /// All successor block IDs (from the terminator).
    pub fn successors(&self) -> Vec<BlockId> {
        self.terminator.targets()
    }

    /// Number of instructions (excluding terminator).
    pub fn len(&self) -> usize {
        self.body.len()
    }

    pub fn is_empty(&self) -> bool {
        self.body.is_empty()
    }
}

/// A single IR instruction.
#[derive(Debug, Clone)]
pub struct Instruction {
    /// The variable this instruction defines. `None` for void ops (StoreProp, etc.).
    pub result: Option<Var>,
    /// The operation.
    pub op: OpCode,
    /// Input operands.
    pub operands: Vec<Operand>,
    /// Back-reference to the original bytecode location.
    pub source: Option<SourceLoc>,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_display() {
        assert_eq!(Var(0).to_string(), "%0");
        assert_eq!(Var(42).to_string(), "%42");
    }

    #[test]
    fn block_id_display() {
        assert_eq!(BlockId(0).to_string(), "@B0");
        assert_eq!(BlockId(3).to_string(), "@B3");
    }

    #[test]
    fn func_id_display() {
        assert_eq!(FuncId(0).to_string(), "fn#0");
    }

    #[test]
    fn module_lookup_by_name() {
        let module = Module {
            functions: vec![
                Function {
                    id: FuncId(0),
                    name: "main".into(),
                    params: vec![],
                    entry: BlockId(0),
                    blocks: vec![Block {
                        id: BlockId(0),
                        label: "entry".into(),
                        body: vec![],
                        terminator: Terminator::Halt,
                    }],
                },
            ],
        };
        assert!(module.function_by_name("main").is_some());
        assert!(module.function_by_name("missing").is_none());
    }

    #[test]
    fn block_successors() {
        let block = Block {
            id: BlockId(0),
            label: "test".into(),
            body: vec![],
            terminator: Terminator::BranchIf {
                cond: Var(0),
                if_true: BlockId(1),
                if_false: BlockId(2),
            },
        };
        let succs = block.successors();
        assert_eq!(succs, vec![BlockId(1), BlockId(2)]);
    }
}

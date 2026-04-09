//! IR instruction operands and source location mapping.

// ============================================================================
// Imports
// ============================================================================

use std::fmt;

use crate::value::Value;

use super::{BlockId, FuncId, Var};

// ============================================================================
// Operand
// ============================================================================

/// An input to an IR instruction.
#[derive(Debug, Clone)]
pub enum Operand {
    /// Reference to another instruction's result.
    Var(Var),
    /// Literal constant.
    Const(Value),
    /// Block reference (used by Phi to identify predecessors).
    Block(BlockId),
    /// Function reference (used by Call).
    Func(FuncId),
}

impl Operand {
    pub fn as_var(&self) -> Option<Var> {
        if let Self::Var(v) = self { Some(*v) } else { None }
    }

    pub fn as_const(&self) -> Option<&Value> {
        if let Self::Const(v) = self { Some(v) } else { None }
    }

    pub fn as_block(&self) -> Option<BlockId> {
        if let Self::Block(b) = self { Some(*b) } else { None }
    }

    pub fn as_func(&self) -> Option<FuncId> {
        if let Self::Func(f) = self { Some(*f) } else { None }
    }
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Var(v) => write!(f, "{v}"),
            Self::Const(val) => match val {
                Value::String(s) => write!(f, "\"{s}\""),
                other => write!(f, "{other}"),
            },
            Self::Block(b) => write!(f, "{b}"),
            Self::Func(fid) => write!(f, "{fid}"),
        }
    }
}

impl From<Var> for Operand {
    fn from(v: Var) -> Self {
        Self::Var(v)
    }
}

impl From<Value> for Operand {
    fn from(v: Value) -> Self {
        Self::Const(v)
    }
}

impl From<BlockId> for Operand {
    fn from(b: BlockId) -> Self {
        Self::Block(b)
    }
}

impl From<FuncId> for Operand {
    fn from(f: FuncId) -> Self {
        Self::Func(f)
    }
}

// ============================================================================
// SourceLoc
// ============================================================================

/// Maps an IR instruction back to the original bytecode.
#[derive(Debug, Clone, Copy)]
pub struct SourceLoc {
    /// Byte offset in the original bytecode.
    pub pc: usize,
    /// The raw opcode number before decoding.
    pub original_opcode: Option<u16>,
}

impl SourceLoc {
    pub fn new(pc: usize) -> Self {
        Self { pc, original_opcode: None }
    }

    pub fn with_opcode(pc: usize, opcode: u16) -> Self {
        Self { pc, original_opcode: Some(opcode) }
    }
}

impl fmt::Display for SourceLoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pc={}", self.pc)?;
        if let Some(op) = self.original_opcode {
            write!(f, " op=0x{op:02X}")?;
        }
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operand_display() {
        assert_eq!(Operand::Var(Var(3)).to_string(), "%3");
        assert_eq!(Operand::Const(Value::number(42.0)).to_string(), "42");
        assert_eq!(Operand::Const(Value::string("hello")).to_string(), "\"hello\"");
        assert_eq!(Operand::Block(BlockId(1)).to_string(), "@B1");
        assert_eq!(Operand::Func(FuncId(0)).to_string(), "fn#0");
    }

    #[test]
    fn from_conversions() {
        let _: Operand = Var(0).into();
        let _: Operand = Value::number(1.0).into();
        let _: Operand = BlockId(2).into();
        let _: Operand = FuncId(3).into();
    }

    #[test]
    fn source_loc_display() {
        assert_eq!(SourceLoc::new(100).to_string(), "pc=100");
        assert_eq!(SourceLoc::with_opcode(42, 0x66).to_string(), "pc=42 op=0x66");
    }
}

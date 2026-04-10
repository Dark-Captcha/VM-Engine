//! OpCode and Terminator definitions.
//!
//! 47 universal operations organized into four categories.
//! Terminators are separate — they end blocks, instructions do not.

// ============================================================================
// Imports
// ============================================================================

use std::fmt;

use crate::value::Value;

use super::{BlockId, Var};

// ============================================================================
// OpCode
// ============================================================================

/// The operation an instruction performs. Four categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpCode {
    // ── Pure: value in, value out, no side effects ──────────
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    BitNot,
    Eq,
    Neq,
    StrictEq,
    StrictNeq,
    Lt,
    Gt,
    Lte,
    Gte,
    Neg,
    Pos,
    LogicalNot,
    TypeOf,
    Void,

    // ── Memory: reads or writes state ───────────────────────
    /// `%r = obj[key]`
    LoadProp,
    /// `obj[key] = val` (void)
    StoreProp,
    /// `delete obj[key]`
    DeleteProp,
    /// `key in obj`
    HasProp,
    /// `%r = arr[index]` — fast numeric index
    LoadIndex,
    /// `arr[index] = val` (void)
    StoreIndex,
    /// `%r = scope_lookup(name)`
    LoadScope,
    /// `scope_set(name, val)` (void)
    StoreScope,
    /// `%r = {}`
    NewObject,
    /// `%r = []`
    NewArray,

    // ── Control: invokes callable ───────────────────────────
    /// `%r = func(args...)`
    Call,
    /// `%r = obj.method(args...)`
    CallMethod,

    // ── Data: define or select values ───────────────────────
    /// `%r = literal`
    Const,
    /// `%r = function parameter N`
    Param,
    /// `%r = merge([@blockA: %x], [@blockB: %y])`
    Phi,
    /// `%r = %other` — explicit copy
    Move,
}

impl OpCode {
    /// Which category this opcode belongs to.
    pub fn category(&self) -> OpCategory {
        match self {
            Self::Add | Self::Sub | Self::Mul | Self::Div | Self::Mod | Self::Pow
            | Self::BitAnd | Self::BitOr | Self::BitXor | Self::Shl | Self::Shr
            | Self::UShr | Self::BitNot | Self::Eq | Self::Neq | Self::StrictEq
            | Self::StrictNeq | Self::Lt | Self::Gt | Self::Lte | Self::Gte
            | Self::Neg | Self::Pos | Self::LogicalNot | Self::TypeOf | Self::Void => {
                OpCategory::Pure
            }
            Self::LoadProp | Self::StoreProp | Self::DeleteProp | Self::HasProp
            | Self::LoadIndex | Self::StoreIndex | Self::LoadScope | Self::StoreScope
            | Self::NewObject | Self::NewArray => OpCategory::Memory,
            Self::Call | Self::CallMethod => OpCategory::Control,
            Self::Const | Self::Param | Self::Phi | Self::Move => OpCategory::Data,
        }
    }

    /// Whether this opcode produces a result value.
    pub fn has_result(&self) -> bool {
        !matches!(self, Self::StoreProp | Self::StoreIndex | Self::StoreScope)
    }
}

/// Opcode category for classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpCategory {
    Pure,
    Memory,
    Control,
    Data,
}

impl fmt::Display for OpCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Add => "add", Self::Sub => "sub", Self::Mul => "mul",
            Self::Div => "div", Self::Mod => "mod", Self::Pow => "pow",
            Self::BitAnd => "bit_and", Self::BitOr => "bit_or",
            Self::BitXor => "bit_xor", Self::Shl => "shl", Self::Shr => "shr",
            Self::UShr => "ushr", Self::BitNot => "bit_not",
            Self::Eq => "eq", Self::Neq => "neq",
            Self::StrictEq => "strict_eq", Self::StrictNeq => "strict_neq",
            Self::Lt => "lt", Self::Gt => "gt", Self::Lte => "lte", Self::Gte => "gte",
            Self::Neg => "neg", Self::Pos => "pos",
            Self::LogicalNot => "not", Self::TypeOf => "typeof", Self::Void => "void",
            Self::LoadProp => "load_prop", Self::StoreProp => "store_prop",
            Self::DeleteProp => "delete_prop", Self::HasProp => "has_prop",
            Self::LoadIndex => "load_index", Self::StoreIndex => "store_index",
            Self::LoadScope => "load_scope", Self::StoreScope => "store_scope",
            Self::NewObject => "new_object", Self::NewArray => "new_array",
            Self::Call => "call", Self::CallMethod => "call_method",
            Self::Const => "const", Self::Param => "param",
            Self::Phi => "phi", Self::Move => "move",
        };
        write!(f, "{s}")
    }
}

// ============================================================================
// Terminator
// ============================================================================

/// How a block ends. Determines which block executes next.
#[derive(Debug, Clone)]
pub enum Terminator {
    /// Unconditional jump.
    Jump { target: BlockId },
    /// Two-way conditional branch.
    BranchIf {
        cond: Var,
        if_true: BlockId,
        if_false: BlockId,
    },
    /// Multi-way branch (switch/case).
    Switch {
        value: Var,
        cases: Vec<(Value, BlockId)>,
        default: BlockId,
    },
    /// Return from function.
    Return { value: Option<Var> },
    /// Stop execution.
    Halt,
    /// Raise exception.
    Throw { value: Var },
    /// Dead code — block should never be reached.
    Unreachable,
}

impl Terminator {
    /// All successor block IDs.
    pub fn targets(&self) -> Vec<BlockId> {
        match self {
            Self::Jump { target } => vec![*target],
            Self::BranchIf { if_true, if_false, .. } => vec![*if_true, *if_false],
            Self::Switch { cases, default, .. } => {
                let mut targets: Vec<BlockId> = cases.iter().map(|(_, b)| *b).collect();
                targets.push(*default);
                targets
            }
            Self::Return { .. } | Self::Halt | Self::Throw { .. } | Self::Unreachable => vec![],
        }
    }

    /// Whether execution can fall through to a successor block.
    pub fn can_continue(&self) -> bool {
        !matches!(self, Self::Return { .. } | Self::Halt | Self::Throw { .. } | Self::Unreachable)
    }
}

impl fmt::Display for Terminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Jump { target } => write!(f, "jump {target}"),
            Self::BranchIf { cond, if_true, if_false } => {
                write!(f, "branch_if {cond}, {if_true}, {if_false}")
            }
            Self::Switch { value, cases, default } => {
                write!(f, "switch {value} [")?;
                for (i, (val, target)) in cases.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{val} => {target}")?;
                }
                write!(f, "] default {default}")
            }
            Self::Return { value: Some(v) } => write!(f, "return {v}"),
            Self::Return { value: None } => write!(f, "return"),
            Self::Halt => write!(f, "halt"),
            Self::Throw { value } => write!(f, "throw {value}"),
            Self::Unreachable => write!(f, "unreachable"),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_categories() {
        assert_eq!(OpCode::Add.category(), OpCategory::Pure);
        assert_eq!(OpCode::BitXor.category(), OpCategory::Pure);
        assert_eq!(OpCode::LoadProp.category(), OpCategory::Memory);
        assert_eq!(OpCode::StoreProp.category(), OpCategory::Memory);
        assert_eq!(OpCode::Call.category(), OpCategory::Control);
        assert_eq!(OpCode::Const.category(), OpCategory::Data);
        assert_eq!(OpCode::Phi.category(), OpCategory::Data);
    }

    #[test]
    fn void_opcodes_have_no_result() {
        assert!(!OpCode::StoreProp.has_result());
        assert!(!OpCode::StoreIndex.has_result());
        assert!(!OpCode::StoreScope.has_result());
        assert!(OpCode::Add.has_result());
        assert!(OpCode::Const.has_result());
    }

    #[test]
    fn terminator_targets() {
        let t = Terminator::BranchIf {
            cond: Var(0),
            if_true: BlockId(1),
            if_false: BlockId(2),
        };
        assert_eq!(t.targets(), vec![BlockId(1), BlockId(2)]);
        assert!(t.can_continue());

        assert!(Terminator::Halt.targets().is_empty());
        assert!(!Terminator::Halt.can_continue());
    }

    #[test]
    fn terminator_display() {
        let t = Terminator::Jump { target: BlockId(5) };
        assert_eq!(t.to_string(), "jump @B5");

        let t = Terminator::Return { value: Some(Var(3)) };
        assert_eq!(t.to_string(), "return %3");

        assert_eq!(Terminator::Halt.to_string(), "halt");
    }

    #[test]
    fn opcode_display() {
        assert_eq!(OpCode::BitXor.to_string(), "bit_xor");
        assert_eq!(OpCode::LoadProp.to_string(), "load_prop");
        assert_eq!(OpCode::Call.to_string(), "call");
    }
}

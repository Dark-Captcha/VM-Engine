//! CFG block and edge types.

// ============================================================================
// Imports
// ============================================================================

use crate::ir::BlockId;
use crate::value::Value;

// ============================================================================
// Types
// ============================================================================

/// A block in the control flow graph (analysis view).
///
/// Unlike [`ir::Block`](crate::ir::Block) which holds IR instructions,
/// `CfgBlock` holds analysis metadata: predecessor list, instruction count,
/// and label. Used by dominator computation, loop detection, and structure recovery.
#[derive(Debug, Clone)]
pub struct CfgBlock {
    pub id: BlockId,
    pub label: String,
    pub instruction_count: usize,
    pub predecessors: Vec<BlockId>,
}

/// A directed edge in the CFG.
#[derive(Debug, Clone)]
pub struct Edge {
    pub from: BlockId,
    pub to: BlockId,
    pub kind: EdgeKind,
}

/// Classification of a CFG edge.
#[derive(Debug, Clone)]
pub enum EdgeKind {
    /// Unconditional jump.
    Jump,
    /// Taken when condition is true.
    TrueBranch,
    /// Taken when condition is false.
    FalseBranch,
    /// Switch case with a specific value.
    SwitchCase(Value),
    /// Switch default.
    SwitchDefault,
    /// Exception handler edge.
    Exception,
}

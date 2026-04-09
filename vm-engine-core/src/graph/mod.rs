//! Control flow analysis built from the IR.
//!
//! Provides CFG construction, dominator computation, loop detection, and
//! call graph building. All analysis is automatic — pass an IR function,
//! get a complete analysis back.

pub mod block;
pub mod callgraph;
pub mod dominator;
pub mod loops;

// ============================================================================
// Imports
// ============================================================================

use std::collections::BTreeMap;

use crate::ir::{BlockId, Function};

use block::{CfgBlock, Edge, EdgeKind};

// ============================================================================
// CFG
// ============================================================================

/// Control flow graph for a single function.
#[derive(Debug, Clone)]
pub struct Cfg {
    pub blocks: Vec<CfgBlock>,
    pub edges: Vec<Edge>,
    pub entry: BlockId,
    /// Map from BlockId to index in `blocks`.
    block_index: BTreeMap<BlockId, usize>,
}

impl Cfg {
    /// Get a block by ID.
    pub fn block(&self, id: BlockId) -> Option<&CfgBlock> {
        self.block_index.get(&id).map(|&i| &self.blocks[i])
    }

    /// All successor block IDs for a given block.
    pub fn successors(&self, id: BlockId) -> Vec<BlockId> {
        self.edges.iter()
            .filter(|e| e.from == id)
            .map(|e| e.to)
            .collect()
    }

    /// All predecessor block IDs for a given block.
    pub fn predecessors(&self, id: BlockId) -> Vec<BlockId> {
        self.edges.iter()
            .filter(|e| e.to == id)
            .map(|e| e.from)
            .collect()
    }

    /// All block IDs in the CFG.
    pub fn block_ids(&self) -> Vec<BlockId> {
        self.blocks.iter().map(|b| b.id).collect()
    }

    /// Number of blocks.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

// ============================================================================
// Build CFG
// ============================================================================

/// Build a CFG from an IR function.
///
/// Each IR [`Block`](crate::ir::Block) becomes a [`CfgBlock`]. Each terminator
/// produces edges. Predecessor lists are filled automatically.
pub fn build_cfg(func: &Function) -> Cfg {
    let mut blocks: Vec<CfgBlock> = Vec::new();
    let mut block_index: BTreeMap<BlockId, usize> = BTreeMap::new();
    let mut edges: Vec<Edge> = Vec::new();

    // Create CfgBlocks
    for (i, ir_block) in func.blocks.iter().enumerate() {
        block_index.insert(ir_block.id, i);
        blocks.push(CfgBlock {
            id: ir_block.id,
            label: ir_block.label.clone(),
            instruction_count: ir_block.body.len(),
            predecessors: Vec::new(),
        });
    }

    // Build edges from terminators
    for ir_block in &func.blocks {
        let from = ir_block.id;
        match &ir_block.terminator {
            crate::ir::Terminator::Jump { target } => {
                edges.push(Edge { from, to: *target, kind: EdgeKind::Jump });
            }
            crate::ir::Terminator::BranchIf { if_true, if_false, .. } => {
                edges.push(Edge { from, to: *if_true, kind: EdgeKind::TrueBranch });
                edges.push(Edge { from, to: *if_false, kind: EdgeKind::FalseBranch });
            }
            crate::ir::Terminator::Switch { cases, default, .. } => {
                for (val, target) in cases {
                    edges.push(Edge {
                        from,
                        to: *target,
                        kind: EdgeKind::SwitchCase(val.clone()),
                    });
                }
                edges.push(Edge { from, to: *default, kind: EdgeKind::SwitchDefault });
            }
            crate::ir::Terminator::Return { .. }
            | crate::ir::Terminator::Halt
            | crate::ir::Terminator::Throw { .. }
            | crate::ir::Terminator::Unreachable => {}
        }
    }

    // Fill predecessors
    for edge in &edges {
        if let Some(&idx) = block_index.get(&edge.to) {
            blocks[idx].predecessors.push(edge.from);
        }
    }

    Cfg {
        blocks,
        edges,
        entry: func.entry,
        block_index,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::IrBuilder;

    #[test]
    fn linear_cfg() {
        let mut b = IrBuilder::new();
        b.begin_function("linear");
        b.create_and_switch("entry");
        b.halt();
        b.end_function();

        let module = b.build();
        let cfg = build_cfg(&module.functions[0]);

        assert_eq!(cfg.len(), 1);
        assert!(cfg.edges.is_empty());
        assert_eq!(cfg.entry, BlockId(0));
    }

    #[test]
    fn branch_cfg() {
        let mut b = IrBuilder::new();
        b.begin_function("branch");
        let entry = b.create_and_switch("entry");
        let cond = b.const_bool(true);
        let yes = b.create_block("yes");
        let no = b.create_block("no");
        b.switch_to(entry);
        b.branch_if(cond, yes, no);

        b.switch_to(yes);
        b.halt();
        b.switch_to(no);
        b.halt();
        b.end_function();

        let module = b.build();
        let cfg = build_cfg(&module.functions[0]);

        assert_eq!(cfg.len(), 3);
        assert_eq!(cfg.edges.len(), 2);
        assert_eq!(cfg.successors(entry), vec![yes, no]);
        assert_eq!(cfg.predecessors(yes), vec![entry]);
        assert_eq!(cfg.predecessors(no), vec![entry]);
    }

    #[test]
    fn diamond_cfg() {
        let mut b = IrBuilder::new();
        b.begin_function("diamond");

        let entry = b.create_and_switch("entry");
        let cond = b.const_bool(true);
        let left = b.create_block("left");
        let right = b.create_block("right");
        let merge = b.create_block("merge");

        b.switch_to(entry);
        b.branch_if(cond, left, right);

        b.switch_to(left);
        b.jump(merge);

        b.switch_to(right);
        b.jump(merge);

        b.switch_to(merge);
        b.halt();

        b.end_function();

        let module = b.build();
        let cfg = build_cfg(&module.functions[0]);

        assert_eq!(cfg.len(), 4);
        assert_eq!(cfg.edges.len(), 4); // 2 from branch + 2 jumps to merge
        let merge_preds = cfg.predecessors(merge);
        assert_eq!(merge_preds.len(), 2);
        assert!(merge_preds.contains(&left));
        assert!(merge_preds.contains(&right));
    }

    #[test]
    fn loop_cfg() {
        let mut b = IrBuilder::new();
        b.begin_function("loop");

        let header = b.create_and_switch("header");
        let cond = b.const_bool(true);
        let body = b.create_block("body");
        let exit = b.create_block("exit");

        b.switch_to(header);
        b.branch_if(cond, body, exit);

        b.switch_to(body);
        b.jump(header); // back edge

        b.switch_to(exit);
        b.halt();
        b.end_function();

        let module = b.build();
        let cfg = build_cfg(&module.functions[0]);

        assert_eq!(cfg.len(), 3);
        // header → body (true), header → exit (false), body → header (back)
        assert_eq!(cfg.edges.len(), 3);
        assert!(cfg.predecessors(header).contains(&body)); // back edge
    }
}

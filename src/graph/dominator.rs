//! Dominator and post-dominator tree computation.
//!
//! Uses the Cooper-Harvey-Kennedy iterative algorithm. Efficient for
//! the CFG sizes encountered in anti-bot VMs (typically < 1000 blocks).

// ============================================================================
// Imports
// ============================================================================

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::ir::BlockId;

use super::Cfg;

// ============================================================================
// Dominator Tree
// ============================================================================

/// Immediate dominator map. `idom[B]` = the block that immediately dominates B.
#[derive(Debug, Clone)]
pub struct DominatorTree {
    pub idom: BTreeMap<BlockId, BlockId>,
    pub entry: BlockId,
}

impl DominatorTree {
    /// Does block `a` dominate block `b`?
    ///
    /// A block dominates itself. A block dominates B if it is on every path
    /// from the entry to B.
    pub fn dominates(&self, a: BlockId, b: BlockId) -> bool {
        if a == b {
            return true;
        }
        let mut cur = b;
        while let Some(&parent) = self.idom.get(&cur) {
            if parent == a {
                return true;
            }
            if parent == cur {
                break; // reached root
            }
            cur = parent;
        }
        false
    }

    /// All blocks dominated by `a` (including `a` itself).
    pub fn dominated_by(&self, a: BlockId, all_blocks: &[BlockId]) -> Vec<BlockId> {
        all_blocks.iter()
            .filter(|&&b| self.dominates(a, b))
            .copied()
            .collect()
    }
}

// ============================================================================
// Compute dominators
// ============================================================================

/// Compute the dominator tree for a CFG using the Cooper-Harvey-Kennedy algorithm.
pub fn compute_dominators(cfg: &Cfg) -> DominatorTree {
    let postorder = dfs_postorder(cfg, cfg.entry);
    let rpo: Vec<BlockId> = postorder.iter().rev().copied().collect();

    let mut rpo_index: HashMap<BlockId, usize> = HashMap::new();
    for (i, &b) in rpo.iter().enumerate() {
        rpo_index.insert(b, i);
    }

    let mut idom: HashMap<BlockId, BlockId> = HashMap::new();
    idom.insert(cfg.entry, cfg.entry);

    let mut changed = true;
    while changed {
        changed = false;
        for &b in &rpo {
            if b == cfg.entry {
                continue;
            }
            let preds = cfg.predecessors(b);
            let mut new_idom: Option<BlockId> = None;

            for pred in &preds {
                if !idom.contains_key(pred) {
                    continue;
                }
                new_idom = Some(match new_idom {
                    None => *pred,
                    Some(current) => intersect(&idom, &rpo_index, current, *pred),
                });
            }

            if let Some(new) = new_idom
                && idom.get(&b) != Some(&new)
            {
                idom.insert(b, new);
                changed = true;
            }
        }
    }

    idom.remove(&cfg.entry);

    DominatorTree {
        idom: idom.into_iter().collect(),
        entry: cfg.entry,
    }
}

// ============================================================================
// Compute post-dominators
// ============================================================================

/// Compute immediate post-dominators.
///
/// Builds a reversed CFG with a synthetic exit node connected to all
/// halt/return blocks, then runs the dominator algorithm on it.
pub fn compute_post_dominators(cfg: &Cfg) -> BTreeMap<BlockId, BlockId> {
    let max_id = cfg.blocks.iter().map(|b| b.id.0).max().unwrap_or(0);
    let virtual_exit = BlockId(max_id + 1);

    // Build reversed graph adjacency
    let mut rev_succs: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
    let mut rev_preds: HashMap<BlockId, Vec<BlockId>> = HashMap::new();

    for block in &cfg.blocks {
        rev_succs.entry(block.id).or_default();
        rev_preds.entry(block.id).or_default();
    }
    rev_succs.entry(virtual_exit).or_default();
    rev_preds.entry(virtual_exit).or_default();

    // Reverse all original edges
    for edge in &cfg.edges {
        rev_succs.entry(edge.to).or_default().push(edge.from);
        rev_preds.entry(edge.from).or_default().push(edge.to);
    }

    // Connect virtual_exit to blocks with no successors in original CFG
    let blocks_with_succs: BTreeSet<BlockId> = cfg.edges.iter().map(|e| e.from).collect();
    for block in &cfg.blocks {
        if !blocks_with_succs.contains(&block.id) || cfg.successors(block.id).is_empty() {
            rev_succs.entry(virtual_exit).or_default().push(block.id);
            rev_preds.entry(block.id).or_default().push(virtual_exit);
        }
    }

    // DFS postorder on reversed graph
    let postorder = dfs_postorder_generic(virtual_exit, &rev_succs);
    let rpo: Vec<BlockId> = postorder.iter().rev().copied().collect();

    let mut rpo_index: HashMap<BlockId, usize> = HashMap::new();
    for (i, &b) in rpo.iter().enumerate() {
        rpo_index.insert(b, i);
    }

    // Dominator on reversed graph
    let mut idom: HashMap<BlockId, BlockId> = HashMap::new();
    idom.insert(virtual_exit, virtual_exit);

    let mut changed = true;
    while changed {
        changed = false;
        for &b in &rpo {
            if b == virtual_exit {
                continue;
            }
            let preds = rev_preds.get(&b).cloned().unwrap_or_default();
            let mut new_idom: Option<BlockId> = None;

            for pred in &preds {
                if !idom.contains_key(pred) {
                    continue;
                }
                new_idom = Some(match new_idom {
                    None => *pred,
                    Some(current) => intersect(&idom, &rpo_index, current, *pred),
                });
            }

            if let Some(new) = new_idom
                && idom.get(&b) != Some(&new)
            {
                idom.insert(b, new);
                changed = true;
            }
        }
    }

    // Filter out virtual_exit references
    idom.into_iter()
        .filter(|(k, v)| *k != virtual_exit && *v != virtual_exit)
        .collect()
}

// ============================================================================
// Helpers
// ============================================================================

fn intersect(
    idom: &HashMap<BlockId, BlockId>,
    rpo_index: &HashMap<BlockId, usize>,
    mut a: BlockId,
    mut b: BlockId,
) -> BlockId {
    while a != b {
        let ai = rpo_index.get(&a).copied().unwrap_or(0);
        let bi = rpo_index.get(&b).copied().unwrap_or(0);
        if ai > bi {
            a = *idom.get(&a).unwrap_or(&a);
        } else {
            b = *idom.get(&b).unwrap_or(&b);
        }
    }
    a
}

fn dfs_postorder(cfg: &Cfg, start: BlockId) -> Vec<BlockId> {
    let mut succs: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
    for edge in &cfg.edges {
        succs.entry(edge.from).or_default().push(edge.to);
    }
    dfs_postorder_generic(start, &succs)
}

fn dfs_postorder_generic(
    start: BlockId,
    succs: &HashMap<BlockId, Vec<BlockId>>,
) -> Vec<BlockId> {
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    dfs_visit(start, succs, &mut visited, &mut order);
    order
}

fn dfs_visit(
    node: BlockId,
    succs: &HashMap<BlockId, Vec<BlockId>>,
    visited: &mut BTreeSet<BlockId>,
    order: &mut Vec<BlockId>,
) {
    if !visited.insert(node) {
        return;
    }
    if let Some(children) = succs.get(&node) {
        for &child in children {
            dfs_visit(child, succs, visited, order);
        }
    }
    order.push(node);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::IrBuilder;
    use crate::graph::build_cfg;

    #[test]
    fn linear_dominators() {
        let mut b = IrBuilder::new();
        b.begin_function("linear");
        let entry = b.create_and_switch("entry");
        let second = b.create_block("second");
        b.jump(second);
        b.switch_to(second);
        b.halt();
        b.end_function();

        let module = b.build();
        let cfg = build_cfg(&module.functions[0]);
        let dom = compute_dominators(&cfg);

        assert!(dom.dominates(entry, second));
        assert!(!dom.dominates(second, entry));
    }

    #[test]
    fn diamond_dominators() {
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
        let dom = compute_dominators(&cfg);

        // Entry dominates everything
        assert!(dom.dominates(entry, left));
        assert!(dom.dominates(entry, right));
        assert!(dom.dominates(entry, merge));

        // Arms don't dominate each other
        assert!(!dom.dominates(left, right));
        assert!(!dom.dominates(right, left));

        // Arms don't dominate merge (either path leads there)
        assert!(!dom.dominates(left, merge));
        assert!(!dom.dominates(right, merge));
    }

    #[test]
    fn post_dominators_diamond() {
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
        let pdom = compute_post_dominators(&cfg);

        // Merge post-dominates entry (every path from entry reaches merge)
        assert_eq!(pdom.get(&entry), Some(&merge));
        // Merge post-dominates left and right
        assert_eq!(pdom.get(&left), Some(&merge));
        assert_eq!(pdom.get(&right), Some(&merge));
    }

    #[test]
    fn self_dominates() {
        let mut b = IrBuilder::new();
        b.begin_function("single");
        b.create_and_switch("entry");
        b.halt();
        b.end_function();

        let module = b.build();
        let cfg = build_cfg(&module.functions[0]);
        let dom = compute_dominators(&cfg);

        assert!(dom.dominates(BlockId(0), BlockId(0)));
    }
}

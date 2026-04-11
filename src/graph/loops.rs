//! Natural loop detection using dominator analysis.
//!
//! A natural loop has a single header block that dominates all blocks
//! in the loop body. The loop is identified by back edges: edges where
//! the target dominates the source.

// ============================================================================
// Imports
// ============================================================================

use std::collections::BTreeSet;

use crate::ir::BlockId;

use super::Cfg;
use super::dominator::DominatorTree;

// ============================================================================
// Types
// ============================================================================

/// Unique loop identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LoopId(pub u32);

/// Information about a detected natural loop.
#[derive(Debug, Clone)]
pub struct LoopInfo {
    pub id: LoopId,
    /// The header block — dominates all blocks in the loop body.
    pub header: BlockId,
    /// All blocks that are part of the loop (including the header).
    pub body: BTreeSet<BlockId>,
    /// Back edges: `(source, target)` where target == header.
    pub back_edges: Vec<(BlockId, BlockId)>,
    /// Blocks that have an edge leaving the loop (break targets).
    pub exits: Vec<BlockId>,
    /// Enclosing loop, if this is a nested inner loop.
    pub parent: Option<LoopId>,
    /// Nesting depth. 0 = outermost.
    pub depth: usize,
}

/// All loops detected in a function.
#[derive(Debug, Clone)]
pub struct LoopForest {
    pub loops: Vec<LoopInfo>,
}

impl LoopForest {
    /// Find the innermost loop containing a block.
    pub fn loop_for_block(&self, block: BlockId) -> Option<&LoopInfo> {
        // Return the deepest loop that contains this block
        self.loops.iter()
            .filter(|l| l.body.contains(&block))
            .max_by_key(|l| l.depth)
    }

    /// Find a loop by header block.
    pub fn loop_for_header(&self, header: BlockId) -> Option<&LoopInfo> {
        self.loops.iter().find(|l| l.header == header)
    }

    /// True if no loops were detected.
    pub fn is_empty(&self) -> bool {
        self.loops.is_empty()
    }

    /// Number of loops.
    pub fn len(&self) -> usize {
        self.loops.len()
    }
}

// ============================================================================
// Detection
// ============================================================================

/// Detect all natural loops in a CFG.
///
/// Algorithm:
/// 1. Find all back edges (edge where target dominates source).
/// 2. For each back edge, collect the natural loop body.
/// 3. Compute exits — blocks with edges leaving the loop.
/// 4. Build nesting hierarchy.
pub fn detect_loops(cfg: &Cfg, dom: &DominatorTree) -> LoopForest {
    // Step 1: Find back edges
    let mut back_edges: Vec<(BlockId, BlockId)> = Vec::new();
    for edge in &cfg.edges {
        if dom.dominates(edge.to, edge.from) {
            back_edges.push((edge.from, edge.to));
        }
    }

    // Step 2: Group back edges by header and collect loop bodies
    let mut loops: Vec<LoopInfo> = Vec::new();
    let mut header_to_loop: std::collections::HashMap<BlockId, usize> =
        std::collections::HashMap::new();

    for &(source, header) in &back_edges {
        if let Some(&loop_idx) = header_to_loop.get(&header) {
            // Add to existing loop
            let body = collect_loop_body(cfg, header, source, dom);
            loops[loop_idx].body.extend(body);
            loops[loop_idx].back_edges.push((source, header));
        } else {
            // New loop
            let body = collect_loop_body(cfg, header, source, dom);
            let id = LoopId(loops.len() as u32);
            header_to_loop.insert(header, loops.len());
            loops.push(LoopInfo {
                id,
                header,
                body,
                back_edges: vec![(source, header)],
                exits: Vec::new(),
                parent: None,
                depth: 0,
            });
        }
    }

    // Step 3: Compute exit blocks
    for loop_info in &mut loops {
        for &block_id in &loop_info.body {
            for succ in cfg.successors(block_id) {
                if !loop_info.body.contains(&succ) {
                    loop_info.exits.push(succ);
                }
            }
        }
        loop_info.exits.sort();
        loop_info.exits.dedup();
    }

    // Step 4: Build nesting hierarchy
    // A loop L1 is nested inside L2 if L1.header is in L2.body and L1 != L2
    let loop_count = loops.len();
    for i in 0..loop_count {
        let header_i = loops[i].header;
        let mut best_parent: Option<(usize, usize)> = None; // (loop_idx, body_size)

        for (j, other_loop) in loops.iter().enumerate() {
            if i == j {
                continue;
            }
            if other_loop.body.contains(&header_i) {
                let size = other_loop.body.len();
                if best_parent.is_none_or(|(_, best_size)| size < best_size) {
                    best_parent = Some((j, size));
                }
            }
        }

        if let Some((parent_idx, _)) = best_parent {
            loops[i].parent = Some(loops[parent_idx].id);
        }
    }

    // Compute depths
    for i in 0..loop_count {
        let mut depth = 0;
        let mut cur = loops[i].parent;
        while let Some(pid) = cur {
            depth += 1;
            cur = loops.iter().find(|l| l.id == pid).and_then(|l| l.parent);
        }
        loops[i].depth = depth;
    }

    LoopForest { loops }
}

/// Collect the natural loop body for a back edge `source → header`.
///
/// The body includes all blocks that are dominated by `header` AND can reach
/// `source` without going through `header`. This matches the classical natural
/// loop definition (Aho/Sethi/Ullman, Dragon Book).
///
/// The dominator check prevents over-inclusion: blocks not dominated by the
/// header cannot be part of the natural loop (they belong to an outer or
/// unrelated region).
fn collect_loop_body(
    cfg: &Cfg,
    header: BlockId,
    back_edge_source: BlockId,
    dom: &DominatorTree,
) -> BTreeSet<BlockId> {
    let mut body = BTreeSet::new();
    body.insert(header);

    if header == back_edge_source {
        return body; // self-loop
    }

    body.insert(back_edge_source);
    let mut worklist = vec![back_edge_source];

    while let Some(block) = worklist.pop() {
        for pred in cfg.predecessors(block) {
            // Only include predecessors that are dominated by the header.
            // This ensures we only capture the natural loop, not unrelated
            // regions that happen to have a path to the back-edge source.
            if pred == header || !dom.dominates(header, pred) {
                continue;
            }
            if body.insert(pred) {
                worklist.push(pred);
            }
        }
    }

    body
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::IrBuilder;
    use crate::graph::{build_cfg, dominator::compute_dominators};

    #[test]
    fn no_loops_in_linear_cfg() {
        let mut b = IrBuilder::new();
        b.begin_function("linear");
        b.create_and_switch("entry");
        b.halt();
        b.end_function();

        let module = b.build();
        let cfg = build_cfg(&module.functions[0]);
        let dom = compute_dominators(&cfg);
        let forest = detect_loops(&cfg, &dom);

        assert!(forest.is_empty());
    }

    #[test]
    fn simple_while_loop() {
        let mut b = IrBuilder::new();
        b.begin_function("while");

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
        let dom = compute_dominators(&cfg);
        let forest = detect_loops(&cfg, &dom);

        assert_eq!(forest.len(), 1);
        let loop0 = &forest.loops[0];
        assert_eq!(loop0.header, header);
        assert!(loop0.body.contains(&header));
        assert!(loop0.body.contains(&body));
        assert!(!loop0.body.contains(&exit));
        assert_eq!(loop0.exits, vec![exit]);
        assert_eq!(loop0.back_edges, vec![(body, header)]);
        assert!(loop0.parent.is_none());
        assert_eq!(loop0.depth, 0);
    }

    #[test]
    fn nested_loops() {
        let mut b = IrBuilder::new();
        b.begin_function("nested");

        let outer_header = b.create_and_switch("outer_header");
        let c1 = b.const_bool(true);
        let inner_header = b.create_block("inner_header");
        let inner_body = b.create_block("inner_body");
        let outer_latch = b.create_block("outer_latch");
        let exit = b.create_block("exit");

        b.switch_to(outer_header);
        b.branch_if(c1, inner_header, exit);

        b.switch_to(inner_header);
        let c2 = b.const_bool(true);
        b.branch_if(c2, inner_body, outer_latch);

        b.switch_to(inner_body);
        b.jump(inner_header); // inner back edge

        b.switch_to(outer_latch);
        b.jump(outer_header); // outer back edge

        b.switch_to(exit);
        b.halt();
        b.end_function();

        let module = b.build();
        let cfg = build_cfg(&module.functions[0]);
        let dom = compute_dominators(&cfg);
        let forest = detect_loops(&cfg, &dom);

        assert_eq!(forest.len(), 2);

        let outer = forest.loop_for_header(outer_header).expect("outer loop");
        let inner = forest.loop_for_header(inner_header).expect("inner loop");

        assert!(outer.body.contains(&inner_header));
        assert!(inner.parent.is_some());
        assert_eq!(inner.depth, 1);
        assert_eq!(outer.depth, 0);
    }

    #[test]
    fn self_loop() {
        let mut b = IrBuilder::new();
        b.begin_function("self_loop");

        let header = b.create_and_switch("header");
        let cond = b.const_bool(true);
        let exit = b.create_block("exit");

        b.switch_to(header);
        b.branch_if(cond, header, exit); // self-loop back edge

        b.switch_to(exit);
        b.halt();
        b.end_function();

        let module = b.build();
        let cfg = build_cfg(&module.functions[0]);
        let dom = compute_dominators(&cfg);
        let forest = detect_loops(&cfg, &dom);

        assert_eq!(forest.len(), 1);
        let loop0 = &forest.loops[0];
        assert_eq!(loop0.header, header);
        assert_eq!(loop0.body.len(), 1); // just the header itself
    }

    #[test]
    fn loop_for_block_finds_innermost() {
        let mut b = IrBuilder::new();
        b.begin_function("nested");

        let outer_header = b.create_and_switch("outer_header");
        let c1 = b.const_bool(true);
        let inner_header = b.create_block("inner_header");
        let inner_body = b.create_block("inner_body");
        let outer_latch = b.create_block("outer_latch");
        let exit = b.create_block("exit");

        b.switch_to(outer_header);
        b.branch_if(c1, inner_header, exit);
        b.switch_to(inner_header);
        let c2 = b.const_bool(true);
        b.branch_if(c2, inner_body, outer_latch);
        b.switch_to(inner_body);
        b.jump(inner_header);
        b.switch_to(outer_latch);
        b.jump(outer_header);
        b.switch_to(exit);
        b.halt();
        b.end_function();

        let module = b.build();
        let cfg = build_cfg(&module.functions[0]);
        let dom = compute_dominators(&cfg);
        let forest = detect_loops(&cfg, &dom);

        // inner_body should be in the inner loop
        let found = forest.loop_for_block(inner_body).expect("should find loop");
        assert_eq!(found.header, inner_header);
    }
}

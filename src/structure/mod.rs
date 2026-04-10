//! Structured control flow recovery.
//!
//! Turns a CFG into nested if/else, while, and function statements.
//! This is what makes output readable — without it, the result is a flat
//! list of labeled blocks with gotos.

pub mod ast;
pub mod expr;
pub mod region;
pub mod simplify;

// ============================================================================
// Imports
// ============================================================================

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::{BlockId, Function};
use crate::graph::Cfg;
use crate::graph::dominator::DominatorTree;
use crate::graph::loops::LoopForest;

use ast::Stmt;
use region::RecoverCtx;

// ============================================================================
// Public API
// ============================================================================

/// Recover structured control flow from an IR function.
///
/// Takes the function, its CFG, dominator tree, post-dominator map, and
/// loop forest. Returns a list of statements representing the function body.
///
/// # Example
///
/// ```
/// use vm_engine::ir::builder::IrBuilder;
/// use vm_engine::graph;
/// use vm_engine::structure;
///
/// let mut b = IrBuilder::new();
/// b.begin_function("example");
/// b.create_and_switch("entry");
/// let v = b.const_number(42.0);
/// b.ret(Some(v));
/// b.end_function();
///
/// let module = b.build();
/// let func = &module.functions[0];
/// let cfg = graph::build_cfg(func);
/// let dom = graph::dominator::compute_dominators(&cfg);
/// let pdom = graph::dominator::compute_post_dominators(&cfg);
/// let loops = graph::loops::detect_loops(&cfg, &dom);
///
/// let stmts = structure::recover(func, &cfg, &dom, &pdom, &loops);
/// let text: String = stmts.iter().map(|s| s.to_string()).collect();
/// assert!(text.contains("return"));
/// ```
pub fn recover(
    func: &Function,
    cfg: &Cfg,
    dom: &DominatorTree,
    pdom: &BTreeMap<BlockId, BlockId>,
    loops: &LoopForest,
) -> Vec<Stmt> {
    let ctx = RecoverCtx {
        func,
        cfg,
        dom,
        pdom,
        loops,
        use_counts: expr::count_uses(func),
        def_blocks: expr::var_def_blocks(func),
        var_names: expr::name_vars(func),
    };

    let mut visited = BTreeSet::new();
    let mut stmts = region::recover_region(func.entry, None, &ctx, &mut visited);

    // Apply simplification
    simplify::simplify(&mut stmts);

    stmts
}

/// Recover and format as a complete function string.
pub fn recover_to_string(
    func: &Function,
    cfg: &Cfg,
    dom: &DominatorTree,
    pdom: &BTreeMap<BlockId, BlockId>,
    loops: &LoopForest,
) -> String {
    let stmts = recover(func, cfg, dom, pdom, loops);

    let mut out = format!("function {}(", func.name);
    let var_names = expr::name_vars(func);
    for (i, param) in func.params.iter().enumerate() {
        if i > 0 { out.push_str(", "); }
        let name = var_names.get(param)
            .cloned()
            .unwrap_or_else(|| format!("arg{i}"));
        out.push_str(&name);
    }
    out.push_str(") {\n");

    for stmt in &stmts {
        // Add one level of indentation
        for line in stmt.to_string().lines() {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
    }

    out.push_str("}\n");
    out
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::IrBuilder;
    use crate::graph;

    fn full_recover(func: &Function) -> String {
        let cfg = graph::build_cfg(func);
        let dom = graph::dominator::compute_dominators(&cfg);
        let pdom = graph::dominator::compute_post_dominators(&cfg);
        let loops = graph::loops::detect_loops(&cfg, &dom);
        recover_to_string(func, &cfg, &dom, &pdom, &loops)
    }

    #[test]
    fn recover_simple_return() {
        let mut b = IrBuilder::new();
        b.begin_function("simple");
        b.create_and_switch("entry");
        let v = b.const_number(42.0);
        b.ret(Some(v));
        b.end_function();

        let text = full_recover(&b.build().functions[0]);
        assert!(text.contains("function simple()"), "got:\n{text}");
        assert!(text.contains("return"), "got:\n{text}");
    }

    #[test]
    fn recover_if_else_diamond() {
        let mut b = IrBuilder::new();
        b.begin_function("check");

        let entry = b.create_and_switch("entry");
        let cond = b.const_bool(true);
        let then_b = b.create_block("then");
        let else_b = b.create_block("else");
        let merge = b.create_block("merge");

        b.switch_to(entry);
        b.branch_if(cond, then_b, else_b);

        b.switch_to(then_b);
        let v1 = b.const_number(1.0);
        b.jump(merge);

        b.switch_to(else_b);
        let v2 = b.const_number(0.0);
        b.jump(merge);

        b.switch_to(merge);
        b.halt();
        b.end_function();

        let text = full_recover(&b.build().functions[0]);
        assert!(text.contains("if ("), "should have if statement:\n{text}");
    }

    #[test]
    fn recover_while_loop() {
        let mut b = IrBuilder::new();
        b.begin_function("cipher_loop");

        let header = b.create_and_switch("header");
        let cond = b.const_bool(true);
        let body = b.create_block("body");
        let exit = b.create_block("exit");

        b.switch_to(header);
        b.branch_if(cond, body, exit);

        b.switch_to(body);
        let _ = b.const_number(99.0);
        b.jump(header);

        b.switch_to(exit);
        b.ret(None);
        b.end_function();

        let text = full_recover(&b.build().functions[0]);
        assert!(text.contains("while ("), "should have while loop:\n{text}");
        assert!(text.contains("return"), "should have return after loop:\n{text}");
    }

    #[test]
    fn recover_nested_if_in_loop() {
        let mut b = IrBuilder::new();
        b.begin_function("nested");

        let header = b.create_and_switch("header");
        let loop_cond = b.const_bool(true);
        let body = b.create_block("body");
        let exit = b.create_block("exit");

        b.switch_to(header);
        b.branch_if(loop_cond, body, exit);

        // Body has an if/else inside
        b.switch_to(body);
        let inner_cond = b.const_bool(false);
        let then_inner = b.create_block("then_inner");
        let else_inner = b.create_block("else_inner");
        let merge_inner = b.create_block("merge_inner");

        b.switch_to(body);
        b.branch_if(inner_cond, then_inner, else_inner);

        b.switch_to(then_inner);
        let _ = b.const_number(1.0);
        b.jump(merge_inner);

        b.switch_to(else_inner);
        let _ = b.const_number(2.0);
        b.jump(merge_inner);

        b.switch_to(merge_inner);
        b.jump(header); // back edge

        b.switch_to(exit);
        b.halt();
        b.end_function();

        let text = full_recover(&b.build().functions[0]);
        assert!(text.contains("while ("), "should have while:\n{text}");
        assert!(text.contains("if ("), "should have nested if:\n{text}");
    }

    #[test]
    fn constant_folding_in_assignment() {
        let mut b = IrBuilder::new();
        b.begin_function("fold");
        b.create_and_switch("entry");
        // 185 ^ 171 — both consts used once, inline into xor, assigned to v2
        let a = b.const_number(185.0);
        let c = b.const_number(171.0);
        let r = b.bit_xor(a, c);
        b.ret(Some(r));
        b.end_function();

        let text = full_recover(&b.build().functions[0]);
        // v2 gets the folded constant (18) or the xor expression, return references v2
        // Copy propagation (inlining v2 into return) is a v2 optimization
        assert!(text.contains("return"), "should have return:\n{text}");
        assert!(text.contains("v2") || text.contains("18"),
            "should reference result variable:\n{text}");
    }
}

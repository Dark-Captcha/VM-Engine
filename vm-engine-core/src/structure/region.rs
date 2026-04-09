//! Region detection: identify if/else, while loops, and sequential flow
//! from CFG structure.
//!
//! Uses the dominator tree, post-dominator map, and loop forest to classify
//! each block's role in the control flow.

// ============================================================================
// Imports
// ============================================================================

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::{BlockId, Function};
use crate::ir::opcode::Terminator;

use crate::graph::Cfg;
use crate::graph::dominator::DominatorTree;
use crate::graph::loops::{LoopForest, LoopInfo};

use super::ast::{Expr, Stmt};
use super::expr;

// ============================================================================
// Recovery context
// ============================================================================

/// Shared context for the recursive structure recovery pass.
/// Shared context for the recursive structure recovery pass.
///
/// `cfg` and `dom` are reserved for v2 scoped region detection
/// (determining which blocks belong to then/else regions using dominance).
#[allow(dead_code)]
pub(crate) struct RecoverCtx<'a> {
    pub func: &'a Function,
    pub cfg: &'a Cfg,
    pub dom: &'a DominatorTree,
    pub pdom: &'a BTreeMap<BlockId, BlockId>,
    pub loops: &'a LoopForest,
    pub use_counts: std::collections::HashMap<crate::ir::Var, usize>,
    pub def_blocks: std::collections::HashMap<crate::ir::Var, BlockId>,
    pub var_names: std::collections::HashMap<crate::ir::Var, String>,
}

impl<'a> RecoverCtx<'a> {
    /// Resolve a Var to an Expr using the naming map.
    pub fn var_expr(&self, var: crate::ir::Var) -> Expr {
        let name = self.var_names.get(&var)
            .cloned()
            .unwrap_or_else(|| format!("v{}", var.0));
        Expr::Var(name)
    }
}

// ============================================================================
// Main recovery
// ============================================================================

/// Recover structured statements for a region of blocks.
///
/// Processes blocks starting from `start`, stopping before `stop_at`.
/// Recursively handles if/else and while structures.
pub(crate) fn recover_region(
    start: BlockId,
    stop_at: Option<BlockId>,
    ctx: &RecoverCtx<'_>,
    visited: &mut BTreeSet<BlockId>,
) -> Vec<Stmt> {
    let mut stmts = Vec::new();
    let mut current = Some(start);

    while let Some(bid) = current {
        // Stop conditions
        if Some(bid) == stop_at {
            break;
        }
        if !visited.insert(bid) {
            break; // already processed (back edge or convergence)
        }

        let Some(block) = ctx.func.block(bid) else {
            break;
        };

        // Check if this block is a loop header
        if let Some(loop_info) = ctx.loops.loop_for_header(bid) {
            let (loop_stmt, next) = recover_while(bid, loop_info, ctx, visited);
            stmts.push(loop_stmt);
            current = next;
            continue;
        }

        // Emit this block's instructions as expressions
        let block_stmts = expr::reconstruct_block(
            block,
            &ctx.use_counts,
            &ctx.def_blocks,
            &ctx.var_names,
        );
        stmts.extend(block_stmts);

        // Process terminator
        match &block.terminator {
            Terminator::Jump { target } => {
                current = Some(*target);
            }
            Terminator::BranchIf { cond, if_true, if_false } => {
                let cond_expr = ctx.var_expr(*cond);
                let merge = ctx.pdom.get(&bid).copied();

                let then_stmts = recover_region(*if_true, merge, ctx, visited);

                let else_stmts = recover_region(*if_false, merge, ctx, visited);
                let else_body = if else_stmts.is_empty() { None } else { Some(else_stmts) };

                stmts.push(Stmt::If {
                    cond: cond_expr,
                    then_body: then_stmts,
                    else_body,
                });

                current = merge;
            }
            Terminator::Return { value } => {
                let expr = value.map(|v| ctx.var_expr(v));
                stmts.push(Stmt::Return(expr));
                current = None;
            }
            Terminator::Halt => {
                current = None;
            }
            Terminator::Throw { value } => {
                stmts.push(Stmt::Throw(ctx.var_expr(*value)));
                current = None;
            }
            Terminator::Switch { value, cases, default } => {
                // v1: emit as if/else chain (switch recovery is v2)
                let val_expr = ctx.var_expr(*value);
                let mut case_stmts: Vec<(Expr, Vec<Stmt>)> = Vec::new();
                for (case_val, target) in cases {
                    let cond = Expr::binary(
                        crate::value::ops::BinaryOp::StrictEq,
                        val_expr.clone(),
                        Expr::Const(case_val.clone()),
                    );
                    let body = recover_region(*target, None, ctx, visited);
                    case_stmts.push((cond, body));
                }
                let default_stmts = recover_region(*default, None, ctx, visited);

                // Build nested if/else from cases
                let mut result = if default_stmts.is_empty() {
                    None
                } else {
                    Some(default_stmts)
                };

                for (cond, body) in case_stmts.into_iter().rev() {
                    let if_stmt = Stmt::If {
                        cond,
                        then_body: body,
                        else_body: result.map(|stmts| vec![Stmt::If {
                            cond: Expr::Unknown("...".into()),
                            then_body: stmts,
                            else_body: None,
                        }]).or(None),
                    };
                    result = Some(vec![if_stmt]);
                }

                if let Some(chain) = result {
                    stmts.extend(chain);
                }

                current = None;
            }
            Terminator::Unreachable => {
                current = None;
            }
        }
    }

    stmts
}

// ============================================================================
// While loop recovery
// ============================================================================

/// Recover a while loop from a loop header block.
///
/// Returns the While statement and the block to continue from after the loop.
fn recover_while(
    header: BlockId,
    loop_info: &LoopInfo,
    ctx: &RecoverCtx<'_>,
    visited: &mut BTreeSet<BlockId>,
) -> (Stmt, Option<BlockId>) {
    visited.insert(header);

    let Some(block) = ctx.func.block(header) else {
        return (Stmt::Comment("missing loop header".into()), None);
    };

    // Emit header's instructions (these compute the loop condition)
    let header_stmts = expr::reconstruct_block(
        block,
        &ctx.use_counts,
        &ctx.def_blocks,
        &ctx.var_names,
    );

    match &block.terminator {
        Terminator::BranchIf { cond, if_true, if_false } => {
            let cond_expr = ctx.var_expr(*cond);

            // Determine body vs exit: the branch target inside the loop body is the body
            let (body_target, exit_target, negate_cond) =
                if loop_info.body.contains(if_true) {
                    (*if_true, *if_false, false)
                } else {
                    (*if_false, *if_true, true)
                };

            let final_cond = if negate_cond {
                Expr::unary(crate::value::ops::UnaryOp::LogicalNot, cond_expr)
            } else {
                cond_expr
            };

            // Recover body blocks (stop at header — back edge)
            let body_stmts = recover_region(body_target, Some(header), ctx, visited);

            // Header instructions go at the START of the body
            // (they execute every iteration to compute the condition)
            let mut full_body = header_stmts;
            full_body.extend(body_stmts);

            let while_stmt = Stmt::While {
                cond: final_cond,
                body: full_body,
            };

            (while_stmt, Some(exit_target))
        }
        Terminator::Jump { target } => {
            // Unconditional loop — infinite loop with break to exit
            let body_stmts = recover_region(*target, Some(header), ctx, visited);

            let mut full_body = header_stmts;
            full_body.extend(body_stmts);

            let exit = loop_info.exits.first().copied();
            (Stmt::Loop { body: full_body }, exit)
        }
        _ => {
            (Stmt::Comment(format!("unhandled loop at {header}")), None)
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::IrBuilder;
    use crate::graph;

    fn recover_function(func: &Function) -> Vec<Stmt> {
        let cfg = graph::build_cfg(func);
        let dom = graph::dominator::compute_dominators(&cfg);
        let pdom = graph::dominator::compute_post_dominators(&cfg);
        let loops = graph::loops::detect_loops(&cfg, &dom);

        let ctx = RecoverCtx {
            func,
            cfg: &cfg,
            dom: &dom,
            pdom: &pdom,
            loops: &loops,
            use_counts: expr::count_uses(func),
            def_blocks: expr::var_def_blocks(func),
            var_names: expr::name_vars(func),
        };

        let mut visited = BTreeSet::new();
        recover_region(func.entry, None, &ctx, &mut visited)
    }

    #[test]
    fn recover_linear() {
        let mut b = IrBuilder::new();
        b.begin_function("linear");
        b.create_and_switch("entry");
        let v = b.const_number(42.0);
        b.ret(Some(v));
        b.end_function();

        let module = b.build();
        let stmts = recover_function(&module.functions[0]);
        let text: String = stmts.iter().map(|s| s.to_string()).collect();

        assert!(text.contains("return"), "got: {text}");
    }

    #[test]
    fn recover_if_else() {
        let mut b = IrBuilder::new();
        b.begin_function("branch");

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

        let module = b.build();
        let stmts = recover_function(&module.functions[0]);
        let text: String = stmts.iter().map(|s| s.to_string()).collect();

        assert!(text.contains("if ("), "got: {text}");
    }

    #[test]
    fn recover_while_loop() {
        let mut b = IrBuilder::new();
        b.begin_function("loop");

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
        b.halt();
        b.end_function();

        let module = b.build();
        let stmts = recover_function(&module.functions[0]);
        let text: String = stmts.iter().map(|s| s.to_string()).collect();

        assert!(text.contains("while ("), "got: {text}");
    }

    #[test]
    fn recover_if_then_no_else() {
        let mut b = IrBuilder::new();
        b.begin_function("if_then");

        let entry = b.create_and_switch("entry");
        let cond = b.const_bool(true);
        let then_b = b.create_block("then");
        let merge = b.create_block("merge");

        b.switch_to(entry);
        b.branch_if(cond, then_b, merge);

        b.switch_to(then_b);
        let _ = b.const_number(1.0);
        b.jump(merge);

        b.switch_to(merge);
        b.ret(None);
        b.end_function();

        let module = b.build();
        let stmts = recover_function(&module.functions[0]);
        let text: String = stmts.iter().map(|s| s.to_string()).collect();

        assert!(text.contains("if ("), "got: {text}");
        // Should NOT have an else clause
        assert!(!text.contains("} else {"), "should be if-then without else, got: {text}");
    }
}

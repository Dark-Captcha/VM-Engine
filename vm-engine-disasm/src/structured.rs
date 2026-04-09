//! Structured output: run the full pipeline and produce readable pseudo-JS.
//!
//! This is the main user-facing output. Takes an IR Module and produces
//! readable code with functions, loops, branches, and inlined expressions.

// ============================================================================
// Imports
// ============================================================================

use vm_engine_core::graph;
use vm_engine_core::ir::{Function, Module};
use vm_engine_core::structure;

// ============================================================================
// Structured output
// ============================================================================

/// Recover and format all functions in a module as structured pseudo-JS.
pub fn format_structured(module: &Module) -> String {
    let mut output = String::new();
    for (index, function) in module.functions.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        output.push_str(&format_function_structured(function));
    }
    output
}

/// Recover and format a single function as structured pseudo-JS.
pub fn format_function_structured(function: &Function) -> String {
    let cfg = graph::build_cfg(function);
    let dominator_tree = graph::dominator::compute_dominators(&cfg);
    let post_dominators = graph::dominator::compute_post_dominators(&cfg);
    let loop_forest = graph::loops::detect_loops(&cfg, &dominator_tree);

    structure::recover_to_string(function, &cfg, &dominator_tree, &post_dominators, &loop_forest)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use vm_engine_core::ir::builder::IrBuilder;

    #[test]
    fn structured_output_shows_while_loop() {
        let mut builder = IrBuilder::new();
        builder.begin_function("cipher");

        let header = builder.create_and_switch("header");
        let cond = builder.const_bool(true);
        let body = builder.create_block("body");
        let exit = builder.create_block("exit");

        builder.switch_to(header);
        builder.branch_if(cond, body, exit);

        builder.switch_to(body);
        let _ = builder.const_number(99.0);
        builder.jump(header);

        builder.switch_to(exit);
        builder.ret(None);
        builder.end_function();

        let module = builder.build();
        let text = format_structured(&module);

        assert!(text.contains("while ("), "should recover while loop:\n{text}");
        assert!(text.contains("return"), "should have return:\n{text}");
    }

    #[test]
    fn structured_output_shows_if_else() {
        let mut builder = IrBuilder::new();
        builder.begin_function("check");

        let entry = builder.create_and_switch("entry");
        let cond = builder.const_bool(true);
        let then_block = builder.create_block("then");
        let else_block = builder.create_block("else");
        let merge = builder.create_block("merge");

        builder.switch_to(entry);
        builder.branch_if(cond, then_block, else_block);

        builder.switch_to(then_block);
        let _ = builder.const_number(1.0);
        builder.jump(merge);

        builder.switch_to(else_block);
        let _ = builder.const_number(0.0);
        builder.jump(merge);

        builder.switch_to(merge);
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let text = format_structured(&module);

        assert!(text.contains("if ("), "should recover if/else:\n{text}");
    }
}

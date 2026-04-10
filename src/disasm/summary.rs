//! Module and function summary statistics.
//!
//! High-level overview showing block counts, instruction counts, loop counts,
//! call relationships — before diving into the details.

// ============================================================================
// Imports
// ============================================================================

use std::fmt::Write;

use crate::graph;
use crate::ir::Module;

// ============================================================================
// Summary
// ============================================================================

/// Generate a summary of an entire module.
pub fn format_summary(module: &Module) -> String {
    let mut output = String::new();
    let total_blocks: usize = module.functions.iter().map(|f| f.blocks.len()).sum();
    let total_instructions: usize = module.functions.iter()
        .flat_map(|f| f.blocks.iter())
        .map(|b| b.body.len())
        .sum();

    writeln!(output, "=== Module Summary ===").unwrap();
    writeln!(output, "  functions:    {}", module.functions.len()).unwrap();
    writeln!(output, "  total blocks: {total_blocks}").unwrap();
    writeln!(output, "  total instrs: {total_instructions}").unwrap();

    // Call graph
    let call_graph = graph::callgraph::build_call_graph(module);
    writeln!(output, "  entry points: {} (functions with no callers)", call_graph.roots.len()).unwrap();
    writeln!(output, "  leaf funcs:   {} (functions that call nothing)", call_graph.leaves.len()).unwrap();

    writeln!(output).unwrap();
    writeln!(output, "=== Functions ===").unwrap();

    for function in &module.functions {
        let cfg = graph::build_cfg(function);
        let dominator_tree = graph::dominator::compute_dominators(&cfg);
        let loop_forest = graph::loops::detect_loops(&cfg, &dominator_tree);

        let instruction_count: usize = function.blocks.iter().map(|b| b.body.len()).sum();
        let edge_count = cfg.edges.len();
        let callees = call_graph.callees.get(&function.id).map(|c| c.len()).unwrap_or(0);

        writeln!(output, "  {} ({}):", function.name, function.id).unwrap();
        writeln!(output, "    params:  {}", function.params.len()).unwrap();
        writeln!(output, "    blocks:  {}", cfg.len()).unwrap();
        writeln!(output, "    edges:   {edge_count}").unwrap();
        writeln!(output, "    instrs:  {instruction_count}").unwrap();
        writeln!(output, "    loops:   {}", loop_forest.len()).unwrap();
        writeln!(output, "    calls:   {callees} functions").unwrap();

        for loop_info in &loop_forest.loops {
            writeln!(
                output,
                "    loop at {}: {} blocks, {} exits, depth {}",
                loop_info.header,
                loop_info.body.len(),
                loop_info.exits.len(),
                loop_info.depth,
            ).unwrap();
        }
    }

    output
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::IrBuilder;

    #[test]
    fn summary_shows_function_stats() {
        let mut builder = IrBuilder::new();

        builder.begin_function("helper");
        builder.create_and_switch("entry");
        let _ = builder.const_number(1.0);
        builder.halt();
        builder.end_function();

        let helper_id = crate::ir::FuncId(0);
        builder.begin_function("main");
        builder.create_and_switch("entry");
        let _ = builder.call(helper_id, &[]);
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let summary = format_summary(&module);

        assert!(summary.contains("functions:    2"), "summary:\n{summary}");
        assert!(summary.contains("helper"), "summary:\n{summary}");
        assert!(summary.contains("main"), "summary:\n{summary}");
        assert!(summary.contains("calls:   1"), "main should call 1 function:\n{summary}");
    }

    #[test]
    fn summary_shows_loops() {
        let mut builder = IrBuilder::new();
        builder.begin_function("loopy");
        let header = builder.create_and_switch("header");
        let cond = builder.const_bool(true);
        let body = builder.create_block("body");
        let exit = builder.create_block("exit");
        builder.switch_to(header);
        builder.branch_if(cond, body, exit);
        builder.switch_to(body);
        builder.jump(header);
        builder.switch_to(exit);
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let summary = format_summary(&module);

        assert!(summary.contains("loops:   1"), "should detect loop:\n{summary}");
    }
}

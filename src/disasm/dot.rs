//! Graphviz DOT export for CFG and call graph visualization.

// ============================================================================
// Imports
// ============================================================================

use std::fmt::Write;

use crate::graph::block::EdgeKind;
use crate::graph::callgraph::CallGraph;
use crate::graph::Cfg;
use crate::ir::{Function, Module};

// ============================================================================
// CFG DOT
// ============================================================================

/// Export a function's CFG as a Graphviz DOT string.
///
/// Nodes are basic blocks labeled with their name and instruction count.
/// Edges are colored by kind: blue for jumps, green for true branches,
/// red for false branches.
pub fn cfg_to_dot(function: &Function, cfg: &Cfg) -> String {
    let mut dot = String::new();
    writeln!(dot, "digraph \"{}\" {{", function.name).unwrap();
    writeln!(dot, "  rankdir=TB;").unwrap();
    writeln!(dot, "  node [shape=box, fontname=\"monospace\", fontsize=10];").unwrap();
    writeln!(dot, "  edge [fontname=\"monospace\", fontsize=9];").unwrap();

    // Nodes
    for block in &cfg.blocks {
        let is_entry = block.id == cfg.entry;
        let style = if is_entry { ", style=bold, color=blue" } else { "" };
        writeln!(
            dot,
            "  {} [label=\"{} ({})\\n{} instrs\"{style}];",
            dot_node_id(block.id),
            block.label,
            block.id,
            block.instruction_count,
        ).unwrap();
    }

    // Edges
    for edge in &cfg.edges {
        let (color, label) = match &edge.kind {
            EdgeKind::Jump => ("blue", "".to_string()),
            EdgeKind::TrueBranch => ("green", "true".to_string()),
            EdgeKind::FalseBranch => ("red", "false".to_string()),
            EdgeKind::SwitchCase(value) => ("purple", format!("case {value}")),
            EdgeKind::SwitchDefault => ("purple", "default".to_string()),
            EdgeKind::Exception => ("orange", "catch".to_string()),
        };
        let label_attr = if label.is_empty() {
            String::new()
        } else {
            format!(", label=\"{label}\"")
        };
        writeln!(
            dot,
            "  {} -> {} [color={color}{label_attr}];",
            dot_node_id(edge.from),
            dot_node_id(edge.to),
        ).unwrap();
    }

    writeln!(dot, "}}").unwrap();
    dot
}

// ============================================================================
// Call Graph DOT
// ============================================================================

/// Export a module's call graph as a Graphviz DOT string.
pub fn callgraph_to_dot(module: &Module, call_graph: &CallGraph) -> String {
    let mut dot = String::new();
    writeln!(dot, "digraph callgraph {{").unwrap();
    writeln!(dot, "  rankdir=TB;").unwrap();
    writeln!(dot, "  node [shape=ellipse, fontname=\"monospace\", fontsize=10];").unwrap();

    // Nodes
    for function in &module.functions {
        let is_root = call_graph.roots.contains(&function.id);
        let is_leaf = call_graph.leaves.contains(&function.id);
        let style = if is_root {
            ", style=bold, color=blue"
        } else if is_leaf {
            ", style=dashed, color=gray"
        } else {
            ""
        };
        let block_count = function.blocks.len();
        let instruction_count: usize = function.blocks.iter().map(|block| block.body.len()).sum();
        writeln!(
            dot,
            "  {} [label=\"{}\\n{} blocks, {} instrs\"{style}];",
            dot_func_id(function.id),
            function.name,
            block_count,
            instruction_count,
        ).unwrap();
    }

    // Edges
    for (caller_id, callees) in &call_graph.callees {
        for callee_id in callees {
            writeln!(
                dot,
                "  {} -> {};",
                dot_func_id(*caller_id),
                dot_func_id(*callee_id),
            ).unwrap();
        }
    }

    writeln!(dot, "}}").unwrap();
    dot
}

// ============================================================================
// Helpers
// ============================================================================

fn dot_node_id(block_id: crate::ir::BlockId) -> String {
    format!("B{}", block_id.0)
}

fn dot_func_id(func_id: crate::ir::FuncId) -> String {
    format!("F{}", func_id.0)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph;
    use crate::ir::builder::IrBuilder;

    #[test]
    fn cfg_dot_has_nodes_and_edges() {
        let mut builder = IrBuilder::new();
        builder.begin_function("test");
        let entry = builder.create_and_switch("entry");
        let cond = builder.const_bool(true);
        let yes = builder.create_block("yes");
        let no = builder.create_block("no");
        builder.switch_to(entry);
        builder.branch_if(cond, yes, no);
        builder.switch_to(yes);
        builder.halt();
        builder.switch_to(no);
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let function = &module.functions[0];
        let cfg = graph::build_cfg(function);
        let dot = cfg_to_dot(function, &cfg);

        assert!(dot.contains("digraph"), "should be valid DOT:\n{dot}");
        assert!(dot.contains("entry"), "should have entry node:\n{dot}");
        assert!(dot.contains("color=green"), "should have true branch:\n{dot}");
        assert!(dot.contains("color=red"), "should have false branch:\n{dot}");
    }

    #[test]
    fn cfg_dot_with_loop_shows_back_edge() {
        let mut builder = IrBuilder::new();
        builder.begin_function("loop");
        let header = builder.create_and_switch("header");
        let cond = builder.const_bool(true);
        let body = builder.create_block("body");
        let exit = builder.create_block("exit");
        builder.switch_to(header);
        builder.branch_if(cond, body, exit);
        builder.switch_to(body);
        builder.jump(header); // back edge
        builder.switch_to(exit);
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let function = &module.functions[0];
        let cfg = graph::build_cfg(function);
        let dot = cfg_to_dot(function, &cfg);

        // Should have edge from body → header (back edge, blue jump)
        assert!(dot.contains("B1 -> B0"), "should have back edge:\n{dot}");
    }

    #[test]
    fn callgraph_dot_has_functions() {
        let mut builder = IrBuilder::new();

        let helper_id = builder.begin_function("helper");
        builder.create_and_switch("entry");
        builder.halt();
        builder.end_function();

        builder.begin_function("main");
        builder.create_and_switch("entry");
        let _ = builder.call(helper_id, &[]);
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let call_graph = graph::callgraph::build_call_graph(&module);
        let dot = callgraph_to_dot(&module, &call_graph);

        assert!(dot.contains("helper"), "should have helper node:\n{dot}");
        assert!(dot.contains("main"), "should have main node:\n{dot}");
        assert!(dot.contains("F1 -> F0"), "should have call edge:\n{dot}");
    }
}

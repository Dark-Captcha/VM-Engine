//! Disassembly and visualization output for VM-Engine IR.
//!
//! Four output modes:
//! - **IR listing**: raw IR with source PC annotations
//! - **Structured**: recovered pseudo-JS with if/else, while, expressions
//! - **DOT**: Graphviz graphs for CFG and call graph
//! - **Summary**: high-level statistics per function
//!
//! # Quick Start
//!
//! ```
//! use vm_engine::ir::builder::IrBuilder;
//! use vm_engine::disasm::{disasm_structured, disasm_summary};
//!
//! let mut builder = IrBuilder::new();
//! builder.begin_function("example");
//! builder.create_and_switch("entry");
//! let value = builder.const_number(42.0);
//! builder.ret(Some(value));
//! builder.end_function();
//!
//! let module = builder.build();
//! println!("{}", disasm_summary(&module));
//! println!("{}", disasm_structured(&module));
//! ```

pub mod dot;
pub mod listing;
pub mod structured;
pub mod summary;

// ============================================================================
// Convenience re-exports
// ============================================================================

/// Format a module as an annotated IR listing with source PCs.
pub fn disasm_ir(module: &crate::ir::Module) -> String {
    listing::format_ir_listing(module)
}

/// Format a module as recovered structured pseudo-JS.
pub fn disasm_structured(module: &crate::ir::Module) -> String {
    structured::format_structured(module)
}

/// Format a module summary with statistics per function.
pub fn disasm_summary(module: &crate::ir::Module) -> String {
    summary::format_summary(module)
}

/// Export a function's CFG as Graphviz DOT.
pub fn disasm_cfg_dot(
    function: &crate::ir::Function,
    cfg: &crate::graph::Cfg,
) -> String {
    dot::cfg_to_dot(function, cfg)
}

/// Export a module's call graph as Graphviz DOT.
pub fn disasm_callgraph_dot(
    module: &crate::ir::Module,
    call_graph: &crate::graph::callgraph::CallGraph,
) -> String {
    dot::callgraph_to_dot(module, call_graph)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::IrBuilder;

    fn sample_module() -> crate::ir::Module {
        let mut builder = IrBuilder::new();
        builder.begin_function("main");
        builder.create_and_switch("entry");
        let value = builder.const_number(42.0);
        builder.ret(Some(value));
        builder.end_function();
        builder.build()
    }

    #[test]
    fn disasm_ir_produces_output() {
        let module = sample_module();
        let output = disasm_ir(&module);
        assert!(output.contains("function main"), "output:\n{output}");
        assert!(output.contains("const 42"), "output:\n{output}");
    }

    #[test]
    fn disasm_structured_produces_output() {
        let module = sample_module();
        let output = disasm_structured(&module);
        assert!(output.contains("function main()"), "output:\n{output}");
        assert!(output.contains("return"), "output:\n{output}");
    }

    #[test]
    fn disasm_summary_produces_output() {
        let module = sample_module();
        let output = disasm_summary(&module);
        assert!(output.contains("functions:    1"), "output:\n{output}");
        assert!(output.contains("main"), "output:\n{output}");
    }

    #[test]
    fn all_outputs_consistent() {
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

        let ir_output = disasm_ir(&module);
        let structured_output = disasm_structured(&module);
        let summary_output = disasm_summary(&module);

        // IR shows blocks
        assert!(ir_output.contains("header"), "IR:\n{ir_output}");
        assert!(ir_output.contains("body"), "IR:\n{ir_output}");

        // Structured shows while loop
        assert!(structured_output.contains("while ("), "structured:\n{structured_output}");

        // Summary shows loop count
        assert!(summary_output.contains("loops:   1"), "summary:\n{summary_output}");
    }
}

//! Proof: decode real DataDome PLV3 bytecode → IR → CFG → structure recovery.
//!
//! Run: cargo run -p plv3-proof

mod decoder;
mod reader;

use vm_engine_core::ir::display::format_module;
use vm_engine_core::ir::validate;
use vm_engine_core::graph;
use vm_engine_core::structure;

const BYTECODE_PATH: &str = "/home/gnusocute/Documents/Dark-Captcha_old/datadome/plv3-vm/source/bytecode.bin";

fn main() {
    // Load bytecode
    let bytecode = std::fs::read(BYTECODE_PATH).expect("failed to read bytecode.bin");
    println!("Loaded {} bytes of PLV3 bytecode\n", bytecode.len());

    // Decode
    let (module, stats) = decoder::decode_plv3(&bytecode);
    println!("=== Decode Stats ===");
    println!("  instructions decoded: {}", stats.instructions_decoded);
    println!("  unknown opcodes:      {}", stats.unknown_opcodes);
    println!("  blocks found:         {}", stats.block_count);
    println!("  jumps:                {}", stats.jumps);
    println!("  branches:             {}", stats.branches);
    println!("  returns:              {}", stats.returns);
    println!("  halts:                {}", stats.halts);

    // Show IR size
    for func in &module.functions {
        let total_instructions: usize = func.blocks.iter().map(|b| b.body.len()).sum();
        println!("  function '{}': {} blocks, {} instructions",
            func.name, func.blocks.len(), total_instructions);
    }

    // Validate
    println!("\n=== Validation ===");
    match validate::validate(&module) {
        Ok(()) => println!("  [ok] IR is well-formed"),
        Err(err) => println!("  [warn] {err}"),
    }

    // Print first 50 lines of IR
    println!("\n=== IR (first 50 lines) ===\n");
    let ir_text = format_module(&module);
    for (index, line) in ir_text.lines().enumerate() {
        if index >= 50 { println!("  ... ({} more lines)", ir_text.lines().count() - 50); break; }
        println!("{line}");
    }

    // CFG analysis
    println!("\n=== CFG Analysis ===\n");
    for func in &module.functions {
        let cfg = graph::build_cfg(func);
        let dominator_tree = graph::dominator::compute_dominators(&cfg);
        let post_dominators = graph::dominator::compute_post_dominators(&cfg);
        let loop_forest = graph::loops::detect_loops(&cfg, &dominator_tree);

        println!("function '{}':", func.name);
        println!("  blocks:          {}", cfg.len());
        println!("  edges:           {}", cfg.edges.len());
        println!("  dominators:      {}", dominator_tree.idom.len());
        println!("  post-dominators: {}", post_dominators.len());
        println!("  loops:           {}", loop_forest.len());

        for loop_info in &loop_forest.loops {
            println!("    loop at {}: {} body blocks, {} exits, depth {}",
                loop_info.header,
                loop_info.body.len(),
                loop_info.exits.len(),
                loop_info.depth,
            );
        }

        // Structure recovery (first 30 lines)
        println!("\n=== Recovered (first 30 lines) ===\n");
        let recovered = structure::recover_to_string(func, &cfg, &dominator_tree, &post_dominators, &loop_forest);
        for (index, line) in recovered.lines().enumerate() {
            if index >= 30 { println!("  ... ({} more lines)", recovered.lines().count() - 30); break; }
            println!("{line}");
        }
    }

    // Call graph
    println!("\n=== Call Graph ===\n");
    let call_graph = graph::callgraph::build_call_graph(&module);
    println!("  roots:  {:?}", call_graph.roots);
    println!("  leaves: {:?}", call_graph.leaves);

    println!("\n=== Done ===");
}

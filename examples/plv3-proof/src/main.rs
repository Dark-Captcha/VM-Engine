//! Proof: decode real DataDome PLV3 bytecode → IR → CFG → structure recovery.
//!
//! Run: cargo run -p plv3-proof

mod cipher;
mod decoder;
mod extract;
mod funcmap;
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

    // S-box extraction
    println!("\n=== S-box Extraction ===");
    let sbox_result = extract::extract_sboxes(&module);
    println!("  total arrays found:     {}", sbox_result.total_arrays);
    println!("  partial (non-256):      {}", sbox_result.partial_arrays);
    println!("  complete S-boxes (256): {}", sbox_result.sboxes.len());
    for sbox in &sbox_result.sboxes {
        println!(
            "  sbox{}: block={}, var={}, first_8=[{}, {}, {}, {}, {}, {}, {}, {}]",
            sbox.index, sbox.source_block, sbox.array_var,
            sbox.bytes[0], sbox.bytes[1], sbox.bytes[2], sbox.bytes[3],
            sbox.bytes[4], sbox.bytes[5], sbox.bytes[6], sbox.bytes[7],
        );
    }

    // Verify against known reference if available
    // Use reference from the SAME bytecode rotation (plv3-vm/out/, not datadome_new/data/)
    let reference_path = "/home/gnusocute/Documents/Dark-Captcha_old/datadome/plv3-vm/out/sbox.json";
    if let Ok(reference_json) = std::fs::read_to_string(reference_path)
        && let Ok(reference_map) = serde_json::from_str::<std::collections::HashMap<String, Vec<u8>>>(&reference_json) {
            let mut reference_ordered: Vec<Vec<u8>> = Vec::new();
            for index in 0..reference_map.len() {
                let key = format!("sbox{index}");
                if let Some(bytes) = reference_map.get(&key) {
                    reference_ordered.push(bytes.clone());
                }
            }
            let (matched, mismatched) = extract::verify_sboxes(&sbox_result.sboxes, &reference_ordered);
            println!("\n  Verification against {reference_path}:");
            println!("    reference S-boxes: {}", reference_ordered.len());
            println!("    matched:           {matched}");
            if !mismatched.is_empty() {
                println!("    mismatched:        {mismatched:?}");
            }
        }

    // ═══ PLV3 Token Generation ═══════════════════════════════════════
    if sbox_result.sboxes.len() >= 25 {
        println!("\n=== PLV3 Token Generation ===");

        let sbox0 = cipher::to_sbox_array(&sbox_result.sboxes[0].bytes);
        let cipher_sboxes = cipher::CipherSboxes {
            sbox4: cipher::to_sbox_array(&sbox_result.sboxes[4].bytes),
            sbox5: cipher::to_sbox_array(&sbox_result.sboxes[5].bytes),
            sbox6: cipher::to_sbox_array(&sbox_result.sboxes[6].bytes),
            sbox7: cipher::to_sbox_array(&sbox_result.sboxes[7].bytes),
            sbox9: cipher::to_sbox_array(&sbox_result.sboxes[9].bytes),
            sbox10: cipher::to_sbox_array(&sbox_result.sboxes[10].bytes),
            sbox11: cipher::to_sbox_array(&sbox_result.sboxes[11].bytes),
            sbox15: cipher::to_sbox_array(&sbox_result.sboxes[15].bytes),
            sbox16: cipher::to_sbox_array(&sbox_result.sboxes[16].bytes),
            sbox22: cipher::to_sbox_array(&sbox_result.sboxes[22].bytes),
            sbox24: cipher::to_sbox_array(&sbox_result.sboxes[24].bytes),
        };

        // Keys: still need automated extraction. Using known keys for this bytecode rotation.
        // TODO: extract keys from IR via interpreter execution or static analysis.
        let keys = cipher::Plv3Keys {
            timestamp: "o4zbWU".into(),
            pathname: "P6qv3g".into(),
            client_width: "IMd3AC".into(),
            elapsed: "yKUvF2".into(),
            perf_now: "mR8kKc".into(),
            is_secure: "oezAbp".into(),
            webdriver: "J1nt2L".into(),
            random: "KJg3g6".into(),
        };

        let token = cipher::generate_plv3_token(&sbox0, &cipher_sboxes, &keys, "/interstitial/");
        println!("  token length: {}", token.len());
        println!("  token: {}", &token[..token.len().min(80)]);
        if token.len() > 80 { println!("         ...{}", &token[token.len()-20..]); }
        println!("  base64url safe: {}", token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    } else {
        println!("\n[skip] Not enough S-boxes for token generation (need 25+, got {})", sbox_result.sboxes.len());
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

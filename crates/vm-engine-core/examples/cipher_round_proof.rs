//! Proof-of-concept: full pipeline on a synthetic cipher-round program.
//!
//! Mimics a DataDome PLV3 pattern:
//!   1. Initialize S-box (256-byte array) in a loop
//!   2. XOR each element with a key
//!   3. Branch on a condition
//!   4. Return the result
//!
//! Validates: IrBuilder → validate → CFG → dominators → loops → structure recovery → output.
//!
//! Run: cargo run -p vm-engine-core --example cipher_round_proof

use vm_engine_core::ir::builder::IrBuilder;
use vm_engine_core::ir::display::format_module;
use vm_engine_core::ir::opcode::OpCode;
use vm_engine_core::ir::operand::Operand;
use vm_engine_core::ir::validate;
use vm_engine_core::value::Value;
use vm_engine_core::{graph, structure};

fn main() {
    println!("=== Building IR ===\n");

    let module = build_cipher_program();

    // Validate
    validate::validate(&module).expect("IR validation failed");
    println!("[ok] IR validation passed\n");

    // Print raw IR
    println!("=== Raw IR ===\n");
    println!("{}", format_module(&module));

    // Analyze and recover structure for each function
    for func in &module.functions {
        println!("=== Analysis: {} ===\n", func.name);

        let cfg = graph::build_cfg(func);
        println!("[cfg] {} blocks, {} edges", cfg.len(), cfg.edges.len());

        let dom = graph::dominator::compute_dominators(&cfg);
        let pdom = graph::dominator::compute_post_dominators(&cfg);
        println!("[dom] {} immediate dominators", dom.idom.len());
        println!("[pdom] {} post-dominators", pdom.len());

        let loops = graph::loops::detect_loops(&cfg, &dom);
        println!("[loops] {} detected", loops.len());
        for loop_info in &loops.loops {
            println!(
                "  loop at {}: {} body blocks, {} exits, depth {}",
                loop_info.header,
                loop_info.body.len(),
                loop_info.exits.len(),
                loop_info.depth,
            );
        }

        println!("\n=== Recovered: {} ===\n", func.name);
        let output = structure::recover_to_string(func, &cfg, &dom, &pdom, &loops);
        println!("{output}");
    }

    // Call graph
    println!("=== Call Graph ===\n");
    let call_graph = graph::callgraph::build_call_graph(&module);
    println!("roots: {:?}", call_graph.roots);
    println!("leaves: {:?}", call_graph.leaves);
    for (func_id, callees) in &call_graph.callees {
        if !callees.is_empty() {
            let name = module.function_by_id(*func_id).map(|f| f.name.as_str()).unwrap_or("?");
            let callee_names: Vec<&str> = callees.iter()
                .map(|fid| module.function_by_id(*fid).map(|f| f.name.as_str()).unwrap_or("?"))
                .collect();
            println!("{name} calls: {callee_names:?}");
        }
    }

    println!("\n=== Done ===");
}

/// Build a synthetic program that mimics a cipher round.
///
/// Structure:
///   function xor_sbox(sbox, key):
///     i = 0
///     while (i < 256):
///       sbox[i] = sbox[i] ^ (key & 0xFF)
///       i = i + 1
///     return sbox
///
///   function main():
///     sbox = new_array()
///     key = 0xAB
///     result = xor_sbox(sbox, key)
///     if (result != null):
///       return result
///     else:
///       return null
fn build_cipher_program() -> vm_engine_core::Module {
    let mut b = IrBuilder::new();

    // ── function xor_sbox(sbox, key) ────────────────────────────────
    let xor_sbox_id = b.begin_function("xor_sbox");
    let param_sbox = b.add_param();
    let param_key = b.add_param();

    // entry: i = 0, jump to header
    let _entry = b.create_and_switch("entry");
    b.emit_void(OpCode::StoreScope, vec![
        Operand::Const(Value::string("i")),
        Operand::Const(Value::number(0.0)),
    ]);
    let header = b.create_block("loop_header");
    b.jump(header);

    // header: if (i < 256) goto body else goto exit
    b.switch_to(header);
    let i_val = b.load_scope("i");
    let limit = b.const_number(256.0);
    let condition = b.lt(i_val, limit);
    let body = b.create_block("loop_body");
    let exit = b.create_block("loop_exit");
    b.branch_if(condition, body, exit);

    // body: sbox[i] = sbox[i] ^ (key & 0xFF); i = i + 1; jump header
    b.switch_to(body);
    let i_load = b.load_scope("i");
    let elem = b.load_index(param_sbox, i_load);
    let mask = b.const_number(255.0);
    let masked_key = b.bit_and(param_key, mask);
    let xored = b.bit_xor(elem, masked_key);
    let i_load2 = b.load_scope("i");
    b.store_index(param_sbox, i_load2, xored);
    let i_load3 = b.load_scope("i");
    let one = b.const_number(1.0);
    let next_i = b.add(i_load3, one);
    b.emit_void(OpCode::StoreScope, vec![
        Operand::Const(Value::string("i")),
        Operand::Var(next_i),
    ]);
    b.jump(header);

    // exit: return sbox
    b.switch_to(exit);
    b.ret(Some(param_sbox));
    b.end_function();

    // ── function main() ─────────────────────────────────────────────
    b.begin_function("main");

    let main_entry = b.create_and_switch("entry");
    let sbox = b.new_array();
    let key = b.const_number(0xAB as f64);
    let result = b.call(xor_sbox_id, &[sbox, key]);

    // if (result != null) return result else return null
    let is_null = b.emit(OpCode::StrictEq, vec![
        Operand::Var(result),
        Operand::Const(Value::Null),
    ]);
    let not_null = b.logical_not(is_null);

    let then_block = b.create_block("then_return");
    let else_block = b.create_block("else_return");
    let merge = b.create_block("merge");

    b.switch_to(main_entry);
    b.branch_if(not_null, then_block, else_block);

    b.switch_to(then_block);
    b.ret(Some(result));

    b.switch_to(else_block);
    let null_val = b.const_null();
    b.ret(Some(null_val));

    b.switch_to(merge);
    b.halt();

    b.end_function();

    b.build()
}

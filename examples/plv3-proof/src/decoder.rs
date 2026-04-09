//! PLV3 bytecode → IR decoder.
//!
//! Converts the stack-based PLV3 VM bytecode into the universal IR format.
//! Uses a symbolic stack to convert stack operations into SSA variables.
//!
//! This is a PARTIAL decoder — handles the most common opcodes to prove
//! the pipeline works. Unhandled opcodes become IR comments.

use vm_engine_core::ir::builder::IrBuilder;
use vm_engine_core::ir::opcode::OpCode;
use vm_engine_core::ir::operand::{Operand, SourceLoc};
use vm_engine_core::ir::{BlockId, Module, Var};
use vm_engine_core::value::Value;

use crate::reader::Plv3Reader;

/// Decode PLV3 bytecode into an IR Module.
///
/// Returns the module and decode statistics.
pub fn decode_plv3(bytecode: &[u8]) -> (Module, DecodeStats) {
    let mut builder = IrBuilder::new();
    let mut stats = DecodeStats::default();
    let mut reader = Plv3Reader::new(bytecode);

    // First pass: find jump targets to know where blocks start
    let block_starts = find_block_starts(bytecode);
    stats.block_count = block_starts.len();

    // Start the main function
    builder.begin_function("main");
    let mut sym_stack: Vec<Var> = Vec::new();

    // Create initial block
    let mut current_block = builder.create_and_switch("entry_0");
    let mut block_map: std::collections::HashMap<usize, BlockId> = std::collections::HashMap::new();
    block_map.insert(0, current_block);

    // Pre-create blocks for all jump targets
    for &target_pc in &block_starts {
        if target_pc > 0 && !block_map.contains_key(&target_pc) {
            let block = builder.create_block(&format!("block_{target_pc}"));
            block_map.insert(target_pc, block);
        }
    }

    while !reader.at_end() {
        let instruction_pc = reader.position;

        // Check if we need to start a new block
        if instruction_pc > 0 && block_map.contains_key(&instruction_pc) {
            let target_block = block_map[&instruction_pc];
            // Jump from current block to this one (if current block has no terminator yet)
            builder.jump(target_block);
            builder.switch_to(target_block);
            current_block = target_block;
            sym_stack.clear();
        }

        let Some(opcode_byte) = reader.read_byte() else { break };
        stats.instructions_decoded += 1;

        let source = SourceLoc::with_opcode(instruction_pc, opcode_byte as u16);

        match opcode_byte {
            // ── Arithmetic (pop 2, push 1) ───────────────────────
            30 => binary_op(&mut builder, &mut sym_stack, OpCode::Add, source),       // ADD
            105 => binary_op(&mut builder, &mut sym_stack, OpCode::Mul, source),      // MUL
            164 => binary_op(&mut builder, &mut sym_stack, OpCode::Sub, source),      // SUB
            2 => binary_op(&mut builder, &mut sym_stack, OpCode::Div, source),        // DIV
            233 => binary_op(&mut builder, &mut sym_stack, OpCode::Mod, source),      // MOD
            19 => binary_op(&mut builder, &mut sym_stack, OpCode::BitXor, source),    // XOR
            104 => binary_op(&mut builder, &mut sym_stack, OpCode::BitAnd, source),   // BAND
            138 => binary_op(&mut builder, &mut sym_stack, OpCode::BitOr, source),    // BOR
            157 => binary_op(&mut builder, &mut sym_stack, OpCode::Shr, source),      // SHR
            72 => binary_op(&mut builder, &mut sym_stack, OpCode::UShr, source),      // USHR
            119 => binary_op(&mut builder, &mut sym_stack, OpCode::Shl, source),      // SHL

            // ── Comparison (pop 2, push 1) ───────────────────────
            21 => binary_op(&mut builder, &mut sym_stack, OpCode::StrictEq, source),  // SEQ
            60 => binary_op(&mut builder, &mut sym_stack, OpCode::Eq, source),        // EQ
            246 => binary_op(&mut builder, &mut sym_stack, OpCode::Lt, source),       // LT
            112 => binary_op(&mut builder, &mut sym_stack, OpCode::Gt, source),       // GT

            // ── Unary (pop 1, push 1) ────────────────────────────
            128 => unary_op(&mut builder, &mut sym_stack, OpCode::Neg, source),       // NEG
            51 => unary_op(&mut builder, &mut sym_stack, OpCode::BitNot, source),     // BNOT
            64 => unary_op(&mut builder, &mut sym_stack, OpCode::LogicalNot, source), // LNOT
            41 => unary_op(&mut builder, &mut sym_stack, OpCode::Pos, source),        // TO_NUM
            175 => unary_op(&mut builder, &mut sym_stack, OpCode::TypeOf, source),    // TYPEOF
            15 => unary_op(&mut builder, &mut sym_stack, OpCode::Void, source),       // VOID

            // ── Immediate arithmetic (pop 1, read imm, push 1) ──
            225 => imm_binary_op(&mut builder, &mut sym_stack, &mut reader, OpCode::Add, source),     // ADD_IMM
            152 | 129 => imm_binary_op(&mut builder, &mut sym_stack, &mut reader, OpCode::BitXor, source), // XOR_IMM
            188 => imm_binary_op(&mut builder, &mut sym_stack, &mut reader, OpCode::BitAnd, source),  // AND_IMM
            253 => imm_binary_op(&mut builder, &mut sym_stack, &mut reader, OpCode::Shr, source),     // SHR_IMM
            125 => imm_binary_op(&mut builder, &mut sym_stack, &mut reader, OpCode::UShr, source),    // USHR_IMM
            73 => imm_binary_op(&mut builder, &mut sym_stack, &mut reader, OpCode::Shl, source),      // SHL_IMM
            62 => imm_binary_op(&mut builder, &mut sym_stack, &mut reader, OpCode::Mod, source),      // MOD_IMM

            // ── PUSH (read typed value) ──────────────────────────
            66 => { // PUSH
                let value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let var = builder.emit_sourced(OpCode::Const, vec![Operand::Const(value)], source);
                sym_stack.push(var);
            }

            // ── PUSH_WINDOW ──────────────────────────────────────
            207 => {
                let var = builder.emit_sourced(OpCode::LoadScope, vec![Operand::Const(Value::string("window"))], source);
                sym_stack.push(var);
            }

            // ── PUSH_REG (read register address) ─────────────────
            29 => {
                let reg = reader.read_u16_be().unwrap_or(0) as u32;
                let var = builder.emit_sourced(
                    OpCode::LoadScope,
                    vec![Operand::Const(Value::string(format!("r{reg}")))],
                    source,
                );
                sym_stack.push(var);
            }

            // ── POP ──────────────────────────────────────────────
            155 => { sym_stack.pop(); } // POP
            0 => { sym_stack.pop(); sym_stack.pop(); } // POP2

            // ── GET_PROP (pop key, pop obj, push obj[key]) ───────
            202 => { // GET_PROP
                let key = stack_pop(&mut sym_stack, &mut builder);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let var = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                sym_stack.push(var);
            }

            // ── GET_PROP_IMM (pop obj, read imm key, push obj[key])
            101 => {
                let key_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let key = builder.emit(OpCode::Const, vec![Operand::Const(key_value)]);
                let var = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                sym_stack.push(var);
            }

            // ── SET_PROP (pop key, pop obj, peek val → obj[key]=val)
            81 => {
                let key = stack_pop(&mut sym_stack, &mut builder);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                if let Some(&val) = sym_stack.last() {
                    builder.store_prop(obj, key, val);
                }
            }

            // ── COLLECT (read count, pop N → array) ──────────────
            191 => {
                let count = reader.read_u16_be().unwrap_or(0) as usize;
                let array_var = builder.emit_sourced(OpCode::NewArray, vec![], source);
                let to_drain = count.min(sym_stack.len());
                let start = sym_stack.len() - to_drain;
                for index in 0..to_drain {
                    let elem = sym_stack[start + index];
                    let idx_const = builder.const_number(index as f64);
                    builder.store_index(array_var, idx_const, elem);
                }
                sym_stack.truncate(start);
                sym_stack.push(array_var);
            }

            // ── MULTI_PUSH (read count, then N typed values) ─────
            238 => {
                let count = reader.read_byte().unwrap_or(0) as usize;
                for _ in 0..count {
                    let value = reader.read_typed_value().unwrap_or(Value::Undefined);
                    let var = builder.emit(OpCode::Const, vec![Operand::Const(value)]);
                    sym_stack.push(var);
                }
            }

            // ── CONTROL: JMP_FWD ─────────────────────────────────
            189 => {
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let target_pc = reader.position + offset;
                if let Some(&target_block) = block_map.get(&target_pc) {
                    builder.jump(target_block);
                }
                stats.jumps += 1;
            }

            // ── CONTROL: JMP_BACK ────────────────────────────────
            20 => {
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let target_pc = reader.position.saturating_sub(offset);
                if let Some(&target_block) = block_map.get(&target_pc) {
                    builder.jump(target_block);
                }
                stats.jumps += 1;
            }

            // ── CONTROL: JMP_IF_FALSY ────────────────────────────
            42 => {
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let target_pc = reader.position + offset;
                let cond = stack_pop(&mut sym_stack, &mut builder);

                if let Some(&false_block) = block_map.get(&target_pc) {
                    let next_pc = reader.position;
                    let true_block = block_map.get(&next_pc).copied()
                        .unwrap_or(current_block);
                    builder.branch_if(cond, true_block, false_block);
                }
                stats.branches += 1;
            }

            // ── CONTROL: HALT ────────────────────────────────────
            195 => {
                builder.halt();
                stats.halts += 1;
            }

            // ── CONTROL: RETURN ──────────────────────────────────
            197 => {
                let return_value = sym_stack.pop();
                builder.ret(return_value);
                stats.returns += 1;
            }

            // ── CONTROL: RETURN_VAL (return frame[g]) ────────────
            116 => {
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                let frame_var = builder.emit_sourced(
                    OpCode::LoadScope,
                    vec![Operand::Const(Value::string(format!("frame_{frame_addr}")))],
                    source,
                );
                builder.ret(Some(frame_var));
                stats.returns += 1;
            }

            // ── CALL (read argc) ─────────────────────────────────
            147 => {
                let argc = reader.read_byte().unwrap_or(0) as usize;
                // Pop callable from stack, then args
                let callable = stack_pop(&mut sym_stack, &mut builder);
                let mut args = Vec::new();
                for _ in 0..argc {
                    args.push(Operand::Var(stack_pop(&mut sym_stack, &mut builder)));
                }
                let mut operands = vec![Operand::Var(callable)];
                operands.extend(args);
                let result = builder.emit_sourced(OpCode::Call, operands, source);
                sym_stack.push(result);
            }

            // ── STORE_REG (pop → reg[g]) ─────────────────────────
            224 => {
                let reg = reader.read_u16_be().unwrap_or(0);
                let value = stack_pop(&mut sym_stack, &mut builder);
                builder.emit_void(OpCode::StoreScope, vec![
                    Operand::Const(Value::string(format!("r{reg}"))),
                    Operand::Var(value),
                ]);
            }

            // ── STORE_PEEK (reg[g] = TOS, no pop) ───────────────
            22 => {
                let reg = reader.read_u16_be().unwrap_or(0);
                if let Some(&tos) = sym_stack.last() {
                    builder.emit_void(OpCode::StoreScope, vec![
                        Operand::Const(Value::string(format!("r{reg}"))),
                        Operand::Var(tos),
                    ]);
                }
            }

            // ── LOAD_FRAME (push frame[g]) ───────────────────────
            85 => {
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                let var = builder.emit_sourced(
                    OpCode::LoadScope,
                    vec![Operand::Const(Value::string(format!("frame_{frame_addr}")))],
                    source,
                );
                sym_stack.push(var);
            }

            // ── STORE_FRAME (frame[g] = TOS) ─────────────────────
            172 => {
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                if let Some(&tos) = sym_stack.last() {
                    builder.emit_void(OpCode::StoreScope, vec![
                        Operand::Const(Value::string(format!("frame_{frame_addr}"))),
                        Operand::Var(tos),
                    ]);
                }
            }

            // ── STORE_FRAME_POP (pop → frame[g]) ─────────────────
            18 => {
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                let value = stack_pop(&mut sym_stack, &mut builder);
                builder.emit_void(OpCode::StoreScope, vec![
                    Operand::Const(Value::string(format!("frame_{frame_addr}"))),
                    Operand::Var(value),
                ]);
            }

            // ── NEW_OBJ (pop 2*count pairs) ──────────────────────
            106 => {
                let count = reader.read_u16_be().unwrap_or(0) as usize;
                for _ in 0..(count * 2) {
                    sym_stack.pop();
                }
                let var = builder.emit_sourced(OpCode::NewObject, vec![], source);
                sym_stack.push(var);
            }

            // ── MAKE_FUNC ────────────────────────────────────────
            55 => {
                let param_count = reader.read_byte().unwrap_or(0);
                let capture_count = reader.read_byte().unwrap_or(0);
                for _ in 0..capture_count {
                    reader.read_byte(); // skip capture indices
                }
                // The closure body starts 3 bytes after (JMP_FWD skips it)
                let var = builder.emit_sourced(
                    OpCode::Const,
                    vec![Operand::Const(Value::string(format!("<closure params={param_count} captures={capture_count}>")))],
                    source,
                );
                sym_stack.push(var);
            }

            // ── BIND_CALL ────────────────────────────────────────
            255 => {
                let _this_obj = stack_pop(&mut sym_stack, &mut builder);
                let _func = stack_pop(&mut sym_stack, &mut builder);
                let var = builder.emit_sourced(
                    OpCode::Const,
                    vec![Operand::Const(Value::string("<bound_call>"))],
                    source,
                );
                sym_stack.push(var);
            }

            // ── SP_ADJ (drop items) ──────────────────────────────
            179 => {
                let count = reader.read_u16_be().unwrap_or(0) as usize;
                for _ in 0..count.min(sym_stack.len()) {
                    sym_stack.pop();
                }
            }

            // ── Unknown: skip operands heuristically ─────────────
            _ => {
                stats.unknown_opcodes += 1;
            }
        }
    }

    builder.end_function();
    (builder.build(), stats)
}

/// Decode statistics.
#[derive(Debug, Default)]
pub struct DecodeStats {
    pub instructions_decoded: usize,
    pub unknown_opcodes: usize,
    pub block_count: usize,
    pub jumps: usize,
    pub branches: usize,
    pub returns: usize,
    pub halts: usize,
}

// ============================================================================
// Helpers
// ============================================================================

fn stack_pop(sym_stack: &mut Vec<Var>, builder: &mut IrBuilder) -> Var {
    sym_stack.pop().unwrap_or_else(|| {
        // Stack underflow — emit an undefined placeholder
        builder.const_undefined()
    })
}

fn binary_op(builder: &mut IrBuilder, sym_stack: &mut Vec<Var>, op: OpCode, source: SourceLoc) {
    let right = stack_pop(sym_stack, builder);
    let left = stack_pop(sym_stack, builder);
    let result = builder.emit_sourced(op, vec![Operand::Var(left), Operand::Var(right)], source);
    sym_stack.push(result);
}

fn unary_op(builder: &mut IrBuilder, sym_stack: &mut Vec<Var>, op: OpCode, source: SourceLoc) {
    let operand = stack_pop(sym_stack, builder);
    let result = builder.emit_sourced(op, vec![Operand::Var(operand)], source);
    sym_stack.push(result);
}

fn imm_binary_op(
    builder: &mut IrBuilder,
    sym_stack: &mut Vec<Var>,
    reader: &mut Plv3Reader<'_>,
    op: OpCode,
    source: SourceLoc,
) {
    let immediate = reader.read_typed_value().unwrap_or(Value::Undefined);
    let operand = stack_pop(sym_stack, builder);
    let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(immediate)]);
    let result = builder.emit_sourced(op, vec![Operand::Var(operand), Operand::Var(imm_var)], source);
    sym_stack.push(result);
}

/// First pass: find all jump targets to create blocks.
fn find_block_starts(bytecode: &[u8]) -> Vec<usize> {
    let mut targets = std::collections::BTreeSet::new();
    targets.insert(0usize); // entry point
    let mut reader = Plv3Reader::new(bytecode);

    while !reader.at_end() {
        let Some(opcode) = reader.read_byte() else { break };
        match opcode {
            // JMP_BACK: target = position - offset
            20 => {
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let target = reader.position.saturating_sub(offset);
                targets.insert(target);
                targets.insert(reader.position); // fallthrough
            }
            // JMP_FWD: target = position + offset
            189 => {
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let target = reader.position + offset;
                targets.insert(target);
                targets.insert(reader.position); // fallthrough
            }
            // JMP_IF_FALSY, JMP_FALSY_KEEP, JMP_TRUTHY_KEEP
            42 | 169 | 89 => {
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let target = reader.position + offset;
                targets.insert(target);
                targets.insert(reader.position); // fallthrough
            }
            // Skip operands for known opcodes
            66 | 46 | 225 | 152 | 129 | 188 | 253 | 125 | 73 | 62
            | 101 | 250 => { reader.read_typed_value(); }
            29 | 85 | 172 | 18 | 22 | 224 | 106 | 191 | 179
            | 136 | 184 | 211 | 124 | 93 | 110 | 121 | 71 => { reader.read_u16_be(); }
            232 | 245 | 150 | 174 | 114 | 151 | 168 => {
                reader.read_u16_be(); reader.read_u16_be();
            }
            115 | 221 | 120 | 45 | 212 | 84 => {
                reader.read_u16_be(); reader.read_u16_be(); reader.read_u16_be();
            }
            31 | 54 | 111 | 183 | 26 | 229 | 249 => {
                reader.read_u16_be(); reader.read_typed_value();
            }
            118 | 47 | 44 => {
                reader.read_u16_be(); reader.read_u16_be(); reader.read_typed_value();
            }
            109 => { // PUSH_REG_PROP_AND_IMM: 2g+1i
                reader.read_u16_be(); reader.read_u16_be(); reader.read_typed_value();
            }
            238 => { // MULTI_PUSH: 1F + N*i
                let count = reader.read_byte().unwrap_or(0);
                for _ in 0..count { reader.read_typed_value(); }
            }
            55 => { // MAKE_FUNC: 1F + 1F + N*F
                let _params = reader.read_byte().unwrap_or(0);
                let captures = reader.read_byte().unwrap_or(0);
                for _ in 0..captures { reader.read_byte(); }
            }
            147 | 87 => { reader.read_byte(); } // CALL, NEW_CALL: 1F
            116 => { reader.read_u16_be(); } // RETURN_VAL: 1g
            251 => { reader.read_u16_be(); reader.read_u16_be(); reader.read_u16_be(); } // GET_STORE_PUSH2
            _ => {} // 0-operand opcodes
        }
    }

    targets.into_iter().collect()
}

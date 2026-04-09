//! PLV3 bytecode → IR decoder (complete: all 101 opcodes).
//!
//! Converts the stack-based PLV3 VM bytecode into the universal IR format.
//! Uses a symbolic stack to convert stack operations into SSA variables.

use vm_engine_core::ir::builder::IrBuilder;
use vm_engine_core::ir::opcode::OpCode;
use vm_engine_core::ir::operand::{Operand, SourceLoc};
use vm_engine_core::ir::{BlockId, Module, Var};
use vm_engine_core::value::Value;

use crate::reader::Plv3Reader;

/// Decode PLV3 bytecode into an IR Module.
pub fn decode_plv3(bytecode: &[u8]) -> (Module, DecodeStats) {
    let mut builder = IrBuilder::new();
    let mut stats = DecodeStats::default();
    let mut reader = Plv3Reader::new(bytecode);

    let block_starts = find_block_starts(bytecode);
    stats.block_count = block_starts.len();

    builder.begin_function("main");
    let mut sym_stack: Vec<Var> = Vec::new();

    let mut current_block = builder.create_and_switch("entry_0");
    let mut block_map: std::collections::HashMap<usize, BlockId> = std::collections::HashMap::new();
    block_map.insert(0, current_block);

    for &target_pc in &block_starts {
        if target_pc > 0 && !block_map.contains_key(&target_pc) {
            let block = builder.create_block(&format!("block_{target_pc}"));
            block_map.insert(target_pc, block);
        }
    }

    // Track whether the current block has been terminated (jump/branch/return/halt).
    // After termination, instructions go into the next block.
    let mut block_terminated = false;

    while !reader.at_end() {
        let instruction_pc = reader.position;

        // Start new block at jump targets OR after a terminated block
        let needs_new_block = (instruction_pc > 0 && block_map.contains_key(&instruction_pc))
            || block_terminated;

        if needs_new_block {
            let target_block = if let Some(&existing) = block_map.get(&instruction_pc) {
                existing
            } else {
                let new_block = builder.create_block(&format!("block_{instruction_pc}"));
                block_map.insert(instruction_pc, new_block);
                new_block
            };
            if !block_terminated {
                builder.jump(target_block);
            }
            builder.switch_to(target_block);
            current_block = target_block;
            sym_stack.clear();
            block_terminated = false;
        }

        let Some(opcode_byte) = reader.read_byte() else { break };
        stats.instructions_decoded += 1;
        let source = SourceLoc::with_opcode(instruction_pc, opcode_byte as u16);

        match opcode_byte {
            // ═══ ARITHMETIC (pop 2, push 1) ══════════════════════
            30  => binary_op(&mut builder, &mut sym_stack, OpCode::Add, source),
            105 => binary_op(&mut builder, &mut sym_stack, OpCode::Mul, source),
            164 => binary_op(&mut builder, &mut sym_stack, OpCode::Sub, source),
            2   => binary_op(&mut builder, &mut sym_stack, OpCode::Div, source),
            233 => binary_op(&mut builder, &mut sym_stack, OpCode::Mod, source),
            19  => binary_op(&mut builder, &mut sym_stack, OpCode::BitXor, source),
            104 => binary_op(&mut builder, &mut sym_stack, OpCode::BitAnd, source),
            138 => binary_op(&mut builder, &mut sym_stack, OpCode::BitOr, source),
            157 => binary_op(&mut builder, &mut sym_stack, OpCode::Shr, source),
            72  => binary_op(&mut builder, &mut sym_stack, OpCode::UShr, source),
            119 => binary_op(&mut builder, &mut sym_stack, OpCode::Shl, source),

            // ═══ UNARY (pop 1, push 1) ═══════════════════════════
            128 => unary_op(&mut builder, &mut sym_stack, OpCode::Neg, source),
            51  => unary_op(&mut builder, &mut sym_stack, OpCode::BitNot, source),
            64  => unary_op(&mut builder, &mut sym_stack, OpCode::LogicalNot, source),
            41  => unary_op(&mut builder, &mut sym_stack, OpCode::Pos, source),
            175 => unary_op(&mut builder, &mut sym_stack, OpCode::TypeOf, source),
            15  => unary_op(&mut builder, &mut sym_stack, OpCode::Void, source),

            // ═══ COMPARISON (pop 2, push 1) ══════════════════════
            21  => binary_op(&mut builder, &mut sym_stack, OpCode::StrictEq, source),
            60  => binary_op(&mut builder, &mut sym_stack, OpCode::Eq, source),
            246 => binary_op(&mut builder, &mut sym_stack, OpCode::Lt, source),
            112 => binary_op(&mut builder, &mut sym_stack, OpCode::Gt, source),
            35 => { // SUB_EQ: (a - b === 0)
                let right = stack_pop(&mut sym_stack, &mut builder);
                let left = stack_pop(&mut sym_stack, &mut builder);
                let diff = builder.emit_sourced(OpCode::Sub, vec![Operand::Var(left), Operand::Var(right)], source);
                let zero = builder.const_number(0.0);
                let result = builder.emit(OpCode::StrictEq, vec![Operand::Var(diff), Operand::Var(zero)]);
                sym_stack.push(result);
            }
            57 => { // OP_IN: a in b
                let right = stack_pop(&mut sym_stack, &mut builder);
                let left = stack_pop(&mut sym_stack, &mut builder);
                let result = builder.emit_sourced(OpCode::HasProp, vec![Operand::Var(left), Operand::Var(right)], source);
                sym_stack.push(result);
            }

            // ═══ IMMEDIATE ARITHMETIC (pop 1, read imm, push 1) ═
            225       => imm_binary(&mut builder, &mut sym_stack, &mut reader, OpCode::Add, source),
            152 | 129 => imm_binary(&mut builder, &mut sym_stack, &mut reader, OpCode::BitXor, source),
            188       => imm_binary(&mut builder, &mut sym_stack, &mut reader, OpCode::BitAnd, source),
            253       => imm_binary(&mut builder, &mut sym_stack, &mut reader, OpCode::Shr, source),
            125       => imm_binary(&mut builder, &mut sym_stack, &mut reader, OpCode::UShr, source),
            73        => imm_binary(&mut builder, &mut sym_stack, &mut reader, OpCode::Shl, source),
            62        => imm_binary(&mut builder, &mut sym_stack, &mut reader, OpCode::Mod, source),

            // ═══ PUSH ════════════════════════════════════════════
            66 => { // PUSH typed value
                let value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let var = builder.emit_sourced(OpCode::Const, vec![Operand::Const(value)], source);
                sym_stack.push(var);
            }
            207 => { // PUSH_WINDOW
                let var = builder.emit_sourced(OpCode::LoadScope, vec![Operand::Const(Value::string("window"))], source);
                sym_stack.push(var);
            }
            46 => { // REPLACE_IMM: pop, push immediate
                sym_stack.pop();
                let value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let var = builder.emit_sourced(OpCode::Const, vec![Operand::Const(value)], source);
                sym_stack.push(var);
            }

            // ═══ PUSH REGISTER(S) ═══════════════════════════════
            29 => { // PUSH_REG (1g)
                let reg = reader.read_u16_be().unwrap_or(0);
                push_reg(&mut builder, &mut sym_stack, reg, source);
            }
            232 | 32 => { // PUSH2_REG / PUSH2_REG_ALT (2g)
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                push_reg(&mut builder, &mut sym_stack, reg_a, source);
                push_reg(&mut builder, &mut sym_stack, reg_b, source);
            }
            221 => { // PUSH3_REG (3g)
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                let reg_c = reader.read_u16_be().unwrap_or(0);
                push_reg(&mut builder, &mut sym_stack, reg_a, source);
                push_reg(&mut builder, &mut sym_stack, reg_b, source);
                push_reg(&mut builder, &mut sym_stack, reg_c, source);
            }
            150 => { // PUSH_REG_IMM (1g+1i): push reg, push imm
                let reg = reader.read_u16_be().unwrap_or(0);
                push_reg(&mut builder, &mut sym_stack, reg, source);
                let value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let var = builder.emit(OpCode::Const, vec![Operand::Const(value)]);
                sym_stack.push(var);
            }
            47 => { // PUSH2_REG_IMM (2g+1i): push reg, push reg, push imm
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                push_reg(&mut builder, &mut sym_stack, reg_a, source);
                push_reg(&mut builder, &mut sym_stack, reg_b, source);
                let value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let var = builder.emit(OpCode::Const, vec![Operand::Const(value)]);
                sym_stack.push(var);
            }
            31 => { // PUSH_REG_AND (1g+1i): push (reg[g] & imm)
                let reg = reader.read_u16_be().unwrap_or(0);
                let imm_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let reg_var = load_reg(&mut builder, reg, source);
                let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(imm_value)]);
                let result = builder.emit_sourced(OpCode::BitAnd, vec![Operand::Var(reg_var), Operand::Var(imm_var)], source);
                sym_stack.push(result);
            }
            118 => { // PUSH_REG_REGAND (2g+1i): push reg[a], push (reg[b] & imm)
                // NOTE: opcode 118 is shared with PUSH_REG_PROP_AND (2g+1i).
                // The cjs file defines it twice. In practice it's PUSH_REG_PROP_AND here.
                // push reg[a][reg[b] & imm]
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                let imm_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let obj = load_reg(&mut builder, reg_a, source);
                let key_raw = load_reg(&mut builder, reg_b, source);
                let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(imm_value)]);
                let masked_key = builder.emit(OpCode::BitAnd, vec![Operand::Var(key_raw), Operand::Var(imm_var)]);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(masked_key)], source);
                sym_stack.push(result);
            }

            // ═══ PUSH FRAME / UPVALUE ═══════════════════════════
            85 => { // LOAD_FRAME (1g)
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                let var = load_frame(&mut builder, frame_addr, source);
                sym_stack.push(var);
            }
            183 => { // PUSH_FRAME_IMM (1g+1i): push frame, push imm
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                let frame_var = load_frame(&mut builder, frame_addr, source);
                sym_stack.push(frame_var);
                let value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(value)]);
                sym_stack.push(imm_var);
            }
            174 => { // PUSH_REG_FRAME (2g): push reg[a], push frame[b]
                let reg = reader.read_u16_be().unwrap_or(0);
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                push_reg(&mut builder, &mut sym_stack, reg, source);
                let frame_var = load_frame(&mut builder, frame_addr, source);
                sym_stack.push(frame_var);
            }
            71 => { // LOAD_UPVAL (1g): push upvalue
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                let var = builder.emit_sourced(
                    OpCode::LoadScope,
                    vec![Operand::Const(Value::string(format!("upval_{frame_addr}")))],
                    source,
                );
                sym_stack.push(var);
            }

            // ═══ PUSH REG PROPERTY ══════════════════════════════
            115 => { // PUSH_REG_PROP (2g): push reg[a][reg[b]]
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                let obj = load_reg(&mut builder, reg_a, source);
                let key = load_reg(&mut builder, reg_b, source);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                sym_stack.push(result);
            }
            250 => { // PUSH_REG_PROP_IMM (1g+1i): push reg[g][imm]
                let reg = reader.read_u16_be().unwrap_or(0);
                let imm_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let obj = load_reg(&mut builder, reg, source);
                let key = builder.emit(OpCode::Const, vec![Operand::Const(imm_value)]);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                sym_stack.push(result);
            }
            109 => { // PUSH_REG_PROP_AND_IMM (2g+1i): push reg[a][reg[b] & imm]
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                let imm_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let obj = load_reg(&mut builder, reg_a, source);
                let key_raw = load_reg(&mut builder, reg_b, source);
                let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(imm_value)]);
                let masked_key = builder.emit(OpCode::BitAnd, vec![Operand::Var(key_raw), Operand::Var(imm_var)]);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(masked_key)], source);
                sym_stack.push(result);
            }

            // ═══ MULTI_PUSH ═════════════════════════════════════
            238 => {
                let count = reader.read_byte().unwrap_or(0) as usize;
                for _ in 0..count {
                    let value = reader.read_typed_value().unwrap_or(Value::Undefined);
                    let var = builder.emit(OpCode::Const, vec![Operand::Const(value)]);
                    sym_stack.push(var);
                }
            }

            // ═══ POP / SP_ADJ ═══════════════════════════════════
            155 => { sym_stack.pop(); }    // POP
            0   => { sym_stack.pop(); sym_stack.pop(); } // POP2
            179 => { // SP_ADJ (1g): drop N items
                let count = reader.read_u16_be().unwrap_or(0) as usize;
                for _ in 0..count.min(sym_stack.len()) { sym_stack.pop(); }
            }

            // ═══ COLLECT (pop N → reversed array) ═══════════════
            191 => {
                let count = reader.read_u16_be().unwrap_or(0) as usize;
                let array_var = builder.emit_sourced(OpCode::NewArray, vec![], source);
                let to_drain = count.min(sym_stack.len());
                let start = sym_stack.len() - to_drain;
                // COLLECT reverses the popped items (per VM spec: "pop N → reversed array")
                for index in 0..to_drain {
                    let elem = sym_stack[start + (to_drain - 1 - index)];
                    let idx_var = builder.const_number(index as f64);
                    builder.store_index(array_var, idx_var, elem);
                }
                sym_stack.truncate(start);
                sym_stack.push(array_var);
            }

            // ═══ ENUM_KEYS (TOS = Object.keys(TOS)) ════════════
            166 => {
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let result = builder.emit_sourced(
                    OpCode::CallMethod,
                    vec![Operand::Var(obj), Operand::Const(Value::string("__keys__"))],
                    source,
                );
                sym_stack.push(result);
            }

            // ═══ NEW_OBJ ════════════════════════════════════════
            106 => {
                let count = reader.read_u16_be().unwrap_or(0) as usize;
                let obj = builder.emit_sourced(OpCode::NewObject, vec![], source);
                // Pop key-value pairs and set properties
                let pairs = count.min(sym_stack.len() / 2);
                for _ in 0..pairs {
                    let val = stack_pop(&mut sym_stack, &mut builder);
                    let key = stack_pop(&mut sym_stack, &mut builder);
                    builder.store_prop(obj, key, val);
                }
                sym_stack.push(obj);
            }

            // ═══ PROPERTY GET ═══════════════════════════════════
            202 => { // GET_PROP: pop key, TOS = TOS[key]
                let key = stack_pop(&mut sym_stack, &mut builder);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                sym_stack.push(result);
            }
            101 => { // GET_PROP_IMM (1i): TOS = TOS[imm]
                let key_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let key = builder.emit(OpCode::Const, vec![Operand::Const(key_value)]);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                sym_stack.push(result);
            }
            136 => { // GET_PROP_REG (1g): TOS = TOS[reg[g]]
                let reg = reader.read_u16_be().unwrap_or(0);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let key = load_reg(&mut builder, reg, source);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                sym_stack.push(result);
            }
            184 => { // GET_PROP_FRAME (1g): TOS = TOS[frame[g]]
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let key = load_frame(&mut builder, frame_addr, source);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                sym_stack.push(result);
            }
            54 => { // GET_PROP_REG_AND (1g+1i): TOS = TOS[reg[g] & imm]
                let reg = reader.read_u16_be().unwrap_or(0);
                let imm_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let key_raw = load_reg(&mut builder, reg, source);
                let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(imm_value)]);
                let masked = builder.emit(OpCode::BitAnd, vec![Operand::Var(key_raw), Operand::Var(imm_var)]);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(masked)], source);
                sym_stack.push(result);
            }
            26 => { // GET_XOR_IMM (1i): pop key, TOS = TOS[key] ^ imm
                let imm_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let key = stack_pop(&mut sym_stack, &mut builder);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let prop = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(imm_value)]);
                let result = builder.emit(OpCode::BitXor, vec![Operand::Var(prop), Operand::Var(imm_var)]);
                sym_stack.push(result);
            }
            229 => { // GET_PROP_PUSH_IMM (1i): pop key, TOS=TOS[key], push imm
                let imm_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let key = stack_pop(&mut sym_stack, &mut builder);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                sym_stack.push(result);
                let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(imm_value)]);
                sym_stack.push(imm_var);
            }
            178 => { // GET_PROP_MASKED: pop idx, pop mask, TOS = TOS[mask & idx]
                let idx = stack_pop(&mut sym_stack, &mut builder);
                let mask = stack_pop(&mut sym_stack, &mut builder);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let masked = builder.emit(OpCode::BitAnd, vec![Operand::Var(mask), Operand::Var(idx)]);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(masked)], source);
                sym_stack.push(result);
            }
            249 => { // GET_PROP_AND_IMM (1i): pop idx, TOS = TOS[idx & imm]
                let imm_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let idx = stack_pop(&mut sym_stack, &mut builder);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(imm_value)]);
                let masked = builder.emit(OpCode::BitAnd, vec![Operand::Var(idx), Operand::Var(imm_var)]);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(masked)], source);
                sym_stack.push(result);
            }

            // ═══ PROPERTY SET ═══════════════════════════════════
            81 => { // SET_PROP: pop key, pop obj, obj[key] = TOS (peek)
                let key = stack_pop(&mut sym_stack, &mut builder);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                if let Some(&val) = sym_stack.last() {
                    builder.store_prop(obj, key, val);
                }
            }
            237 => { // SET_PROP3: pop key, pop obj, pop val → obj[key]=val
                let key = stack_pop(&mut sym_stack, &mut builder);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let val = stack_pop(&mut sym_stack, &mut builder);
                builder.store_prop(obj, key, val);
            }

            // ═══ REGISTER STORE ═════════════════════════════════
            224 => { // STORE_REG (1g): pop → reg[g]
                let reg = reader.read_u16_be().unwrap_or(0);
                let value = stack_pop(&mut sym_stack, &mut builder);
                store_reg(&mut builder, reg, value);
            }
            22 => { // STORE_PEEK (1g): reg[g] = TOS (no pop)
                let reg = reader.read_u16_be().unwrap_or(0);
                if let Some(&tos) = sym_stack.last() {
                    store_reg(&mut builder, reg, tos);
                }
            }

            // ═══ REGISTER ARITHMETIC ════════════════════════════
            211 => { // XOR_REG (1g): TOS ^= reg[g]
                let reg = reader.read_u16_be().unwrap_or(0);
                let tos = stack_pop(&mut sym_stack, &mut builder);
                let reg_var = load_reg(&mut builder, reg, source);
                let result = builder.emit_sourced(OpCode::BitXor, vec![Operand::Var(tos), Operand::Var(reg_var)], source);
                sym_stack.push(result);
            }
            124 => { // SUB_REG (1g): TOS -= reg[g]
                let reg = reader.read_u16_be().unwrap_or(0);
                let tos = stack_pop(&mut sym_stack, &mut builder);
                let reg_var = load_reg(&mut builder, reg, source);
                let result = builder.emit_sourced(OpCode::Sub, vec![Operand::Var(tos), Operand::Var(reg_var)], source);
                sym_stack.push(result);
            }

            // ═══ REGISTER PROPERTY SET ══════════════════════════
            245 => { // SET_REG_PROP (2g): pop val → reg[a][reg[b]] = val
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                let val = stack_pop(&mut sym_stack, &mut builder);
                let obj = load_reg(&mut builder, reg_a, source);
                let key = load_reg(&mut builder, reg_b, source);
                builder.store_prop(obj, key, val);
            }
            44 => { // SET_REG_PROP_AND (2g+1i): reg[a][reg[b] & imm] = TOS, pop
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                let imm_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let val = stack_pop(&mut sym_stack, &mut builder);
                let obj = load_reg(&mut builder, reg_a, source);
                let key_raw = load_reg(&mut builder, reg_b, source);
                let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(imm_value)]);
                let masked_key = builder.emit(OpCode::BitAnd, vec![Operand::Var(key_raw), Operand::Var(imm_var)]);
                builder.store_prop(obj, masked_key, val);
            }
            84 => { // REG_PROP_TO_REG (3g): reg[c] = reg[a][reg[b]]
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                let reg_c = reader.read_u16_be().unwrap_or(0);
                let obj = load_reg(&mut builder, reg_a, source);
                let key = load_reg(&mut builder, reg_b, source);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                store_reg(&mut builder, reg_c, result);
            }

            // ═══ FRAME STORE ════════════════════════════════════
            172 => { // STORE_FRAME (1g): frame[g] = TOS (peek)
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                if let Some(&tos) = sym_stack.last() {
                    store_frame(&mut builder, frame_addr, tos);
                }
            }
            18 => { // STORE_FRAME_POP (1g): pop → frame[g]
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                let value = stack_pop(&mut sym_stack, &mut builder);
                store_frame(&mut builder, frame_addr, value);
            }
            110 => { // AUTO_INC (1g): frame[g]++
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                let current = load_frame(&mut builder, frame_addr, source);
                let one = builder.const_number(1.0);
                let incremented = builder.emit(OpCode::Add, vec![Operand::Var(current), Operand::Var(one)]);
                store_frame(&mut builder, frame_addr, incremented);
            }
            121 => { // STORE_UPVAL (1g): upvalue[g] = TOS (peek)
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                if let Some(&tos) = sym_stack.last() {
                    builder.emit_void(OpCode::StoreScope, vec![
                        Operand::Const(Value::string(format!("upval_{frame_addr}"))),
                        Operand::Var(tos),
                    ]);
                }
            }

            // ═══ FUSED: POP + PUSH REG(S) ══════════════════════
            93 => { // POP_PUSH_REG (1g): pop, push reg
                let reg = reader.read_u16_be().unwrap_or(0);
                sym_stack.pop();
                push_reg(&mut builder, &mut sym_stack, reg, source);
            }
            176 => { // POP_PUSH_REG_B (1g): pop, push reg (alt)
                let reg = reader.read_u16_be().unwrap_or(0);
                sym_stack.pop();
                push_reg(&mut builder, &mut sym_stack, reg, source);
            }
            168 => { // POP_PUSH2_REG (2g): pop, push 2 regs
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                sym_stack.pop();
                push_reg(&mut builder, &mut sym_stack, reg_a, source);
                push_reg(&mut builder, &mut sym_stack, reg_b, source);
            }

            // ═══ FUSED: STORE + POP + PUSH ═════════════════════
            151 => { // STORE_POP_PUSH (2g): store TOS→reg, pop, push reg
                let reg_store = reader.read_u16_be().unwrap_or(0);
                let reg_push = reader.read_u16_be().unwrap_or(0);
                if let Some(&tos) = sym_stack.last() { store_reg(&mut builder, reg_store, tos); }
                sym_stack.pop();
                push_reg(&mut builder, &mut sym_stack, reg_push, source);
            }
            10 => { // STORE_POP_PUSH2_B (3g): store TOS→reg, pop, push 2 regs
                let reg_store = reader.read_u16_be().unwrap_or(0);
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                if let Some(&tos) = sym_stack.last() { store_reg(&mut builder, reg_store, tos); }
                sym_stack.pop();
                push_reg(&mut builder, &mut sym_stack, reg_a, source);
                push_reg(&mut builder, &mut sym_stack, reg_b, source);
            }
            251 => { // STORE_POP_PUSH2 (3g): store TOS→reg, pop, push 2 regs
                let reg_store = reader.read_u16_be().unwrap_or(0);
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                if let Some(&tos) = sym_stack.last() { store_reg(&mut builder, reg_store, tos); }
                sym_stack.pop();
                push_reg(&mut builder, &mut sym_stack, reg_a, source);
                push_reg(&mut builder, &mut sym_stack, reg_b, source);
            }
            111 => { // STORE_PUSH_IMM (1g+1i): pop→reg, push imm
                let reg = reader.read_u16_be().unwrap_or(0);
                let imm_value = reader.read_typed_value().unwrap_or(Value::Undefined);
                let value = stack_pop(&mut sym_stack, &mut builder);
                store_reg(&mut builder, reg, value);
                let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(imm_value)]);
                sym_stack.push(imm_var);
            }

            // ═══ FUSED: XOR + STORE + PUSH2 ════════════════════
            212 => { // XOR_STORE_PUSH2 (3g): XOR top 2, store result, pop, push 2 regs
                let reg_store = reader.read_u16_be().unwrap_or(0);
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                let right = stack_pop(&mut sym_stack, &mut builder);
                let left = stack_pop(&mut sym_stack, &mut builder);
                let xored = builder.emit_sourced(OpCode::BitXor, vec![Operand::Var(left), Operand::Var(right)], source);
                store_reg(&mut builder, reg_store, xored);
                push_reg(&mut builder, &mut sym_stack, reg_a, source);
                push_reg(&mut builder, &mut sym_stack, reg_b, source);
            }

            // ═══ FUSED: SET_PROP from REGS ═════════════════════
            120 => { // SET_3REG (3g): push 3 regs, set_prop, pop
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                let reg_c = reader.read_u16_be().unwrap_or(0);
                let obj = load_reg(&mut builder, reg_a, source);
                let key = load_reg(&mut builder, reg_b, source);
                let val = load_reg(&mut builder, reg_c, source);
                builder.store_prop(obj, key, val);
            }
            45 => { // SET_POP_PUSH3 (3g): set_prop(3 from stack), pop, push 3 regs
                let reg_a = reader.read_u16_be().unwrap_or(0);
                let reg_b = reader.read_u16_be().unwrap_or(0);
                let reg_c = reader.read_u16_be().unwrap_or(0);
                // set_prop from stack: pop key, pop obj, peek val
                let key = stack_pop(&mut sym_stack, &mut builder);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                if let Some(&val) = sym_stack.last() { builder.store_prop(obj, key, val); }
                sym_stack.pop(); // pop the val too
                push_reg(&mut builder, &mut sym_stack, reg_a, source);
                push_reg(&mut builder, &mut sym_stack, reg_b, source);
                push_reg(&mut builder, &mut sym_stack, reg_c, source);
            }

            // ═══ FUSED: GET_PROP + STORE + PUSH ════════════════
            114 => { // GET_STORE_PUSH (2g): get_prop, store→reg, pop, push reg
                let reg_store = reader.read_u16_be().unwrap_or(0);
                let reg_push = reader.read_u16_be().unwrap_or(0);
                let key = stack_pop(&mut sym_stack, &mut builder);
                let obj = stack_pop(&mut sym_stack, &mut builder);
                let result = builder.emit_sourced(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)], source);
                store_reg(&mut builder, reg_store, result);
                push_reg(&mut builder, &mut sym_stack, reg_push, source);
            }

            // ═══ CALLS ══════════════════════════════════════════
            255 => { // BIND_CALL: pop func + thisObj → push bound
                let this_obj = stack_pop(&mut sym_stack, &mut builder);
                let func = stack_pop(&mut sym_stack, &mut builder);
                let result = builder.emit_sourced(
                    OpCode::Call,
                    vec![Operand::Const(Value::string("__bind__")), Operand::Var(func), Operand::Var(this_obj)],
                    source,
                );
                sym_stack.push(result);
            }
            147 => { // CALL (1F): pop callable, pop argc args → push result
                let argc = reader.read_byte().unwrap_or(0) as usize;
                let mut args: Vec<Var> = Vec::new();
                for _ in 0..argc { args.push(stack_pop(&mut sym_stack, &mut builder)); }
                args.reverse();
                let callable = stack_pop(&mut sym_stack, &mut builder);
                let mut operands = vec![Operand::Var(callable)];
                operands.extend(args.iter().map(|var| Operand::Var(*var)));
                let result = builder.emit_sourced(OpCode::Call, operands, source);
                sym_stack.push(result);
            }
            87 => { // NEW_CALL (1F): pop constructor, pop argc args → push result
                let argc = reader.read_byte().unwrap_or(0) as usize;
                let mut args: Vec<Var> = Vec::new();
                for _ in 0..argc { args.push(stack_pop(&mut sym_stack, &mut builder)); }
                args.reverse();
                let constructor = stack_pop(&mut sym_stack, &mut builder);
                let mut operands = vec![Operand::Const(Value::string("new")), Operand::Var(constructor)];
                operands.extend(args.iter().map(|var| Operand::Var(*var)));
                let result = builder.emit_sourced(OpCode::Call, operands, source);
                sym_stack.push(result);
            }

            // ═══ MAKE_FUNC ══════════════════════════════════════
            55 => {
                let param_count = reader.read_byte().unwrap_or(0);
                let capture_count = reader.read_byte().unwrap_or(0);
                for _ in 0..capture_count { reader.read_byte(); }
                let var = builder.emit_sourced(
                    OpCode::Const,
                    vec![Operand::Const(Value::string(format!("<closure params={param_count} captures={capture_count}>")))],
                    source,
                );
                sym_stack.push(var);
            }

            // ═══ CONTROL FLOW ═══════════════════════════════════
            189 => { // JMP_FWD (1g)
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let target_pc = reader.position + offset;
                if let Some(&target_block) = block_map.get(&target_pc) {
                    builder.jump(target_block);
                }
                stats.jumps += 1; block_terminated = true;
            }
            20 => { // JMP_BACK (1g)
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let target_pc = reader.position.saturating_sub(offset);
                if let Some(&target_block) = block_map.get(&target_pc) {
                    builder.jump(target_block);
                }
                stats.jumps += 1; block_terminated = true;
            }
            42 => { // JMP_IF_FALSY (1g): pop, if falsy jump
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let target_pc = reader.position + offset;
                let cond = stack_pop(&mut sym_stack, &mut builder);
                if let Some(&false_block) = block_map.get(&target_pc) {
                    let true_block = block_map.get(&reader.position).copied().unwrap_or(current_block);
                    builder.branch_if(cond, true_block, false_block);
                }
                stats.branches += 1; block_terminated = true;
            }
            169 => { // JMP_FALSY_KEEP (1g): if TOS falsy → keep TOS, jump; else pop, continue
                // This is JS || short-circuit
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let target_pc = reader.position + offset;
                if let Some(&cond_var) = sym_stack.last()
                    && let Some(&target_block) = block_map.get(&target_pc)
                {
                    let continue_block = block_map.get(&reader.position).copied().unwrap_or(current_block);
                    builder.branch_if(cond_var, continue_block, target_block);
                }
                stats.branches += 1; block_terminated = true;
            }
            89 => { // JMP_TRUTHY_KEEP (1g): if TOS truthy → keep TOS, jump; else pop, continue
                // This is JS && short-circuit
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let target_pc = reader.position + offset;
                if let Some(&cond_var) = sym_stack.last()
                    && let Some(&target_block) = block_map.get(&target_pc)
                {
                    let continue_block = block_map.get(&reader.position).copied().unwrap_or(current_block);
                    builder.branch_if(cond_var, target_block, continue_block);
                }
                stats.branches += 1; block_terminated = true;
            }

            // ═══ RETURN / HALT ══════════════════════════════════
            197 => { // RETURN: pop retval
                let return_value = sym_stack.pop();
                builder.ret(return_value);
                stats.returns += 1; block_terminated = true;
            }
            116 => { // RETURN_VAL (1g): return frame[g]
                let frame_addr = reader.read_u16_be().unwrap_or(0);
                let frame_var = load_frame(&mut builder, frame_addr, source);
                builder.ret(Some(frame_var));
                stats.returns += 1; block_terminated = true;
            }
            195 => { // HALT
                builder.halt();
                stats.halts += 1; block_terminated = true;
            }

            _ => { stats.unknown_opcodes += 1; }
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
    sym_stack.pop().unwrap_or_else(|| builder.const_undefined())
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

fn imm_binary(builder: &mut IrBuilder, sym_stack: &mut Vec<Var>, reader: &mut Plv3Reader<'_>, op: OpCode, source: SourceLoc) {
    let immediate_value = reader.read_typed_value().unwrap_or(Value::Undefined);
    let operand = stack_pop(sym_stack, builder);
    let imm_var = builder.emit(OpCode::Const, vec![Operand::Const(immediate_value)]);
    let result = builder.emit_sourced(op, vec![Operand::Var(operand), Operand::Var(imm_var)], source);
    sym_stack.push(result);
}

/// Load a register value as an IR variable.
fn load_reg(builder: &mut IrBuilder, reg: u16, source: SourceLoc) -> Var {
    builder.emit_sourced(
        OpCode::LoadScope,
        vec![Operand::Const(Value::string(format!("r{reg}")))],
        source,
    )
}

/// Push a register value onto the symbolic stack.
fn push_reg(builder: &mut IrBuilder, sym_stack: &mut Vec<Var>, reg: u16, source: SourceLoc) {
    let var = load_reg(builder, reg, source);
    sym_stack.push(var);
}

/// Store a value into a register.
fn store_reg(builder: &mut IrBuilder, reg: u16, value: Var) {
    builder.emit_void(OpCode::StoreScope, vec![
        Operand::Const(Value::string(format!("r{reg}"))),
        Operand::Var(value),
    ]);
}

/// Load a frame slot value as an IR variable.
fn load_frame(builder: &mut IrBuilder, frame_addr: u16, source: SourceLoc) -> Var {
    builder.emit_sourced(
        OpCode::LoadScope,
        vec![Operand::Const(Value::string(format!("frame_{frame_addr}")))],
        source,
    )
}

/// Store a value into a frame slot.
fn store_frame(builder: &mut IrBuilder, frame_addr: u16, value: Var) {
    builder.emit_void(OpCode::StoreScope, vec![
        Operand::Const(Value::string(format!("frame_{frame_addr}"))),
        Operand::Var(value),
    ]);
}

/// First pass: find all jump targets to create blocks.
fn find_block_starts(bytecode: &[u8]) -> Vec<usize> {
    let mut targets = std::collections::BTreeSet::new();
    targets.insert(0usize);
    let mut reader = Plv3Reader::new(bytecode);

    while !reader.at_end() {
        let Some(opcode) = reader.read_byte() else { break };
        match opcode {
            20 => { // JMP_BACK
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                targets.insert(reader.position.saturating_sub(offset));
                targets.insert(reader.position);
            }
            189 => { // JMP_FWD
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                targets.insert(reader.position + offset);
                targets.insert(reader.position);
            }
            42 | 169 | 89 => { // conditional jumps
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                targets.insert(reader.position + offset);
                targets.insert(reader.position);
            }
            // Skip operands for all known opcodes (must match decode lengths exactly)
            66 | 46 | 225 | 152 | 129 | 188 | 253 | 125 | 73 | 62 | 101 | 250 => {
                reader.read_typed_value();
            }
            29 | 85 | 172 | 18 | 22 | 224 | 106 | 191 | 179 | 136 | 184 | 211
            | 124 | 93 | 110 | 121 | 71 | 176 => {
                reader.read_u16_be();
            }
            232 | 32 | 245 | 150 | 174 | 114 | 151 | 168 | 115 => {
                reader.read_u16_be(); reader.read_u16_be();
            }
            221 | 120 | 45 | 212 | 84 | 10 | 251 => {
                reader.read_u16_be(); reader.read_u16_be(); reader.read_u16_be();
            }
            31 | 54 | 111 | 183 | 26 | 229 | 249 => {
                reader.read_u16_be(); reader.read_typed_value();
            }
            118 | 47 | 44 | 109 => {
                reader.read_u16_be(); reader.read_u16_be(); reader.read_typed_value();
            }
            238 => { // MULTI_PUSH
                let count = reader.read_byte().unwrap_or(0);
                for _ in 0..count { reader.read_typed_value(); }
            }
            55 => { // MAKE_FUNC
                let _params = reader.read_byte().unwrap_or(0);
                let captures = reader.read_byte().unwrap_or(0);
                for _ in 0..captures { reader.read_byte(); }
            }
            147 | 87 => { reader.read_byte(); } // CALL, NEW_CALL
            116 => { reader.read_u16_be(); }     // RETURN_VAL
            _ => {} // 0-operand opcodes
        }
    }

    targets.into_iter().collect()
}

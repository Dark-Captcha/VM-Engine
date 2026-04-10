//! Direct PLV3 bytecode interpreter — stack-based, no IR conversion.
//!
//! This is the fix for the key extraction problem. The IR interpreter fails
//! because SSA IR can't model 170+ stack values persisting across block
//! boundaries. This interpreter maintains a real runtime stack, just like
//! the original JS VM.

use std::collections::HashMap;
use vm_engine::exec::heap::Heap;
use vm_engine::value::{ObjectId, Value};
use vm_engine::value::coerce;
use vm_engine::value::ops::{self, BinaryOp, UnaryOp};

use crate::reader::Plv3Reader;
use crate::funcmap::{find_func_ranges, FuncRange};

// ============================================================================
// VM State
// ============================================================================

/// A bound call: function + thisObj, created by BIND_CALL (opcode 255).
/// When called via CALL, receives argc, grabs args from VM stack, calls func.apply(this, args).
#[derive(Debug, Clone)]
struct BoundCall {
    /// The actual function to call (heap object or PLV3 closure).
    func: Value,
    /// The `this` binding.
    this_obj: Value,
}

/// A PLV3 closure created by MAKE_FUNC (opcode 55).
#[derive(Debug, Clone)]
struct Plv3Closure {
    body_start: usize,
    body_end: usize,
    param_count: u8,
    captures: Vec<Value>,
}

/// A call frame on the call stack.
#[derive(Debug, Clone)]
struct CallFrame {
    /// Return PC (where to resume after return).
    return_pc: usize,
    /// Base of frame slots for this call.
    frame_base: usize,
    /// Stack depth at call entry (to restore on return).
    stack_base: usize,
}

/// Hook trait for intercepting specific calls.
pub trait InterpreterHook {
    /// Called when a native or closure function is invoked by name.
    /// Return Some(value) to override the return value.
    fn on_native_call(&mut self, name: &str, args: &[Value], heap: &mut Heap) -> Option<Value>;
}

pub struct Plv3Vm<'a> {
    bytecode: &'a [u8],
    reader: Plv3Reader<'a>,
    /// Runtime stack (the whole point — persists across everything).
    pub stack: Vec<Value>,
    /// Registers (r0..rN) — addressed by u16.
    regs: Vec<Value>,
    /// Call stack.
    call_stack: Vec<CallFrame>,
    /// Heap for objects.
    pub heap: Heap,
    /// Global object id.
    pub global: ObjectId,
    /// PLV3 closures (indexed by body_start PC).
    closures: HashMap<usize, Plv3Closure>,
    /// Bound calls (indexed by wrapper ObjectId).
    bound_calls: HashMap<u32, BoundCall>,
    /// Function ranges from funcmap.
    func_ranges: Vec<FuncRange>,
    /// Skip ranges (function bodies to skip in main code).
    skip_ranges: Vec<(usize, usize)>,
    /// Instruction counter.
    pub instruction_count: u64,
    /// Max instructions (0 = unlimited).
    pub max_instructions: u64,
    /// Whether execution completed normally (HALT).
    pub halted: bool,
}

impl<'a> Plv3Vm<'a> {
    pub fn new(bytecode: &'a [u8]) -> Self {
        let func_map = find_func_ranges(bytecode);
        let skip_ranges: Vec<(usize, usize)> = func_map.functions.iter()
            .map(|f| (f.body_start, f.body_end))
            .collect();
        let func_ranges = func_map.functions.clone();

        let mut heap = Heap::new();
        let global = heap.alloc();

        Self {
            bytecode,
            reader: Plv3Reader::new(bytecode),
            stack: Vec::with_capacity(256),
            regs: vec![Value::Undefined; 1024],
            call_stack: Vec::new(),
            heap,
            global,
            closures: HashMap::new(),
            bound_calls: HashMap::new(),
            func_ranges,
            skip_ranges,
            instruction_count: 0,
            max_instructions: 10_000_000,
            halted: false,
        }
    }

    /// Run the VM until HALT, RETURN from top level, or instruction limit.
    pub fn run(&mut self, hook: &mut dyn InterpreterHook) -> Result<(), String> {
        self.reader.position = 0;

        while !self.reader.at_end() && !self.halted {
            if self.max_instructions > 0 && self.instruction_count >= self.max_instructions {
                return Err(format!("instruction limit {} reached", self.max_instructions));
            }

            let pc = self.reader.position;

            // Skip function bodies in main code
            if let Some(&(_, end)) = self.skip_ranges.iter().find(|(s, e)| pc >= *s && pc < *e) {
                self.reader.position = end;
                continue;
            }

            let opcode = match self.reader.read_byte() {
                Some(b) => b,
                None => break,
            };
            self.instruction_count += 1;

            self.dispatch(opcode, pc, hook)?;
        }

        Ok(())
    }

    fn dispatch(&mut self, opcode: u8, pc: usize, hook: &mut dyn InterpreterHook) -> Result<(), String> {
        match opcode {
            // ═══ ARITHMETIC (pop 2, push 1) ══════════════════════
            30  => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::Add, &l, &r)); }
            105 => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::Mul, &l, &r)); }
            164 => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::Sub, &l, &r)); }
            2   => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::Div, &l, &r)); }
            233 => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::Mod, &l, &r)); }
            19  => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::BitXor, &l, &r)); }
            104 => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::BitAnd, &l, &r)); }
            138 => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::BitOr, &l, &r)); }
            157 => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::Shr, &l, &r)); }
            72  => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::UShr, &l, &r)); }
            119 => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::Shl, &l, &r)); }

            // ═══ UNARY (pop 1, push 1) ═══════════════════════════
            128 => { let v = self.pop(); self.push(ops::unary(UnaryOp::Neg, &v)); }
            51  => { let v = self.pop(); self.push(ops::unary(UnaryOp::BitNot, &v)); }
            64  => { let v = self.pop(); self.push(ops::unary(UnaryOp::LogicalNot, &v)); }
            41  => { let v = self.pop(); self.push(Value::number(coerce::to_number(&v))); } // Pos
            175 => { let v = self.pop(); self.push(Value::string(type_of(&v))); } // TypeOf
            15  => { self.pop(); self.push(Value::Undefined); } // Void

            // ═══ COMPARISON (pop 2, push 1) ══════════════════════
            21  => { let r = self.pop(); let l = self.pop(); self.push(Value::bool(strict_eq(&l, &r))); }
            60  => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::Eq, &l, &r)); }
            246 => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::Lt, &l, &r)); }
            112 => { let r = self.pop(); let l = self.pop(); self.push(ops::binary(BinaryOp::Gt, &l, &r)); }
            35  => { // SUB_EQ: (a - b === 0)
                let r = self.pop(); let l = self.pop();
                let diff = coerce::to_number(&l) - coerce::to_number(&r);
                self.push(Value::bool(diff == 0.0));
            }
            57  => { // OP_IN: a in b
                let r = self.pop(); let l = self.pop();
                let result = match (&l, &r) {
                    (Value::String(key), Value::Object(oid)) => self.heap.has_property(*oid, key),
                    _ => false,
                };
                self.push(Value::bool(result));
            }

            // ═══ IMMEDIATE ARITHMETIC (pop 1, read imm, push 1) ═
            225       => { let imm = self.read_typed_value(); let v = self.pop(); self.push(ops::binary(BinaryOp::Add, &v, &imm)); }
            152 | 129 => { let imm = self.read_typed_value(); let v = self.pop(); self.push(ops::binary(BinaryOp::BitXor, &v, &imm)); }
            188       => { let imm = self.read_typed_value(); let v = self.pop(); self.push(ops::binary(BinaryOp::BitAnd, &v, &imm)); }
            253       => { let imm = self.read_typed_value(); let v = self.pop(); self.push(ops::binary(BinaryOp::Shr, &v, &imm)); }
            125       => { let imm = self.read_typed_value(); let v = self.pop(); self.push(ops::binary(BinaryOp::UShr, &v, &imm)); }
            73        => { let imm = self.read_typed_value(); let v = self.pop(); self.push(ops::binary(BinaryOp::Shl, &v, &imm)); }
            62        => { let imm = self.read_typed_value(); let v = self.pop(); self.push(ops::binary(BinaryOp::Mod, &v, &imm)); }

            // ═══ PUSH ════════════════════════════════════════════
            66  => { let v = self.read_typed_value(); self.push(v); }
            207 => { self.push(Value::Object(self.global)); } // PUSH_WINDOW
            46  => { self.pop(); let v = self.read_typed_value(); self.push(v); } // REPLACE_IMM

            // ═══ PUSH REGISTER(S) ═══════════════════════════════
            29  => { let r = self.read_u16(); self.push(self.get_reg(r)); }
            232 | 32 => { let a = self.read_u16(); let b = self.read_u16(); self.push(self.get_reg(a)); self.push(self.get_reg(b)); }
            221 => { let a = self.read_u16(); let b = self.read_u16(); let c = self.read_u16(); self.push(self.get_reg(a)); self.push(self.get_reg(b)); self.push(self.get_reg(c)); }
            150 => { let r = self.read_u16(); let v = self.read_typed_value(); self.push(self.get_reg(r)); self.push(v); }
            47  => { let a = self.read_u16(); let b = self.read_u16(); let v = self.read_typed_value(); self.push(self.get_reg(a)); self.push(self.get_reg(b)); self.push(v); }
            31  => { // PUSH_REG_AND (1g+1i)
                let r = self.read_u16(); let imm = self.read_typed_value();
                let rv = self.get_reg(r);
                self.push(ops::binary(BinaryOp::BitAnd, &rv, &imm));
            }
            118 => { // PUSH_REG_PROP_AND (2g+1i): push reg[a][reg[b] & imm]
                let a = self.read_u16(); let b = self.read_u16(); let imm = self.read_typed_value();
                let obj = self.get_reg(a);
                let key_raw = self.get_reg(b);
                let masked = ops::binary(BinaryOp::BitAnd, &key_raw, &imm);
                self.push(self.get_prop(&obj, &masked));
            }

            // ═══ PUSH FRAME / UPVALUE ═══════════════════════════
            85  => { let f = self.read_u16(); self.push(self.get_frame(f)); }
            183 => { let f = self.read_u16(); let v = self.read_typed_value(); self.push(self.get_frame(f)); self.push(v); }
            174 => { let r = self.read_u16(); let f = self.read_u16(); self.push(self.get_reg(r)); self.push(self.get_frame(f)); }
            71  => { let f = self.read_u16(); self.push(self.get_frame(f)); } // LOAD_UPVAL (same as frame for now)

            // ═══ PUSH REG PROPERTY ══════════════════════════════
            115 => { let a = self.read_u16(); let b = self.read_u16(); let obj = self.get_reg(a); let key = self.get_reg(b); self.push(self.get_prop(&obj, &key)); }
            250 => { let r = self.read_u16(); let imm = self.read_typed_value(); let obj = self.get_reg(r); self.push(self.get_prop(&obj, &imm)); }
            109 => { // PUSH_REG_PROP_AND_IMM (2g+1i)
                let a = self.read_u16(); let b = self.read_u16(); let imm = self.read_typed_value();
                let obj = self.get_reg(a);
                let key = ops::binary(BinaryOp::BitAnd, &self.get_reg(b), &imm);
                self.push(self.get_prop(&obj, &key));
            }

            // ═══ MULTI_PUSH ═════════════════════════════════════
            238 => {
                let count = self.reader.read_byte().unwrap_or(0) as usize;
                for _ in 0..count {
                    let v = self.read_typed_value();
                    self.push(v);
                }
            }

            // ═══ POP / SP_ADJ ═══════════════════════════════════
            155 => { self.pop(); }
            0   => { self.pop(); self.pop(); }
            179 => { let n = self.read_u16() as usize; for _ in 0..n.min(self.stack.len()) { self.pop(); } }

            // ═══ COLLECT (pop N → array object) ═════════════════
            191 => {
                let count = self.read_u16() as usize;
                let to_drain = count.min(self.stack.len());
                let start = self.stack.len() - to_drain;
                let items: Vec<Value> = self.stack.drain(start..).collect();
                // COLLECT reverses
                let arr = self.heap.alloc();
                for (i, item) in items.iter().rev().enumerate() {
                    self.heap.set_property(arr, &i.to_string(), item.clone());
                }
                self.heap.set_property(arr, "length", Value::number(items.len() as f64));
                self.push(Value::Object(arr));
            }

            // ═══ ENUM_KEYS ══════════════════════════════════════
            166 => {
                let obj = self.pop();
                if let Value::Object(oid) = obj {
                    if let Some(o) = self.heap.get(oid) {
                        let keys: Vec<String> = o.properties.keys().cloned().collect();
                        let arr = self.heap.alloc();
                        for (i, k) in keys.iter().enumerate() {
                            self.heap.set_property(arr, &i.to_string(), Value::string(k.clone()));
                        }
                        self.heap.set_property(arr, "length", Value::number(keys.len() as f64));
                        self.push(Value::Object(arr));
                    } else {
                        self.push(Value::Undefined);
                    }
                } else {
                    self.push(Value::Undefined);
                }
            }

            // ═══ NEW_OBJ ════════════════════════════════════════
            106 => {
                let count = self.read_u16() as usize;
                let obj = self.heap.alloc();
                for _ in 0..count {
                    let val = self.pop();
                    let key = self.pop();
                    let key_str = coerce::to_string(&key);
                    self.heap.set_property(obj, &key_str, val);
                }
                self.push(Value::Object(obj));
            }

            // ═══ PROPERTY GET ═══════════════════════════════════
            202 => { let key = self.pop(); let obj = self.pop(); self.push(self.get_prop(&obj, &key)); }
            101 => { let key = self.read_typed_value(); let obj = self.pop(); self.push(self.get_prop(&obj, &key)); }
            136 => { let r = self.read_u16(); let obj = self.pop(); let key = self.get_reg(r); self.push(self.get_prop(&obj, &key)); }
            184 => { let f = self.read_u16(); let obj = self.pop(); let key = self.get_frame(f); self.push(self.get_prop(&obj, &key)); }
            54  => { // GET_PROP_REG_AND (1g+1i)
                let r = self.read_u16(); let imm = self.read_typed_value();
                let obj = self.pop();
                let masked = ops::binary(BinaryOp::BitAnd, &self.get_reg(r), &imm);
                self.push(self.get_prop(&obj, &masked));
            }
            26  => { // GET_XOR_IMM (1i)
                let imm = self.read_typed_value();
                let key = self.pop(); let obj = self.pop();
                let prop = self.get_prop(&obj, &key);
                self.push(ops::binary(BinaryOp::BitXor, &prop, &imm));
            }
            229 => { // GET_PROP_PUSH_IMM (1i)
                let imm = self.read_typed_value();
                let key = self.pop(); let obj = self.pop();
                self.push(self.get_prop(&obj, &key));
                self.push(imm);
            }
            178 => { // GET_PROP_MASKED: pop idx, pop mask, TOS = TOS[mask & idx]
                let idx = self.pop(); let mask = self.pop(); let obj = self.pop();
                let masked = ops::binary(BinaryOp::BitAnd, &mask, &idx);
                self.push(self.get_prop(&obj, &masked));
            }
            249 => { // GET_PROP_AND_IMM (1i)
                let imm = self.read_typed_value();
                let idx = self.pop(); let obj = self.pop();
                let masked = ops::binary(BinaryOp::BitAnd, &idx, &imm);
                self.push(self.get_prop(&obj, &masked));
            }

            // ═══ PROPERTY SET ═══════════════════════════════════
            81  => { // SET_PROP: pop key, pop obj, obj[key] = TOS (peek)
                let key = self.pop(); let obj = self.pop();
                if let Some(tos) = self.stack.last() {
                    self.set_prop_val(&obj, &key, tos.clone());
                }
            }
            237 => { // SET_PROP3: pop key, pop obj, pop val
                let key = self.pop(); let obj = self.pop(); let val = self.pop();
                self.set_prop_val(&obj, &key, val);
            }

            // ═══ REGISTER STORE ═════════════════════════════════
            224 => { let r = self.read_u16(); let v = self.pop(); self.set_reg(r, v); }
            22  => { let r = self.read_u16(); if let Some(tos) = self.stack.last() { self.set_reg(r, tos.clone()); } }

            // ═══ REGISTER ARITHMETIC ════════════════════════════
            211 => { let r = self.read_u16(); let tos = self.pop(); let rv = self.get_reg(r); self.push(ops::binary(BinaryOp::BitXor, &tos, &rv)); }
            124 => { let r = self.read_u16(); let tos = self.pop(); let rv = self.get_reg(r); self.push(ops::binary(BinaryOp::Sub, &tos, &rv)); }

            // ═══ REGISTER PROPERTY SET ══════════════════════════
            245 => { let a = self.read_u16(); let b = self.read_u16(); let val = self.pop(); let obj = self.get_reg(a); let key = self.get_reg(b); self.set_prop_val(&obj, &key, val); }
            44  => { // SET_REG_PROP_AND (2g+1i)
                let a = self.read_u16(); let b = self.read_u16(); let imm = self.read_typed_value();
                let val = self.pop();
                let obj = self.get_reg(a);
                let key = ops::binary(BinaryOp::BitAnd, &self.get_reg(b), &imm);
                self.set_prop_val(&obj, &key, val);
            }
            84  => { // REG_PROP_TO_REG (3g)
                let a = self.read_u16(); let b = self.read_u16(); let c = self.read_u16();
                let obj = self.get_reg(a); let key = self.get_reg(b);
                let val = self.get_prop(&obj, &key);
                self.set_reg(c, val);
            }

            // ═══ FRAME STORE ════════════════════════════════════
            172 => { let f = self.read_u16(); if let Some(tos) = self.stack.last() { self.set_frame(f, tos.clone()); } }
            18  => { let f = self.read_u16(); let v = self.pop(); self.set_frame(f, v); }
            110 => { // AUTO_INC
                let f = self.read_u16();
                let current = coerce::to_number(&self.get_frame(f));
                self.set_frame(f, Value::number(current + 1.0));
            }
            121 => { let f = self.read_u16(); if let Some(tos) = self.stack.last() { self.set_frame(f, tos.clone()); } } // STORE_UPVAL

            // ═══ FUSED: POP + PUSH REG(S) ══════════════════════
            93 | 176 => { let r = self.read_u16(); self.pop(); self.push(self.get_reg(r)); }
            168 => { let a = self.read_u16(); let b = self.read_u16(); self.pop(); self.push(self.get_reg(a)); self.push(self.get_reg(b)); }

            // ═══ FUSED: STORE + POP + PUSH ═════════════════════
            151 => {
                let rs = self.read_u16(); let rp = self.read_u16();
                if let Some(tos) = self.stack.last() { self.set_reg(rs, tos.clone()); }
                self.pop();
                self.push(self.get_reg(rp));
            }
            10 | 251 => {
                let rs = self.read_u16(); let ra = self.read_u16(); let rb = self.read_u16();
                if let Some(tos) = self.stack.last() { self.set_reg(rs, tos.clone()); }
                self.pop();
                self.push(self.get_reg(ra)); self.push(self.get_reg(rb));
            }
            111 => { // STORE_PUSH_IMM (1g+1i)
                let r = self.read_u16(); let imm = self.read_typed_value();
                let v = self.pop(); self.set_reg(r, v);
                self.push(imm);
            }

            // ═══ FUSED: XOR + STORE + PUSH2 ════════════════════
            212 => {
                let rs = self.read_u16(); let ra = self.read_u16(); let rb = self.read_u16();
                let right = self.pop(); let left = self.pop();
                let xored = ops::binary(BinaryOp::BitXor, &left, &right);
                self.set_reg(rs, xored);
                self.push(self.get_reg(ra)); self.push(self.get_reg(rb));
            }

            // ═══ FUSED: SET_PROP from REGS ═════════════════════
            120 => {
                let a = self.read_u16(); let b = self.read_u16(); let c = self.read_u16();
                let obj = self.get_reg(a); let key = self.get_reg(b); let val = self.get_reg(c);
                self.set_prop_val(&obj, &key, val);
            }
            45  => { // SET_POP_PUSH3 (3g)
                let ra = self.read_u16(); let rb = self.read_u16(); let rc = self.read_u16();
                let key = self.pop(); let obj = self.pop();
                if let Some(val) = self.stack.last() { self.set_prop_val(&obj, &key, val.clone()); }
                self.pop();
                self.push(self.get_reg(ra)); self.push(self.get_reg(rb)); self.push(self.get_reg(rc));
            }

            // ═══ FUSED: GET_PROP + STORE + PUSH ════════════════
            114 => {
                let rs = self.read_u16(); let rp = self.read_u16();
                let key = self.pop(); let obj = self.pop();
                let result = self.get_prop(&obj, &key);
                self.set_reg(rs, result);
                self.push(self.get_reg(rp));
            }

            // ═══ CALLS ══════════════════════════════════════════
            255 => { // BIND_CALL: pop func, pop thisObj → push wrapper
                // Per PLV3 handler: var e = pop(func); var B = pop(thisObj);
                // Creates wrapper that, when called with argc, grabs args from stack.
                let func = self.pop();
                let this_obj = self.pop();
                let wrapper = self.heap.alloc();
                self.heap.set_property(wrapper, "__bound__", Value::bool(true));
                self.bound_calls.insert(wrapper.0, BoundCall {
                    func: func.clone(),
                    this_obj,
                });
                self.push(Value::Object(wrapper));
            }
            147 => { // CALL (1F): pop wrapper, call it with argc
                // Per PLV3 handler: var e = F(); (0, A[--sp])(e)
                // The wrapper (from BIND_CALL) receives argc, grabs args from stack.
                let argc = self.reader.read_byte().unwrap_or(0) as usize;
                let callable = self.pop();

                if let Value::Object(oid) = &callable {
                    if let Some(bound) = self.bound_calls.get(&oid.0).cloned() {
                        // Bound call: grab argc args from stack, call func.apply(this, args)
                        let mut args = Vec::with_capacity(argc);
                        for _ in 0..argc { args.push(self.pop()); }
                        args.reverse();
                        let result = self.call_value(&bound.func, &args, hook)?;
                        self.push(result);
                    } else {
                        // Direct call (not bound) — shouldn't happen often in PLV3
                        let mut args = Vec::with_capacity(argc);
                        for _ in 0..argc { args.push(self.pop()); }
                        args.reverse();
                        let result = self.call_value(&callable, &args, hook)?;
                        self.push(result);
                    }
                } else {
                    // Non-object callable
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc { args.push(self.pop()); }
                    args.reverse();
                    let result = self.call_value(&callable, &args, hook)?;
                    self.push(result);
                }
            }
            87  => { // NEW_CALL (1F): same as CALL but with `new`
                let argc = self.reader.read_byte().unwrap_or(0) as usize;
                let callable = self.pop();

                if let Value::Object(oid) = &callable {
                    if let Some(bound) = self.bound_calls.get(&oid.0).cloned() {
                        let mut args = Vec::with_capacity(argc);
                        for _ in 0..argc { args.push(self.pop()); }
                        args.reverse();
                        let result = self.new_call(&bound.func, &args, hook)?;
                        self.push(result);
                    } else {
                        let mut args = Vec::with_capacity(argc);
                        for _ in 0..argc { args.push(self.pop()); }
                        args.reverse();
                        let result = self.new_call(&callable, &args, hook)?;
                        self.push(result);
                    }
                } else {
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc { args.push(self.pop()); }
                    args.reverse();
                    let result = self.new_call(&callable, &args, hook)?;
                    self.push(result);
                }
            }

            // ═══ MAKE_FUNC ══════════════════════════════════════
            55  => {
                let _param_count = self.reader.read_byte().unwrap_or(0);
                let capture_count = self.reader.read_byte().unwrap_or(0);
                let mut captures = Vec::with_capacity(capture_count as usize);
                // Per handler 55: capture values from current frame
                let frame_base = self.call_stack.last().map(|f| f.frame_base).unwrap_or(0);
                for _ in 0..capture_count {
                    let cap_idx = self.reader.read_byte().unwrap_or(0) as usize;
                    let val = self.stack.get(frame_base + cap_idx).cloned().unwrap_or(Value::Undefined);
                    captures.push(val);
                }

                // Find the matching FuncRange to get body_start/body_end
                let make_func_pc = pc;
                if let Some(fr) = self.func_ranges.iter().find(|fr| fr.make_func_pc == make_func_pc) {
                    let closure = Plv3Closure {
                        body_start: fr.body_start,
                        body_end: fr.body_end,
                        param_count: fr.param_count,
                        captures: captures.clone(),
                    };
                    self.closures.insert(fr.body_start, closure);

                    // Push closure as a heap object with a marker
                    let closure_obj = self.heap.alloc();
                    self.heap.set_property(closure_obj, "__plv3_closure__", Value::number(fr.body_start as f64));
                    self.heap.set_property(closure_obj, "__param_count__", Value::number(fr.param_count as f64));
                    self.push(Value::Object(closure_obj));
                } else {
                    self.push(Value::Undefined);
                }

                // Consume JMP_FWD that follows MAKE_FUNC
                if !self.reader.at_end() && self.bytecode.get(self.reader.position) == Some(&189) {
                    self.reader.read_byte(); // consume 189
                    let offset = self.reader.read_u16_be().unwrap_or(0) as usize;
                    self.reader.position += offset;
                }
            }

            // ═══ CONTROL FLOW ═══════════════════════════════════
            189 => { // JMP_FWD (1g)
                let offset = self.read_u16() as usize;
                self.reader.position += offset;
            }
            20  => { // JMP_BACK (1g)
                let offset = self.read_u16() as usize;
                self.reader.position = self.reader.position.saturating_sub(offset);
            }
            42  => { // JMP_IF_FALSY (1g): pop, if falsy jump
                let offset = self.read_u16() as usize;
                let cond = self.pop();
                if is_falsy(&cond) {
                    self.reader.position += offset;
                }
            }
            169 => { // JMP_FALSY_KEEP (1g): if TOS falsy → keep, jump; else pop
                let offset = self.read_u16() as usize;
                let target = self.reader.position + offset;
                if let Some(tos) = self.stack.last() {
                    if is_falsy(tos) {
                        self.reader.position = target;
                    } else {
                        self.pop();
                    }
                }
            }
            89  => { // JMP_TRUTHY_KEEP (1g): if TOS truthy → keep, jump; else pop
                let offset = self.read_u16() as usize;
                let target = self.reader.position + offset;
                if let Some(tos) = self.stack.last() {
                    if !is_falsy(tos) {
                        self.reader.position = target;
                    } else {
                        self.pop();
                    }
                }
            }

            // ═══ RETURN / HALT ══════════════════════════════════
            197 => { // RETURN: pop retval, restore sp/ip/fp, push retval
                let retval = self.pop();
                if let Some(frame) = self.call_stack.pop() {
                    // Restore: sp = frame_base, pop saved_ip, pop saved_fp
                    self.stack.truncate(frame.frame_base);
                    self.stack.pop(); // saved return address (we use call_stack instead)
                    self.stack.pop(); // saved frame pointer
                    self.reader.position = frame.return_pc;
                    self.push(retval);
                } else {
                    self.halted = true;
                }
            }
            116 => { // RETURN_VAL (1g): return frame[g]
                let f = self.read_u16();
                let retval = self.get_frame(f);
                if let Some(frame) = self.call_stack.pop() {
                    self.stack.truncate(frame.frame_base);
                    self.stack.pop(); // saved return address
                    self.stack.pop(); // saved frame pointer
                    self.reader.position = frame.return_pc;
                    self.push(retval);
                } else {
                    self.halted = true;
                }
            }
            195 => { // HALT
                eprintln!("[HALT] PC={pc} instrs={} stack_depth={}", self.instruction_count, self.stack.len());
                self.halted = true;
            }

            _ => {
                // Unknown opcode — skip silently (matching decoder behavior)
            }
        }
        Ok(())
    }

    // ════════════════════════════════════════════════════════════════
    // Stack helpers
    // ════════════════════════════════════════════════════════════════

    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or(Value::Undefined)
    }

    // ════════════════════════════════════════════════════════════════
    // Reader helpers
    // ════════════════════════════════════════════════════════════════

    fn read_u16(&mut self) -> u16 {
        self.reader.read_u16_be().unwrap_or(0)
    }

    fn read_typed_value(&mut self) -> Value {
        self.reader.read_typed_value().unwrap_or(Value::Undefined)
    }

    // ════════════════════════════════════════════════════════════════
    // Register / frame access
    // ════════════════════════════════════════════════════════════════

    fn get_reg(&self, r: u16) -> Value {
        self.regs.get(r as usize).cloned().unwrap_or(Value::Undefined)
    }

    fn set_reg(&mut self, r: u16, v: Value) {
        let idx = r as usize;
        if idx >= self.regs.len() {
            self.regs.resize(idx + 1, Value::Undefined);
        }
        self.regs[idx] = v;
    }

    fn get_frame(&self, f: u16) -> Value {
        let idx = f as usize;
        let base = self.call_stack.last().map(|f| f.frame_base).unwrap_or(0);
        let abs = base + idx;
        self.stack.get(abs).cloned().unwrap_or(Value::Undefined)
    }

    fn set_frame(&mut self, f: u16, v: Value) {
        let idx = f as usize;
        let base = self.call_stack.last().map(|f| f.frame_base).unwrap_or(0);
        let abs = base + idx;
        // Extend stack if needed
        while self.stack.len() <= abs {
            self.stack.push(Value::Undefined);
        }
        self.stack[abs] = v;
    }

    // ════════════════════════════════════════════════════════════════
    // Property access
    // ════════════════════════════════════════════════════════════════

    fn get_prop(&self, obj: &Value, key: &Value) -> Value {
        match obj {
            Value::Object(oid) => {
                let key_str = match key {
                    Value::Number(n) => {
                        let i = *n as i64;
                        if (i as f64) == *n { i.to_string() } else { coerce::to_string(key) }
                    }
                    _ => coerce::to_string(key),
                };
                self.heap.get_property(*oid, &key_str)
            }
            Value::String(s) => {
                match key {
                    Value::String(k) if k == "length" => Value::number(s.len() as f64),
                    Value::Number(n) => {
                        let i = *n as usize;
                        s.chars().nth(i).map(|c| Value::string(c.to_string())).unwrap_or(Value::Undefined)
                    }
                    _ => Value::Undefined,
                }
            }
            _ => Value::Undefined,
        }
    }

    fn set_prop_val(&mut self, obj: &Value, key: &Value, val: Value) {
        if let Value::Object(oid) = obj {
            let key_str = match key {
                Value::Number(n) => {
                    let i = *n as i64;
                    if (i as f64) == *n { i.to_string() } else { coerce::to_string(key) }
                }
                _ => coerce::to_string(key),
            };
            self.heap.set_property(*oid, &key_str, val);
        }
    }

    // ════════════════════════════════════════════════════════════════
    // Call dispatch
    // ════════════════════════════════════════════════════════════════

    fn call_value(&mut self, callable: &Value, args: &[Value], hook: &mut dyn InterpreterHook) -> Result<Value, String> {
        match callable {
            Value::Object(oid) => {
                // Check if it's a PLV3 closure
                let body_start = self.heap.get_property(*oid, "__plv3_closure__");
                if let Value::Number(pc) = body_start {
                    static CC: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                    let cc = CC.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if cc < 5 {
                        let arg_preview: Vec<String> = args.iter().take(8).map(|a| match a {
                            Value::Number(n) => format!("{n}"),
                            Value::String(s) => format!("\"{s}\""),
                            Value::Object(oid) => format!("obj#{}", oid.0),
                            Value::Undefined => "undef".into(),
                            Value::Bool(b) => format!("{b}"),
                            _ => format!("{a:?}"),
                        }).collect();
                        eprintln!("[closure-call] #{cc} body={} argc={} args=[{}]", pc as usize, args.len(), arg_preview.join(", "));
                    }
                    return self.call_plv3_closure(pc as usize, args, hook);
                }

                // Resolve name: check __name__ property, or reverse-lookup from global
                let name_str = self.resolve_callable_name(*oid);

                // Try hook first
                if !name_str.is_empty() {
                    if let Some(result) = hook.on_native_call(&name_str, args, &mut self.heap) {
                        return Ok(result);
                    }
                }

                // Try heap.call
                if let Some(result) = self.heap.call(*oid, args) {
                    static NC: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                    let nc = NC.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if nc < 20 {
                        eprintln!("[native-ok] {name_str}({}) → {:?}", args.len(), &result);
                    }
                    return Ok(result);
                }

                static NF: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                let nf = NF.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if nf < 20 {
                    eprintln!("[native-FAIL] {name_str} oid={} not callable, args={}", oid.0, args.len());
                }

                Ok(Value::Undefined)
            }
            _ => Ok(Value::Undefined),
        }
    }

    /// Resolve a callable's name by checking __name__ or reverse-looking up from global/known objects.
    fn resolve_callable_name(&self, oid: ObjectId) -> String {
        // Check __name__ first
        if let Value::String(s) = self.heap.get_property(oid, "__name__") {
            return s;
        }

        // Reverse-lookup: is this a direct property of global?
        if let Some(global_obj) = self.heap.get(self.global) {
            for (name, val) in &global_obj.properties {
                if let Value::Object(id) = val {
                    if *id == oid {
                        return name.clone();
                    }
                }
            }
            // Check nested: JSON.stringify, String.fromCharCode, etc.
            for parent_name in &["JSON", "String", "Math", "Date", "console"] {
                if let Some(Value::Object(parent_oid)) = global_obj.properties.get(*parent_name) {
                    if let Some(parent_obj) = self.heap.get(*parent_oid) {
                        for (method_name, val) in &parent_obj.properties {
                            if let Value::Object(id) = val {
                                if *id == oid {
                                    return format!("{parent_name}.{method_name}");
                                }
                            }
                        }
                    }
                }
            }
        }

        String::new()
    }

    fn new_call(&mut self, constructor: &Value, args: &[Value], hook: &mut dyn InterpreterHook) -> Result<Value, String> {
        if let Value::Object(oid) = constructor {
            let name = self.heap.get_property(*oid, "__name__");
            let name_str = match &name {
                Value::String(s) => s.clone(),
                _ => String::new(),
            };

            // Try hook
            if let Some(result) = hook.on_native_call(&format!("new {name_str}"), args, &mut self.heap) {
                return Ok(result);
            }

            // Uint8Array constructor
            if name_str == "Uint8Array" {
                return Ok(self.construct_uint8array(args));
            }

            // Try heap.call for generic constructors
            if let Some(result) = self.heap.call(*oid, args) {
                return Ok(result);
            }
        }
        Ok(Value::Undefined)
    }

    fn call_plv3_closure(&mut self, body_start: usize, args: &[Value], hook: &mut dyn InterpreterHook) -> Result<Value, String> {
        let closure = match self.closures.get(&body_start).cloned() {
            Some(c) => c,
            None => return Ok(Value::Undefined),
        };

        // PLV3 calling convention (from handler 55):
        // Stack layout: [saved_frame_ptr] [saved_return_pc] [param0..paramN] [captures...]
        //                                                    ^frame_base (A[E])

        let return_pc = self.reader.position;
        let old_frame_base = self.call_stack.last().map(|f| f.frame_base).unwrap_or(0);
        let stack_base_before = self.stack.len();

        // Push saved frame pointer and return address (below frame)
        self.stack.push(Value::number(old_frame_base as f64));  // saved A[E]
        self.stack.push(Value::number(return_pc as f64));        // saved A[D]

        let frame_base = self.stack.len();

        // Push args as frame slots
        for arg in args {
            self.stack.push(arg.clone());
        }
        // Pad to param_count
        let param_count = closure.param_count as usize;
        while self.stack.len() < frame_base + param_count {
            self.stack.push(Value::Undefined);
        }
        // Push captures
        for cap in &closure.captures {
            self.stack.push(cap.clone());
        }

        self.call_stack.push(CallFrame {
            return_pc,
            frame_base,
            stack_base: stack_base_before,
        });

        // Jump to function body
        self.reader.position = body_start;
        let body_end = closure.body_end;

        // Execute until RETURN or body_end
        let depth_before = self.call_stack.len();
        while self.reader.position < body_end && !self.halted {
            if self.max_instructions > 0 && self.instruction_count >= self.max_instructions {
                return Err(format!("instruction limit {} reached in closure", self.max_instructions));
            }

            let opc_pc = self.reader.position;
            let opcode = match self.reader.read_byte() {
                Some(b) => b,
                None => break,
            };
            self.instruction_count += 1;

            self.dispatch(opcode, opc_pc, hook)?;

            // If a RETURN popped our frame, we're done
            if self.call_stack.len() < depth_before {
                return Ok(self.stack.last().cloned().unwrap_or(Value::Undefined));
            }
        }

        // Fell off end without RETURN
        if self.call_stack.len() >= depth_before {
            self.call_stack.pop();
        }
        self.stack.truncate(stack_base_before);
        Ok(Value::Undefined)
    }

    fn construct_uint8array(&mut self, args: &[Value]) -> Value {
        let arr = self.heap.alloc();
        if let Some(Value::Object(src_oid)) = args.first() {
            // Copy from array-like object
            let len = match self.heap.get_property(*src_oid, "length") {
                Value::Number(n) => n as usize,
                _ => 0,
            };
            for i in 0..len {
                let val = self.heap.get_property(*src_oid, &i.to_string());
                let byte = coerce::to_number(&val) as u8;
                self.heap.set_property(arr, &i.to_string(), Value::number(byte as f64));
            }
            self.heap.set_property(arr, "length", Value::number(len as f64));
        } else if let Some(Value::Number(n)) = args.first() {
            let len = *n as usize;
            for i in 0..len {
                self.heap.set_property(arr, &i.to_string(), Value::number(0.0));
            }
            self.heap.set_property(arr, "length", Value::number(len as f64));
        }
        Value::Object(arr)
    }
}

// ════════════════════════════════════════════════════════════════
// Helpers
// ════════════════════════════════════════════════════════════════

fn is_falsy(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => *n == 0.0 || n.is_nan(),
        Value::String(s) => s.is_empty(),
        _ => false,
    }
}

fn strict_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Null, Value::Null) => true,
        (Value::Undefined, Value::Undefined) => true,
        (Value::Object(x), Value::Object(y)) => x == y,
        _ => false,
    }
}

fn type_of(v: &Value) -> String {
    match v {
        Value::Number(_) => "number".into(),
        Value::String(_) => "string".into(),
        Value::Bool(_) => "boolean".into(),
        Value::Null => "object".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) => "object".into(),
        Value::Array(_) => "object".into(),
        Value::Bytes(_) => "object".into(),
        Value::Closure(_) => "function".into(),
    }
}

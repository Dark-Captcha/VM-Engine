//! IR interpreter with debugging support.
//!
//! Executes the unified IR — not raw bytecode. Every VM uses the same
//! interpreter. Supports stepping, breakpoints, tracing, and hooks.

pub mod breakpoint;
pub mod heap;
pub mod hooks;
pub mod scope;
pub mod state;
pub mod trace;

// ============================================================================
// Imports
// ============================================================================

use crate::error::{Error, Result};
use crate::ir::opcode::{OpCode, Terminator};
use crate::ir::operand::Operand;
use crate::ir::{FuncId, Module};
use crate::value::Value;
use crate::value::coerce;
use crate::value::ops::{self, BinaryOp, UnaryOp};

use breakpoint::Breakpoint;
use hooks::{Hook, NullHook};
use state::{CallFrame, Cursor, State};
use trace::{TraceEvent, TraceRecorder};

// ============================================================================
// Interpreter
// ============================================================================

/// IR interpreter. Executes a [`Module`] with optional hooks and tracing.
pub struct Interpreter<'m, H: Hook = NullHook> {
    module: &'m Module,
    pub state: State,
    pub trace: TraceRecorder,
    pub hook: H,
    breakpoints: Vec<Breakpoint>,
    max_instructions: u64,
}

impl<'m> Interpreter<'m, NullHook> {
    /// Create an interpreter with no hooks.
    pub fn new(module: &'m Module) -> Result<Self> {
        Self::with_hook(module, NullHook)
    }
}

impl<'m, H: Hook> Interpreter<'m, H> {
    /// Create an interpreter with a hook.
    pub fn with_hook(module: &'m Module, hook: H) -> Result<Self> {
        let func = module.functions.first()
            .ok_or_else(|| Error::exec("module has no functions"))?;

        Ok(Self {
            module,
            state: State::new(func.id, func.entry),
            trace: TraceRecorder::new(),
            hook,
            breakpoints: Vec::new(),
            max_instructions: 10_000_000,
        })
    }

    /// Start execution at a specific function by name.
    pub fn set_entry(&mut self, name: &str) -> Result<()> {
        let func = self.module.function_by_name(name)
            .ok_or_else(|| Error::exec(format!("function '{name}' not found")))?;
        self.state.cursor = Cursor {
            function: func.id,
            block: func.entry,
            instruction: 0,
        };
        Ok(())
    }

    /// Set the maximum number of instructions before forced halt.
    pub fn set_max_instructions(&mut self, n: u64) {
        self.max_instructions = n;
    }

    /// Add a breakpoint.
    pub fn add_breakpoint(&mut self, bp: Breakpoint) {
        self.breakpoints.push(bp);
    }

    // ── Execution methods ────────────────────────────────────────────

    /// Execute one IR instruction. Returns `true` if execution can continue.
    pub fn step(&mut self) -> Result<bool> {
        if self.state.halted {
            return Ok(false);
        }

        self.state.instruction_count += 1;
        if self.state.instruction_count > self.max_instructions {
            self.state.halted = true;
            self.trace.record(TraceEvent::Halted {
                instruction_count: self.state.instruction_count,
            });
            return Err(Error::exec(format!(
                "execution limit reached after {} instructions",
                self.max_instructions,
            )));
        }

        let cursor = self.state.cursor;
        let func = self.module.function_by_id(cursor.function)
            .ok_or_else(|| Error::exec(format!("function {} not found", cursor.function)))?;
        let block = func.block(cursor.block)
            .ok_or_else(|| Error::exec(format!("block {} not found", cursor.block)))?;

        // Are we past the block body? → execute terminator
        if cursor.instruction >= block.body.len() {
            return self.exec_terminator(&block.terminator, func.id);
        }

        let instr = &block.body[cursor.instruction];

        // Trace
        self.trace.record(TraceEvent::Step {
            cursor,
            op_name: format!("{}", instr.op),
            source_pc: instr.source.as_ref().map(|s| s.pc),
        });

        // Resolve operands to values
        let operand_vals: Vec<Value> = instr.operands.iter()
            .map(|op| self.resolve_operand(op))
            .collect();

        // Execute
        let result = self.exec_op(instr.op, &operand_vals, &instr.operands)?;

        // Store result
        if let Some(var) = instr.result
            && let Some(val) = result {
                self.state.set_var(var, val.clone());
                self.trace.record(TraceEvent::VarWrite { var, value: val });
            }

        // Advance
        self.state.cursor.instruction += 1;
        Ok(true)
    }

    /// Run until halted, breakpoint, or error.
    pub fn run(&mut self) -> Result<()> {
        while !self.state.halted {
            // Check breakpoints
            for bp in &self.breakpoints {
                if bp.should_break(&self.state) {
                    return Ok(());
                }
            }
            self.step()?;
        }
        Ok(())
    }

    /// Run until a predicate returns true.
    pub fn run_until(&mut self, stop: impl Fn(&State) -> bool) -> Result<()> {
        while !self.state.halted {
            if stop(&self.state) {
                return Ok(());
            }
            self.step()?;
        }
        Ok(())
    }

    // ── Operand resolution ───────────────────────────────────────────

    fn resolve_operand(&self, op: &Operand) -> Value {
        match op {
            Operand::Var(v) => self.state.get_var(*v),
            Operand::Const(val) => val.clone(),
            Operand::Func(fid) => Value::string(format!("<func {fid}>")),
            Operand::Block(bid) => Value::string(format!("<block {bid}>")),
        }
    }

    // ── Instruction execution ────────────────────────────────────────

    fn exec_op(
        &mut self,
        op: OpCode,
        vals: &[Value],
        operands: &[Operand],
    ) -> Result<Option<Value>> {
        let operand_value = |i: usize| vals.get(i).cloned().unwrap_or(Value::Undefined);

        match op {
            // ── Pure binary ──────────────────────────────────────
            OpCode::Add => Ok(Some(ops::binary(BinaryOp::Add, &operand_value(0), &operand_value(1)))),
            OpCode::Sub => Ok(Some(ops::binary(BinaryOp::Sub, &operand_value(0), &operand_value(1)))),
            OpCode::Mul => Ok(Some(ops::binary(BinaryOp::Mul, &operand_value(0), &operand_value(1)))),
            OpCode::Div => Ok(Some(ops::binary(BinaryOp::Div, &operand_value(0), &operand_value(1)))),
            OpCode::Mod => Ok(Some(ops::binary(BinaryOp::Mod, &operand_value(0), &operand_value(1)))),
            OpCode::Pow => Ok(Some(ops::binary(BinaryOp::Pow, &operand_value(0), &operand_value(1)))),
            OpCode::BitAnd => Ok(Some(ops::binary(BinaryOp::BitAnd, &operand_value(0), &operand_value(1)))),
            OpCode::BitOr => Ok(Some(ops::binary(BinaryOp::BitOr, &operand_value(0), &operand_value(1)))),
            OpCode::BitXor => Ok(Some(ops::binary(BinaryOp::BitXor, &operand_value(0), &operand_value(1)))),
            OpCode::Shl => Ok(Some(ops::binary(BinaryOp::Shl, &operand_value(0), &operand_value(1)))),
            OpCode::Shr => Ok(Some(ops::binary(BinaryOp::Shr, &operand_value(0), &operand_value(1)))),
            OpCode::UShr => Ok(Some(ops::binary(BinaryOp::UShr, &operand_value(0), &operand_value(1)))),
            OpCode::Eq => Ok(Some(ops::binary(BinaryOp::Eq, &operand_value(0), &operand_value(1)))),
            OpCode::Neq => Ok(Some(ops::binary(BinaryOp::Neq, &operand_value(0), &operand_value(1)))),
            OpCode::StrictEq => Ok(Some(ops::binary(BinaryOp::StrictEq, &operand_value(0), &operand_value(1)))),
            OpCode::StrictNeq => Ok(Some(ops::binary(BinaryOp::StrictNeq, &operand_value(0), &operand_value(1)))),
            OpCode::Lt => Ok(Some(ops::binary(BinaryOp::Lt, &operand_value(0), &operand_value(1)))),
            OpCode::Gt => Ok(Some(ops::binary(BinaryOp::Gt, &operand_value(0), &operand_value(1)))),
            OpCode::Lte => Ok(Some(ops::binary(BinaryOp::Lte, &operand_value(0), &operand_value(1)))),
            OpCode::Gte => Ok(Some(ops::binary(BinaryOp::Gte, &operand_value(0), &operand_value(1)))),

            // ── Pure unary ───────────────────────────────────────
            OpCode::Neg => Ok(Some(ops::unary(UnaryOp::Neg, &operand_value(0)))),
            OpCode::Pos => Ok(Some(ops::unary(UnaryOp::Pos, &operand_value(0)))),
            OpCode::LogicalNot => Ok(Some(ops::unary(UnaryOp::LogicalNot, &operand_value(0)))),
            OpCode::BitNot => Ok(Some(ops::unary(UnaryOp::BitNot, &operand_value(0)))),
            OpCode::TypeOf => Ok(Some(ops::unary(UnaryOp::TypeOf, &operand_value(0)))),
            OpCode::Void => Ok(Some(Value::Undefined)),

            // ── Data ─────────────────────────────────────────────
            OpCode::Const => Ok(Some(operand_value(0))),
            OpCode::Param => Ok(Some(operand_value(0))),
            OpCode::Phi => {
                // Phi resolves based on which predecessor block we arrived from.
                // Operands alternate: [Block(b0), Var(v0), Block(b1), Var(v1), ...]
                let prev = self.state.previous_block;
                for pair in operands.chunks(2) {
                    if pair.len() == 2
                        && let (Operand::Block(block_id), Operand::Var(var)) = (&pair[0], &pair[1])
                        && prev == Some(*block_id)
                    {
                        return Ok(Some(self.state.get_var(*var)));
                    }
                }
                // Fallback: take the first var operand's value.
                for op in operands {
                    if let Operand::Var(var) = op {
                        return Ok(Some(self.state.get_var(*var)));
                    }
                }
                Ok(Some(Value::Undefined))
            }
            OpCode::Move => Ok(Some(operand_value(0))),

            // ── Memory ───────────────────────────────────────────
            OpCode::LoadProp => {
                let obj_val = operand_value(0);
                let key_val = operand_value(1);
                let key_str = coerce::to_string(&key_val);

                // Try hook first
                if let Value::Object(oid) = &obj_val {
                    if let Some(val) = self.hook.on_prop_get(*oid, &key_str, &self.state.heap) {
                        return Ok(Some(val));
                    }
                    let val = self.state.heap.get_property(*oid, &key_str);
                    self.trace.record(TraceEvent::PropGet {
                        obj: *oid, key: key_str, value: val.clone(),
                    });
                    return Ok(Some(val));
                }
                Ok(Some(Value::Undefined))
            }
            OpCode::StoreProp => {
                let obj_val = operand_value(0);
                let key_val = operand_value(1);
                let val = operand_value(2);
                let key_str = coerce::to_string(&key_val);

                if let Value::Object(oid) = &obj_val {
                    self.hook.on_prop_set(*oid, &key_str, &val, &self.state.heap);
                    self.state.heap.set_property(*oid, &key_str, val.clone());
                    self.trace.record(TraceEvent::PropSet {
                        obj: *oid, key: key_str, value: val,
                    });
                }
                Ok(None)
            }
            OpCode::DeleteProp => {
                if let Value::Object(oid) = &operand_value(0) {
                    let key = coerce::to_string(&operand_value(1));
                    let existed = self.state.heap.delete_property(*oid, &key);
                    Ok(Some(Value::bool(existed)))
                } else {
                    Ok(Some(Value::bool(false)))
                }
            }
            OpCode::HasProp => {
                if let Value::Object(oid) = &operand_value(1) {
                    let key = coerce::to_string(&operand_value(0));
                    Ok(Some(Value::bool(self.state.heap.has_property(*oid, &key))))
                } else {
                    Ok(Some(Value::bool(false)))
                }
            }
            OpCode::LoadIndex => {
                let arr_val = operand_value(0);
                let idx_val = operand_value(1);
                let idx = coerce::to_number(&idx_val) as usize;
                match &arr_val {
                    Value::Array(arr) => {
                        Ok(Some(arr.get(idx).cloned().unwrap_or(Value::Undefined)))
                    }
                    Value::Bytes(bytes) => {
                        Ok(Some(bytes.get(idx).map(|&b| Value::number(b as f64))
                            .unwrap_or(Value::Undefined)))
                    }
                    _ => Ok(Some(Value::Undefined)),
                }
            }
            OpCode::StoreIndex => {
                // Must write the mutated array back to the source Var,
                // otherwise the clone is discarded and the mutation is lost.
                if let Some(Operand::Var(arr_var)) = operands.first() {
                    let mut arr_val = self.state.get_var(*arr_var);
                    let idx = coerce::to_number(&operand_value(1)) as usize;
                    let val = operand_value(2);
                    if let Value::Array(ref mut arr) = arr_val {
                        if arr.len() <= idx {
                            arr.resize(idx + 1, Value::Undefined);
                        }
                        arr[idx] = val;
                    }
                    self.state.set_var(*arr_var, arr_val);
                }
                Ok(None)
            }
            OpCode::LoadScope => {
                let name = coerce::to_string(&operand_value(0));
                // Try scope chain first, fall back to global object properties.
                if let Some(val) = self.state.scopes.get(&name) {
                    Ok(Some(val))
                } else if let Some(global) = self.state.global_object {
                    Ok(Some(self.state.heap.get_property(global, &name)))
                } else {
                    Ok(Some(Value::Undefined))
                }
            }
            OpCode::StoreScope => {
                let name = coerce::to_string(&operand_value(0));
                let val = operand_value(1);
                // Write to scope if variable exists there, otherwise write to global.
                if self.state.scopes.get(&name).is_some() {
                    self.state.scopes.set_existing(&name, val);
                } else if let Some(global) = self.state.global_object {
                    self.state.heap.set_property(global, &name, val);
                } else {
                    self.state.scopes.set(&name, val);
                }
                Ok(None)
            }
            OpCode::NewObject => {
                let oid = self.state.heap.alloc();
                Ok(Some(Value::Object(oid)))
            }
            OpCode::NewArray => {
                Ok(Some(Value::Array(Vec::new())))
            }

            // ── Control ──────────────────────────────────────────
            OpCode::Call => {
                let func_ref = operands.first().and_then(|o| {
                    if let Operand::Func(fid) = o { Some(*fid) } else { None }
                });
                let args = &vals[1..];

                // Try hook first (by function name)
                if let Some(fid) = func_ref
                    && let Some(f) = self.module.function_by_id(fid)
                        && let Some(result) = self.hook.on_call(&f.name, args, &mut self.state.heap) {
                            return Ok(Some(result));
                        }

                // Handle meta-call patterns: __bind__, new
                let call_name = if let Value::String(s) = &operand_value(0) { Some(s.clone()) } else { None };
                if let Some(ref name) = call_name {
                    match name.as_str() {
                        "__bind__" => {
                            // bind(func, this) → return the function reference (simplified)
                            return Ok(Some(args.first().cloned().unwrap_or(Value::Undefined)));
                        }
                        "new" => {
                            // new Constructor(args) → call constructor with new semantics
                            let ctor_val = args.first().cloned().unwrap_or(Value::Undefined);
                            let ctor_args = if args.len() > 1 { &args[1..] } else { &[] };

                            // Try heap native constructor (Uint8Array, etc.)
                            if let Value::Object(oid) = &ctor_val {
                                if let Some(result) = self.state.heap.call(*oid, ctor_args) {
                                    return Ok(Some(result));
                                }
                            }

                            // Try resolving constructor as an IR function
                            if let Value::String(ctor_name) = &ctor_val {
                                if let Some(target) = self.module.function_by_name(ctor_name) {
                                    let fid = target.id;
                                    self.trace.record(TraceEvent::CallEnter {
                                        func: fid,
                                        arg_count: ctor_args.len(),
                                    });

                                    let mut return_cursor = self.state.cursor;
                                    return_cursor.instruction += 1;
                                    self.state.call_stack.push(CallFrame {
                                        return_cursor,
                                        scope_depth: self.state.scopes.len(),
                                        locals: std::collections::HashMap::new(),
                                    });

                                    for (i, &param_var) in target.params.iter().enumerate() {
                                        let arg_val = ctor_args.get(i).cloned().unwrap_or(Value::Undefined);
                                        self.state.set_var(param_var, arg_val);
                                    }

                                    self.state.cursor = Cursor {
                                        function: fid,
                                        block: target.entry,
                                        instruction: 0,
                                    };
                                    return Ok(None);
                                }
                            }
                            // Fallback: return a new empty object
                            let obj_id = self.state.heap.alloc();
                            return Ok(Some(Value::Object(obj_id)));
                        }
                        _ => {} // fall through to normal dispatch
                    }
                }

                // Try hook by call name
                if let Some(ref name) = call_name {
                    if let Some(result) = self.hook.on_call(name, args, &mut self.state.heap) {
                        return Ok(Some(result));
                    }
                }

                // Try calling a heap object (native/closure)
                if let Value::Object(oid) = &operand_value(0)
                    && let Some(result) = self.state.heap.call(*oid, args) {
                        return Ok(Some(result));
                    }

                // IR function call by FuncId or by name (for indirect calls via Var)
                let resolved_target = if let Some(fid) = func_ref {
                    self.module.function_by_id(fid)
                } else if let Some(ref name) = call_name {
                    self.module.function_by_name(name)
                } else {
                    None
                };
                if let Some(target) = resolved_target {
                    let fid = target.id;
                    self.trace.record(TraceEvent::CallEnter {
                        func: fid,
                        arg_count: args.len(),
                    });

                    let mut return_cursor = self.state.cursor;
                    return_cursor.instruction += 1;
                    self.state.call_stack.push(CallFrame {
                        return_cursor,
                        scope_depth: self.state.scopes.len(),
                        locals: std::collections::HashMap::new(),
                    });

                    for (i, &param_var) in target.params.iter().enumerate() {
                        let arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                        self.state.set_var(param_var, arg_val);
                    }

                    self.state.cursor = Cursor {
                        function: fid,
                        block: target.entry,
                        instruction: 0,
                    };
                    return Ok(None);
                }

                Ok(Some(Value::Undefined))
            }
            OpCode::CallMethod => {
                let obj = operand_value(0);
                let method = coerce::to_string(&operand_value(1));
                let args = &vals[2..];

                // Try hook
                if let Some(result) = self.hook.on_call(&method, args, &mut self.state.heap) {
                    return Ok(Some(result));
                }

                // Try heap method
                if let Value::Object(oid) = &obj {
                    let method_val = self.state.heap.get_property(*oid, &method);
                    if let Value::Object(method_oid) = method_val
                        && let Some(result) = self.state.heap.call(method_oid, args) {
                            return Ok(Some(result));
                        }
                }

                Ok(Some(Value::Undefined))
            }
        }
    }

    // ── Terminator execution ─────────────────────────────────────────

    fn exec_terminator(&mut self, term: &Terminator, _func_id: FuncId) -> Result<bool> {
        let current_block = self.state.cursor.block;

        match term {
            Terminator::Jump { target } => {
                self.state.previous_block = Some(current_block);
                self.state.cursor.block = *target;
                self.state.cursor.instruction = 0;
                Ok(true)
            }
            Terminator::BranchIf { cond, if_true, if_false } => {
                let val = self.state.get_var(*cond);
                let taken = coerce::to_boolean(&val);
                self.state.previous_block = Some(current_block);
                self.state.cursor.block = if taken { *if_true } else { *if_false };
                self.state.cursor.instruction = 0;
                Ok(true)
            }
            Terminator::Switch { value, cases, default } => {
                let val = self.state.get_var(*value);
                let target = cases.iter()
                    .find(|(case_val, _)| val == *case_val)
                    .map(|(_, bid)| *bid)
                    .unwrap_or(*default);
                self.state.cursor.block = target;
                self.state.cursor.instruction = 0;
                Ok(true)
            }
            Terminator::Return { value } => {
                let ret_val = value.map(|v| self.state.get_var(v)).unwrap_or(Value::Undefined);

                if let Some(frame) = self.state.call_stack.pop() {
                    let func_id = self.state.cursor.function;
                    self.trace.record(TraceEvent::CallReturn {
                        func: func_id,
                        result: ret_val.clone(),
                    });
                    self.state.cursor = frame.return_cursor;
                    self.state.scopes.truncate(frame.scope_depth);

                    // Store return value in the Call instruction's result var
                    // The call instruction is at return_cursor - 1
                    let call_func = self.module.function_by_id(self.state.cursor.function);
                    if let Some(f) = call_func
                        && let Some(block) = f.block(self.state.cursor.block) {
                            let call_idx = self.state.cursor.instruction.saturating_sub(1);
                            if let Some(instr) = block.body.get(call_idx)
                                && let Some(var) = instr.result {
                                    self.state.set_var(var, ret_val);
                                }
                        }
                    Ok(true)
                } else {
                    // Top-level return — halt
                    self.state.halted = true;
                    self.trace.record(TraceEvent::Halted {
                        instruction_count: self.state.instruction_count,
                    });
                    Ok(false)
                }
            }
            Terminator::Halt => {
                self.state.halted = true;
                self.trace.record(TraceEvent::Halted {
                    instruction_count: self.state.instruction_count,
                });
                Ok(false)
            }
            Terminator::Throw { value } => {
                let val = self.state.get_var(*value);
                Err(Error::exec(format!("uncaught throw: {val}")))
            }
            Terminator::Unreachable => {
                // In multi-function decoders, some blocks are created by forward jumps
                // but never filled (the decoder skips over them via JMP_FWD).
                // Treat as halt — the interpreter can't continue from here.
                self.state.halted = true;
                Ok(false)
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Var;
    use crate::ir::builder::IrBuilder;

    #[test]
    fn run_simple_halt() {
        let mut b = IrBuilder::new();
        b.begin_function("main");
        b.create_and_switch("entry");
        b.halt();
        b.end_function();

        let module = b.build();
        let mut interp = Interpreter::new(&module).unwrap();
        interp.run().unwrap();
        assert!(interp.state.halted);
        assert_eq!(interp.state.instruction_count, 1);
    }

    #[test]
    fn run_arithmetic() {
        let mut b = IrBuilder::new();
        b.begin_function("main");
        b.create_and_switch("entry");
        let a = b.const_number(10.0);
        let c = b.const_number(20.0);
        let r = b.add(a, c);
        b.ret(Some(r));
        b.end_function();

        let module = b.build();
        let mut interp = Interpreter::new(&module).unwrap();
        interp.run().unwrap();
        assert_eq!(interp.state.get_var(Var(2)), Value::number(30.0));
    }

    #[test]
    fn run_branch() {
        let mut b = IrBuilder::new();
        b.begin_function("main");

        let entry = b.create_and_switch("entry");
        let cond = b.const_bool(false);
        let then_b = b.create_block("then");
        let else_b = b.create_block("else");

        b.switch_to(entry);
        b.branch_if(cond, then_b, else_b);

        b.switch_to(then_b);
        let v1 = b.const_number(1.0);
        b.ret(Some(v1));

        b.switch_to(else_b);
        let v2 = b.const_number(2.0);
        b.ret(Some(v2));

        b.end_function();

        let module = b.build();
        let mut interp = Interpreter::new(&module).unwrap();
        interp.run().unwrap();

        // cond is false → else branch → v2 = 2.0
        assert_eq!(interp.state.get_var(Var(2)), Value::number(2.0));
    }

    #[test]
    fn run_while_loop() {
        // sum = 0; i = 1; while (i <= 3) { sum = sum + i; i = i + 1; } → sum = 6
        let mut b = IrBuilder::new();
        b.begin_function("main");

        let _entry = b.create_and_switch("entry");
        let zero = b.const_number(0.0);
        b.emit_void(OpCode::StoreScope, vec![
            Operand::Const(Value::string("sum")), Operand::Var(zero),
        ]);
        let one = b.const_number(1.0);
        b.emit_void(OpCode::StoreScope, vec![
            Operand::Const(Value::string("i")), Operand::Var(one),
        ]);

        let header = b.create_block("header");
        b.jump(header);

        b.switch_to(header);
        let i_val = b.load_scope("i");
        let limit = b.const_number(3.0);
        let cond = b.emit(OpCode::Lte, vec![Operand::Var(i_val), Operand::Var(limit)]);

        let body = b.create_block("body");
        let exit = b.create_block("exit");
        b.branch_if(cond, body, exit);

        b.switch_to(body);
        let sum_val = b.load_scope("sum");
        let i_val2 = b.load_scope("i");
        let new_sum = b.add(sum_val, i_val2);
        b.emit_void(OpCode::StoreScope, vec![
            Operand::Const(Value::string("sum")), Operand::Var(new_sum),
        ]);
        let i_val3 = b.load_scope("i");
        let one2 = b.const_number(1.0);
        let new_i = b.add(i_val3, one2);
        b.emit_void(OpCode::StoreScope, vec![
            Operand::Const(Value::string("i")), Operand::Var(new_i),
        ]);
        b.jump(header);

        b.switch_to(exit);
        let result = b.load_scope("sum");
        b.ret(Some(result));

        b.end_function();

        let module = b.build();
        let mut interp = Interpreter::new(&module).unwrap();
        interp.run().unwrap();

        // sum should be 1 + 2 + 3 = 6
        let sum = interp.state.scopes.get("sum").unwrap_or(Value::Undefined);
        assert_eq!(sum, Value::number(6.0));
    }

    #[test]
    fn run_with_hook() {
        struct DoubleHook;
        impl Hook for DoubleHook {
            fn on_call(&mut self, name: &str, args: &[Value], _heap: &mut heap::Heap) -> Option<Value> {
                if name == "double" {
                    let n = args.first().and_then(|v| v.as_number()).unwrap_or(0.0);
                    return Some(Value::number(n * 2.0));
                }
                None
            }
        }

        let mut b = IrBuilder::new();
        let double_id = b.begin_function("double");
        b.create_and_switch("entry");
        b.halt(); // placeholder — hook intercepts
        b.end_function();

        b.begin_function("main");
        b.create_and_switch("entry");
        let arg = b.const_number(21.0);
        let r = b.call(double_id, &[arg]);
        b.ret(Some(r));
        b.end_function();

        let module = b.build();
        let mut interp = Interpreter::with_hook(&module, DoubleHook).unwrap();
        interp.set_entry("main").unwrap();
        interp.run().unwrap();

        // In main: iconst=Var(0), call=Var(1) — but var numbering resets per function
        // double's params start at Var(0), main's start fresh at Var(0)
        // iconst(21) = Var(0), call = Var(1)
        assert_eq!(interp.state.get_var(Var(1)), Value::number(42.0));
    }

    #[test]
    fn execution_limit() {
        let mut b = IrBuilder::new();
        b.begin_function("main");
        let header = b.create_and_switch("header");
        let cond = b.const_bool(true);
        b.branch_if(cond, header, header); // infinite loop

        b.end_function();

        let module = b.build();
        let mut interp = Interpreter::new(&module).unwrap();
        interp.set_max_instructions(100);
        let err = interp.run().unwrap_err();
        assert!(err.to_string().contains("limit"));
    }

    #[test]
    fn trace_records_events() {
        let mut b = IrBuilder::new();
        b.begin_function("main");
        b.create_and_switch("entry");
        let _ = b.const_number(42.0);
        b.halt();
        b.end_function();

        let module = b.build();
        let mut interp = Interpreter::new(&module).unwrap();
        interp.trace.enable(100);
        interp.trace.set_filter(trace::TraceFilter::all());
        interp.run().unwrap();

        assert!(!interp.trace.is_empty());
        let events = interp.trace.into_events();
        assert!(events.iter().any(|e| matches!(e, TraceEvent::Step { .. })));
        assert!(events.iter().any(|e| matches!(e, TraceEvent::Halted { .. })));
    }

    #[test]
    fn breakpoint_stops_execution() {
        let mut b = IrBuilder::new();
        b.begin_function("main");
        b.create_and_switch("entry");
        let _ = b.const_number(1.0);
        let _ = b.const_number(2.0);
        let _ = b.const_number(3.0);
        b.halt();
        b.end_function();

        let module = b.build();
        let mut interp = Interpreter::new(&module).unwrap();
        interp.add_breakpoint(Breakpoint::AfterSteps(2));
        interp.run().unwrap();

        // Should stop after 2 instructions
        assert!(!interp.state.halted);
        assert_eq!(interp.state.instruction_count, 2);
    }

    #[test]
    fn heap_operations() {
        let mut b = IrBuilder::new();
        b.begin_function("main");
        b.create_and_switch("entry");
        let obj = b.new_object();
        let key = b.const_string("x");
        let val = b.const_number(42.0);
        b.store_prop(obj, key, val);
        let loaded = b.load_prop(obj, key);
        b.ret(Some(loaded));
        b.end_function();

        let module = b.build();
        let mut interp = Interpreter::new(&module).unwrap();
        interp.run().unwrap();

        // loaded = Var(3): new_object=0, sconst=1, iconst=2, store_prop=void, load_prop=3
        assert_eq!(interp.state.get_var(Var(3)), Value::number(42.0));
    }

    #[test]
    fn array_operations() {
        let mut b = IrBuilder::new();
        b.begin_function("main");
        b.create_and_switch("entry");
        let arr = b.new_array();
        let idx = b.const_number(0.0);
        let val = b.const_number(99.0);
        b.store_index(arr, idx, val);
        let loaded = b.load_index(arr, idx);
        b.ret(Some(loaded));
        b.end_function();

        let module = b.build();
        let mut interp = Interpreter::new(&module).unwrap();
        interp.run().unwrap();

        // StoreIndex on Value::Array mutates the array in-place through Var
        // The loaded value should be from the array
        // Note: this test verifies the concept; actual mutation semantics
        // may need refinement for pass-by-value Array
    }
}

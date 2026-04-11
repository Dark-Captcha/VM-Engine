//! IR construction API for decoder authors.
//!
//! Provides ergonomic methods for building IR programmatically. Handles
//! variable numbering, block management, and function structure automatically.

// ============================================================================
// Imports
// ============================================================================

use crate::value::Value;

use super::opcode::{OpCode, Terminator};
use super::operand::{Operand, SourceLoc};
use super::{Block, BlockId, FuncId, Function, Instruction, Module, Var};

// ============================================================================
// IrBuilder
// ============================================================================

/// Builds an IR [`Module`] incrementally.
///
/// # Usage
///
/// ```
/// use vm_engine::ir::builder::IrBuilder;
/// use vm_engine::value::Value;
///
/// let mut b = IrBuilder::new();
/// let f = b.begin_function("main");
/// let entry = b.create_block("entry");
/// b.switch_to(entry);
/// let v0 = b.const_number(10.0);
/// let v1 = b.const_number(20.0);
/// let v2 = b.add(v0, v1);
/// b.halt();
/// b.end_function();
/// let module = b.build();
/// assert_eq!(module.functions.len(), 1);
/// ```
pub struct IrBuilder {
    functions: Vec<Function>,
    current_func: Option<usize>,
    current_block: Option<BlockId>,
    next_var: u32,
    next_block: u32,
    next_func: u32,
}

impl IrBuilder {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            current_func: None,
            current_block: None,
            next_var: 0,
            next_block: 0,
            next_func: 0,
        }
    }

    // ── Function management ──────────────────────────────────────────

    /// Start building a new function. Returns its [`FuncId`].
    pub fn begin_function(&mut self, name: &str) -> FuncId {
        let id = FuncId(self.next_func);
        self.next_func += 1;
        self.next_block = 0;
        self.next_var = 0;
        self.current_block = None;

        self.functions.push(Function {
            id,
            name: name.to_string(),
            params: Vec::new(),
            entry: BlockId(0), // set when first block is created
            blocks: Vec::new(),
        });
        self.current_func = Some(self.functions.len() - 1);
        id
    }

    /// Declare a function parameter. Returns the [`Var`] representing it.
    pub fn add_param(&mut self) -> Var {
        let var = self.fresh_var();
        let func = self.current_func_mut();
        func.params.push(var);
        var
    }

    /// Finish the current function.
    pub fn end_function(&mut self) {
        self.current_func = None;
        self.current_block = None;
    }

    // ── Block management ─────────────────────────────────────────────

    /// Create a new block in the current function. Returns its [`BlockId`].
    pub fn create_block(&mut self, label: &str) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;

        let func = self.current_func_mut();
        if func.blocks.is_empty() {
            func.entry = id;
        }
        func.blocks.push(Block {
            id,
            label: label.to_string(),
            body: Vec::new(),
            terminator: Terminator::Unreachable,
        });
        id
    }

    /// Set the current block for subsequent `emit` calls.
    pub fn switch_to(&mut self, block: BlockId) {
        self.current_block = Some(block);
    }

    /// Create a block and switch to it. Convenience for `create_block` + `switch_to`.
    pub fn create_and_switch(&mut self, label: &str) -> BlockId {
        let id = self.create_block(label);
        self.switch_to(id);
        id
    }

    /// The current block ID.
    pub fn current_block_id(&self) -> Option<BlockId> {
        self.current_block
    }

    // ── Instruction emission ─────────────────────────────────────────

    /// Emit an instruction that produces a result.
    ///
    /// # Panics
    /// Panics if the current block already has a terminator set (other than the
    /// `Unreachable` placeholder). Emit instructions before calling jump/branch/ret/halt/throw.
    pub fn emit(&mut self, op: OpCode, operands: Vec<Operand>) -> Var {
        let var = self.fresh_var();
        let block = self.current_block_mut();
        debug_assert!(
            matches!(block.terminator, Terminator::Unreachable),
            "IrBuilder::emit called after terminator was set on block '{}'",
            block.label,
        );
        block.body.push(Instruction {
            result: Some(var),
            op,
            operands,
            source: None,
        });
        var
    }

    /// Emit an instruction that produces no result (void).
    ///
    /// # Panics
    /// Panics if the current block already has a terminator set.
    pub fn emit_void(&mut self, op: OpCode, operands: Vec<Operand>) {
        let block = self.current_block_mut();
        debug_assert!(
            matches!(block.terminator, Terminator::Unreachable),
            "IrBuilder::emit_void called after terminator was set on block '{}'",
            block.label,
        );
        block.body.push(Instruction {
            result: None,
            op,
            operands,
            source: None,
        });
    }

    /// Emit an instruction with source location.
    ///
    /// # Panics
    /// Panics if the current block already has a terminator set.
    pub fn emit_sourced(&mut self, op: OpCode, operands: Vec<Operand>, source: SourceLoc) -> Var {
        let var = self.fresh_var();
        let block = self.current_block_mut();
        debug_assert!(
            matches!(block.terminator, Terminator::Unreachable),
            "IrBuilder::emit_sourced called after terminator was set on block '{}'",
            block.label,
        );
        block.body.push(Instruction {
            result: Some(var),
            op,
            operands,
            source: Some(source),
        });
        var
    }

    // ── Convenience: data ────────────────────────────────────────────

    /// `%r = const <number>` — push a numeric constant.
    pub fn const_number(&mut self, n: f64) -> Var {
        self.emit(OpCode::Const, vec![Operand::Const(Value::number(n))])
    }

    /// `%r = const <string>` — push a string constant.
    pub fn const_string(&mut self, s: &str) -> Var {
        self.emit(OpCode::Const, vec![Operand::Const(Value::string(s))])
    }

    /// `%r = const <bool>` — push a boolean constant.
    pub fn const_bool(&mut self, b: bool) -> Var {
        self.emit(OpCode::Const, vec![Operand::Const(Value::bool(b))])
    }

    /// `%r = const null` — push null.
    pub fn const_null(&mut self) -> Var {
        self.emit(OpCode::Const, vec![Operand::Const(Value::Null)])
    }

    /// `%r = const undefined` — push undefined.
    pub fn const_undefined(&mut self) -> Var {
        self.emit(OpCode::Const, vec![Operand::Const(Value::Undefined)])
    }

    /// `%r = copy %src` — explicit variable copy.
    pub fn copy_var(&mut self, src: Var) -> Var {
        self.emit(OpCode::Move, vec![Operand::Var(src)])
    }

    // ── Convenience: pure ops ────────────────────────────────────────

    /// `%r = %left + %right`
    pub fn add(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::Add, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = %left - %right`
    pub fn sub(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::Sub, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = %left * %right`
    pub fn mul(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::Mul, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = %left / %right`
    pub fn div(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::Div, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = %left ^ %right`
    pub fn bit_xor(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::BitXor, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = %left & %right`
    pub fn bit_and(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::BitAnd, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = %left | %right`
    pub fn bit_or(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::BitOr, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = %left << %right`
    pub fn shl(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::Shl, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = %left >> %right` (signed)
    pub fn shr(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::Shr, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = %left >>> %right` (unsigned)
    pub fn ushr(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::UShr, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = !%operand`
    pub fn logical_not(&mut self, operand: Var) -> Var {
        self.emit(OpCode::LogicalNot, vec![Operand::Var(operand)])
    }

    /// `%r = ~%operand`
    pub fn bit_not(&mut self, operand: Var) -> Var {
        self.emit(OpCode::BitNot, vec![Operand::Var(operand)])
    }

    /// `%r = -%operand`
    pub fn neg(&mut self, operand: Var) -> Var {
        self.emit(OpCode::Neg, vec![Operand::Var(operand)])
    }

    /// `%r = %left === %right`
    pub fn strict_eq(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::StrictEq, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = %left < %right`
    pub fn lt(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::Lt, vec![Operand::Var(left), Operand::Var(right)])
    }

    /// `%r = %left > %right`
    pub fn gt(&mut self, left: Var, right: Var) -> Var {
        self.emit(OpCode::Gt, vec![Operand::Var(left), Operand::Var(right)])
    }

    // ── Convenience: memory ops ──────────────────────────────────────

    /// `%r = obj[key]`
    pub fn load_prop(&mut self, obj: Var, key: Var) -> Var {
        self.emit(OpCode::LoadProp, vec![Operand::Var(obj), Operand::Var(key)])
    }

    /// `obj[key] = val`
    pub fn store_prop(&mut self, obj: Var, key: Var, val: Var) {
        self.emit_void(OpCode::StoreProp, vec![
            Operand::Var(obj), Operand::Var(key), Operand::Var(val),
        ]);
    }

    /// `%r = arr[index]`
    pub fn load_index(&mut self, arr: Var, index: Var) -> Var {
        self.emit(OpCode::LoadIndex, vec![Operand::Var(arr), Operand::Var(index)])
    }

    /// `arr[index] = val`
    pub fn store_index(&mut self, arr: Var, index: Var, val: Var) {
        self.emit_void(OpCode::StoreIndex, vec![
            Operand::Var(arr), Operand::Var(index), Operand::Var(val),
        ]);
    }

    /// `%r = key in obj`
    pub fn has_prop(&mut self, obj: Var, key: Var) -> Var {
        self.emit(OpCode::HasProp, vec![Operand::Var(obj), Operand::Var(key)])
    }

    /// `%r = delete obj[key]`
    pub fn delete_prop(&mut self, obj: Var, key: Var) -> Var {
        self.emit(OpCode::DeleteProp, vec![Operand::Var(obj), Operand::Var(key)])
    }

    /// `%r = scope[name]`
    pub fn load_scope(&mut self, name: &str) -> Var {
        self.emit(OpCode::LoadScope, vec![Operand::Const(Value::string(name))])
    }

    /// `scope[name] = val`
    pub fn store_scope(&mut self, name: &str, val: Var) {
        self.emit_void(OpCode::StoreScope, vec![
            Operand::Const(Value::string(name)), Operand::Var(val),
        ]);
    }

    /// `%r = {}`
    pub fn new_object(&mut self) -> Var {
        self.emit(OpCode::NewObject, vec![])
    }

    /// `%r = []`
    pub fn new_array(&mut self) -> Var {
        self.emit(OpCode::NewArray, vec![])
    }

    // ── Convenience: control ops ─────────────────────────────────────

    /// `%r = func(args...)`
    pub fn call(&mut self, func: FuncId, args: &[Var]) -> Var {
        let mut operands = vec![Operand::Func(func)];
        operands.extend(args.iter().map(|v| Operand::Var(*v)));
        self.emit(OpCode::Call, operands)
    }

    /// `%r = obj.method(args...)`
    pub fn call_method(&mut self, obj: Var, method: &str, args: &[Var]) -> Var {
        let mut operands = vec![Operand::Var(obj), Operand::Const(Value::string(method))];
        operands.extend(args.iter().map(|v| Operand::Var(*v)));
        self.emit(OpCode::CallMethod, operands)
    }

    // ── Terminators ──────────────────────────────────────────────────

    pub fn jump(&mut self, target: BlockId) {
        self.set_terminator(Terminator::Jump { target });
    }

    pub fn branch_if(&mut self, cond: Var, if_true: BlockId, if_false: BlockId) {
        self.set_terminator(Terminator::BranchIf { cond, if_true, if_false });
    }

    pub fn ret(&mut self, value: Option<Var>) {
        self.set_terminator(Terminator::Return { value });
    }

    pub fn halt(&mut self) {
        self.set_terminator(Terminator::Halt);
    }

    pub fn throw(&mut self, value: Var) {
        self.set_terminator(Terminator::Throw { value });
    }

    // ── Build ────────────────────────────────────────────────────────

    /// Finalize and return the constructed [`Module`] without validation.
    ///
    /// Use [`build_validated`] instead if you want to check for structural
    /// errors like undefined variables, missing terminators, or bad operand
    /// arities.
    ///
    /// [`build_validated`]: Self::build_validated
    pub fn build(self) -> Module {
        Module { functions: self.functions }
    }

    /// Finalize and validate the constructed [`Module`].
    ///
    /// Returns an error if the module has structural issues such as:
    /// - Undefined variable references
    /// - Undefined function references
    /// - Blocks with unset terminators (still `Unreachable`)
    /// - Wrong operand counts for opcodes
    /// - Duplicate switch case values
    ///
    /// Use this at the end of a decoder to catch bugs early.
    pub fn build_validated(self) -> crate::error::Result<Module> {
        let module = Module { functions: self.functions };
        super::validate::validate(&module)?;
        Ok(module)
    }

    // ── Internal ─────────────────────────────────────────────────────

    fn fresh_var(&mut self) -> Var {
        let v = Var(self.next_var);
        self.next_var += 1;
        v
    }

    fn current_func_mut(&mut self) -> &mut Function {
        let idx = self.current_func
            .expect("IrBuilder: no current function — call begin_function() first");
        &mut self.functions[idx]
    }

    fn current_block_mut(&mut self) -> &mut Block {
        let block_id = self.current_block
            .expect("IrBuilder: no current block — call create_block() + switch_to() first");
        let func = self.current_func_mut();
        func.blocks.iter_mut()
            .find(|b| b.id == block_id)
            .expect("IrBuilder: current block not found in function")
    }

    fn set_terminator(&mut self, term: Terminator) {
        let block = self.current_block_mut();
        block.terminator = term;
    }
}

impl Default for IrBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::opcode::OpCode;

    #[test]
    fn build_simple_program() {
        // Program: const 10 + const 20 → halt
        let mut b = IrBuilder::new();
        b.begin_function("main");
        b.create_and_switch("entry");
        let v0 = b.const_number(10.0);
        let v1 = b.const_number(20.0);
        let _v2 = b.add(v0, v1);
        b.halt();
        b.end_function();

        let module = b.build();
        assert_eq!(module.functions.len(), 1);

        let func = &module.functions[0];
        assert_eq!(func.name, "main");
        assert_eq!(func.blocks.len(), 1);

        let block = &func.blocks[0];
        assert_eq!(block.body.len(), 3); // 2 consts + 1 add
        assert!(matches!(block.terminator, Terminator::Halt));
    }

    #[test]
    fn build_branching_program() {
        let mut b = IrBuilder::new();
        b.begin_function("check");
        let entry = b.create_and_switch("entry");
        let cond = b.const_bool(true);

        let then_block = b.create_block("then");
        let else_block = b.create_block("else");
        b.branch_if(cond, then_block, else_block);

        b.switch_to(then_block);
        let v1 = b.const_number(1.0);
        b.ret(Some(v1));

        b.switch_to(else_block);
        let v2 = b.const_number(0.0);
        b.ret(Some(v2));

        b.end_function();
        let module = b.build();

        let func = &module.functions[0];
        assert_eq!(func.blocks.len(), 3);
        assert_eq!(func.entry, entry);
    }

    #[test]
    fn build_with_params() {
        let mut b = IrBuilder::new();
        b.begin_function("add_one");
        let p0 = b.add_param();
        b.create_and_switch("entry");
        let one = b.const_number(1.0);
        let result = b.add(p0, one);
        b.ret(Some(result));
        b.end_function();

        let module = b.build();
        let func = &module.functions[0];
        assert_eq!(func.params.len(), 1);
        assert_eq!(func.params[0], Var(0));
    }

    #[test]
    fn build_memory_ops() {
        let mut b = IrBuilder::new();
        b.begin_function("prop_access");
        b.create_and_switch("entry");
        let obj = b.new_object();
        let key = b.const_string("x");
        let val = b.const_number(42.0);
        b.store_prop(obj, key, val);
        let loaded = b.load_prop(obj, key);
        b.ret(Some(loaded));
        b.end_function();

        let module = b.build();
        let block = &module.functions[0].blocks[0];
        // new_object, sconst, iconst, store_prop (void), load_prop = 5
        assert_eq!(block.body.len(), 5);
        // store_prop has no result
        assert!(block.body[3].result.is_none());
    }

    #[test]
    fn build_multiple_functions() {
        let mut b = IrBuilder::new();

        b.begin_function("helper");
        b.create_and_switch("entry");
        b.halt();
        b.end_function();

        b.begin_function("main");
        b.create_and_switch("entry");
        let r = b.call(FuncId(0), &[]);
        b.ret(Some(r));
        b.end_function();

        let module = b.build();
        assert_eq!(module.functions.len(), 2);
        assert_eq!(module.functions[0].name, "helper");
        assert_eq!(module.functions[1].name, "main");
    }

    #[test]
    fn source_loc_preserved() {
        let mut b = IrBuilder::new();
        b.begin_function("traced");
        b.create_and_switch("entry");
        let _v = b.emit_sourced(
            OpCode::Const,
            vec![Operand::Const(Value::number(0.0))],
            SourceLoc::with_opcode(42, 0x66),
        );
        b.halt();
        b.end_function();

        let module = b.build();
        let instr = &module.functions[0].blocks[0].body[0];
        let loc = instr.source.as_ref().expect("source location should be set");
        assert_eq!(loc.pc, 42);
        assert_eq!(loc.original_opcode, Some(0x66));
    }
}

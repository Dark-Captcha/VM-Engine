//! Expression reconstruction from IR instructions.
//!
//! Turns a sequence of IR instructions within a basic block into compound
//! expressions. Single-use variables are inlined; multi-use variables become
//! named assignments.
//!
//! Before (IR):
//!   %0 = load_prop %obj, "x"
//!   %1 = const 0xFF
//!   %2 = bit_and %0, %1
//!   %3 = load_scope "key"
//!   %4 = bit_xor %2, %3
//!
//! After (expression reconstruction):
//!   result = (obj.x & 255) ^ key

// ============================================================================
// Imports
// ============================================================================

use std::collections::HashMap;

use crate::ir::opcode::OpCode;
use crate::ir::operand::Operand;
use crate::ir::{Block, Function, Instruction, Var};
use crate::value::Value;
use crate::value::ops::{BinaryOp, UnaryOp};

use super::ast::{Expr, Stmt};

// ============================================================================
// Use counting
// ============================================================================

/// Count how many times each Var is referenced across the entire function.
pub fn count_uses(func: &Function) -> HashMap<Var, usize> {
    let mut counts: HashMap<Var, usize> = HashMap::new();

    for block in &func.blocks {
        for instr in &block.body {
            for operand in &instr.operands {
                if let Operand::Var(v) = operand {
                    *counts.entry(*v).or_default() += 1;
                }
            }
        }
        // Count uses in terminators
        match &block.terminator {
            crate::ir::Terminator::BranchIf { cond, .. } => {
                *counts.entry(*cond).or_default() += 1;
            }
            crate::ir::Terminator::Return { value: Some(v) }
            | crate::ir::Terminator::Throw { value: v } => {
                *counts.entry(*v).or_default() += 1;
            }
            crate::ir::Terminator::Switch { value, .. } => {
                *counts.entry(*value).or_default() += 1;
            }
            _ => {}
        }
        // Count uses in phi operands
        for instr in &block.body {
            if instr.op == OpCode::Phi {
                for op in &instr.operands {
                    if let Operand::Var(v) = op {
                        *counts.entry(*v).or_default() += 1;
                    }
                }
            }
        }
    }

    counts
}

/// Determine which block each Var is defined in.
pub fn var_def_blocks(func: &Function) -> HashMap<Var, crate::ir::BlockId> {
    let mut map = HashMap::new();
    for block in &func.blocks {
        for instr in &block.body {
            if let Some(var) = instr.result {
                map.insert(var, block.id);
            }
        }
    }
    map
}

// ============================================================================
// Var naming
// ============================================================================

/// Assign readable names to variables.
pub fn name_vars(func: &Function) -> HashMap<Var, String> {
    let mut names = HashMap::new();

    // Name params
    for (i, &param) in func.params.iter().enumerate() {
        names.insert(param, format!("arg{i}"));
    }

    // Name other vars based on how they're computed
    for block in &func.blocks {
        for instr in &block.body {
            let Some(var) = instr.result else { continue };
            if names.contains_key(&var) {
                continue;
            }
            let name = match &instr.op {
                OpCode::LoadScope => {
                    if let Some(Operand::Const(Value::String(s))) = instr.operands.first() {
                        s.clone()
                    } else {
                        format!("v{}", var.0)
                    }
                }
                OpCode::Param => format!("arg{}", var.0),
                OpCode::Phi => format!("phi{}", var.0),
                _ => format!("v{}", var.0),
            };
            names.insert(var, name);
        }
    }

    names
}

// ============================================================================
// Expression reconstruction
// ============================================================================

/// Reconstruct expressions for a single block.
///
/// Returns a list of statements. Single-use variables are inlined into parent
/// expressions. Multi-use variables become named assignments.
pub fn reconstruct_block(
    block: &Block,
    use_counts: &HashMap<Var, usize>,
    def_blocks: &HashMap<Var, crate::ir::BlockId>,
    var_names: &HashMap<Var, String>,
) -> Vec<Stmt> {
    let mut expr_map: HashMap<Var, Expr> = HashMap::new();
    let mut stmts = Vec::new();

    for instr in &block.body {
        let expr = build_instruction_expr(instr, &expr_map, var_names);

        match instr.result {
            Some(var) if can_inline(var, block.id, use_counts, def_blocks) => {
                // Used once, defined in this block → store for inlining
                expr_map.insert(var, expr);
            }
            Some(var) => {
                // Multi-use or cross-block → emit named assignment
                let name = var_names.get(&var)
                    .cloned()
                    .unwrap_or_else(|| format!("v{}", var.0));
                stmts.push(Stmt::VarAssign { name: name.clone(), expr });
                expr_map.insert(var, Expr::Var(name));
            }
            None => {
                // Void instruction → emit as statement
                match instr.op {
                    OpCode::StoreProp => {
                        if instr.operands.len() >= 3 {
                            let obj = resolve_operand(&instr.operands[0], &expr_map, var_names);
                            let key = resolve_operand(&instr.operands[1], &expr_map, var_names);
                            let val = resolve_operand(&instr.operands[2], &expr_map, var_names);
                            stmts.push(Stmt::PropSet { obj, key, val });
                        }
                    }
                    OpCode::StoreIndex => {
                        if instr.operands.len() >= 3 {
                            let arr = resolve_operand(&instr.operands[0], &expr_map, var_names);
                            let idx = resolve_operand(&instr.operands[1], &expr_map, var_names);
                            let val = resolve_operand(&instr.operands[2], &expr_map, var_names);
                            stmts.push(Stmt::PropSet { obj: arr, key: idx, val });
                        }
                    }
                    OpCode::StoreScope => {
                        if instr.operands.len() >= 2 {
                            let name = match &instr.operands[0] {
                                Operand::Const(Value::String(s)) => s.clone(),
                                _ => "?".into(),
                            };
                            let val = resolve_operand(&instr.operands[1], &expr_map, var_names);
                            stmts.push(Stmt::VarAssign { name, expr: val });
                        }
                    }
                    _ => {
                        stmts.push(Stmt::ExprStmt(expr));
                    }
                }
            }
        }
    }

    stmts
}

/// Can this variable be inlined (used once, defined in the same block)?
fn can_inline(
    var: Var,
    current_block: crate::ir::BlockId,
    use_counts: &HashMap<Var, usize>,
    def_blocks: &HashMap<Var, crate::ir::BlockId>,
) -> bool {
    let uses = use_counts.get(&var).copied().unwrap_or(0);
    let same_block = def_blocks.get(&var).copied() == Some(current_block);
    // Inline only if used exactly once AND in the same block.
    // 0 uses means dead code — emit it anyway for visibility.
    uses == 1 && same_block
}

/// Convert an IR instruction to an expression.
fn build_instruction_expr(
    instr: &Instruction,
    expr_map: &HashMap<Var, Expr>,
    var_names: &HashMap<Var, String>,
) -> Expr {
    match instr.op {
        // ── Data ─────────────────────────────────────────────────────
        OpCode::Const => {
            match instr.operands.first() {
                Some(Operand::Const(val)) => Expr::Const(val.clone()),
                _ => Expr::Unknown("const?".into()),
            }
        }
        OpCode::Param => {
            let name = instr.result
                .and_then(|v| var_names.get(&v))
                .cloned()
                .unwrap_or_else(|| "arg?".into());
            Expr::Var(name)
        }
        OpCode::Phi => {
            let name = instr.result
                .and_then(|v| var_names.get(&v))
                .cloned()
                .unwrap_or_else(|| "phi?".into());
            Expr::Var(name)
        }
        OpCode::Move => resolve_first_var(&instr.operands, expr_map, var_names),

        // ── Pure binary ──────────────────────────────────────────────
        OpCode::Add => binary_expr(BinaryOp::Add, instr, expr_map, var_names),
        OpCode::Sub => binary_expr(BinaryOp::Sub, instr, expr_map, var_names),
        OpCode::Mul => binary_expr(BinaryOp::Mul, instr, expr_map, var_names),
        OpCode::Div => binary_expr(BinaryOp::Div, instr, expr_map, var_names),
        OpCode::Mod => binary_expr(BinaryOp::Mod, instr, expr_map, var_names),
        OpCode::Pow => binary_expr(BinaryOp::Pow, instr, expr_map, var_names),
        OpCode::BitAnd => binary_expr(BinaryOp::BitAnd, instr, expr_map, var_names),
        OpCode::BitOr => binary_expr(BinaryOp::BitOr, instr, expr_map, var_names),
        OpCode::BitXor => binary_expr(BinaryOp::BitXor, instr, expr_map, var_names),
        OpCode::Shl => binary_expr(BinaryOp::Shl, instr, expr_map, var_names),
        OpCode::Shr => binary_expr(BinaryOp::Shr, instr, expr_map, var_names),
        OpCode::UShr => binary_expr(BinaryOp::UShr, instr, expr_map, var_names),
        OpCode::Eq => binary_expr(BinaryOp::Eq, instr, expr_map, var_names),
        OpCode::Neq => binary_expr(BinaryOp::Neq, instr, expr_map, var_names),
        OpCode::StrictEq => binary_expr(BinaryOp::StrictEq, instr, expr_map, var_names),
        OpCode::StrictNeq => binary_expr(BinaryOp::StrictNeq, instr, expr_map, var_names),
        OpCode::Lt => binary_expr(BinaryOp::Lt, instr, expr_map, var_names),
        OpCode::Gt => binary_expr(BinaryOp::Gt, instr, expr_map, var_names),
        OpCode::Lte => binary_expr(BinaryOp::Lte, instr, expr_map, var_names),
        OpCode::Gte => binary_expr(BinaryOp::Gte, instr, expr_map, var_names),

        // ── Pure unary ───────────────────────────────────────────────
        OpCode::Neg => unary_expr(UnaryOp::Neg, instr, expr_map, var_names),
        OpCode::Pos => unary_expr(UnaryOp::Pos, instr, expr_map, var_names),
        OpCode::LogicalNot => unary_expr(UnaryOp::LogicalNot, instr, expr_map, var_names),
        OpCode::BitNot => unary_expr(UnaryOp::BitNot, instr, expr_map, var_names),
        OpCode::TypeOf => unary_expr(UnaryOp::TypeOf, instr, expr_map, var_names),
        OpCode::Void => unary_expr(UnaryOp::Void, instr, expr_map, var_names),

        // ── Memory ───────────────────────────────────────────────────
        OpCode::LoadProp => {
            let obj = resolve_operand_at(0, &instr.operands, expr_map, var_names);
            let key = resolve_operand_at(1, &instr.operands, expr_map, var_names);
            Expr::PropAccess { obj: Box::new(obj), key: Box::new(key) }
        }
        OpCode::LoadIndex => {
            let arr = resolve_operand_at(0, &instr.operands, expr_map, var_names);
            let idx = resolve_operand_at(1, &instr.operands, expr_map, var_names);
            Expr::Index { array: Box::new(arr), index: Box::new(idx) }
        }
        OpCode::LoadScope => {
            match instr.operands.first() {
                Some(Operand::Const(Value::String(s))) => Expr::Var(s.clone()),
                _ => Expr::Unknown("scope?".into()),
            }
        }
        OpCode::NewObject => Expr::ObjectLit(vec![]),
        OpCode::NewArray => Expr::ArrayLit(vec![]),

        // ── Control ──────────────────────────────────────────────────
        OpCode::Call => {
            let func_expr = resolve_operand_at(0, &instr.operands, expr_map, var_names);
            let args: Vec<Expr> = instr.operands[1..].iter()
                .map(|op| resolve_operand(op, expr_map, var_names))
                .collect();
            Expr::Call { func: Box::new(func_expr), args }
        }
        OpCode::CallMethod => {
            let obj = resolve_operand_at(0, &instr.operands, expr_map, var_names);
            let method = match instr.operands.get(1) {
                Some(Operand::Const(Value::String(s))) => s.clone(),
                _ => "?".into(),
            };
            let args: Vec<Expr> = instr.operands[2..].iter()
                .map(|op| resolve_operand(op, expr_map, var_names))
                .collect();
            Expr::MethodCall { obj: Box::new(obj), method, args }
        }

        // ── Void ops (handled by caller) ─────────────────────────────
        OpCode::StoreProp | OpCode::StoreIndex | OpCode::StoreScope
        | OpCode::DeleteProp | OpCode::HasProp => {
            Expr::Unknown(format!("{}", instr.op))
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn resolve_operand(
    op: &Operand,
    expr_map: &HashMap<Var, Expr>,
    var_names: &HashMap<Var, String>,
) -> Expr {
    match op {
        Operand::Var(v) => {
            if let Some(expr) = expr_map.get(v) {
                expr.clone()
            } else if let Some(name) = var_names.get(v) {
                Expr::Var(name.clone())
            } else {
                Expr::Var(format!("v{}", v.0))
            }
        }
        Operand::Const(val) => Expr::Const(val.clone()),
        Operand::Func(fid) => Expr::Var(format!("fn#{}", fid.0)),
        Operand::Block(bid) => Expr::Unknown(format!("block {bid}")),
    }
}

fn resolve_operand_at(
    idx: usize,
    operands: &[Operand],
    expr_map: &HashMap<Var, Expr>,
    var_names: &HashMap<Var, String>,
) -> Expr {
    operands.get(idx)
        .map(|op| resolve_operand(op, expr_map, var_names))
        .unwrap_or_else(|| Expr::Unknown("missing".into()))
}

fn resolve_first_var(
    operands: &[Operand],
    expr_map: &HashMap<Var, Expr>,
    var_names: &HashMap<Var, String>,
) -> Expr {
    resolve_operand_at(0, operands, expr_map, var_names)
}

fn binary_expr(
    op: BinaryOp,
    instr: &Instruction,
    expr_map: &HashMap<Var, Expr>,
    var_names: &HashMap<Var, String>,
) -> Expr {
    let left = resolve_operand_at(0, &instr.operands, expr_map, var_names);
    let right = resolve_operand_at(1, &instr.operands, expr_map, var_names);
    Expr::binary(op, left, right)
}

fn unary_expr(
    op: UnaryOp,
    instr: &Instruction,
    expr_map: &HashMap<Var, Expr>,
    var_names: &HashMap<Var, String>,
) -> Expr {
    let operand = resolve_operand_at(0, &instr.operands, expr_map, var_names);
    Expr::unary(op, operand)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::IrBuilder;

    #[test]
    fn inline_single_use_constants() {
        // %0 = const 10, %1 = const 20, %2 = add %0, %1, halt
        // → single statement: v2 = 10 + 20
        let mut b = IrBuilder::new();
        b.begin_function("test");
        b.create_and_switch("entry");
        let v0 = b.const_number(10.0);
        let v1 = b.const_number(20.0);
        let _v2 = b.add(v0, v1);
        b.halt();
        b.end_function();

        let module = b.build();
        let func = &module.functions[0];
        let block = &func.blocks[0];

        let use_counts = count_uses(func);
        let def_blocks = var_def_blocks(func);
        let var_names = name_vars(func);

        let stmts = reconstruct_block(block, &use_counts, &def_blocks, &var_names);

        // v0 and v1 are each used once → inlined
        // v2 is used 0 times (only defined) → still emitted as assignment
        // The result should fold to something like: v2 = 10 + 20
        assert!(!stmts.is_empty());
        let text: String = stmts.iter().map(|s| s.to_string()).collect();
        assert!(text.contains("10 + 20"), "got: {text}");
    }

    #[test]
    fn multi_use_var_gets_named() {
        // %0 = const 5, %1 = add %0, %0 → %0 used twice, must be named
        let mut b = IrBuilder::new();
        b.begin_function("test");
        b.create_and_switch("entry");
        let v0 = b.const_number(5.0);
        let _v1 = b.add(v0, v0);
        b.halt();
        b.end_function();

        let module = b.build();
        let func = &module.functions[0];
        let block = &func.blocks[0];

        let use_counts = count_uses(func);
        let def_blocks = var_def_blocks(func);
        let var_names = name_vars(func);

        let stmts = reconstruct_block(block, &use_counts, &def_blocks, &var_names);
        let text: String = stmts.iter().map(|s| s.to_string()).collect();

        // v0 is used twice → named "v0"
        assert!(text.contains("v0 = 5"), "got: {text}");
        assert!(text.contains("v0 + v0") || text.contains("v0, v0"), "got: {text}");
    }

    #[test]
    fn scope_load_uses_variable_name() {
        let mut b = IrBuilder::new();
        b.begin_function("test");
        b.create_and_switch("entry");
        let key = b.load_scope("secret_key");
        b.ret(Some(key));
        b.end_function();

        let module = b.build();
        let func = &module.functions[0];
        let var_names = name_vars(func);

        // LoadScope("secret_key") should name the var "secret_key"
        assert_eq!(var_names.get(&Var(0)), Some(&"secret_key".to_string()));
    }

    #[test]
    fn cipher_expression_inlines() {
        // Simulate: result = sbox[i] ^ (key & 0xFF)
        // IR: %0=load_scope "sbox", %1=load_scope "i", %2=load_index %0 %1,
        //     %3=load_scope "key", %4=const 255, %5=bit_and %3 %4,
        //     %6=bit_xor %2 %5
        let mut b = IrBuilder::new();
        b.begin_function("test");
        b.create_and_switch("entry");
        let sbox = b.load_scope("sbox");
        let i = b.load_scope("i");
        let elem = b.load_index(sbox, i);
        let key = b.load_scope("key");
        let mask = b.const_number(255.0);
        let masked = b.bit_and(key, mask);
        let _result = b.bit_xor(elem, masked);
        b.halt();
        b.end_function();

        let module = b.build();
        let func = &module.functions[0];
        let block = &func.blocks[0];

        let use_counts = count_uses(func);
        let def_blocks = var_def_blocks(func);
        let var_names = name_vars(func);

        let stmts = reconstruct_block(block, &use_counts, &def_blocks, &var_names);
        let text: String = stmts.iter().map(|s| s.to_string()).collect();

        // All intermediate vars used once → fully inlined
        // Result should be something like: v6 = sbox[i] ^ (key & 255)
        assert!(text.contains("sbox[i]"), "missing sbox[i] in: {text}");
        assert!(text.contains("key & 255") || text.contains("key &255"), "missing key & 255 in: {text}");
        assert!(text.contains("^"), "missing XOR in: {text}");
    }
}

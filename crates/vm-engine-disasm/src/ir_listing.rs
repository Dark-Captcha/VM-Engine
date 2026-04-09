//! IR listing with source PC annotations and block structure.
//!
//! Produces a disassembly-style listing where each instruction shows
//! its IR form with the original bytecode PC in the left margin.

// ============================================================================
// Imports
// ============================================================================

use std::fmt::Write;

use vm_engine_core::ir::{Block, Function, Instruction, Module};

// ============================================================================
// IR Listing
// ============================================================================

/// Format a complete module as an annotated IR listing.
pub fn format_ir_listing(module: &Module) -> String {
    let mut output = String::new();
    for (index, function) in module.functions.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        format_function_ir(&mut output, function);
    }
    output
}

/// Format a single function as an annotated IR listing.
pub fn format_function_ir(output: &mut String, function: &Function) {
    // Function header
    write!(output, "function {}(", function.name).unwrap();
    for (index, param) in function.params.iter().enumerate() {
        if index > 0 {
            write!(output, ", ").unwrap();
        }
        write!(output, "{param}").unwrap();
    }
    writeln!(output, "):").unwrap();

    // Blocks
    for block in &function.blocks {
        format_block_ir(output, block);
    }
}

/// Format a single block with PC annotations.
fn format_block_ir(output: &mut String, block: &Block) {
    writeln!(output, "  {} ({}):", block.label, block.id).unwrap();

    for instruction in &block.body {
        format_instruction_ir(output, instruction);
    }

    // Terminator (no source PC — always from IR structure, not bytecode)
    let terminator_text = format!("{}", block.terminator);
    writeln!(output, "         |     {terminator_text}").unwrap();
}

/// Format one instruction with source PC in the left margin.
fn format_instruction_ir(output: &mut String, instruction: &Instruction) {
    let source_annotation = match &instruction.source {
        Some(source_loc) => format!("{:>6}", source_loc.pc),
        None => "      ".to_string(),
    };

    let mut instruction_text = String::new();
    if let Some(var) = &instruction.result {
        write!(instruction_text, "{var} = ").unwrap();
    }
    write!(instruction_text, "{}", instruction.op).unwrap();
    for (index, operand) in instruction.operands.iter().enumerate() {
        if index == 0 {
            write!(instruction_text, " {operand}").unwrap();
        } else {
            write!(instruction_text, ", {operand}").unwrap();
        }
    }

    writeln!(output, "  {source_annotation} |     {instruction_text}").unwrap();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use vm_engine_core::ir::builder::IrBuilder;
    use vm_engine_core::ir::operand::SourceLoc;
    use vm_engine_core::ir::opcode::OpCode;
    use vm_engine_core::ir::operand::Operand;
    use vm_engine_core::value::Value;

    #[test]
    fn ir_listing_shows_source_pc() {
        let mut builder = IrBuilder::new();
        builder.begin_function("test");
        builder.create_and_switch("entry");
        let _var = builder.emit_sourced(
            OpCode::Const,
            vec![Operand::Const(Value::number(42.0))],
            SourceLoc::with_opcode(100, 0x66),
        );
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let listing = format_ir_listing(&module);

        assert!(listing.contains("100"), "should show source PC:\n{listing}");
        assert!(listing.contains("const 42"), "should show instruction:\n{listing}");
    }

    #[test]
    fn ir_listing_with_params() {
        let mut builder = IrBuilder::new();
        builder.begin_function("add_one");
        let param = builder.add_param();
        builder.create_and_switch("entry");
        let one = builder.const_number(1.0);
        let _ = builder.add(param, one);
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let listing = format_ir_listing(&module);

        assert!(listing.contains("function add_one(%0):"), "should show param:\n{listing}");
        assert!(listing.contains("add %0"), "should reference param in add:\n{listing}");
    }

    #[test]
    fn ir_listing_void_instruction() {
        let mut builder = IrBuilder::new();
        builder.begin_function("setter");
        builder.create_and_switch("entry");
        let obj = builder.new_object();
        let key = builder.const_string("x");
        let val = builder.const_number(42.0);
        builder.store_prop(obj, key, val);
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let listing = format_ir_listing(&module);

        // store_prop has no result, so no "%N = " prefix
        assert!(listing.contains("store_prop"), "should show void op:\n{listing}");
        assert!(!listing.contains("= store_prop"), "void op should have no result:\n{listing}");
    }

    #[test]
    fn ir_listing_shows_blocks() {
        let mut builder = IrBuilder::new();
        builder.begin_function("branched");
        let entry = builder.create_and_switch("entry");
        let cond = builder.const_bool(true);
        let yes = builder.create_block("yes");
        let no = builder.create_block("no");
        builder.switch_to(entry);
        builder.branch_if(cond, yes, no);
        builder.switch_to(yes);
        builder.halt();
        builder.switch_to(no);
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let listing = format_ir_listing(&module);

        assert!(listing.contains("entry (@B0):"), "listing:\n{listing}");
        assert!(listing.contains("yes (@B1):"), "listing:\n{listing}");
        assert!(listing.contains("no (@B2):"), "listing:\n{listing}");
    }
}

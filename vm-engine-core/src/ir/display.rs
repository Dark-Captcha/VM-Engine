//! Pretty-print IR as readable text.

// ============================================================================
// Imports
// ============================================================================

use std::fmt;

use super::{Block, Function, Instruction, Module};

// ============================================================================
// Display implementations
// ============================================================================

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, func) in self.functions.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{func}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "function {}(", self.name)?;
        for (i, param) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{param}")?;
        }
        writeln!(f, "):")?;

        for block in &self.blocks {
            write!(f, "{block}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  {} ({}):", self.label, self.id)?;

        for instr in &self.body {
            write!(f, "    {instr}")?;
            if let Some(src) = &instr.source {
                write!(f, "  ; {src}")?;
            }
            writeln!(f)?;
        }
        writeln!(f, "    {}", self.terminator)
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(var) = &self.result {
            write!(f, "{var} = ")?;
        }
        write!(f, "{}", self.op)?;
        for (i, operand) in self.operands.iter().enumerate() {
            if i == 0 {
                write!(f, " {operand}")?;
            } else {
                write!(f, ", {operand}")?;
            }
        }
        Ok(())
    }
}

// ============================================================================
// Convenience
// ============================================================================

/// Format an entire module as a string.
pub fn format_module(module: &Module) -> String {
    module.to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::builder::IrBuilder;

    #[test]
    fn display_simple_function() {
        let mut b = IrBuilder::new();
        b.begin_function("add_numbers");
        b.create_and_switch("entry");
        let a = b.const_number(10.0);
        let c = b.const_number(20.0);
        let _r = b.add(a, c);
        b.halt();
        b.end_function();

        let module = b.build();
        let text = format_module(&module);

        assert!(text.contains("function add_numbers():"));
        assert!(text.contains("entry (@B0):"));
        assert!(text.contains("%0 = const 10"));
        assert!(text.contains("%1 = const 20"));
        assert!(text.contains("%2 = add %0, %1"));
        assert!(text.contains("halt"));
    }

    #[test]
    fn display_branching_function() {
        let mut b = IrBuilder::new();
        b.begin_function("branch");
        let entry = b.create_and_switch("entry");
        let cond = b.const_bool(true);
        let yes = b.create_block("yes");
        let no = b.create_block("no");
        b.switch_to(entry);
        b.branch_if(cond, yes, no);

        b.switch_to(yes);
        let v1 = b.const_number(1.0);
        b.ret(Some(v1));

        b.switch_to(no);
        let v2 = b.const_number(0.0);
        b.ret(Some(v2));

        b.end_function();
        let text = format_module(&b.build());

        assert!(text.contains("branch_if %0, @B1, @B2"));
        assert!(text.contains("yes (@B1):"));
        assert!(text.contains("no (@B2):"));
        assert!(text.contains("return %1"));
        assert!(text.contains("return %2"));
    }

    #[test]
    fn display_with_params() {
        let mut b = IrBuilder::new();
        b.begin_function("identity");
        let p = b.add_param();
        b.create_and_switch("entry");
        b.ret(Some(p));
        b.end_function();

        let text = format_module(&b.build());
        assert!(text.contains("function identity(%0):"));
        assert!(text.contains("return %0"));
    }
}

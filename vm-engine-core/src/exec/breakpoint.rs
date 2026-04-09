//! Breakpoint conditions for the IR interpreter.

// ============================================================================
// Imports
// ============================================================================

use crate::ir::{BlockId, FuncId, Var};

use super::state::State;

// ============================================================================
// Breakpoint
// ============================================================================

/// Condition that pauses execution.
pub enum Breakpoint {
    /// Break at a specific instruction index in a specific block.
    AtInstruction { func: FuncId, block: BlockId, index: usize },
    /// Break at an original bytecode PC (via SourceLoc).
    AtSourcePc(usize),
    /// Break when a specific variable is assigned.
    OnVarWrite(Var),
    /// Break when a specific function is called.
    OnCall(FuncId),
    /// Break after N instructions total.
    AfterSteps(u64),
    /// Break on a custom predicate.
    Custom(Box<dyn Fn(&State) -> bool>),
}

impl Breakpoint {
    /// Check if this breakpoint triggers given the current state.
    pub fn should_break(&self, state: &State) -> bool {
        match self {
            Self::AtInstruction { func, block, index } => {
                state.cursor.function == *func
                    && state.cursor.block == *block
                    && state.cursor.instruction == *index
            }
            Self::AtSourcePc(_pc) => {
                // Checked by the interpreter against the current instruction's SourceLoc
                false
            }
            Self::OnVarWrite(_) => {
                // Checked by the interpreter after variable assignment
                false
            }
            Self::OnCall(_) => {
                // Checked by the interpreter before function call
                false
            }
            Self::AfterSteps(n) => state.instruction_count >= *n,
            Self::Custom(pred) => pred(state),
        }
    }
}

impl std::fmt::Debug for Breakpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AtInstruction { func, block, index } => {
                write!(f, "Break@{func}:{block}.{index}")
            }
            Self::AtSourcePc(pc) => write!(f, "Break@pc={pc}"),
            Self::OnVarWrite(var) => write!(f, "Break@write({var})"),
            Self::OnCall(func) => write!(f, "Break@call({func})"),
            Self::AfterSteps(n) => write!(f, "Break@step={n}"),
            Self::Custom(_) => write!(f, "Break@custom"),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_steps_triggers() {
        let bp = Breakpoint::AfterSteps(5);
        let mut state = State::new(FuncId(0), BlockId(0));
        state.instruction_count = 3;
        assert!(!bp.should_break(&state));
        state.instruction_count = 5;
        assert!(bp.should_break(&state));
    }

    #[test]
    fn at_instruction_triggers() {
        let bp = Breakpoint::AtInstruction {
            func: FuncId(0),
            block: BlockId(1),
            index: 3,
        };
        let mut state = State::new(FuncId(0), BlockId(0));
        assert!(!bp.should_break(&state));
        state.cursor.block = BlockId(1);
        state.cursor.instruction = 3;
        assert!(bp.should_break(&state));
    }

    #[test]
    fn custom_breakpoint() {
        let bp = Breakpoint::Custom(Box::new(|s| s.halted));
        let mut state = State::new(FuncId(0), BlockId(0));
        assert!(!bp.should_break(&state));
        state.halted = true;
        assert!(bp.should_break(&state));
    }
}

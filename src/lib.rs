//! Universal intermediate representation and analysis framework for bytecode VMs.
//!
//! Converts any VM bytecode into one structured IR format, then analyzes,
//! recovers control flow, and optionally executes it.

pub mod error;
pub mod exec;
pub mod graph;
pub mod ir;
pub mod structure;
pub mod value;
pub mod web;
pub mod disasm;

// ── Key re-exports for common use ───────────────────────────────────────────

pub use error::{Error, Result};
pub use ir::builder::IrBuilder;
pub use ir::opcode::{OpCode, Terminator};
pub use ir::operand::{Operand, SourceLoc};
pub use ir::{BlockId, FuncId, Function, Instruction, Module, Var};
pub use value::{ClosureId, ObjectId, Value};

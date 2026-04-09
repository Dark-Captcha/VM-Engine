//! S-box extraction from IR — finds 256-element constant array patterns.
//!
//! PLV3 initializes S-boxes via MULTI_PUSH + COLLECT, which the decoder
//! converts to NewArray + 256 StoreIndex with constant values. This module
//! walks the IR to find and extract those patterns.
//!
//! This is a pure IR analysis — no execution, no bytecode-level knowledge.
//! Works on any rotation as long as the S-box initialization pattern is the same.

// ============================================================================
// Imports
// ============================================================================

use std::collections::HashMap;

use vm_engine_core::ir::opcode::OpCode;
use vm_engine_core::ir::operand::Operand;
use vm_engine_core::ir::{BlockId, Module, Var};
use vm_engine_core::value::Value;

// ============================================================================
// Types
// ============================================================================

/// A 256-byte substitution box extracted from the IR.
#[derive(Debug, Clone)]
pub struct ExtractedSbox {
    /// Index by order of appearance (0 = first S-box in bytecode).
    pub index: usize,
    /// The 256-byte lookup table.
    pub bytes: Vec<u8>,
    /// Which block this S-box was found in.
    pub source_block: BlockId,
    /// The IR variable holding this array.
    pub array_var: Var,
}

/// Result of S-box extraction.
#[derive(Debug)]
pub struct SboxExtraction {
    /// All S-boxes found (256-element constant arrays).
    pub sboxes: Vec<ExtractedSbox>,
    /// Total NewArray instructions found in the IR.
    pub total_arrays: usize,
    /// Arrays that had some constant StoreIndex entries but not exactly 256.
    pub partial_arrays: usize,
}

// ============================================================================
// Extraction
// ============================================================================

/// Extract all S-boxes from an IR module.
///
/// Finds patterns: NewArray followed by exactly 256 StoreIndex instructions
/// where both the index and value resolve to numeric constants.
pub fn extract_sboxes(module: &Module) -> SboxExtraction {
    let mut sboxes = Vec::new();
    let mut total_arrays = 0;
    let mut partial_arrays = 0;

    for function in &module.functions {
        // Build constant resolution map: Var → numeric value
        let const_map = build_const_map(function);

        for block in &function.blocks {
            let body = &block.body;

            for (instruction_index, instruction) in body.iter().enumerate() {
                if instruction.op != OpCode::NewArray {
                    continue;
                }
                total_arrays += 1;

                let Some(array_var) = instruction.result else { continue };

                // Scan forward in this block for StoreIndex into this array
                let mut entries: HashMap<usize, u8> = HashMap::new();
                let mut store_count = 0;

                for subsequent in &body[instruction_index + 1..] {
                    if subsequent.op != OpCode::StoreIndex {
                        continue;
                    }
                    // StoreIndex operands: [array, index, value]
                    if subsequent.operands.len() < 3 {
                        continue;
                    }

                    let target_array = resolve_var_to_var(&subsequent.operands[0]);
                    if target_array != Some(array_var) {
                        continue;
                    }

                    store_count += 1;

                    // Resolve index to a constant number
                    let index_value = resolve_to_number(
                        &subsequent.operands[1],
                        &const_map,
                    );
                    let element_value = resolve_to_number(
                        &subsequent.operands[2],
                        &const_map,
                    );

                    if let (Some(index), Some(value)) = (index_value, element_value) {
                        let index_usize = index as usize;
                        // Clamp to byte range (values may be negative from signed encoding)
                        let byte_value = ((value as i64) & 0xFF) as u8;
                        entries.insert(index_usize, byte_value);
                    }
                }

                if entries.len() == 256 && store_count == 256 {
                    // All 256 entries are constant — this is an S-box
                    let mut bytes = vec![0u8; 256];
                    let mut complete = true;
                    for (index, slot) in bytes.iter_mut().enumerate() {
                        if let Some(&byte) = entries.get(&index) {
                            *slot = byte;
                        } else {
                            complete = false;
                            break;
                        }
                    }
                    if complete {
                        sboxes.push(ExtractedSbox {
                            index: sboxes.len(),
                            bytes,
                            source_block: block.id,
                            array_var,
                        });
                    }
                } else if store_count > 0 && store_count < 256 {
                    partial_arrays += 1;
                }
            }
        }
    }

    SboxExtraction {
        sboxes,
        total_arrays,
        partial_arrays,
    }
}

/// Verify extracted S-boxes against known reference data.
///
/// Returns (matched_count, mismatched_indices).
pub fn verify_sboxes(
    extracted: &[ExtractedSbox],
    reference: &[Vec<u8>],
) -> (usize, Vec<usize>) {
    let mut matched = 0;
    let mut mismatched = Vec::new();

    for (index, extracted_sbox) in extracted.iter().enumerate() {
        if let Some(reference_sbox) = reference.get(index) {
            if extracted_sbox.bytes == *reference_sbox {
                matched += 1;
            } else {
                mismatched.push(index);
            }
        }
    }

    (matched, mismatched)
}

// ============================================================================
// Helpers
// ============================================================================

/// Build a map from Var → numeric Value for all Const instructions.
fn build_const_map(
    function: &vm_engine_core::ir::Function,
) -> HashMap<Var, f64> {
    let mut constants = HashMap::new();

    for block in &function.blocks {
        for instruction in &block.body {
            if instruction.op == OpCode::Const
                && let (Some(var), Some(Operand::Const(value))) =
                    (instruction.result, instruction.operands.first())
                    && let Some(number) = value.as_number() {
                        constants.insert(var, number);
                    }
        }
    }

    constants
}

/// Extract the Var from an operand (if it is a Var).
fn resolve_var_to_var(operand: &Operand) -> Option<Var> {
    match operand {
        Operand::Var(var) => Some(*var),
        _ => None,
    }
}

/// Resolve an operand to a numeric value — either directly from Const
/// or by looking up the Var in the const map.
fn resolve_to_number(
    operand: &Operand,
    const_map: &HashMap<Var, f64>,
) -> Option<f64> {
    match operand {
        Operand::Const(Value::Number(number)) => Some(*number),
        Operand::Var(var) => const_map.get(var).copied(),
        _ => None,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use vm_engine_core::ir::builder::IrBuilder;

    #[test]
    fn extract_single_sbox() {
        let mut builder = IrBuilder::new();
        builder.begin_function("main");
        builder.create_and_switch("entry");

        // Simulate: array = []; array[0]=10; array[1]=20; ... array[255]=255
        let array = builder.new_array();
        for index in 0..256u16 {
            let idx_var = builder.const_number(index as f64);
            let val_var = builder.const_number((index ^ 0xAB) as f64);
            builder.store_index(array, idx_var, val_var);
        }
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let result = extract_sboxes(&module);

        assert_eq!(result.sboxes.len(), 1, "should find 1 S-box");
        assert_eq!(result.sboxes[0].bytes[0], (0 ^ 0xAB) as u8);
        assert_eq!(result.sboxes[0].bytes[1], (1 ^ 0xAB) as u8);
        assert_eq!(result.sboxes[0].bytes[255], (255 ^ 0xAB) as u8);
    }

    #[test]
    fn ignore_non_256_arrays() {
        let mut builder = IrBuilder::new();
        builder.begin_function("main");
        builder.create_and_switch("entry");

        // Small array (not an S-box)
        let small_array = builder.new_array();
        for index in 0..10u16 {
            let idx_var = builder.const_number(index as f64);
            let val_var = builder.const_number(index as f64);
            builder.store_index(small_array, idx_var, val_var);
        }
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let result = extract_sboxes(&module);

        assert_eq!(result.sboxes.len(), 0, "should NOT extract non-256 arrays");
        assert_eq!(result.partial_arrays, 1, "should count as partial");
    }

    #[test]
    fn extract_multiple_sboxes() {
        let mut builder = IrBuilder::new();
        builder.begin_function("main");
        builder.create_and_switch("entry");

        // Two S-boxes
        for sbox_index in 0..2 {
            let array = builder.new_array();
            for index in 0..256u16 {
                let idx_var = builder.const_number(index as f64);
                let val_var = builder.const_number(((index + sbox_index * 100) & 0xFF) as f64);
                builder.store_index(array, idx_var, val_var);
            }
        }
        builder.halt();
        builder.end_function();

        let module = builder.build();
        let result = extract_sboxes(&module);

        assert_eq!(result.sboxes.len(), 2);
        assert_eq!(result.sboxes[0].index, 0);
        assert_eq!(result.sboxes[1].index, 1);
        // Values should differ between the two
        assert_ne!(result.sboxes[0].bytes, result.sboxes[1].bytes);
    }

    #[test]
    fn verify_against_reference() {
        let extracted = vec![
            ExtractedSbox {
                index: 0,
                bytes: vec![1; 256],
                source_block: BlockId(0),
                array_var: Var(0),
            },
            ExtractedSbox {
                index: 1,
                bytes: vec![2; 256],
                source_block: BlockId(0),
                array_var: Var(1),
            },
        ];
        let reference = vec![vec![1u8; 256], vec![2u8; 256]];

        let (matched, mismatched) = verify_sboxes(&extracted, &reference);
        assert_eq!(matched, 2);
        assert!(mismatched.is_empty());
    }
}

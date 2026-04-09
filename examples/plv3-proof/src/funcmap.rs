#![allow(dead_code)] // Used by decoder restructuring (in progress)
//! MAKE_FUNC boundary detection for PLV3 bytecode.
//!
//! Scans bytecode to find all function definitions and their exact byte ranges.
//! Used by the decoder to emit separate IR functions.

use crate::reader::Plv3Reader;

// ============================================================================
// Types
// ============================================================================

/// A function definition found in the bytecode.
#[derive(Debug, Clone)]
pub struct FuncRange {
    /// Index (0-based, order of appearance).
    pub index: usize,
    /// PC of the MAKE_FUNC opcode.
    pub make_func_pc: usize,
    /// First byte of the function body (after JMP_FWD).
    pub body_start: usize,
    /// First byte AFTER the function body.
    pub body_end: usize,
    /// Number of parameters.
    pub param_count: u8,
    /// Number of captured variables.
    pub capture_count: u8,
}

impl FuncRange {
    pub fn body_size(&self) -> usize {
        self.body_end - self.body_start
    }
}

/// All function definitions + main code regions.
#[derive(Debug)]
pub struct FuncMap {
    /// The 132 (or however many) function definitions.
    pub functions: Vec<FuncRange>,
    /// Main code before the first MAKE_FUNC.
    pub main_pre_end: usize,
    /// Main code after the last function body.
    pub main_post_start: usize,
    /// Total bytecode length.
    pub bytecode_len: usize,
}

// ============================================================================
// Scanning
// ============================================================================

/// Scan bytecode to find all MAKE_FUNC boundaries.
pub fn find_func_ranges(bytecode: &[u8]) -> FuncMap {
    let mut functions = Vec::new();
    let mut reader = Plv3Reader::new(bytecode);

    while !reader.at_end() {
        let opcode_pc = reader.position;
        let Some(opcode) = reader.read_byte() else { break };

        if opcode == 55 {
            // MAKE_FUNC: paramCount(1F), captureCount(1F), captures(captureCount * 1F)
            let param_count = reader.read_byte().unwrap_or(0);
            let capture_count = reader.read_byte().unwrap_or(0);
            for _ in 0..capture_count {
                reader.read_byte();
            }

            // Next should be JMP_FWD (189) + u16 offset that skips the body
            if reader.position + 2 < bytecode.len() && bytecode[reader.position] == 189 {
                reader.read_byte(); // consume 189
                let offset = reader.read_u16_be().unwrap_or(0) as usize;
                let body_start = reader.position;
                let body_end = reader.position + offset;

                functions.push(FuncRange {
                    index: functions.len(),
                    make_func_pc: opcode_pc,
                    body_start,
                    body_end,
                    param_count,
                    capture_count,
                });

                // Skip past the function body
                reader.position = body_end;
            }
        } else {
            // Skip operands for non-MAKE_FUNC opcodes
            skip_operands(opcode, &mut reader);
        }
    }

    let main_pre_end = functions.first().map(|f| f.make_func_pc).unwrap_or(bytecode.len());
    let main_post_start = functions.last().map(|f| f.body_end).unwrap_or(0);

    FuncMap {
        functions,
        main_pre_end,
        main_post_start,
        bytecode_len: bytecode.len(),
    }
}

/// Skip operands for a known opcode (used during MAKE_FUNC scanning).
fn skip_operands(opcode: u8, reader: &mut Plv3Reader<'_>) {
    match opcode {
        // 0 operands
        0 | 15 | 19 | 21 | 30 | 35 | 41 | 51 | 57 | 60 | 64 | 72 | 81 | 104
        | 105 | 112 | 119 | 128 | 138 | 155 | 157 | 164 | 166 | 175 | 178 | 195
        | 197 | 202 | 207 | 233 | 237 | 246 | 255 => {}

        // 1i (typed value)
        66 | 46 | 225 | 152 | 129 | 188 | 253 | 125 | 73 | 62 | 101 | 249 => {
            reader.read_typed_value();
        }

        // 1g (u16)
        29 | 85 | 172 | 18 | 22 | 224 | 106 | 191 | 179 | 136 | 184 | 211
        | 124 | 93 | 110 | 121 | 71 | 176 | 116 | 20 | 189 | 42 | 169 | 89 => {
            reader.read_u16_be();
        }

        // 2g
        232 | 32 | 245 | 174 | 114 | 151 | 168 | 115 | 150 => {
            reader.read_u16_be();
            reader.read_u16_be();
        }

        // 3g
        221 | 120 | 45 | 212 | 84 | 10 | 251 => {
            reader.read_u16_be();
            reader.read_u16_be();
            reader.read_u16_be();
        }

        // 1g + 1i
        31 | 54 | 111 | 183 | 26 | 229 | 250 => {
            reader.read_u16_be();
            reader.read_typed_value();
        }

        // 2g + 1i
        118 | 47 | 44 | 109 => {
            reader.read_u16_be();
            reader.read_u16_be();
            reader.read_typed_value();
        }

        // MULTI_PUSH: 1F + N*i
        238 => {
            let count = reader.read_byte().unwrap_or(0);
            for _ in 0..count {
                reader.read_typed_value();
            }
        }

        // CALL, NEW_CALL: 1F
        147 | 87 => {
            reader.read_byte();
        }

        _ => {} // unknown — 0 operands assumed
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_func_ranges_on_real_bytecode() {
        let bytecode = std::fs::read(
            "/home/gnusocute/Documents/Dark-Captcha_old/datadome/plv3-vm/source/bytecode.bin"
        );
        let Ok(bytecode) = bytecode else { return }; // skip if file not available

        let func_map = find_func_ranges(&bytecode);

        assert!(func_map.functions.len() >= 130, "should find ~132 functions, got {}", func_map.functions.len());
        assert_eq!(func_map.functions[0].body_start, 16566);
        assert_eq!(func_map.functions[0].param_count, 13);
        assert_eq!(func_map.functions[1].body_start, 17209);
        assert_eq!(func_map.functions[1].param_count, 15);

        // No overlapping function bodies
        for i in 0..func_map.functions.len() {
            for j in (i + 1)..func_map.functions.len() {
                let func_a = &func_map.functions[i];
                let func_b = &func_map.functions[j];
                assert!(
                    func_a.body_end <= func_b.body_start || func_b.body_end <= func_a.body_start,
                    "functions {} and {} overlap: [{}-{}) and [{}-{})",
                    i, j, func_a.body_start, func_a.body_end, func_b.body_start, func_b.body_end,
                );
            }
        }

        // Main code regions
        assert!(func_map.main_pre_end > 0);
        assert!(func_map.main_post_start < func_map.bytecode_len);
    }
}

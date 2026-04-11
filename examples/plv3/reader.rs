//! PLV3 bytecode reader: typed values, register addresses, raw bytes.
//!
//! Implements the PLV3 VM's operand encoding:
//! - `F()` — 1 raw byte
//! - `g()` — 2 bytes big-endian uint16 (register/frame address)
//! - `i()` — typed value with tag byte (variable length)

use vm_engine::value::Value;

/// PLV3 bytecode reader with position tracking.
pub struct Plv3Reader<'a> {
    pub bytecode: &'a [u8],
    pub position: usize,
}

impl<'a> Plv3Reader<'a> {
    pub fn new(bytecode: &'a [u8]) -> Self {
        Self { bytecode, position: 0 }
    }

    pub fn at_end(&self) -> bool {
        self.position >= self.bytecode.len()
    }

    #[allow(dead_code)]
    pub fn remaining(&self) -> usize {
        self.bytecode.len().saturating_sub(self.position)
    }

    /// `F()` — read 1 raw byte, advance by 1.
    pub fn read_byte(&mut self) -> Option<u8> {
        let byte = self.bytecode.get(self.position).copied()?;
        self.position += 1;
        Some(byte)
    }

    /// `g()` — read 2 bytes big-endian as u16, advance by 2.
    pub fn read_u16_be(&mut self) -> Option<u16> {
        let high = self.bytecode.get(self.position).copied()? as u16;
        let low = self.bytecode.get(self.position + 1).copied()? as u16;
        self.position += 2;
        Some((high << 8) | low)
    }

    /// `i()` — read a typed value (variable length).
    pub fn read_typed_value(&mut self) -> Option<Value> {
        let tag = self.read_byte()?;

        // Bit 7 set → small unsigned integer (0..127)
        if tag & 0x80 != 0 {
            return Some(Value::number((tag & 0x7F) as f64));
        }

        match tag {
            // This bytecode uses different tags than the vm_module.js source.
            // These tag values were determined empirically from the actual bytecode.
            38 => Some(Value::bool(true)),
            107 => Some(Value::bool(false)),
            // Tag 98 is NULL in this bytecode version.
            // Treating as NULL (coerces to 0 in arithmetic) matches PLV3 semantics.
            98 => Some(Value::Null),

            // ASCII string: XOR-decoded with incrementing key starting at 27
            63 => {
                let mut result = String::new();
                let mut key: u8 = 27;
                loop {
                    let byte = self.read_byte()?;
                    let decoded = byte ^ key;
                    key = key.wrapping_add(1);
                    if decoded == 0 {
                        break;
                    }
                    result.push(decoded as char);
                }
                Some(Value::string(result))
            }

            // UTF-8 string: XOR-decoded with incrementing key starting at 61
            47 => {
                let mut bytes_decoded = Vec::new();
                let mut key: u8 = 61;
                loop {
                    let byte = self.read_byte()?;
                    let decoded = byte ^ key;
                    key = key.wrapping_add(1);
                    if decoded == 0 {
                        break;
                    }
                    bytes_decoded.push(decoded);
                }
                Some(Value::string(String::from_utf8_lossy(&bytes_decoded).into_owned()))
            }

            // float64: 8 bytes IEEE 754 big-endian
            61 => {
                let mut bytes = [0u8; 8];
                for slot in &mut bytes {
                    *slot = self.read_byte()?;
                }
                Some(Value::number(f64::from_be_bytes(bytes)))
            }

            // int32: 4 bytes big-endian
            122 => {
                let mut bytes = [0u8; 4];
                for slot in &mut bytes {
                    *slot = self.read_byte()?;
                }
                Some(Value::number(i32::from_be_bytes(bytes) as f64))
            }

            // int24: 3 bytes sign-extended
            68 => {
                let b0 = self.read_byte()? as u32;
                let b1 = self.read_byte()? as u32;
                let b2 = self.read_byte()? as u32;
                let raw = (b0 << 16) | (b1 << 8) | b2;
                let signed = ((raw << 8) as i32) >> 8;
                Some(Value::number(signed as f64))
            }

            // int16: 2 bytes sign-extended
            123 => {
                let high = self.read_byte()? as u16;
                let low = self.read_byte()? as u16;
                let raw = (high << 8) | low;
                Some(Value::number((raw as i16) as f64))
            }

            // int8: 1 byte sign-extended
            53 => {
                let byte = self.read_byte()?;
                Some(Value::number((byte as i8) as f64))
            }

            // Unknown tag - return as number (fallback)
            _ => Some(Value::number(tag as f64)),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn read_small_integer() {
        let mut reader = Plv3Reader::new(&[0x85]); // 0x80 | 5 = 133
        let value = reader.read_typed_value().unwrap();
        assert_eq!(value, Value::number(5.0));
    }

    #[test]
    fn read_boolean_true() {
        let mut reader = Plv3Reader::new(&[38]);
        assert_eq!(reader.read_typed_value().unwrap(), Value::bool(true));
    }

    #[test]
    fn read_boolean_false() {
        let mut reader = Plv3Reader::new(&[107]);
        assert_eq!(reader.read_typed_value().unwrap(), Value::bool(false));
    }

    #[test]
    fn read_ascii_string() {
        // Tag 63, XOR key starting at 27
        let h_encoded = b'H' ^ 27u8;
        let i_encoded = b'i' ^ 28u8;
        let terminator = 0u8 ^ 29u8;
        let bytecode = [63, h_encoded, i_encoded, terminator];
        let mut reader = Plv3Reader::new(&bytecode);
        let value = reader.read_typed_value().unwrap();
        assert_eq!(value, Value::string("Hi"));
    }

    #[test]
    fn read_u16_be() {
        let mut reader = Plv3Reader::new(&[0x01, 0x00]);
        assert_eq!(reader.read_u16_be(), Some(256));
    }

    #[test]
    fn read_int16() {
        // Tag 123 for int16, -1 as int16: 0xFF 0xFF
        let mut reader = Plv3Reader::new(&[123, 0xFF, 0xFF]);
        let value = reader.read_typed_value().unwrap();
        assert_eq!(value, Value::number(-1.0));
    }

    #[test]
    fn read_float64() {
        let bytes_42_5: [u8; 8] = 42.5f64.to_be_bytes();
        let mut data = vec![61]; // tag for f64
        data.extend_from_slice(&bytes_42_5);
        let mut reader = Plv3Reader::new(&data);
        let value = reader.read_typed_value().unwrap();
        assert_eq!(value, Value::number(42.5));
    }
}

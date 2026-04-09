//! Base64 encoding/decoding and URI encoding.
//!
//! - `btoa(string)` — binary string to base64
//! - `atob(base64)` — base64 to binary string
//! - `encodeURIComponent(string)` — percent-encode
//! - `decodeURIComponent(string)` — percent-decode

// ============================================================================
// Imports
// ============================================================================

use vm_engine_core::exec::heap::Heap;
use vm_engine_core::value::{ObjectId, Value};

// ============================================================================
// Install
// ============================================================================

/// Install encoding functions on the global object.
pub fn install_encoding(heap: &mut Heap, global: ObjectId) {
    let btoa_fn = heap.alloc_native(|args, _heap| {
        let input = match args.first() {
            Some(Value::String(s)) => s.clone(),
            Some(other) => vm_engine_core::value::coerce::to_string(other),
            None => return Value::Undefined,
        };
        Value::string(base64_encode(input.as_bytes()))
    });
    heap.set_property(global, "btoa", Value::Object(btoa_fn));

    let atob_fn = heap.alloc_native(|args, _heap| {
        let Some(Value::String(input)) = args.first() else {
            return Value::Undefined;
        };
        match base64_decode(input) {
            Some(bytes) => Value::string(String::from_utf8_lossy(&bytes).into_owned()),
            None => Value::Undefined,
        }
    });
    heap.set_property(global, "atob", Value::Object(atob_fn));

    let encode_uri = heap.alloc_native(|args, _heap| {
        let input = args.first().map(vm_engine_core::value::coerce::to_string).unwrap_or_default();
        Value::string(percent_encode(&input))
    });
    heap.set_property(global, "encodeURIComponent", Value::Object(encode_uri));

    let decode_uri = heap.alloc_native(|args, _heap| {
        let input = args.first().map(vm_engine_core::value::coerce::to_string).unwrap_or_default();
        Value::string(percent_decode(&input))
    });
    heap.set_property(global, "decodeURIComponent", Value::Object(decode_uri));
}

// ============================================================================
// Base64
// ============================================================================

const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(input: &[u8]) -> String {
    let mut output = String::new();
    let mut index = 0;
    while index < input.len() {
        let byte0 = input[index];
        let byte1 = input.get(index + 1).copied().unwrap_or(0);
        let byte2 = input.get(index + 2).copied().unwrap_or(0);

        let triple = ((byte0 as u32) << 16) | ((byte1 as u32) << 8) | (byte2 as u32);
        output.push(BASE64_TABLE[((triple >> 18) & 63) as usize] as char);
        output.push(BASE64_TABLE[((triple >> 12) & 63) as usize] as char);
        output.push(if index + 1 < input.len() { BASE64_TABLE[((triple >> 6) & 63) as usize] as char } else { '=' });
        output.push(if index + 2 < input.len() { BASE64_TABLE[(triple & 63) as usize] as char } else { '=' });
        index += 3;
    }
    output
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn char_value(character: u8) -> Option<u8> {
        match character {
            b'A'..=b'Z' => Some(character - b'A'),
            b'a'..=b'z' => Some(character - b'a' + 26),
            b'0'..=b'9' => Some(character - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }

    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let c0 = bytes[index];
        let c1 = bytes[index + 1];
        let c2 = bytes[index + 2];
        let c3 = bytes[index + 3];

        let v0 = char_value(c0)?;
        let v1 = char_value(c1)?;
        let v2 = if c2 == b'=' { 0 } else { char_value(c2)? };
        let v3 = if c3 == b'=' { 0 } else { char_value(c3)? };

        let triple = ((v0 as u32) << 18) | ((v1 as u32) << 12) | ((v2 as u32) << 6) | (v3 as u32);
        output.push(((triple >> 16) & 0xFF) as u8);
        if c2 != b'=' { output.push(((triple >> 8) & 0xFF) as u8); }
        if c3 != b'=' { output.push((triple & 0xFF) as u8); }
        index += 4;
    }
    Some(output)
}

// ============================================================================
// Percent encoding
// ============================================================================

fn percent_encode(input: &str) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'!' | b'\'' | b'(' | b')' | b'*') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn percent_decode(input: &str) -> String {
    let mut output = Vec::new();
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&input[index + 1..index + 3], 16)
        {
            output.push(byte);
            index += 3;
            continue;
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btoa_roundtrip() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_encoding(&mut heap, global);

        let btoa_id = heap.get_property(global, "btoa").as_object().unwrap();
        let encoded = heap.call(btoa_id, &[Value::string("Hello, World!")]).unwrap();
        assert_eq!(encoded, Value::string("SGVsbG8sIFdvcmxkIQ=="));

        let atob_id = heap.get_property(global, "atob").as_object().unwrap();
        let decoded = heap.call(atob_id, &[encoded]).unwrap();
        assert_eq!(decoded, Value::string("Hello, World!"));
    }

    #[test]
    fn base64_empty() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_decode(""), Some(vec![]));
    }

    #[test]
    fn percent_encode_special_chars() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("a=b&c=d"), "a%3Db%26c%3Dd");
    }

    #[test]
    fn percent_decode_roundtrip() {
        let encoded = percent_encode("hello world/path?q=1");
        let decoded = percent_decode(&encoded);
        assert_eq!(decoded, "hello world/path?q=1");
    }
}

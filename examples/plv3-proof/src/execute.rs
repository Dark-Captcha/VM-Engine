//! Execute PLV3 IR with zeroed S-boxes to extract obfuscated key names.
//!
//! Zero-S-box trick: when all S-boxes are [0,0,...,0], the cipher becomes
//! identity (output = plaintext). The btoa output IS the JSON plaintext,
//! and the JSON property names ARE the obfuscated keys.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use vm_engine_core::exec::heap::Heap;
use vm_engine_core::exec::hooks::Hook;
use vm_engine_core::exec::Interpreter;
use vm_engine_core::ir::Module;
use vm_engine_core::value::Value;

/// Extracted PLV3 key names.
#[derive(Debug, Clone)]
pub struct ExtractedKeys {
    /// The obfuscated key names found in JSON output.
    pub keys: Vec<String>,
    /// Raw btoa input captured.
    pub btoa_input: Option<String>,
    /// Execution stats.
    pub instructions_executed: u64,
    /// Whether execution completed or hit a limit.
    pub completed: bool,
}

/// Hook that intercepts btoa and other host calls.
struct KeyExtractionHook {
    btoa_captures: Arc<Mutex<Vec<String>>>,
    json_stringify_captures: Arc<Mutex<Vec<String>>>,
}

impl Hook for KeyExtractionHook {
    fn on_call(&mut self, name: &str, args: &[Value], heap: &mut Heap) -> Option<Value> {
        match name {
            "btoa" => {
                let input = args.first().map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => vm_engine_core::value::coerce::to_string(other),
                }).unwrap_or_default();

                self.btoa_captures.lock().unwrap().push(input.clone());

                // Actually encode it (so the VM gets a valid result)
                let encoded = vm_engine_web::encoding::base64_encode_raw(input.as_bytes());
                Some(Value::string(encoded))
            }
            "stringify" | "JSON.stringify" => {
                // Capture the stringified JSON
                if let Some(Value::String(s)) = args.first() {
                    self.json_stringify_captures.lock().unwrap().push(s.clone());
                }
                None // let the normal handler process it
            }
            "fromCharCode" | "String.fromCharCode" => {
                // Build string from char codes
                let result: String = args.iter()
                    .map(|v| vm_engine_core::value::coerce::to_number(v) as u32)
                    .filter_map(char::from_u32)
                    .collect();
                Some(Value::string(result))
            }
            _ => None,
        }
    }
}

/// Execute the PLV3 module with zeroed S-boxes and extract key names.
pub fn extract_keys(module: &Module) -> ExtractedKeys {
    let btoa_captures = Arc::new(Mutex::new(Vec::new()));
    let json_captures = Arc::new(Mutex::new(Vec::new()));

    let hook = KeyExtractionHook {
        btoa_captures: Arc::clone(&btoa_captures),
        json_stringify_captures: Arc::clone(&json_captures),
    };

    let mut interp = match Interpreter::with_hook(module, hook) {
        Ok(interp) => interp,
        Err(err) => {
            eprintln!("[key-extract] interpreter creation failed: {err}");
            return ExtractedKeys { keys: vec![], btoa_input: None, instructions_executed: 0, completed: false };
        }
    };

    // Set entry to "main" function
    if let Err(err) = interp.set_entry("main") {
        eprintln!("[key-extract] set_entry failed: {err}");
        return ExtractedKeys { keys: vec![], btoa_input: None, instructions_executed: 0, completed: false };
    }

    // Set up global object with web environment
    let global = interp.state.heap.alloc();
    interp.state.global_object = Some(global);
    vm_engine_web::install_all(&mut interp.state.heap, global, &vm_engine_web::WebConfig::default());

    // Limit execution to prevent infinite loops
    interp.set_max_instructions(5_000_000);

    // Run
    let completed = match interp.run() {
        Ok(()) => true,
        Err(err) => {
            // Print info about where execution stopped
            let cursor = interp.state.cursor;
            if let Some(func) = module.function_by_id(cursor.function) {
                if let Some(block) = func.block(cursor.block) {
                    eprintln!("[key-extract] stopped at block '{}' ({}), {} instructions in block",
                        block.label, block.id, block.body.len());
                    eprintln!("[key-extract] terminator: {}", block.terminator);
                }
            }
            eprintln!("[key-extract] execution stopped: {err}");
            false
        }
    };

    let instructions_executed = interp.state.instruction_count;

    // Extract keys from captured btoa inputs
    let btoa_inputs = btoa_captures.lock().unwrap();
    let json_inputs = json_captures.lock().unwrap();

    let mut keys = Vec::new();
    let mut btoa_input = None;

    // Try btoa captures first — with zeroed S-boxes, btoa input = JSON plaintext
    for captured in btoa_inputs.iter() {
        btoa_input = Some(captured.clone());
        if let Some(extracted) = extract_json_keys(captured) {
            keys = extracted;
            break;
        }
    }

    // Fallback: try JSON.stringify captures
    if keys.is_empty() {
        for captured in json_inputs.iter() {
            if let Some(extracted) = extract_json_keys(captured) {
                keys = extracted;
                break;
            }
        }
    }

    ExtractedKeys {
        keys,
        btoa_input,
        instructions_executed,
        completed,
    }
}

/// Extract property names from a JSON string.
fn extract_json_keys(json_text: &str) -> Option<Vec<String>> {
    // Simple regex-like extraction: find all "key": patterns
    let mut keys = Vec::new();
    let mut chars = json_text.chars().peekable();

    while let Some(&ch) = chars.peek() {
        if ch == '"' {
            chars.next(); // consume opening "
            let mut key = String::new();
            while let Some(&c) = chars.peek() {
                if c == '"' { chars.next(); break; }
                if c == '\\' { chars.next(); chars.next(); continue; }
                key.push(c);
                chars.next();
            }
            // Check if followed by ':'
            // Skip whitespace
            while chars.peek().is_some_and(|c| c.is_whitespace()) { chars.next(); }
            if chars.peek() == Some(&':') {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        } else {
            chars.next();
        }
    }

    if keys.is_empty() { None } else { Some(keys) }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_keys_works() {
        let json = r#"{"abc123":1700000000,"xyz789":"/test","foo":true}"#;
        let keys = extract_json_keys(json).unwrap();
        assert_eq!(keys, vec!["abc123", "xyz789", "foo"]);
    }

    #[test]
    fn extract_json_keys_empty_on_non_json() {
        assert!(extract_json_keys("not json").is_none());
    }
}

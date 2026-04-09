//! JSON.stringify and JSON.parse.

// ============================================================================
// Imports
// ============================================================================

use vm_engine_core::exec::heap::Heap;
use vm_engine_core::value::{ObjectId, Value};

// ============================================================================
// Install
// ============================================================================

/// Install the `JSON` object with `stringify` and `parse` on the global.
pub fn install_json(heap: &mut Heap, global: ObjectId) {
    let json_obj = heap.alloc();

    let stringify_fn = heap.alloc_native(json_stringify);
    heap.set_property(json_obj, "stringify", Value::Object(stringify_fn));

    let parse_fn = heap.alloc_native(json_parse);
    heap.set_property(json_obj, "parse", Value::Object(parse_fn));

    heap.set_property(global, "JSON", Value::Object(json_obj));
}

// ============================================================================
// JSON.stringify
// ============================================================================

fn json_stringify(args: &[Value], heap: &mut Heap) -> Value {
    let value = args.first().unwrap_or(&Value::Undefined);
    Value::string(value_to_json(value, heap, 0))
}

fn value_to_json(value: &Value, heap: &Heap, depth: usize) -> String {
    // Guard against circular references / excessive depth
    if depth > 32 {
        return "null".into();
    }

    match value {
        Value::Number(number) => {
            if number.is_nan() || number.is_infinite() {
                "null".into()
            } else if number.fract() == 0.0 && number.abs() < 1e20 {
                format!("{}", *number as i64)
            } else {
                format!("{number}")
            }
        }
        Value::String(string) => format!("\"{}\"", escape_json_string(string)),
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        Value::Null => "null".into(),
        Value::Undefined | Value::Closure(_) => "undefined".into(),
        Value::Object(object_id) => {
            if let Some(object) = heap.get(*object_id) {
                let mut pairs: Vec<String> = Vec::new();
                // Sort keys for deterministic output
                let mut keys: Vec<&String> = object.properties.keys().collect();
                keys.sort();
                for key in keys {
                    let property_value = &object.properties[key];
                    // Skip undefined and function values (per JSON.stringify spec)
                    if matches!(property_value, Value::Undefined | Value::Closure(_)) {
                        continue;
                    }
                    pairs.push(format!(
                        "\"{}\":{}",
                        escape_json_string(key),
                        value_to_json(property_value, heap, depth + 1),
                    ));
                }
                format!("{{{}}}", pairs.join(","))
            } else {
                "null".into()
            }
        }
        Value::Array(elements) => {
            let items: Vec<String> = elements.iter()
                .map(|element| value_to_json(element, heap, depth + 1))
                .collect();
            format!("[{}]", items.join(","))
        }
        Value::Bytes(bytes) => {
            // Serialize as a JSON array of numbers
            let items: Vec<String> = bytes.iter().map(|byte| byte.to_string()).collect();
            format!("[{}]", items.join(","))
        }
    }
}

fn escape_json_string(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                output.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => output.push(c),
        }
    }
    output
}

// ============================================================================
// JSON.parse
// ============================================================================

fn json_parse(args: &[Value], _heap: &mut Heap) -> Value {
    let Some(Value::String(input)) = args.first() else {
        return Value::Undefined;
    };
    parse_json_value(input.trim())
}

/// Minimal JSON parser for primitive values and simple structures.
fn parse_json_value(input: &str) -> Value {
    let trimmed = input.trim();
    match trimmed {
        "null" => Value::Null,
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "undefined" => Value::Undefined,
        _ if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 => {
            Value::string(&trimmed[1..trimmed.len() - 1])
        }
        _ => {
            // Try parsing as number
            if let Ok(number) = trimmed.parse::<f64>() {
                Value::number(number)
            } else {
                Value::Undefined
            }
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
    fn stringify_primitives() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_json(&mut heap, global);

        let json = heap.get_property(global, "JSON").as_object().unwrap();
        let stringify = heap.get_property(json, "stringify").as_object().unwrap();

        assert_eq!(heap.call(stringify, &[Value::number(42.0)]).unwrap(), Value::string("42"));
        assert_eq!(heap.call(stringify, &[Value::string("hello")]).unwrap(), Value::string("\"hello\""));
        assert_eq!(heap.call(stringify, &[Value::bool(true)]).unwrap(), Value::string("true"));
        assert_eq!(heap.call(stringify, &[Value::Null]).unwrap(), Value::string("null"));
    }

    #[test]
    fn stringify_object() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_json(&mut heap, global);

        let obj = heap.alloc();
        heap.set_property(obj, "name", Value::string("test"));
        heap.set_property(obj, "value", Value::number(42.0));

        let json = heap.get_property(global, "JSON").as_object().unwrap();
        let stringify = heap.get_property(json, "stringify").as_object().unwrap();
        let result = heap.call(stringify, &[Value::Object(obj)]).unwrap();
        let text = result.as_str().unwrap();

        assert!(text.contains("\"name\":\"test\""), "got: {text}");
        assert!(text.contains("\"value\":42"), "got: {text}");
    }

    #[test]
    fn stringify_array() {
        let mut heap = Heap::new();
        let array = Value::Array(vec![Value::number(1.0), Value::number(2.0), Value::number(3.0)]);

        assert_eq!(value_to_json(&array, &heap, 0), "[1,2,3]");
    }

    #[test]
    fn stringify_nan_becomes_null() {
        let heap = Heap::new();
        assert_eq!(value_to_json(&Value::number(f64::NAN), &heap, 0), "null");
    }

    #[test]
    fn escape_special_chars() {
        assert_eq!(escape_json_string("hello\nworld"), "hello\\nworld");
        assert_eq!(escape_json_string("say \"hi\""), "say \\\"hi\\\"");
    }

    #[test]
    fn parse_primitives() {
        assert_eq!(parse_json_value("42"), Value::number(42.0));
        assert_eq!(parse_json_value("true"), Value::Bool(true));
        assert_eq!(parse_json_value("null"), Value::Null);
        assert_eq!(parse_json_value("\"hello\""), Value::string("hello"));
    }
}

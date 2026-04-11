//! JSON.stringify and JSON.parse.

// ============================================================================
// Imports
// ============================================================================

use crate::exec::heap::Heap;
use crate::value::{ObjectId, Value};

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
        // Per spec: JSON.stringify(undefined) returns undefined (not a string).
        // But inside objects/arrays, undefined values are omitted (handled above).
        // At top level, we return "null" to avoid breaking callers expecting a string.
        Value::Undefined | Value::Closure(_) => {
            if depth == 0 { return "null".into(); }
            "null".into()
        }
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

fn json_parse(args: &[Value], heap: &mut Heap) -> Value {
    let Some(Value::String(input)) = args.first() else {
        return Value::Undefined;
    };
    let mut parser = JsonParser::new(input, heap);
    parser.parse_value().unwrap_or(Value::Undefined)
}

/// Recursive JSON parser with object/array support.
struct JsonParser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    heap: &'a mut Heap,
    depth: usize,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a str, heap: &'a mut Heap) -> Self {
        Self {
            chars: input.chars().peekable(),
            heap,
            depth: 0,
        }
    }

    fn parse_value(&mut self) -> Option<Value> {
        if self.depth > 32 {
            return None; // Guard against deeply nested JSON
        }
        self.skip_whitespace();
        match self.chars.peek()? {
            '{' => self.parse_object(),
            '[' => self.parse_array(),
            '"' => self.parse_string(),
            't' | 'f' => self.parse_bool(),
            'n' => self.parse_null(),
            c if c.is_ascii_digit() || *c == '-' => self.parse_number(),
            _ => None,
        }
    }

    fn parse_object(&mut self) -> Option<Value> {
        self.depth += 1;
        self.chars.next(); // consume '{'
        self.skip_whitespace();

        let obj = self.heap.alloc();

        if self.chars.peek() == Some(&'}') {
            self.chars.next();
            self.depth -= 1;
            return Some(Value::Object(obj));
        }

        loop {
            self.skip_whitespace();
            // Parse key (must be string)
            if self.chars.peek() != Some(&'"') {
                self.depth -= 1;
                return None;
            }
            let key = self.parse_string()?.as_str().map(|s| s.to_string())?;

            self.skip_whitespace();
            if self.chars.next() != Some(':') {
                self.depth -= 1;
                return None;
            }

            // Parse value
            let value = self.parse_value()?;
            self.heap.set_property(obj, &key, value);

            self.skip_whitespace();
            match self.chars.peek() {
                Some(',') => { self.chars.next(); }
                Some('}') => { self.chars.next(); break; }
                _ => { self.depth -= 1; return None; }
            }
        }
        self.depth -= 1;
        Some(Value::Object(obj))
    }

    fn parse_array(&mut self) -> Option<Value> {
        self.depth += 1;
        self.chars.next(); // consume '['
        self.skip_whitespace();

        let mut elements = Vec::new();

        if self.chars.peek() == Some(&']') {
            self.chars.next();
            self.depth -= 1;
            return Some(Value::Array(elements));
        }

        loop {
            let value = self.parse_value()?;
            elements.push(value);

            self.skip_whitespace();
            match self.chars.peek() {
                Some(',') => { self.chars.next(); }
                Some(']') => { self.chars.next(); break; }
                _ => { self.depth -= 1; return None; }
            }
        }
        self.depth -= 1;
        Some(Value::Array(elements))
    }

    fn parse_string(&mut self) -> Option<Value> {
        self.chars.next(); // consume '"'
        let mut s = String::new();
        loop {
            match self.chars.next()? {
                '"' => break,
                '\\' => {
                    match self.chars.next()? {
                        '"' => s.push('"'),
                        '\\' => s.push('\\'),
                        '/' => s.push('/'),
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        'b' => s.push('\x08'),
                        'f' => s.push('\x0C'),
                        'u' => {
                            // Parse \uXXXX
                            let mut hex = String::with_capacity(4);
                            for _ in 0..4 {
                                hex.push(self.chars.next()?);
                            }
                            let code = u32::from_str_radix(&hex, 16).ok()?;
                            if let Some(c) = char::from_u32(code) {
                                s.push(c);
                            }
                        }
                        _ => return None,
                    }
                }
                c => s.push(c),
            }
        }
        Some(Value::string(s))
    }

    fn parse_number(&mut self) -> Option<Value> {
        let mut num_str = String::new();
        if self.chars.peek() == Some(&'-') {
            num_str.push(self.chars.next()?);
        }
        while let Some(&c) = self.chars.peek() {
            if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-' {
                // Only allow +/- after e/E
                if (c == '+' || c == '-') && !num_str.ends_with('e') && !num_str.ends_with('E') {
                    break;
                }
                num_str.push(self.chars.next()?);
            } else {
                break;
            }
        }
        num_str.parse::<f64>().ok().map(Value::number)
    }

    fn parse_bool(&mut self) -> Option<Value> {
        let word: String = self.chars.by_ref().take(4).collect();
        if word == "true" {
            Some(Value::bool(true))
        } else if &word[..] == "fals" {
            if self.chars.next() == Some('e') {
                Some(Value::bool(false))
            } else {
                None
            }
        } else {
            None
        }
    }

    fn parse_null(&mut self) -> Option<Value> {
        let word: String = self.chars.by_ref().take(4).collect();
        if word == "null" {
            Some(Value::Null)
        } else {
            None
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(&c) = self.chars.peek() {
            if c.is_whitespace() {
                self.chars.next();
            } else {
                break;
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
        let heap = Heap::new();
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
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_json(&mut heap, global);
        let json = heap.get_property(global, "JSON").as_object().unwrap();
        let parse = heap.get_property(json, "parse").as_object().unwrap();

        assert_eq!(heap.call(parse, &[Value::string("42")]).unwrap(), Value::number(42.0));
        assert_eq!(heap.call(parse, &[Value::string("true")]).unwrap(), Value::bool(true));
        assert_eq!(heap.call(parse, &[Value::string("null")]).unwrap(), Value::Null);
        assert_eq!(heap.call(parse, &[Value::string("\"hello\"")]).unwrap(), Value::string("hello"));
    }

    #[test]
    fn parse_object() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_json(&mut heap, global);
        let json = heap.get_property(global, "JSON").as_object().unwrap();
        let parse = heap.get_property(json, "parse").as_object().unwrap();

        let result = heap.call(parse, &[Value::string(r#"{"name":"test","value":42}"#)]).unwrap();
        if let Value::Object(oid) = result {
            assert_eq!(heap.get_property(oid, "name"), Value::string("test"));
            assert_eq!(heap.get_property(oid, "value"), Value::number(42.0));
        } else {
            panic!("expected object, got: {result:?}");
        }
    }

    #[test]
    fn parse_array() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_json(&mut heap, global);
        let json = heap.get_property(global, "JSON").as_object().unwrap();
        let parse = heap.get_property(json, "parse").as_object().unwrap();

        let result = heap.call(parse, &[Value::string("[1,2,3]")]).unwrap();
        if let Value::Array(arr) = result {
            assert_eq!(arr.len(), 3);
            assert_eq!(arr[0], Value::number(1.0));
            assert_eq!(arr[1], Value::number(2.0));
            assert_eq!(arr[2], Value::number(3.0));
        } else {
            panic!("expected array, got: {result:?}");
        }
    }

    #[test]
    fn parse_escape_sequences() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_json(&mut heap, global);
        let json = heap.get_property(global, "JSON").as_object().unwrap();
        let parse = heap.get_property(json, "parse").as_object().unwrap();

        let result = heap.call(parse, &[Value::string(r#""hello\nworld""#)]).unwrap();
        assert_eq!(result, Value::string("hello\nworld"));

        let result2 = heap.call(parse, &[Value::string(r#""say \"hi\"""#)]).unwrap();
        assert_eq!(result2, Value::string("say \"hi\""));
    }

    #[test]
    fn stringify_nested_object() {
        let mut heap = Heap::new();
        let inner = heap.alloc();
        heap.set_property(inner, "x", Value::number(1.0));
        let outer = heap.alloc();
        heap.set_property(outer, "inner", Value::Object(inner));
        heap.set_property(outer, "y", Value::number(2.0));

        let result = value_to_json(&Value::Object(outer), &heap, 0);
        assert!(result.contains("\"inner\":{\"x\":1}"), "got: {result}");
        assert!(result.contains("\"y\":2"), "got: {result}");
    }

    #[test]
    fn stringify_skips_undefined_in_object() {
        let mut heap = Heap::new();
        let obj = heap.alloc();
        heap.set_property(obj, "present", Value::number(1.0));
        heap.set_property(obj, "missing", Value::Undefined);

        let result = value_to_json(&Value::Object(obj), &heap, 0);
        assert!(result.contains("\"present\":1"), "got: {result}");
        assert!(!result.contains("missing"), "undefined should be omitted: {result}");
    }
}

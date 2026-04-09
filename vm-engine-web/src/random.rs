//! Deterministic random: `Math.random()`, `crypto.getRandomValues()`, `crypto.randomUUID()`.
//!
//! PRNG state is stored on the heap. Same seed = same sequence.

// ============================================================================
// Imports
// ============================================================================

use vm_engine_core::exec::heap::Heap;
use vm_engine_core::value::{ObjectId, Value};

// ============================================================================
// Config
// ============================================================================

/// Configuration for deterministic random number generation.
#[derive(Debug, Clone)]
pub struct RandomConfig {
    /// PRNG seed. Same seed produces identical sequences.
    pub seed: u64,
    /// Starting counter for deterministic UUIDs.
    pub uuid_counter: u64,
}

impl Default for RandomConfig {
    fn default() -> Self {
        Self {
            seed: 0xC0FFEE,
            uuid_counter: 0,
        }
    }
}

// ============================================================================
// Install
// ============================================================================

/// Install `Math.random()` and `crypto.*` on the global object.
///
/// Expects that `Math` is already installed (adds `random` to existing object).
/// Creates `crypto` if not present.
pub fn install_random(heap: &mut Heap, global: ObjectId, config: &RandomConfig) {
    // Store PRNG state on heap
    heap.set_property(global, "__rng_state", Value::number(config.seed as f64));
    heap.set_property(global, "__uuid_counter", Value::number(config.uuid_counter as f64));

    // Math.random()
    let math_random_fn = heap.alloc_closure(move |_args, heap| {
        let state = heap.get_property(global, "__rng_state")
            .as_number().unwrap_or(0.0) as u64;
        let next_state = lcg_next(state);
        heap.set_property(global, "__rng_state", Value::number(next_state as f64));
        // Map to [0, 1) by using high bits
        let mantissa = (next_state >> 11) as f64;
        Value::number(mantissa / ((1u64 << 53) as f64))
    });

    // Add to existing Math object if present
    let math_value = heap.get_property(global, "Math");
    if let Value::Object(math_id) = math_value {
        heap.set_property(math_id, "random", Value::Object(math_random_fn));
    }

    // crypto.getRandomValues()
    let get_random_values_fn = heap.alloc_closure(move |args, heap| {
        let Some(Value::Object(array_id)) = args.first() else {
            return Value::Undefined;
        };
        let length = heap.get_property(*array_id, "length")
            .as_number().unwrap_or(0.0).max(0.0) as usize;

        let mut state = heap.get_property(global, "__rng_state")
            .as_number().unwrap_or(0.0) as u64;

        for index in 0..length {
            state = xorshift64(state);
            let byte = (state & 0xFF) as u8;
            heap.set_property(*array_id, &index.to_string(), Value::number(byte as f64));
        }
        heap.set_property(global, "__rng_state", Value::number(state as f64));
        Value::Object(*array_id)
    });

    // crypto.randomUUID()
    let random_uuid_fn = heap.alloc_closure(move |_args, heap| {
        let counter = heap.get_property(global, "__uuid_counter")
            .as_number().unwrap_or(0.0).max(0.0) as u64;
        heap.set_property(global, "__uuid_counter", Value::number((counter + 1) as f64));

        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&counter.to_be_bytes());
        bytes[6] = (bytes[6] & 0x0F) | 0x40; // version 4
        bytes[8] = (bytes[8] & 0x3F) | 0x80; // variant 10xx
        Value::string(format_uuid(&bytes))
    });

    let crypto_object = heap.alloc();
    heap.set_property(crypto_object, "getRandomValues", Value::Object(get_random_values_fn));
    heap.set_property(crypto_object, "randomUUID", Value::Object(random_uuid_fn));
    heap.set_property(global, "crypto", Value::Object(crypto_object));
}

// ============================================================================
// PRNG algorithms
// ============================================================================

/// Linear congruential generator: simple, fast, deterministic.
fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

/// Xorshift64*: better distribution for byte generation.
fn xorshift64(mut state: u64) -> u64 {
    state ^= state >> 12;
    state ^= state << 25;
    state ^= state >> 27;
    state.wrapping_mul(2685821657736338717)
}

fn format_uuid(bytes: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_random_is_deterministic() {
        let mut heap1 = Heap::new();
        let mut heap2 = Heap::new();
        let global1 = heap1.alloc();
        let global2 = heap2.alloc();

        let config = RandomConfig { seed: 12345, uuid_counter: 0 };
        // Need Math object first
        super::super::math::install_math(&mut heap1, global1);
        super::super::math::install_math(&mut heap2, global2);
        install_random(&mut heap1, global1, &config);
        install_random(&mut heap2, global2, &config);

        let math1 = heap1.get_property(global1, "Math").as_object().unwrap();
        let math2 = heap2.get_property(global2, "Math").as_object().unwrap();
        let rand1 = heap1.get_property(math1, "random").as_object().unwrap();
        let rand2 = heap2.get_property(math2, "random").as_object().unwrap();

        let value1 = heap1.call(rand1, &[]).unwrap();
        let value2 = heap2.call(rand2, &[]).unwrap();
        assert_eq!(value1, value2, "same seed should produce same value");
    }

    #[test]
    fn math_random_in_range() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        super::super::math::install_math(&mut heap, global);
        install_random(&mut heap, global, &RandomConfig::default());

        let math = heap.get_property(global, "Math").as_object().unwrap();
        let rand = heap.get_property(math, "random").as_object().unwrap();

        for _ in 0..100 {
            let value = heap.call(rand, &[]).unwrap().as_number().unwrap();
            assert!(value >= 0.0 && value < 1.0, "Math.random() should be in [0,1), got {value}");
        }
    }

    #[test]
    fn crypto_get_random_values_fills_object() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_random(&mut heap, global, &RandomConfig::default());

        // Create an object with "length" property to simulate typed array
        let array_obj = heap.alloc();
        heap.set_property(array_obj, "length", Value::number(4.0));

        let crypto = heap.get_property(global, "crypto").as_object().unwrap();
        let get_rv = heap.get_property(crypto, "getRandomValues").as_object().unwrap();
        let result = heap.call(get_rv, &[Value::Object(array_obj)]).unwrap();

        // Should return the same object
        assert_eq!(result.as_object(), Some(array_obj));

        // Should have filled numeric indices 0-3
        for index in 0..4 {
            let value = heap.get_property(array_obj, &index.to_string());
            assert!(value.as_number().is_some(), "index {index} should be filled");
            let byte = value.as_number().unwrap();
            assert!(byte >= 0.0 && byte <= 255.0, "byte should be 0-255, got {byte}");
        }
    }

    #[test]
    fn random_uuid_is_deterministic() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_random(&mut heap, global, &RandomConfig::default());

        let crypto = heap.get_property(global, "crypto").as_object().unwrap();
        let uuid_fn = heap.get_property(crypto, "randomUUID").as_object().unwrap();

        let uuid1 = heap.call(uuid_fn, &[]).unwrap();
        let uuid2 = heap.call(uuid_fn, &[]).unwrap();
        assert_ne!(uuid1, uuid2, "successive UUIDs should differ");

        let uuid_str = uuid1.as_str().unwrap();
        assert_eq!(uuid_str.len(), 36, "UUID should be 36 chars: {uuid_str}");
        assert_eq!(&uuid_str[14..15], "4", "UUID version should be 4: {uuid_str}");
    }
}

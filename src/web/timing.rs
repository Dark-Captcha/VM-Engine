//! Deterministic timing: `Date.now()` and `performance.now()`.
//!
//! State is stored on the heap as hidden properties on the global object.
//! Each call advances the counter by a configurable tick.

// ============================================================================
// Imports
// ============================================================================

use crate::exec::heap::Heap;
use crate::value::{ObjectId, Value};

// ============================================================================
// Config
// ============================================================================

/// Configuration for deterministic timing.
#[derive(Debug, Clone)]
pub struct TimingConfig {
    /// Starting value for `Date.now()` in milliseconds.
    pub date_now_start_ms: f64,
    /// How much `Date.now()` advances per call.
    pub date_now_tick_ms: f64,
    /// Starting value for `performance.now()` in milliseconds.
    pub performance_now_start_ms: f64,
    /// How much `performance.now()` advances per call.
    pub performance_now_tick_ms: f64,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            date_now_start_ms: 1_700_000_000_000.0,
            date_now_tick_ms: 17.0,
            performance_now_start_ms: 1_000.0,
            performance_now_tick_ms: 1.0,
        }
    }
}

// ============================================================================
// Install
// ============================================================================

/// Install `Date.now()` and `performance.now()` on the global object.
pub fn install_timing(heap: &mut Heap, global: ObjectId, config: &TimingConfig) {
    // Store state as hidden properties
    heap.set_property(global, "__timing_date_ms", Value::number(config.date_now_start_ms));
    heap.set_property(global, "__timing_perf_ms", Value::number(config.performance_now_start_ms));

    let date_tick = config.date_now_tick_ms;
    let perf_tick = config.performance_now_tick_ms;

    // Date.now()
    let date_now_fn = heap.alloc_closure(move |_args, heap| {
        let current = heap.get_property(global, "__timing_date_ms")
            .as_number().unwrap_or(0.0);
        let next = current + date_tick;
        heap.set_property(global, "__timing_date_ms", Value::number(next));
        Value::number(next)
    });
    let date_object = heap.alloc();
    heap.set_property(date_object, "now", Value::Object(date_now_fn));
    heap.set_property(global, "Date", Value::Object(date_object));

    // performance.now()
    let perf_now_fn = heap.alloc_closure(move |_args, heap| {
        let current = heap.get_property(global, "__timing_perf_ms")
            .as_number().unwrap_or(0.0);
        let next = current + perf_tick;
        heap.set_property(global, "__timing_perf_ms", Value::number(next));
        Value::number(next)
    });
    let performance_object = heap.alloc();
    heap.set_property(performance_object, "now", Value::Object(perf_now_fn));
    heap.set_property(global, "performance", Value::Object(performance_object));
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_now_is_monotonic() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_timing(&mut heap, global, &TimingConfig::default());

        let date = heap.get_property(global, "Date").as_object().unwrap();
        let now = heap.get_property(date, "now").as_object().unwrap();

        let first = heap.call(now, &[]).unwrap().as_number().unwrap();
        let second = heap.call(now, &[]).unwrap().as_number().unwrap();
        assert!(second > first);
        assert!((second - first - 17.0).abs() < 0.001);
    }

    #[test]
    fn performance_now_is_monotonic() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_timing(&mut heap, global, &TimingConfig::default());

        let perf = heap.get_property(global, "performance").as_object().unwrap();
        let now = heap.get_property(perf, "now").as_object().unwrap();

        let first = heap.call(now, &[]).unwrap().as_number().unwrap();
        let second = heap.call(now, &[]).unwrap().as_number().unwrap();
        assert!(second > first);
    }

    #[test]
    fn custom_config() {
        let config = TimingConfig {
            date_now_start_ms: 5000.0,
            date_now_tick_ms: 100.0,
            ..Default::default()
        };
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_timing(&mut heap, global, &config);

        let date = heap.get_property(global, "Date").as_object().unwrap();
        let now = heap.get_property(date, "now").as_object().unwrap();

        let first = heap.call(now, &[]).unwrap().as_number().unwrap();
        assert_eq!(first, 5100.0); // 5000 + 100
    }
}

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

    // Date.now() — advances monotonically, returns value after advance.
    //
    // NOTE: This differs from real browser Date.now() semantics (which returns
    // the current wall clock without "advancing"), but PLV3-style anti-bot VMs
    // rely on this behavior: successive calls must return monotonically
    // increasing values, and often the first call is expected to be
    // start + tick (not exactly start). Reverting to spec-strict semantics
    // breaks PLV3 token generation.
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

    // performance.now() — advances monotonically (see Date.now() note)
    let perf_now_fn = heap.alloc_closure(move |_args, heap| {
        let current = heap.get_property(global, "__timing_perf_ms")
            .as_number().unwrap_or(0.0);
        let next = current + perf_tick;
        heap.set_property(global, "__timing_perf_ms", Value::number(next));
        Value::number(next)
    });
    let performance_object = heap.alloc();
    heap.set_property(performance_object, "now", Value::Object(perf_now_fn));

    // performance.timing (Navigation Timing API — deprecated but widely used)
    let timing = heap.alloc();
    let base_time = config.date_now_start_ms;
    heap.set_property(timing, "navigationStart", Value::number(base_time - 500.0));
    heap.set_property(timing, "fetchStart", Value::number(base_time - 400.0));
    heap.set_property(timing, "domainLookupStart", Value::number(base_time - 350.0));
    heap.set_property(timing, "domainLookupEnd", Value::number(base_time - 340.0));
    heap.set_property(timing, "connectStart", Value::number(base_time - 340.0));
    heap.set_property(timing, "connectEnd", Value::number(base_time - 300.0));
    heap.set_property(timing, "requestStart", Value::number(base_time - 300.0));
    heap.set_property(timing, "responseStart", Value::number(base_time - 200.0));
    heap.set_property(timing, "responseEnd", Value::number(base_time - 100.0));
    heap.set_property(timing, "domLoading", Value::number(base_time - 80.0));
    heap.set_property(timing, "domInteractive", Value::number(base_time - 50.0));
    heap.set_property(timing, "domContentLoadedEventStart", Value::number(base_time - 40.0));
    heap.set_property(timing, "domContentLoadedEventEnd", Value::number(base_time - 30.0));
    heap.set_property(timing, "domComplete", Value::number(base_time - 10.0));
    heap.set_property(timing, "loadEventStart", Value::number(base_time - 5.0));
    heap.set_property(timing, "loadEventEnd", Value::number(base_time));
    heap.set_property(performance_object, "timing", Value::Object(timing));

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
        assert_eq!(first, 5100.0); // 5000 + 100 (first call advances)
    }
}

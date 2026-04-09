//! Math object: 35 methods + 8 constants.
//!
//! All methods are pure (no state). `Math.random` is installed separately
//! by the `random` module to keep state management isolated.

// ============================================================================
// Imports
// ============================================================================

use vm_engine_core::exec::heap::Heap;
use vm_engine_core::value::{ObjectId, Value};
use vm_engine_core::value::coerce;

// ============================================================================
// Install
// ============================================================================

/// Install `Math` object with all pure methods and constants.
///
/// Does NOT install `Math.random` — that requires PRNG state and is
/// handled by [`super::random::install_random`].
pub fn install_math(heap: &mut Heap, global: ObjectId) {
    let math = heap.alloc();

    // ── Constants ────────────────────────────────────────────────────
    heap.set_property(math, "PI", Value::number(std::f64::consts::PI));
    heap.set_property(math, "E", Value::number(std::f64::consts::E));
    heap.set_property(math, "LN10", Value::number(std::f64::consts::LN_10));
    heap.set_property(math, "LN2", Value::number(std::f64::consts::LN_2));
    heap.set_property(math, "LOG10E", Value::number(std::f64::consts::LOG10_E));
    heap.set_property(math, "LOG2E", Value::number(std::f64::consts::LOG2_E));
    heap.set_property(math, "SQRT2", Value::number(std::f64::consts::SQRT_2));
    heap.set_property(math, "SQRT1_2", Value::number(std::f64::consts::FRAC_1_SQRT_2));

    // ── Single-arg methods ───────────────────────────────────────────
    install_math_unary(heap, math, "abs", f64::abs);
    install_math_unary(heap, math, "ceil", f64::ceil);
    install_math_unary(heap, math, "floor", f64::floor);
    install_math_unary(heap, math, "round", f64::round);
    install_math_unary(heap, math, "trunc", f64::trunc);
    install_math_unary(heap, math, "sign", f64::signum);
    install_math_unary(heap, math, "sqrt", f64::sqrt);
    install_math_unary(heap, math, "cbrt", f64::cbrt);
    install_math_unary(heap, math, "log", f64::ln);
    install_math_unary(heap, math, "log2", f64::log2);
    install_math_unary(heap, math, "log10", f64::log10);
    install_math_unary(heap, math, "exp", f64::exp);
    install_math_unary(heap, math, "sin", f64::sin);
    install_math_unary(heap, math, "cos", f64::cos);
    install_math_unary(heap, math, "tan", f64::tan);
    install_math_unary(heap, math, "asin", f64::asin);
    install_math_unary(heap, math, "acos", f64::acos);
    install_math_unary(heap, math, "atan", f64::atan);
    install_math_unary(heap, math, "sinh", f64::sinh);
    install_math_unary(heap, math, "cosh", f64::cosh);
    install_math_unary(heap, math, "tanh", f64::tanh);
    install_math_unary(heap, math, "asinh", f64::asinh);
    install_math_unary(heap, math, "acosh", f64::acosh);
    install_math_unary(heap, math, "atanh", f64::atanh);

    // ── Two-arg methods ──────────────────────────────────────────────
    install_math_binary(heap, math, "pow", f64::powf);
    install_math_binary(heap, math, "atan2", f64::atan2);

    // ── Variadic methods ─────────────────────────────────────────────
    let min_fn = heap.alloc_native(|args, _heap| {
        if args.is_empty() { return Value::number(f64::INFINITY); }
        let mut result = f64::INFINITY;
        for arg in args {
            let number = coerce::to_number(arg);
            if number.is_nan() { return Value::number(f64::NAN); }
            if number < result { result = number; }
        }
        Value::number(result)
    });
    heap.set_property(math, "min", Value::Object(min_fn));

    let max_fn = heap.alloc_native(|args, _heap| {
        if args.is_empty() { return Value::number(f64::NEG_INFINITY); }
        let mut result = f64::NEG_INFINITY;
        for arg in args {
            let number = coerce::to_number(arg);
            if number.is_nan() { return Value::number(f64::NAN); }
            if number > result { result = number; }
        }
        Value::number(result)
    });
    heap.set_property(math, "max", Value::Object(max_fn));

    let hypot_fn = heap.alloc_native(|args, _heap| {
        let sum_sq: f64 = args.iter()
            .map(|arg| { let number = coerce::to_number(arg); number * number })
            .sum();
        Value::number(sum_sq.sqrt())
    });
    heap.set_property(math, "hypot", Value::Object(hypot_fn));

    // ── Integer methods ──────────────────────────────────────────────
    let clz32_fn = heap.alloc_native(|args, _heap| {
        let value = args.first().map(coerce::to_uint32).unwrap_or(0);
        Value::number(value.leading_zeros() as f64)
    });
    heap.set_property(math, "clz32", Value::Object(clz32_fn));

    let imul_fn = heap.alloc_native(|args, _heap| {
        let left = args.first().map(coerce::to_int32).unwrap_or(0);
        let right = args.get(1).map(coerce::to_int32).unwrap_or(0);
        Value::number(left.wrapping_mul(right) as f64)
    });
    heap.set_property(math, "imul", Value::Object(imul_fn));

    let fround_fn = heap.alloc_native(|args, _heap| {
        let number = args.first().map(coerce::to_number).unwrap_or(0.0);
        Value::number((number as f32) as f64)
    });
    heap.set_property(math, "fround", Value::Object(fround_fn));

    heap.set_property(global, "Math", Value::Object(math));
}

// ============================================================================
// Helpers
// ============================================================================

fn install_math_unary(heap: &mut Heap, math_object: ObjectId, name: &str, operation: fn(f64) -> f64) {
    let function = heap.alloc_closure(move |args, _heap| {
        let input = args.first().map(coerce::to_number).unwrap_or(f64::NAN);
        Value::number(operation(input))
    });
    heap.set_property(math_object, name, Value::Object(function));
}

fn install_math_binary(heap: &mut Heap, math_object: ObjectId, name: &str, operation: fn(f64, f64) -> f64) {
    let function = heap.alloc_closure(move |args, _heap| {
        let left = args.first().map(coerce::to_number).unwrap_or(f64::NAN);
        let right = args.get(1).map(coerce::to_number).unwrap_or(f64::NAN);
        Value::number(operation(left, right))
    });
    heap.set_property(math_object, name, Value::Object(function));
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn call_math(heap: &mut Heap, global: ObjectId, method: &str, args: &[Value]) -> Value {
        let math_obj = heap.get_property(global, "Math").as_object().unwrap();
        let func = heap.get_property(math_obj, method).as_object().unwrap();
        heap.call(func, args).unwrap()
    }

    #[test]
    fn math_constants() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_math(&mut heap, global);

        let math_obj = heap.get_property(global, "Math").as_object().unwrap();
        let pi = heap.get_property(math_obj, "PI").as_number().unwrap();
        assert!((pi - std::f64::consts::PI).abs() < 1e-15);
    }

    #[test]
    fn math_floor_ceil_round() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_math(&mut heap, global);

        assert_eq!(call_math(&mut heap, global, "floor", &[Value::number(3.7)]), Value::number(3.0));
        assert_eq!(call_math(&mut heap, global, "ceil", &[Value::number(3.2)]), Value::number(4.0));
        assert_eq!(call_math(&mut heap, global, "round", &[Value::number(3.5)]), Value::number(4.0));
    }

    #[test]
    fn math_abs() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_math(&mut heap, global);

        assert_eq!(call_math(&mut heap, global, "abs", &[Value::number(-42.0)]), Value::number(42.0));
    }

    #[test]
    fn math_min_max() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_math(&mut heap, global);

        assert_eq!(
            call_math(&mut heap, global, "min", &[Value::number(3.0), Value::number(1.0), Value::number(2.0)]),
            Value::number(1.0),
        );
        assert_eq!(
            call_math(&mut heap, global, "max", &[Value::number(3.0), Value::number(1.0), Value::number(2.0)]),
            Value::number(3.0),
        );
    }

    #[test]
    fn math_clz32() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_math(&mut heap, global);

        assert_eq!(call_math(&mut heap, global, "clz32", &[Value::number(1.0)]), Value::number(31.0));
        assert_eq!(call_math(&mut heap, global, "clz32", &[Value::number(0.0)]), Value::number(32.0));
    }

    #[test]
    fn math_imul() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_math(&mut heap, global);

        assert_eq!(
            call_math(&mut heap, global, "imul", &[Value::number(0xFFFFFFFF_u32 as f64), Value::number(5.0)]),
            Value::number(-5.0), // wrapping multiplication
        );
    }

    #[test]
    fn math_pow() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_math(&mut heap, global);

        assert_eq!(call_math(&mut heap, global, "pow", &[Value::number(2.0), Value::number(10.0)]), Value::number(1024.0));
    }

    #[test]
    fn math_trig() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_math(&mut heap, global);

        let sin_0 = call_math(&mut heap, global, "sin", &[Value::number(0.0)]).as_number().unwrap();
        assert!((sin_0 - 0.0).abs() < 1e-15);

        let cos_0 = call_math(&mut heap, global, "cos", &[Value::number(0.0)]).as_number().unwrap();
        assert!((cos_0 - 1.0).abs() < 1e-15);
    }

    #[test]
    fn math_log_exp() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_math(&mut heap, global);

        let log_e = call_math(&mut heap, global, "log", &[Value::number(std::f64::consts::E)]).as_number().unwrap();
        assert!((log_e - 1.0).abs() < 1e-15);

        let exp_1 = call_math(&mut heap, global, "exp", &[Value::number(1.0)]).as_number().unwrap();
        assert!((exp_1 - std::f64::consts::E).abs() < 1e-15);
    }

    #[test]
    fn math_sqrt_cbrt() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_math(&mut heap, global);

        assert_eq!(call_math(&mut heap, global, "sqrt", &[Value::number(144.0)]), Value::number(12.0));
        let cbrt_27 = call_math(&mut heap, global, "cbrt", &[Value::number(27.0)]).as_number().unwrap();
        assert!((cbrt_27 - 3.0).abs() < 1e-15);
    }

    #[test]
    fn math_sign_trunc() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_math(&mut heap, global);

        assert_eq!(call_math(&mut heap, global, "sign", &[Value::number(-42.0)]), Value::number(-1.0));
        assert_eq!(call_math(&mut heap, global, "sign", &[Value::number(42.0)]), Value::number(1.0));
        assert_eq!(call_math(&mut heap, global, "trunc", &[Value::number(3.9)]), Value::number(3.0));
        assert_eq!(call_math(&mut heap, global, "trunc", &[Value::number(-3.9)]), Value::number(-3.0));
    }
}

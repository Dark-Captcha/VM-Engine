//! Browser-like environment for the VM-Engine IR interpreter.
//!
//! Installs browser globals (Math, JSON, Date, navigator, etc.) onto the heap
//! as native callable objects. Each module is independently installable.
//!
//! # Quick Start
//!
//! ```
//! use vm_engine_core::exec::heap::Heap;
//! use vm_engine_core::value::Value;
//! use vm_engine_web::{WebConfig, install_all};
//!
//! let mut heap = Heap::new();
//! let global = heap.alloc();
//! install_all(&mut heap, global, &WebConfig::default());
//!
//! // Now heap has: window, Math, JSON, Date, navigator, screen, document, etc.
//! let math = heap.get_property(global, "Math").as_object().unwrap();
//! let floor = heap.get_property(math, "floor").as_object().unwrap();
//! let result = heap.call(floor, &[Value::number(3.7)]).unwrap();
//! assert_eq!(result, Value::number(3.0));
//! ```

pub mod console;
pub mod document;
pub mod encoding;
pub mod globals;
pub mod json;
pub mod math;
pub mod navigator;
pub mod random;
pub mod screen;
pub mod string_utils;
pub mod timing;

// ============================================================================
// Re-exports
// ============================================================================

pub use document::DocumentConfig;
pub use navigator::NavigatorConfig;
pub use random::RandomConfig;
pub use screen::ScreenConfig;
pub use timing::TimingConfig;

// ============================================================================
// Imports
// ============================================================================

use vm_engine_core::exec::heap::Heap;
use vm_engine_core::value::ObjectId;

// ============================================================================
// WebConfig
// ============================================================================

/// Combined configuration for the full browser environment.
#[derive(Debug, Clone)]
pub struct WebConfig {
    pub navigator: NavigatorConfig,
    pub screen: ScreenConfig,
    pub timing: TimingConfig,
    pub random: RandomConfig,
    pub document: DocumentConfig,
}

impl Default for WebConfig {
    #[allow(clippy::derivable_impls)]
    fn default() -> Self {
        Self {
            navigator: NavigatorConfig::default(),
            screen: ScreenConfig::default(),
            timing: TimingConfig::default(),
            random: RandomConfig::default(),
            document: DocumentConfig::default(),
        }
    }
}

// ============================================================================
// Install
// ============================================================================

/// Install the complete browser environment on the global object.
///
/// Calls each module's install function in the correct order.
/// For selective installation, call individual `install_*` functions directly.
pub fn install_all(heap: &mut Heap, global: ObjectId, config: &WebConfig) {
    globals::install_globals(heap, global);
    math::install_math(heap, global);
    json::install_json(heap, global);
    encoding::install_encoding(heap, global);
    string_utils::install_string_utils(heap, global);
    timing::install_timing(heap, global, &config.timing);
    random::install_random(heap, global, &config.random);  // after math (adds Math.random)
    navigator::install_navigator(heap, global, &config.navigator);
    screen::install_screen(heap, global, &config.screen);
    document::install_document(heap, global, &config.document);
    console::install_console(heap, global);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use vm_engine_core::value::Value;

    #[test]
    fn install_all_creates_complete_environment() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_all(&mut heap, global, &WebConfig::default());

        // Verify key globals exist
        assert!(heap.get_property(global, "window").as_object().is_some());
        assert!(heap.get_property(global, "Math").as_object().is_some());
        assert!(heap.get_property(global, "JSON").as_object().is_some());
        assert!(heap.get_property(global, "Date").as_object().is_some());
        assert!(heap.get_property(global, "navigator").as_object().is_some());
        assert!(heap.get_property(global, "screen").as_object().is_some());
        assert!(heap.get_property(global, "document").as_object().is_some());
        assert!(heap.get_property(global, "crypto").as_object().is_some());
        assert!(heap.get_property(global, "console").as_object().is_some());
        assert!(heap.get_property(global, "btoa").as_object().is_some());
        assert!(heap.get_property(global, "atob").as_object().is_some());
        assert!(heap.get_property(global, "String").as_object().is_some());
    }

    #[test]
    fn full_pipeline_btoa_encode() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_all(&mut heap, global, &WebConfig::default());

        // Simulate: btoa(JSON.stringify({key: "value"}))
        let json = heap.get_property(global, "JSON").as_object().unwrap();
        let stringify = heap.get_property(json, "stringify").as_object().unwrap();

        let obj = heap.alloc();
        heap.set_property(obj, "key", Value::string("value"));
        let json_string = heap.call(stringify, &[Value::Object(obj)]).unwrap();
        assert!(json_string.as_str().unwrap().contains("key"));

        let btoa = heap.get_property(global, "btoa").as_object().unwrap();
        let encoded = heap.call(btoa, &[json_string]).unwrap();
        assert!(encoded.as_str().is_some(), "btoa should return a string");
    }

    #[test]
    fn plv3_required_apis_all_present() {
        // DataDome PLV3 needs: Date.now, performance.now, Math.random,
        // JSON.stringify, String.fromCharCode, btoa, navigator.webdriver,
        // document.location.pathname, isSecureContext, clientWidth
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_all(&mut heap, global, &WebConfig::default());

        let date = heap.get_property(global, "Date").as_object().unwrap();
        assert!(heap.get_property(date, "now").as_object().is_some(), "Date.now missing");

        let perf = heap.get_property(global, "performance").as_object().unwrap();
        assert!(heap.get_property(perf, "now").as_object().is_some(), "performance.now missing");

        let math = heap.get_property(global, "Math").as_object().unwrap();
        assert!(heap.get_property(math, "random").as_object().is_some(), "Math.random missing");

        let json = heap.get_property(global, "JSON").as_object().unwrap();
        assert!(heap.get_property(json, "stringify").as_object().is_some(), "JSON.stringify missing");

        let string = heap.get_property(global, "String").as_object().unwrap();
        assert!(heap.get_property(string, "fromCharCode").as_object().is_some(), "String.fromCharCode missing");

        assert!(heap.get_property(global, "btoa").as_object().is_some(), "btoa missing");

        let nav = heap.get_property(global, "navigator").as_object().unwrap();
        assert_eq!(heap.get_property(nav, "webdriver"), Value::bool(false));

        let doc = heap.get_property(global, "document").as_object().unwrap();
        let loc = heap.get_property(doc, "location").as_object().unwrap();
        assert!(heap.get_property(loc, "pathname").as_str().is_some(), "location.pathname missing");

        assert_eq!(heap.get_property(global, "isSecureContext"), Value::bool(true));

        let doc_elem = heap.get_property(doc, "documentElement").as_object().unwrap();
        assert!(heap.get_property(doc_elem, "clientWidth").as_number().is_some(), "clientWidth missing");
    }
}

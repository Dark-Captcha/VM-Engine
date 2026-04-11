//! Navigator object with configurable properties.

// ============================================================================
// Imports
// ============================================================================

use crate::exec::heap::Heap;
use crate::value::{ObjectId, Value};

// ============================================================================
// Config
// ============================================================================

/// Configuration for navigator properties.
#[derive(Debug, Clone)]
pub struct NavigatorConfig {
    pub user_agent: String,
    pub platform: String,
    pub language: String,
    pub languages: Vec<String>,
    pub hardware_concurrency: u32,
    pub max_touch_points: u32,
    pub webdriver: bool,
    pub vendor: String,
    pub cookie_enabled: bool,
    pub online: bool,
}

impl Default for NavigatorConfig {
    fn default() -> Self {
        Self {
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".into(),
            platform: "Win32".into(),
            language: "en-US".into(),
            languages: vec!["en-US".into(), "en".into()],
            hardware_concurrency: 8,
            max_touch_points: 0,
            webdriver: false,
            vendor: "Google Inc.".into(),
            cookie_enabled: true,
            online: true,
        }
    }
}

// ============================================================================
// Install
// ============================================================================

/// Install the `navigator` object on the global.
pub fn install_navigator(heap: &mut Heap, global: ObjectId, config: &NavigatorConfig) {
    let navigator = heap.alloc();

    heap.set_property(navigator, "userAgent", Value::string(&config.user_agent));
    heap.set_property(navigator, "platform", Value::string(&config.platform));
    heap.set_property(navigator, "language", Value::string(&config.language));
    heap.set_property(navigator, "hardwareConcurrency", Value::number(config.hardware_concurrency as f64));
    heap.set_property(navigator, "maxTouchPoints", Value::number(config.max_touch_points as f64));
    heap.set_property(navigator, "webdriver", Value::bool(config.webdriver));
    heap.set_property(navigator, "vendor", Value::string(&config.vendor));
    heap.set_property(navigator, "cookieEnabled", Value::bool(config.cookie_enabled));
    heap.set_property(navigator, "onLine", Value::bool(config.online));

    // languages as Array
    let languages = Value::Array(config.languages.iter().map(Value::string).collect());
    heap.set_property(navigator, "languages", languages);

    // Additional properties fingerprinted by anti-bots
    heap.set_property(navigator, "appName", Value::string("Netscape"));
    heap.set_property(navigator, "appVersion", Value::string("5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"));
    heap.set_property(navigator, "product", Value::string("Gecko"));
    heap.set_property(navigator, "productSub", Value::string("20030107"));
    heap.set_property(navigator, "appCodeName", Value::string("Mozilla"));
    heap.set_property(navigator, "doNotTrack", Value::Null);
    heap.set_property(navigator, "deviceMemory", Value::number(8.0));

    // Plugin and mime type stubs
    let plugins = heap.alloc();
    heap.set_property(plugins, "length", Value::number(5.0));
    heap.set_property(navigator, "plugins", Value::Object(plugins));

    let mime_types = heap.alloc();
    heap.set_property(mime_types, "length", Value::number(4.0));
    heap.set_property(navigator, "mimeTypes", Value::Object(mime_types));

    // Permissions stub
    let permissions = heap.alloc();
    let permissions_query = heap.alloc_native(|_args, heap| {
        // Return a Promise-like object with state: "prompt"
        let result = heap.alloc();
        heap.set_property(result, "state", Value::string("prompt"));
        let then_fn = heap.alloc_native(|callback_args, heap| {
            if let Some(Value::Object(callback)) = callback_args.first() {
                let perm_status = heap.alloc();
                heap.set_property(perm_status, "state", Value::string("prompt"));
                let _ = heap.call(*callback, &[Value::Object(perm_status)]);
            }
            Value::Undefined
        });
        heap.set_property(result, "then", Value::Object(then_fn));
        Value::Object(result)
    });
    heap.set_property(permissions, "query", Value::Object(permissions_query));
    heap.set_property(navigator, "permissions", Value::Object(permissions));

    // Geolocation stub
    let geolocation = heap.alloc();
    let get_current_position = heap.alloc_native(|_args, _heap| {
        // No-op - would need async support for real implementation
        Value::Undefined
    });
    heap.set_property(geolocation, "getCurrentPosition", Value::Object(get_current_position));
    heap.set_property(navigator, "geolocation", Value::Object(geolocation));

    // MediaDevices stub
    let media_devices = heap.alloc();
    let enumerate_devices = heap.alloc_native(|_args, heap| {
        // Return empty array wrapped in Promise-like
        let result = heap.alloc();
        let then_fn = heap.alloc_native(|callback_args, heap| {
            if let Some(Value::Object(callback)) = callback_args.first() {
                let _ = heap.call(*callback, &[Value::Array(vec![])]);
            }
            Value::Undefined
        });
        heap.set_property(result, "then", Value::Object(then_fn));
        Value::Object(result)
    });
    heap.set_property(media_devices, "enumerateDevices", Value::Object(enumerate_devices));
    heap.set_property(navigator, "mediaDevices", Value::Object(media_devices));

    // ServiceWorker stub (container object)
    let service_worker = heap.alloc();
    heap.set_property(service_worker, "controller", Value::Null);
    heap.set_property(navigator, "serviceWorker", Value::Object(service_worker));

    // Credentials stub
    let credentials = heap.alloc();
    heap.set_property(navigator, "credentials", Value::Object(credentials));

    heap.set_property(global, "navigator", Value::Object(navigator));
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigator_properties() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_navigator(&mut heap, global, &NavigatorConfig::default());

        let nav = heap.get_property(global, "navigator").as_object().unwrap();
        assert!(heap.get_property(nav, "userAgent").as_str().unwrap().contains("Chrome"));
        assert_eq!(heap.get_property(nav, "webdriver"), Value::bool(false));
        assert_eq!(heap.get_property(nav, "hardwareConcurrency"), Value::number(8.0));
    }

    #[test]
    fn custom_navigator() {
        let config = NavigatorConfig {
            user_agent: "CustomBot/1.0".into(),
            webdriver: true,
            ..Default::default()
        };
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_navigator(&mut heap, global, &config);

        let nav = heap.get_property(global, "navigator").as_object().unwrap();
        assert_eq!(heap.get_property(nav, "userAgent"), Value::string("CustomBot/1.0"));
        assert_eq!(heap.get_property(nav, "webdriver"), Value::bool(true));
    }
}

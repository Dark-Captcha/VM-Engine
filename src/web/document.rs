//! Document object: location, documentElement, createElement stub.

// ============================================================================
// Imports
// ============================================================================

use crate::exec::heap::Heap;
use crate::value::{ObjectId, Value};

// ============================================================================
// Config
// ============================================================================

/// Configuration for document properties.
#[derive(Debug, Clone)]
pub struct DocumentConfig {
    pub location_href: String,
    pub location_pathname: String,
    pub location_protocol: String,
    pub location_hostname: String,
    pub character_set: String,
    pub compat_mode: String,
    pub client_width: u32,
    pub client_height: u32,
    pub is_secure_context: bool,
}

impl Default for DocumentConfig {
    fn default() -> Self {
        Self {
            location_href: "https://example.com/".into(),
            location_pathname: "/".into(),
            location_protocol: "https:".into(),
            location_hostname: "example.com".into(),
            character_set: "UTF-8".into(),
            compat_mode: "CSS1Compat".into(),
            client_width: 1920,
            client_height: 1080,
            is_secure_context: true,
        }
    }
}

// ============================================================================
// Install
// ============================================================================

/// Install `document` and `location` on the global object.
pub fn install_document(heap: &mut Heap, global: ObjectId, config: &DocumentConfig) {
    // location
    let location = heap.alloc();
    heap.set_property(location, "href", Value::string(&config.location_href));
    heap.set_property(location, "pathname", Value::string(&config.location_pathname));
    heap.set_property(location, "protocol", Value::string(&config.location_protocol));
    heap.set_property(location, "hostname", Value::string(&config.location_hostname));

    // document
    let document = heap.alloc();
    heap.set_property(document, "location", Value::Object(location));
    heap.set_property(document, "characterSet", Value::string(&config.character_set));
    heap.set_property(document, "compatMode", Value::string(&config.compat_mode));

    // document.documentElement
    let document_element = heap.alloc();
    heap.set_property(document_element, "clientWidth", Value::number(config.client_width as f64));
    heap.set_property(document_element, "clientHeight", Value::number(config.client_height as f64));
    heap.set_property(document, "documentElement", Value::Object(document_element));

    // document.createElement — returns a minimal element stub
    let create_element = heap.alloc_native(|args, heap| {
        let tag = args.first()
            .and_then(|v| v.as_str())
            .unwrap_or("div");
        let element = heap.alloc();
        heap.set_property(element, "tagName", Value::string(tag.to_uppercase()));
        let style = heap.alloc();
        heap.set_property(element, "style", Value::Object(style));
        Value::Object(element)
    });
    heap.set_property(document, "createElement", Value::Object(create_element));

    // document.querySelector — stub returning null
    let query_selector = heap.alloc_native(|_args, _heap| Value::Null);
    heap.set_property(document, "querySelector", Value::Object(query_selector));

    // document.body — PLV3 needs body.appendChild/removeChild for clientWidth measurement
    let body = heap.alloc();
    let append_child = heap.alloc_native(|args, heap| {
        // appendChild returns the child; set clientWidth on it for PLV3 measurement
        if let Some(Value::Object(child)) = args.first() {
            // PLV3 creates a div with width:29px + paddingLeft:17px in content-box → clientWidth=46
            heap.set_property(*child, "clientWidth", Value::number(46.0));
        }
        args.first().cloned().unwrap_or(Value::Undefined)
    });
    heap.set_property(body, "appendChild", Value::Object(append_child));
    let remove_child = heap.alloc_native(|args, _heap| {
        args.first().cloned().unwrap_or(Value::Undefined)
    });
    heap.set_property(body, "removeChild", Value::Object(remove_child));
    heap.set_property(document, "body", Value::Object(body));

    heap.set_property(global, "document", Value::Object(document));

    // Also expose location directly on global (common JS pattern)
    heap.set_property(global, "location", Value::Object(location));

    // Window-level properties derived from document config
    heap.set_property(global, "isSecureContext", Value::bool(config.is_secure_context));
    heap.set_property(global, "innerWidth", Value::number(config.client_width as f64));
    heap.set_property(global, "innerHeight", Value::number(config.client_height as f64));
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_location() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_document(&mut heap, global, &DocumentConfig::default());

        let doc = heap.get_property(global, "document").as_object().unwrap();
        let loc = heap.get_property(doc, "location").as_object().unwrap();
        assert_eq!(heap.get_property(loc, "pathname"), Value::string("/"));
        assert_eq!(heap.get_property(loc, "protocol"), Value::string("https:"));
    }

    #[test]
    fn document_element_dimensions() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_document(&mut heap, global, &DocumentConfig::default());

        let doc = heap.get_property(global, "document").as_object().unwrap();
        let elem = heap.get_property(doc, "documentElement").as_object().unwrap();
        assert_eq!(heap.get_property(elem, "clientWidth"), Value::number(1920.0));
    }

    #[test]
    fn create_element_returns_object() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_document(&mut heap, global, &DocumentConfig::default());

        let doc = heap.get_property(global, "document").as_object().unwrap();
        let create_fn = heap.get_property(doc, "createElement").as_object().unwrap();
        let element = heap.call(create_fn, &[Value::string("div")]).unwrap();

        assert!(element.as_object().is_some());
        let elem_id = element.as_object().unwrap();
        assert_eq!(heap.get_property(elem_id, "tagName"), Value::string("DIV"));
    }

    #[test]
    fn is_secure_context() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_document(&mut heap, global, &DocumentConfig::default());

        assert_eq!(heap.get_property(global, "isSecureContext"), Value::bool(true));
    }
}

//! Screen object with configurable dimensions.

// ============================================================================
// Imports
// ============================================================================

use crate::exec::heap::Heap;
use crate::value::{ObjectId, Value};

// ============================================================================
// Config
// ============================================================================

/// Configuration for screen dimensions.
#[derive(Debug, Clone)]
pub struct ScreenConfig {
    pub width: u32,
    pub height: u32,
    pub avail_width: u32,
    pub avail_height: u32,
    pub color_depth: u32,
    pub pixel_depth: u32,
}

impl Default for ScreenConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            avail_width: 1920,
            avail_height: 1040,
            color_depth: 24,
            pixel_depth: 24,
        }
    }
}

// ============================================================================
// Install
// ============================================================================

/// Install the `screen` object on the global.
pub fn install_screen(heap: &mut Heap, global: ObjectId, config: &ScreenConfig) {
    let screen = heap.alloc();

    heap.set_property(screen, "width", Value::number(config.width as f64));
    heap.set_property(screen, "height", Value::number(config.height as f64));
    heap.set_property(screen, "availWidth", Value::number(config.avail_width as f64));
    heap.set_property(screen, "availHeight", Value::number(config.avail_height as f64));
    heap.set_property(screen, "colorDepth", Value::number(config.color_depth as f64));
    heap.set_property(screen, "pixelDepth", Value::number(config.pixel_depth as f64));

    heap.set_property(global, "screen", Value::Object(screen));
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_dimensions() {
        let mut heap = Heap::new();
        let global = heap.alloc();
        install_screen(&mut heap, global, &ScreenConfig::default());

        let screen = heap.get_property(global, "screen").as_object().unwrap();
        assert_eq!(heap.get_property(screen, "width"), Value::number(1920.0));
        assert_eq!(heap.get_property(screen, "height"), Value::number(1080.0));
        assert_eq!(heap.get_property(screen, "colorDepth"), Value::number(24.0));
    }
}

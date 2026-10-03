//! Actual native client geometry at each measured phase boundary.

#[cfg(feature = "gpui-kit-engine")]
use gpui_kit as ui;
#[cfg(feature = "kael-engine")]
use kael as ui;

pub struct NativeGeometry {
    pub samples: Vec<serde_json::Value>,
}

impl NativeGeometry {
    pub fn new() -> Self {
        Self {
            samples: Vec::with_capacity(8),
        }
    }

    pub fn record(&mut self, phase: &str, boundary: &str, window: &ui::Window) {
        assert!(self.samples.len() < 8, "unexpected geometry boundary");
        let viewport = window.viewport_size();
        self.samples.push(serde_json::json!({
            "phase": phase,
            "boundary": boundary,
            "viewport_width_px": f32::from(viewport.width),
            "viewport_height_px": f32::from(viewport.height),
            "scale_factor": window.scale_factor(),
        }));
    }
}

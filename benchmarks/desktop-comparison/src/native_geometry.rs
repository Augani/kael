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

#[derive(Default)]
pub struct PhaseFrames {
    renders: [u64; 4],
    draws: [u64; 4],
    submissions: [u64; 4],
}

const PHASES: [&str; 4] = ["idle-before", "active", "idle-after", "churn"];

impl PhaseFrames {
    fn index(phase: &str) -> Option<usize> {
        match phase {
            "idle-before" => Some(0),
            "active" => Some(1),
            "idle-after" => Some(2),
            "churn" => Some(3),
            _ => None,
        }
    }

    pub fn rendered(&mut self, phase: &str) {
        if let Some(index) = Self::index(phase) {
            self.renders[index] += 1;
        }
    }

    #[cfg(feature = "frame-timing")]
    pub fn drew(&mut self, phase: &str) {
        if let Some(index) = Self::index(phase) {
            self.draws[index] += 1;
        }
    }

    #[cfg(feature = "frame-timing")]
    pub fn submitted(&mut self, phase: &str) {
        if let Some(index) = Self::index(phase) {
            self.submissions[index] += 1;
        }
    }

    pub fn report(&self) -> serde_json::Value {
        serde_json::json!(
            PHASES
                .iter()
                .enumerate()
                .map(|(index, phase)| {
                    serde_json::json!({
                        "phase": phase,
                        "renders": self.renders[index],
                        "draws": self.draws[index],
                        "submissions": self.submissions[index],
                    })
                })
                .collect::<Vec<_>>()
        )
    }
}

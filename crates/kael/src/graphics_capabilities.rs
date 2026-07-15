//! Capability reporting for Kael's graphics escape hatches.

/// The current support level for a graphics feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphicsCapabilityStatus {
    /// Fully supported by Kael's public native rendering APIs.
    Full,
    /// Usable today with documented limitations or missing browser parity.
    Partial,
    /// Available by embedding browser content in a WebView island.
    WebView,
    /// Planned, but not exposed as a stable public Kael API yet.
    Roadmap,
}

impl GraphicsCapabilityStatus {
    /// Return a stable label for capability reports and agent traces.
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
            Self::WebView => "webview",
            Self::Roadmap => "roadmap",
        }
    }
}

/// A compact report of Kael's current graphics escape hatches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphicsCapabilityReport {
    /// Styled layout, backgrounds, borders, shadows, transforms, filters, and animations.
    pub styled_elements: GraphicsCapabilityStatus,
    /// Immediate-mode drawing with `canvas`, `Window::paint_quad`, and `Window::paint_path`.
    pub immediate_canvas: GraphicsCapabilityStatus,
    /// Vector path building and stroke/fill rendering.
    pub vector_paths: GraphicsCapabilityStatus,
    /// Linear, radial, conic, and multi-stop gradients.
    pub gradients: GraphicsCapabilityStatus,
    /// SVG asset rendering.
    pub svg: GraphicsCapabilityStatus,
    /// Lottie/dotLottie playback.
    pub lottie: GraphicsCapabilityStatus,
    /// Clip shapes and mask-like composition available through public element APIs.
    pub clip_shapes: GraphicsCapabilityStatus,
    /// Cached subtree effects such as content blur and silhouette drop shadows.
    pub effect_layers: GraphicsCapabilityStatus,
    /// Off-screen rendering for benchmarks and golden-image tests.
    pub headless_rendering: GraphicsCapabilityStatus,
    /// Browser-canvas/WebGL/WebGPU fallback through WebView islands.
    pub browser_graphics_fallback: GraphicsCapabilityStatus,
    /// Public custom render targets and render graph integration.
    pub public_render_targets: GraphicsCapabilityStatus,
    /// Public custom shader injection for native Kael drawing.
    pub public_custom_shaders: GraphicsCapabilityStatus,
}

impl GraphicsCapabilityReport {
    /// Return whether every field is fully implemented as a native Kael API.
    pub fn is_full_native(&self) -> bool {
        self.statuses()
            .into_iter()
            .all(|status| status == GraphicsCapabilityStatus::Full)
    }

    /// Return the report statuses as a compact list for dashboards/tests.
    pub fn statuses(&self) -> [GraphicsCapabilityStatus; 12] {
        [
            self.styled_elements,
            self.immediate_canvas,
            self.vector_paths,
            self.gradients,
            self.svg,
            self.lottie,
            self.clip_shapes,
            self.effect_layers,
            self.headless_rendering,
            self.browser_graphics_fallback,
            self.public_render_targets,
            self.public_custom_shaders,
        ]
    }

    /// Return true when the report contains any roadmap-only graphics gaps.
    pub fn has_roadmap_gaps(&self) -> bool {
        self.statuses()
            .into_iter()
            .any(|status| status == GraphicsCapabilityStatus::Roadmap)
    }

    /// Return whether any graphics capability is available only through WebView.
    pub fn has_webview_fallbacks(&self) -> bool {
        self.statuses()
            .into_iter()
            .any(|status| status == GraphicsCapabilityStatus::WebView)
    }

    /// Count features with the requested status.
    pub fn count_status(&self, status: GraphicsCapabilityStatus) -> usize {
        self.statuses()
            .into_iter()
            .filter(|current| *current == status)
            .count()
    }

    /// Return a content-safe summary for docs, dashboards, and agent traces.
    pub fn to_text(&self) -> String {
        let full = self.count_status(GraphicsCapabilityStatus::Full);
        let partial = self.count_status(GraphicsCapabilityStatus::Partial);
        let webview = self.count_status(GraphicsCapabilityStatus::WebView);
        let roadmap = self.count_status(GraphicsCapabilityStatus::Roadmap);

        format!(
            "graphics capabilities: full {full}, partial {partial}, webview {webview}, roadmap {roadmap}, all native full {}",
            self.is_full_native()
        )
    }
}

/// Return Kael's current graphics capability report.
pub fn graphics_capability_report() -> GraphicsCapabilityReport {
    GraphicsCapabilityReport {
        styled_elements: GraphicsCapabilityStatus::Full,
        immediate_canvas: GraphicsCapabilityStatus::Full,
        vector_paths: GraphicsCapabilityStatus::Full,
        gradients: GraphicsCapabilityStatus::Full,
        svg: GraphicsCapabilityStatus::Full,
        lottie: GraphicsCapabilityStatus::Full,
        clip_shapes: GraphicsCapabilityStatus::Partial,
        effect_layers: GraphicsCapabilityStatus::Partial,
        headless_rendering: GraphicsCapabilityStatus::Partial,
        browser_graphics_fallback: GraphicsCapabilityStatus::WebView,
        public_render_targets: GraphicsCapabilityStatus::Roadmap,
        public_custom_shaders: GraphicsCapabilityStatus::Roadmap,
    }
}

#[cfg(test)]
mod tests {
    use super::{GraphicsCapabilityStatus, graphics_capability_report};

    #[test]
    fn graphics_capability_report_is_honest_about_public_gpu_gaps() {
        let report = graphics_capability_report();

        assert!(!report.is_full_native());
        assert!(report.has_roadmap_gaps());
        assert!(report.has_webview_fallbacks());
        assert_eq!(GraphicsCapabilityStatus::Full.to_text(), "full");
        assert_eq!(GraphicsCapabilityStatus::Partial.to_text(), "partial");
        assert_eq!(GraphicsCapabilityStatus::WebView.to_text(), "webview");
        assert_eq!(GraphicsCapabilityStatus::Roadmap.to_text(), "roadmap");
        assert_eq!(report.count_status(GraphicsCapabilityStatus::Full), 6);
        assert_eq!(report.count_status(GraphicsCapabilityStatus::Partial), 3);
        assert_eq!(report.count_status(GraphicsCapabilityStatus::WebView), 1);
        assert_eq!(report.count_status(GraphicsCapabilityStatus::Roadmap), 2);
        assert_eq!(report.styled_elements, GraphicsCapabilityStatus::Full);
        assert_eq!(report.immediate_canvas, GraphicsCapabilityStatus::Full);
        assert_eq!(report.vector_paths, GraphicsCapabilityStatus::Full);
        assert_eq!(report.gradients, GraphicsCapabilityStatus::Full);
        assert_eq!(report.svg, GraphicsCapabilityStatus::Full);
        assert_eq!(report.lottie, GraphicsCapabilityStatus::Full);
        assert_eq!(report.clip_shapes, GraphicsCapabilityStatus::Partial);
        assert_eq!(report.effect_layers, GraphicsCapabilityStatus::Partial);
        assert_eq!(report.headless_rendering, GraphicsCapabilityStatus::Partial);
        assert_eq!(
            report.browser_graphics_fallback,
            GraphicsCapabilityStatus::WebView
        );
        assert_eq!(
            report.public_render_targets,
            GraphicsCapabilityStatus::Roadmap
        );
        assert_eq!(
            report.public_custom_shaders,
            GraphicsCapabilityStatus::Roadmap
        );
        assert_eq!(
            report.to_text(),
            "graphics capabilities: full 6, partial 3, webview 1, roadmap 2, all native full false"
        );
    }
}

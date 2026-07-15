use crate::{
    AccessibilityAttributes, AccessibilityRole, AccessibilityState, AccessibilityValue, App,
    Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels,
    Style, StyleRefinement, Styled, Window, fill, px, relative, rgb,
};
use refineable::Refineable;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
/// Snapshot of progress state passed to a custom renderer.
pub struct ProgressRenderState {
    /// The clamped current value.
    pub value: f64,
    /// The current maximum value.
    pub max: f64,
    /// The normalized completion fraction between 0.0 and 1.0 when determinate.
    pub percentage: Option<f64>,
    /// Whether the progress bar is currently indeterminate.
    pub indeterminate: bool,
}

impl ProgressRenderState {
    /// Returns true when progress has a determinate percentage.
    pub fn is_determinate(&self) -> bool {
        self.percentage.is_some() && !self.indeterminate
    }

    /// Coarse completion class for content-safe diagnostics.
    pub fn completion_class(&self) -> &'static str {
        if self.indeterminate {
            "indeterminate"
        } else {
            match self.percentage.unwrap_or(0.0) {
                percentage if percentage <= 0.0 => "empty",
                percentage if percentage >= 1.0 => "complete",
                _ => "partial",
            }
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "progress_render_state(indeterminate={}, determinate={}, completion_class={})",
            self.indeterminate,
            self.is_determinate(),
            self.completion_class()
        )
    }
}

type ProgressCustomRenderer =
    Rc<dyn Fn(ProgressRenderState, Bounds<Pixels>, &mut Window, &mut App)>;

/// Construct a progress indicator from the current value.
#[track_caller]
pub fn progress(id: impl Into<ElementId>, value: f64) -> Progress {
    Progress::new(id.into(), value)
}

/// A styled progress indicator with caller-owned rendering.
pub struct Progress {
    element_id: ElementId,
    value: f64,
    max: f64,
    indeterminate: bool,
    custom_renderer: Option<ProgressCustomRenderer>,
    style: StyleRefinement,
    source_location: &'static core::panic::Location<'static>,
}

impl Progress {
    #[track_caller]
    fn new(element_id: ElementId, value: f64) -> Self {
        let mut style = StyleRefinement::default();
        style.size.width = Some(relative(1.0).into());
        style.size.height = Some(px(12.0).into());

        Self {
            element_id,
            value,
            max: 1.0,
            indeterminate: false,
            custom_renderer: None,
            style,
            source_location: core::panic::Location::caller(),
        }
    }

    /// Set the maximum progress value.
    pub fn max(mut self, max: f64) -> Self {
        if max.is_finite() && max > 0.0 {
            self.max = max;
        }
        self
    }

    /// Mark the progress indicator as indeterminate.
    pub fn indeterminate(mut self) -> Self {
        self.indeterminate = true;
        self
    }

    /// Render the progress indicator with caller-owned visuals.
    pub fn render_with(
        mut self,
        renderer: impl Fn(ProgressRenderState, Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.custom_renderer = Some(Rc::new(renderer));
        self
    }

    fn normalized_value(&self) -> f64 {
        self.value.clamp(0.0, self.max)
    }

    fn render_state(&self) -> ProgressRenderState {
        let value = self.normalized_value();
        ProgressRenderState {
            value,
            max: self.max,
            percentage: (!self.indeterminate).then_some((value / self.max).clamp(0.0, 1.0)),
            indeterminate: self.indeterminate,
        }
    }
}

impl Element for Progress {
    type RequestLayoutState = Style;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.element_id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        Some(self.source_location)
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style.clone(), [], cx);
        (layout_id, style)
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let render_state = self.render_state();
        let accessibility_value = (!self.indeterminate).then_some(AccessibilityValue::Range {
            current: render_state.value,
            min: 0.0,
            max: render_state.max,
            step: None,
        });

        style.paint(bounds, window, cx, |window, cx| {
            if let Some(renderer) = &self.custom_renderer {
                renderer(render_state, bounds, window, cx);
            } else {
                paint_default_progress(render_state, bounds, window);
            }
        });

        let accessibility = AccessibilityAttributes::new(AccessibilityRole::ProgressBar).states(
            if self.indeterminate {
                AccessibilityState::BUSY
            } else {
                AccessibilityState::NONE
            },
        );

        let accessibility = if self.indeterminate {
            accessibility
        } else {
            accessibility.value(accessibility_value.expect("determinate progress has range value"))
        };

        let accessibility_id = window.next_anonymous_accessibility_id();
        window.register_accessibility_node_at(accessibility.to_node(accessibility_id), bounds);
    }
}

impl IntoElement for Progress {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for Progress {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn paint_default_progress(state: ProgressRenderState, bounds: Bounds<Pixels>, window: &mut Window) {
    window.paint_quad(fill(bounds, rgb(0xe2e8f0)).corner_radii(px(999.0)));

    let fill_bounds = if state.indeterminate {
        let width = (bounds.size.width * 0.35)
            .max(px(12.0))
            .min(bounds.size.width);
        Bounds::new(bounds.origin, crate::size(width, bounds.size.height))
    } else {
        Bounds::new(
            bounds.origin,
            crate::size(
                bounds.size.width * state.percentage.unwrap_or(0.0) as f32,
                bounds.size.height,
            ),
        )
    };

    if fill_bounds.size.width > Pixels::ZERO {
        window.paint_quad(fill(fill_bounds, rgb(0x1d4ed8)).corner_radii(px(999.0)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AccessibilityRole, AccessibilityState, AccessibilityValue, Context, ParentElement, Render,
        TestAppContext, div,
    };
    use std::cell::Cell;

    struct ProgressView;

    struct CustomProgressView {
        snapshot: Rc<Cell<Option<(f64, Option<f64>, bool)>>>,
    }

    impl Render for ProgressView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(240.0)).h(px(12.0)).child(progress("task", 0.25))
        }
    }

    impl Render for CustomProgressView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let snapshot = self.snapshot.clone();
            div().w(px(240.0)).h(px(12.0)).child(
                progress("task_custom", 0.5)
                    .indeterminate()
                    .render_with(move |state, _, _, _| {
                        snapshot.set(Some((state.value, state.percentage, state.indeterminate)));
                    }),
            )
        }
    }

    #[test]
    fn progress_render_state_clamps_to_max() {
        let state = progress("task", 2.0).render_state();
        assert_eq!(state.value, 1.0);
        assert_eq!(state.percentage, Some(1.0));
    }

    #[crate::test]
    fn progress_registers_accessibility_range(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| ProgressView);

        window.update(|window, cx| {
            window.draw(cx).clear();

            let progress = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::ProgressBar)
                .unwrap();

            assert_eq!(
                progress.value,
                Some(AccessibilityValue::Range {
                    current: 0.25,
                    min: 0.0,
                    max: 1.0,
                    step: None,
                })
            );
            assert_eq!(progress.states, AccessibilityState::NONE);
        });
    }

    #[crate::test]
    fn progress_render_with_receives_indeterminate_state(cx: &mut TestAppContext) {
        let snapshot = Rc::new(Cell::new(None));
        let snapshot_ref = snapshot.clone();
        let (_view, mut window) = cx.add_window_view(|_, _| CustomProgressView {
            snapshot: snapshot_ref,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(snapshot.get(), Some((0.5, None, true)));

        window.update(|window, cx| {
            window.draw(cx).clear();

            let progress = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::ProgressBar)
                .unwrap();
            assert!(progress.states.contains(AccessibilityState::BUSY));
            assert_eq!(progress.value, None);
        });
    }

    #[test]
    fn progress_render_state_summary_is_content_safe() {
        let state = progress("task", 0.42).max(2.5).render_state();

        assert!(state.is_determinate());
        assert_eq!(state.completion_class(), "partial");

        let summary = state.to_text();
        assert!(summary.contains("indeterminate=false"));
        assert!(summary.contains("determinate=true"));
        assert!(summary.contains("completion_class=partial"));
        assert!(!summary.contains("0.42"));
        assert!(!summary.contains("2.5"));
        assert!(!summary.contains("0.168"));
    }
}

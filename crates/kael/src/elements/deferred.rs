use crate::{
    AnyElement, App, Bounds, Element, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Window,
};

/// Builds a `Deferred` element, which delays the layout and paint of its child.
pub fn deferred(child: impl IntoElement) -> Deferred {
    Deferred {
        child: Some(child.into_any_element()),
        priority: 0,
    }
}

/// An element which delays the painting of its child until after all of
/// its ancestors, while keeping its layout as part of the current element tree.
pub struct Deferred {
    child: Option<AnyElement>,
    priority: usize,
}

impl Deferred {
    /// Returns true when the deferred wrapper still owns a child element.
    pub fn has_child(&self) -> bool {
        self.child.is_some()
    }

    /// Returns the configured draw priority.
    pub fn priority_value(&self) -> usize {
        self.priority
    }

    /// Coarse priority class for content-safe diagnostics.
    pub fn priority_class(&self) -> &'static str {
        match self.priority {
            0 => "default",
            1..=99 => "raised",
            _ => "overlay",
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "deferred(has_child={}, priority={}, priority_class={})",
            self.has_child(),
            self.priority_value(),
            self.priority_class()
        )
    }

    /// Sets the `priority` value of the `deferred` element, which
    /// determines the drawing order relative to other deferred elements,
    /// with higher values being drawn on top.
    pub fn with_priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }
}

impl Element for Deferred {
    type RequestLayoutState = ();
    type PrepaintState = crate::window::DeferredAccessibilityState;

    fn id(&self) -> Option<crate::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let layout_id = self.child.as_mut().unwrap().request_layout(window, cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        let child = self.child.take().unwrap();
        let element_offset = window.element_offset();
        window.defer_draw_with_accessibility(child, element_offset, self.priority)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        // Ancestor semantics exist during paint, after this child was queued
        // during prepaint. Keep only the identity, never its owning view.
        window.capture_deferred_accessibility_context(prepaint);
    }
}

impl IntoElement for Deferred {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Deferred {
    /// Sets a priority for the element. A higher priority conceptually means painting the element
    /// on top of deferred draws with a lower priority (i.e. closer to the viewer).
    pub fn priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::deferred;
    use crate::{
        AccessibilityAction, AccessibilityActionRequest, AccessibilityAttributes, AccessibilityId,
        AccessibilityRole, AnyView, AppContext, Context, Entity, InteractiveElement, IntoElement,
        ParentElement, Render, StyleRefinement, Styled, TestAppContext, Window, div,
        prelude::FluentBuilder, px,
    };
    use std::{cell::Cell, rc::Rc};

    struct DeferredChild {
        renders: Rc<Cell<usize>>,
        actions: Rc<Cell<[usize; 3]>>,
    }

    impl Render for DeferredChild {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let actions = [
                AccessibilityAction::Click,
                AccessibilityAction::Expand,
                AccessibilityAction::Collapse,
            ];
            let mut popup = div().id("deferred-button").size(px(24.0)).accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Button)
                    .label("Popup action")
                    .actions(actions.to_vec()),
            );
            for (index, action) in actions.into_iter().enumerate() {
                popup = popup.on_accessibility_action(
                    action,
                    cx.listener(move |this, _, _, _| {
                        let mut counts = this.actions.get();
                        counts[index] += 1;
                        this.actions.set(counts);
                    }),
                );
            }
            deferred(popup)
        }
    }

    struct DeferredHost {
        child: Entity<DeferredChild>,
        leading_overlay: bool,
    }

    impl Render for DeferredHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let mut cached_style = StyleRefinement::default();
            cached_style.size.width = Some(px(100.0).into());
            cached_style.size.height = Some(px(100.0).into());
            div()
                .when(self.leading_overlay, |this| {
                    // Shift queue positions when the unchanged child reuses its
                    // prepaint. Ownership must not depend on the old index.
                    this.child(deferred(div().absolute().size(px(1.0))))
                })
                .child(
                    div()
                        .id("popup-owner")
                        .size(px(100.0))
                        .accessibility(
                            AccessibilityAttributes::new(AccessibilityRole::Group)
                                .label("Popup owner"),
                        )
                        .child(AnyView::from(self.child.clone()).cached(cached_style)),
                )
        }
    }

    #[crate::test]
    fn deferred_semantic_parent_and_nodes_survive_cached_draw_reuse(cx: &mut TestAppContext) {
        let renders = Rc::new(Cell::new(0));
        let actions = Rc::new(Cell::new([0; 3]));
        let child = cx.new({
            let renders = renders.clone();
            let actions = actions.clone();
            move |_| DeferredChild { renders, actions }
        });
        let (host, window) = cx.add_window_view(move |_, _| DeferredHost {
            child,
            leading_overlay: false,
        });
        let mut previous: Option<(AccessibilityId, AccessibilityId)> = None;
        for frame in 0..3 {
            let identities = window.update(|window, cx| {
                if frame > 0 {
                    host.update(cx, |host, cx| {
                        host.leading_overlay = frame == 1;
                        cx.notify();
                    });
                }
                window.draw(cx).clear();
                let tree = window.accessibility_tree();
                let owner = tree
                    .nodes
                    .values()
                    .find(|node| node.label.as_deref() == Some("Popup owner"))
                    .unwrap();
                let popup = tree
                    .nodes
                    .values()
                    .find(|node| node.label.as_deref() == Some("Popup action"))
                    .expect("deferred semantics must survive cached scene replay");
                assert_eq!(popup.parent, Some(owner.id));
                assert_eq!(owner.children.as_slice(), &[popup.id]);
                (owner.id, popup.id)
            });
            assert_eq!(
                renders.get(),
                1,
                "the popup view must actually reuse its cache"
            );
            if let Some(previous) = previous {
                assert_eq!(identities, previous);
            }
            previous = Some(identities);
        }
        window.update(|window, _| {
            for action in [
                AccessibilityAction::Click,
                AccessibilityAction::Expand,
                AccessibilityAction::Collapse,
            ] {
                let popup = previous.unwrap().1;
                assert!(window.has_accessibility_action_handler(popup, action));
                window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    popup, action,
                ));
            }
        });
        window.run_until_parked();
        assert_eq!(actions.get(), [1, 1, 1]);
    }

    #[test]
    fn deferred_summary_is_content_safe() {
        let overlay = deferred(div().child("private overlay contents")).with_priority(120);

        assert!(overlay.has_child());
        assert_eq!(overlay.priority_value(), 120);
        assert_eq!(overlay.priority_class(), "overlay");

        let summary = overlay.to_text();
        assert!(summary.contains("priority=120"));
        assert!(summary.contains("priority_class=overlay"));
        assert!(!summary.contains("private overlay contents"));
    }
}

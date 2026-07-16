use crate::{
    AccessibilityAttributes, AccessibilityRole, AnyElement, App, Component, ElementId, FocusHandle,
    InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};

/// Construct a label primitive.
#[track_caller]
pub fn label(id: impl Into<ElementId>) -> Label {
    Label::new(id.into())
}

/// A text label primitive that can forward focus to another control when clicked.
pub struct Label {
    element_id: ElementId,
    text: Option<SharedString>,
    target_focus: Option<FocusHandle>,
    children: Vec<AnyElement>,
}

impl Label {
    fn new(element_id: ElementId) -> Self {
        Self {
            element_id,
            text: None,
            target_focus: None,
            children: Vec::new(),
        }
    }

    /// Set the visible text for the label.
    pub fn text(mut self, text: impl Into<SharedString>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Focus the given control when the label is clicked.
    pub fn for_focus_handle(mut self, focus_handle: FocusHandle) -> Self {
        self.target_focus = Some(focus_handle);
        self
    }

    /// Returns true when explicit label text is configured.
    pub fn has_text(&self) -> bool {
        self.text.is_some()
    }

    /// Length of the configured text in bytes, without exposing text.
    pub fn text_len_bytes(&self) -> usize {
        self.text.as_ref().map_or(0, |text| text.len())
    }

    /// Returns true when the label forwards focus to a control.
    pub fn has_target_focus(&self) -> bool {
        self.target_focus.is_some()
    }

    /// Number of custom child elements configured on the label.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "label(has_text={}, text_len_bytes={}, has_target_focus={}, child_count={})",
            self.has_text(),
            self.text_len_bytes(),
            self.has_target_focus(),
            self.child_count()
        )
    }
}

impl ParentElement for Label {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Label {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let Label {
            element_id,
            text,
            target_focus,
            mut children,
        } = self;
        #[cfg(any(test, feature = "test-support"))]
        let label_id = element_id.to_string();

        if children.is_empty() {
            if let Some(text) = text.clone() {
                children.push(div().child(text).into_any_element());
            }
        }

        let mut root = div().id(element_id).accessibility(
            AccessibilityAttributes::new(AccessibilityRole::StaticText).label(
                text.clone()
                    .unwrap_or_else(|| SharedString::from("Label"))
                    .to_string(),
            ),
        );

        if let Some(target_focus) = target_focus {
            root = root.track_focus(&target_focus).cursor_pointer().on_click(
                move |_: &crate::ClickEvent, window: &mut Window, _: &mut App| {
                    window.focus(&target_focus);
                },
            );
        }

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector = format!("label-{}", label_id);
            root = root.debug_selector(move || selector);
        }

        root.children(children)
    }
}

impl IntoElement for Label {
    type Element = Component<Self>;

    fn into_element(self) -> Self::Element {
        Component::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext, Context, Modifiers, Render, Styled, TestAppContext};

    struct LabelView {
        focus_handle: FocusHandle,
    }

    struct CustomLabelView {
        focus_handle: FocusHandle,
    }

    impl Render for LabelView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    label("project_name")
                        .text("Project name")
                        .for_focus_handle(self.focus_handle.clone()),
                )
                .child(
                    div()
                        .track_focus(&self.focus_handle)
                        .debug_selector(|| "label-target".to_string()),
                )
        }
    }

    impl Render for CustomLabelView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            label("custom_label")
                .for_focus_handle(self.focus_handle.clone())
                .child(
                    div()
                        .debug_selector(|| "custom-label-body".to_string())
                        .child("Body"),
                )
        }
    }

    #[crate::test]
    fn label_click_focuses_target(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, cx| LabelView {
            focus_handle: cx.focus_handle(),
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(!view.read(cx).focus_handle.is_focused(window));
        });

        let bounds = window.debug_bounds("label-project_name").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(view.read(cx).focus_handle.is_focused(window));
        });
    }

    #[crate::test]
    fn label_renders_custom_children(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, cx| CustomLabelView {
            focus_handle: cx.focus_handle(),
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window.debug_bounds("custom-label-body").is_some());
    }

    #[crate::test]
    fn label_summary_is_content_safe(cx: &mut TestAppContext) {
        let (view, _window) = cx.add_window_view(|_, cx| LabelView {
            focus_handle: cx.focus_handle(),
        });
        let focus_handle = cx.read_entity(&view, |view, _| view.focus_handle.clone());
        let label = label("secret_label")
            .text("Secret project name")
            .for_focus_handle(focus_handle)
            .child(div().child("Visible child"));

        assert!(label.has_text());
        assert_eq!(label.text_len_bytes(), "Secret project name".len());
        assert!(label.has_target_focus());
        assert_eq!(label.child_count(), 1);

        let summary = label.to_text();
        assert!(summary.contains("has_text=true"));
        assert!(summary.contains("text_len_bytes=19"));
        assert!(summary.contains("has_target_focus=true"));
        assert!(summary.contains("child_count=1"));
        assert!(!summary.contains("Secret"));
        assert!(!summary.contains("project"));
        assert!(!summary.contains("Visible child"));
    }
}

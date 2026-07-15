//! Outline component - structured in-page navigation list.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;

#[derive(Clone)]
pub struct OutlineItem {
    pub id: SharedString,
    pub label: SharedString,
    pub level: usize,
    pub active: bool,
}

impl OutlineItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            level: 0,
            active: false,
        }
    }

    pub fn level(mut self, level: usize) -> Self {
        self.level = level;
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

#[derive(IntoElement)]
pub struct Outline {
    id: ElementId,
    label: SharedString,
    items: Vec<OutlineItem>,
    on_select: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Outline {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "outline:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            label: "On this page".into(),
            items: Vec::new(),
            on_select: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn item(mut self, item: OutlineItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn items(mut self, items: impl IntoIterator<Item = OutlineItem>) -> Self {
        self.items.extend(items);
        self
    }

    pub fn on_select(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl Default for Outline {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Outline {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Outline {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let on_select = self.on_select.clone();
        let user_style = self.style;
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);

        div()
            .id(self.id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::List).label(self.label.to_string()),
            )
            .tab_group()
            .flex()
            .flex_row()
            .gap(px(2.0))
            .child(
                div()
                    .relative()
                    .w(px(2.0))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(theme.tokens.border),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .flex_1()
                    .min_w(px(0.0))
                    .children(self.items.into_iter().map(move |item| {
                        let id = item.id.clone();
                        let item_element_id =
                            ElementId::NamedChild(Box::new(self.id.clone()), item.id.clone());
                        let on_select = on_select.clone();
                        let interactive = on_select.is_some();
                        let mut accessibility = AccessibilityAttributes::new(if interactive {
                            AccessibilityRole::Link
                        } else {
                            AccessibilityRole::ListItem
                        })
                        .label(item.label.to_string())
                        .states(if item.active {
                            AccessibilityState::SELECTED
                        } else {
                            AccessibilityState::NONE
                        });
                        if interactive {
                            accessibility = accessibility.actions(vec![
                                AccessibilityAction::Focus,
                                AccessibilityAction::Click,
                            ]);
                        }
                        let indent = match item.level {
                            0..=2 => px(12.0),
                            3 => px(28.0),
                            4 => px(44.0),
                            _ => px(48.0),
                        };

                        div()
                            .id(item_element_id)
                            .accessibility(accessibility)
                            .flex()
                            .items_center()
                            .h(px(36.0))
                            .pl(indent)
                            .pr(px(8.0))
                            .rounded(theme.tokens.radius_md)
                            .text_size(px(14.0))
                            .line_height(px(20.0))
                            .font_weight(if item.active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if item.active {
                                theme.tokens.foreground
                            } else {
                                theme.tokens.muted_foreground
                            })
                            .when(interactive, |this| {
                                this.focusable()
                                    .tab_index(0)
                                    .tab_stop(true)
                                    .cursor_pointer()
                                    .hover(move |style| {
                                        style.bg(overlay_hover).text_color(theme.tokens.foreground)
                                    })
                                    .focus_visible(move |style| {
                                        style
                                            .bg(overlay_hover)
                                            .text_color(theme.tokens.foreground)
                                            .shadow(smallvec::smallvec![
                                                crate::astryx::focus_ring_outer(theme.tokens.ring)
                                            ])
                                    })
                            })
                            .child(
                                div()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(item.label),
                            )
                            .when_some(on_select, |this, handler| {
                                let key_handler = handler.clone();
                                let key_id = id.clone();
                                this.on_click(move |_event, window, cx| {
                                    handler(id.clone(), window, cx);
                                })
                                .on_key_down(
                                    move |event, window, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            key_handler(key_id.clone(), window, cx);
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                    },
                                )
                            })
                    })),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

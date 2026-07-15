//! # Radio and RadioGroup Components
//!
//! Radio button components for single-selection input within a group.
//! Follows shadcn/ui design patterns with focus rings and accessibility support.
//! ## Components
//!
//! - `Radio`: Individual radio button with label
//! - `RadioGroup`: Container managing radio button selection state
//!
//! ## Features
//!
//! - Single selection within a group
//! - Focus ring on keyboard navigation
//! - Disabled state support
//! - Horizontal and vertical layouts
//! - Accessibility with proper ARIA attributes
//! - Theme-integrated styling with shadows
//!
//! ## Design Decisions
//!
//! - Uses primary color for selected state
//! - Inner circle indicator for selection
//! - Focus ring follows our theme system (3px spread)
//! - Supports keyboard navigation with tab stops
//! - RadioGroup automatically manages checked state
//!

use crate::{
    components::{
        checkbox::CheckboxSize,
        field::{Field, FieldStatusType},
        field_status::FieldStatusVariant,
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

/// Layout direction for RadioGroup
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RadioLayout {
    /// Vertical stack (default)
    #[default]
    Vertical,
    /// Horizontal row
    Horizontal,
}

/// Individual radio button component
#[derive(IntoElement)]
pub struct Radio {
    base: Stateful<Div>,
    id: ElementId,
    label: Option<SharedString>,
    checked: bool,
    disabled: bool,
    size: CheckboxSize,
    on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Radio {
    /// Create a new radio button
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            base: div().id(id),
            label: None,
            checked: false,
            disabled: false,
            size: CheckboxSize::Md,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    /// Set the label text
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set checked state
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Set disabled state
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn size(mut self, size: CheckboxSize) -> Self {
        self.size = size;
        self
    }

    /// Set click handler
    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl InteractiveElement for Radio {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Radio {}

impl Styled for Radio {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Radio {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;

        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let is_focused = focus_handle.is_focused(window);
        let focus_on_mouse = focus_handle.clone();
        let accessibility_label = self
            .label
            .clone()
            .unwrap_or_else(|| "Radio option".into())
            .to_string();

        let tokens = &Theme::of(cx).tokens;
        let primary = tokens.primary;
        let input = tokens.input;
        let card = tokens.card;
        let muted_foreground = tokens.muted_foreground;
        let foreground = tokens.foreground;
        let font_family = tokens.font_family.clone();
        let radius_md = tokens.radius_md;
        let transition_fast = tokens.transition_fast;
        let focus_ring = crate::astryx::focus_ring(primary);
        let hover_ring = crate::astryx::input_hover_ring(input);

        let (border_color, bg, dot_color) = if self.checked {
            (primary, primary, tokens.primary_foreground)
        } else {
            (input, card, kael::transparent_black())
        };

        let (border_color, bg) = if self.disabled {
            (border_color.opacity(0.5), bg.opacity(0.5))
        } else {
            (border_color, bg)
        };
        let (wrapper_size, circle_size, dot_size) = match self.size {
            CheckboxSize::Sm => (px(20.0), px(18.0), px(8.0)),
            CheckboxSize::Md => (px(24.0), px(22.0), px(10.0)),
        };

        self.base
            .accessibility(
                AccessibilityAttributes::radio_button(accessibility_label, self.checked)
                    .disabled(self.disabled)
                    .focused(is_focused),
            )
            .when(!self.disabled, |this| {
                this.track_focus(&focus_handle.tab_index(0).tab_stop(true))
            })
            .flex()
            .gap(px(8.0))
            .items_center()
            .text_sm()
            .font_family(font_family.clone())
            .text_color(if self.disabled {
                muted_foreground
            } else {
                foreground
            })
            .rounded(radius_md)
            .child(
                div()
                    .id(ElementId::Name(format!("{}-wrapper", self.id).into()))
                    .size(wrapper_size)
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .rounded_full()
                    .when(is_focused && !self.disabled, |this| {
                        this.shadow(smallvec::smallvec![focus_ring])
                    })
                    .child(
                        div()
                            .id(ElementId::Name(format!("{}-circle", self.id).into()))
                            .size(circle_size)
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .border_1()
                            .border_color(border_color)
                            .bg(bg)
                            .transition(transition_fast)
                            .when(!self.disabled && !self.checked, |this| {
                                this.hover(move |style| {
                                    style.shadow(smallvec::smallvec![hover_ring])
                                })
                            })
                            .when(self.checked, |this| {
                                this.child(div().size(dot_size).rounded_full().bg(dot_color))
                            }),
                    ),
            )
            .when_some(self.label, |this, label| {
                this.child(div().line_height(relative(1.0)).child(label))
            })
            .when(!self.disabled, |this| {
                this.cursor(CursorStyle::PointingHand)
                    .on_mouse_down(MouseButton::Left, move |_, window, _| {
                        window.prevent_default();
                        window.focus(&focus_on_mouse);
                    })
                    .when_some(self.on_click, |this, handler| {
                        let handler_for_key = handler.clone();
                        this.on_click(move |_, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            handler(window, cx);
                        })
                        .on_key_down(
                            move |event: &KeyDownEvent, window, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    handler_for_key(window, cx);
                                    cx.stop_propagation();
                                    window.prevent_default();
                                }
                            },
                        )
                    })
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

/// Radio group container managing selection state
///
/// # Example
///
/// ```rust,ignore
/// RadioGroup::new("theme-selection")
///     .selected_index(Some(0))
///     .on_change(|index, window, cx| {
///         println!("Selected: {}", index);
///     })
///     .child(Radio::new("light").label("Light"))
///     .child(Radio::new("dark").label("Dark"))
///     .child(Radio::new("system").label("System"))
/// ```
#[derive(IntoElement)]
pub struct RadioGroup {
    id: ElementId,
    radios: Vec<Radio>,
    layout: RadioLayout,
    selected_index: Option<usize>,
    disabled: bool,
    on_change: Option<Rc<dyn Fn(&usize, &mut Window, &mut App)>>,
}

impl RadioGroup {
    /// Create a new radio group
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            radios: Vec::new(),
            layout: RadioLayout::default(),
            selected_index: None,
            disabled: false,
            on_change: None,
        }
    }

    /// Create a vertical radio group
    pub fn vertical(id: impl Into<ElementId>) -> Self {
        Self::new(id)
    }

    /// Create a horizontal radio group
    pub fn horizontal(id: impl Into<ElementId>) -> Self {
        Self::new(id).layout(RadioLayout::Horizontal)
    }

    /// Set the layout direction
    pub fn layout(mut self, layout: RadioLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Set the selected radio index
    pub fn selected_index(mut self, index: Option<usize>) -> Self {
        self.selected_index = index;
        self
    }

    /// Set disabled state for all radios
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set change handler
    pub fn on_change(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    /// Add a child radio
    pub fn child(mut self, child: impl Into<Radio>) -> Self {
        self.radios.push(child.into());
        self
    }

    /// Add multiple child radios
    pub fn children(mut self, children: impl IntoIterator<Item = impl Into<Radio>>) -> Self {
        self.radios.extend(children.into_iter().map(Into::into));
        self
    }
}

impl RenderOnce for RadioGroup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let on_change = self.on_change;
        let disabled = self.disabled;
        let selected_ix = self.selected_index;

        div()
            .id(self.id)
            .flex()
            .when(self.layout == RadioLayout::Vertical, |this| this.flex_col())
            .when(self.layout == RadioLayout::Horizontal, |this| {
                this.flex_row().flex_wrap()
            })
            .gap(px(12.0))
            .children(self.radios.into_iter().enumerate().map(|(ix, radio)| {
                let checked = selected_ix == Some(ix);
                radio.checked(checked).disabled(disabled).when_some(
                    on_change.clone(),
                    |this, on_change| {
                        this.on_click(move |window, cx| {
                            on_change(&ix, window, cx);
                        })
                    },
                )
            }))
    }
}

#[derive(Clone)]
pub struct RadioListItem {
    pub value: SharedString,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub end_content: Option<SharedString>,
    pub disabled: bool,
}

impl RadioListItem {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            description: None,
            end_content: None,
            disabled: false,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn end_content(mut self, content: impl Into<SharedString>) -> Self {
        self.end_content = Some(content.into());
        self
    }

    #[allow(non_snake_case)]
    pub fn endContent(self, content: impl Into<SharedString>) -> Self {
        self.end_content(content)
    }
}

#[derive(IntoElement)]
pub struct RadioList {
    label: SharedString,
    items: Vec<RadioListItem>,
    selected_value: Option<SharedString>,
    orientation: RadioLayout,
    size: CheckboxSize,
    description: Option<SharedString>,
    status: Option<(FieldStatusType, SharedString)>,
    disabled: bool,
    required: bool,
    optional: bool,
    hidden_label: bool,
    width: Option<Pixels>,
    on_change: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl RadioList {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            items: Vec::new(),
            selected_value: None,
            orientation: RadioLayout::Vertical,
            size: CheckboxSize::Md,
            description: None,
            status: None,
            disabled: false,
            required: false,
            optional: false,
            hidden_label: false,
            width: None,
            on_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn item(mut self, item: RadioListItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn items(mut self, items: impl IntoIterator<Item = RadioListItem>) -> Self {
        self.items.extend(items);
        self
    }

    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.selected_value = Some(value.into());
        self
    }

    pub fn selected_value(self, value: impl Into<SharedString>) -> Self {
        self.value(value)
    }

    pub fn orientation(mut self, orientation: RadioLayout) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn layout(self, layout: RadioLayout) -> Self {
        self.orientation(layout)
    }

    pub fn size(mut self, size: CheckboxSize) -> Self {
        self.size = size;
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.status = Some((status, message.into()));
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    #[allow(non_snake_case)]
    pub fn isRequired(self, required: bool) -> Self {
        self.required(required)
    }

    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    #[allow(non_snake_case)]
    pub fn isOptional(self, optional: bool) -> Self {
        self.optional(optional)
    }

    pub fn hidden_label(mut self, hidden: bool) -> Self {
        self.hidden_label = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.hidden_label(hidden)
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for RadioList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for RadioList {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let selected_value = self.selected_value.clone();
        let disabled = self.disabled;
        let size = self.size;
        let on_change = self.on_change.clone();
        let list = div()
            .flex()
            .when(self.orientation == RadioLayout::Vertical, |this| {
                this.flex_col().gap(px(8.0))
            })
            .when(self.orientation == RadioLayout::Horizontal, |this| {
                this.flex_row().flex_wrap().gap(px(20.0))
            })
            .children(self.items.into_iter().map(move |item| {
                let is_disabled = disabled || item.disabled;
                let checked = selected_value.as_ref() == Some(&item.value);
                let item_value = item.value.clone();
                let on_change = on_change.clone();

                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(theme.tokens.radius_md)
                    .when(!is_disabled, |this| this.cursor_pointer())
                    .child(
                        Radio::new(ElementId::Name(item.value.clone()))
                            .checked(checked)
                            .disabled(is_disabled)
                            .size(size)
                            .on_click(move |window, cx| {
                                if let Some(handler) = on_change.as_ref() {
                                    handler(item_value.clone(), window, cx);
                                }
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .line_height(px(20.0))
                                    .text_color(if is_disabled {
                                        theme.tokens.muted_foreground
                                    } else {
                                        theme.tokens.foreground
                                    })
                                    .child(item.label),
                            )
                            .when_some(item.description, |this, description| {
                                this.child(
                                    div()
                                        .text_size(px(12.0))
                                        .line_height(px(16.0))
                                        .text_color(theme.tokens.muted_foreground)
                                        .child(description),
                                )
                            }),
                    )
                    .when_some(item.end_content, |this, content| {
                        this.child(
                            div()
                                .ml_auto()
                                .text_size(px(12.0))
                                .line_height(px(16.0))
                                .text_color(theme.tokens.muted_foreground)
                                .child(content),
                        )
                    })
            }));

        let field = Field::new(self.label, list)
            .hidden_label(self.hidden_label)
            .optional(self.optional)
            .required(self.required)
            .disabled(disabled)
            .status_variant(FieldStatusVariant::Detached)
            .when_some(self.description, |field, description| {
                field.description(description)
            })
            .when_some(self.status, |field, (status, message)| {
                field.status(status, message)
            })
            .when_some(self.width, |field, width| field.width(width));

        field.map(|this| {
            let mut field = this;
            field.style().refine(&self.style);
            field
        })
    }
}

// Convenience From implementations
impl From<&'static str> for Radio {
    fn from(label: &'static str) -> Self {
        Self::new(label).label(label)
    }
}

impl From<SharedString> for Radio {
    fn from(label: SharedString) -> Self {
        Self::new(label.clone()).label(label)
    }
}

impl From<String> for Radio {
    fn from(label: String) -> Self {
        let shared: SharedString = label.into();
        Self::new(shared.clone()).label(shared)
    }
}

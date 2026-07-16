//! Toggle group component - Grouped toggle buttons for toolbars and view switchers.

use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;

use crate::{
    components::{
        button::{Button, ButtonColors, ButtonSize, ButtonVariant},
        icon_source::IconSource,
    },
    theme::Theme,
};

#[derive(IntoElement)]
pub struct ToggleButton {
    id: ElementId,
    label: SharedString,
    pressed: bool,
    disabled: bool,
    loading: bool,
    icon_only: bool,
    size: ButtonSize,
    icon: Option<IconSource>,
    pressed_icon: Option<IconSource>,
    tooltip: Option<SharedString>,
    on_pressed_change: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl ToggleButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            pressed: false,
            disabled: false,
            loading: false,
            icon_only: false,
            size: ButtonSize::Md,
            icon: None,
            pressed_icon: None,
            tooltip: None,
            on_pressed_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    pub fn is_pressed(self, pressed: bool) -> Self {
        self.pressed(pressed)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn icon_only(mut self, icon_only: bool) -> Self {
        self.icon_only = icon_only;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn pressed_icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.pressed_icon = Some(icon.into());
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn on_pressed_change(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_pressed_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for ToggleButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ToggleButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let dark = theme.tokens.background.l < 0.5;
        let pressed_bg = crate::astryx::overlay_pressed(dark);
        let hover_bg = crate::astryx::overlay_hover(dark);
        let icon = if self.pressed {
            self.pressed_icon.clone().or(self.icon.clone())
        } else {
            self.icon.clone()
        };

        let mut button = Button::new(self.id, self.label.clone())
            .variant(ButtonVariant::Ghost)
            .pressed(self.pressed)
            .size(if self.icon_only {
                ButtonSize::Icon
            } else {
                self.size
            })
            .disabled(self.disabled)
            .loading(self.loading)
            .colors(if self.pressed {
                ButtonColors {
                    background: pressed_bg,
                    foreground: theme.tokens.foreground,
                    border: kael::transparent_black(),
                    hover_background: hover_bg,
                    hover_foreground: theme.tokens.foreground,
                    has_shadow: false,
                    has_border: false,
                }
            } else {
                ButtonColors {
                    background: kael::transparent_black(),
                    foreground: theme.tokens.foreground,
                    border: kael::transparent_black(),
                    hover_background: hover_bg,
                    hover_foreground: theme.tokens.foreground,
                    has_shadow: false,
                    has_border: false,
                }
            });

        if let Some(icon) = icon {
            button = button.icon(icon);
        }
        if let Some(tooltip) = self.tooltip {
            button = button.tooltip(tooltip);
        }
        if let Some(handler) = self.on_pressed_change {
            let next_pressed = !self.pressed;
            button = button.on_click(move |_, window, cx| {
                handler(next_pressed, window, cx);
            });
        }
        button.style().refine(&self.style);
        button
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ToggleGroupVariant {
    #[default]
    Single,
    Multiple,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ToggleGroupSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl ToggleGroupSize {
    fn height(&self) -> Pixels {
        match self {
            Self::Sm => px(28.0),
            Self::Md => px(32.0),
            Self::Lg => px(36.0),
        }
    }

    fn px_value(&self) -> Pixels {
        match self {
            Self::Sm => px(8.0),
            Self::Md => px(12.0),
            Self::Lg => px(16.0),
        }
    }

    fn text_size(&self) -> Pixels {
        match self {
            Self::Sm => px(12.0),
            Self::Md => px(14.0),
            Self::Lg => px(16.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToggleGroupItem {
    value: SharedString,
    label: SharedString,
    icon: Option<SharedString>,
    disabled: bool,
}

impl ToggleGroupItem {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            icon: None,
            disabled: false,
        }
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

fn toggled_values(values: &[SharedString], value: &SharedString) -> Vec<SharedString> {
    let mut next_values = values.to_vec();
    if let Some(position) = next_values.iter().position(|selected| selected == value) {
        next_values.remove(position);
    } else {
        next_values.push(value.clone());
    }
    next_values
}

#[derive(IntoElement)]
pub struct ToggleGroup {
    id: ElementId,
    variant: ToggleGroupVariant,
    size: ToggleGroupSize,
    items: Vec<ToggleGroupItem>,
    value: Option<SharedString>, // For single selection
    values: Vec<SharedString>,   // For multiple selection
    disabled: bool,
    on_change: Option<Rc<dyn Fn(&SharedString, &mut Window, &mut App)>>,
    on_multiple_change: Option<Rc<dyn Fn(&Vec<SharedString>, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl ToggleGroup {
    /// Create a new toggle group
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "toggle-group:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            variant: ToggleGroupVariant::default(),
            size: ToggleGroupSize::default(),
            items: Vec::new(),
            value: None,
            values: Vec::new(),
            disabled: false,
            on_change: None,
            on_multiple_change: None,
            style: StyleRefinement::default(),
        }
    }

    /// Set the selection behavior variant
    pub fn variant(mut self, variant: ToggleGroupVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set the size of toggle buttons
    pub fn size(mut self, size: ToggleGroupSize) -> Self {
        self.size = size;
        self
    }

    /// Add an item to the group
    pub fn item(mut self, item: ToggleGroupItem) -> Self {
        self.items.push(item);
        self
    }

    /// Add multiple items to the group
    pub fn items(mut self, items: Vec<ToggleGroupItem>) -> Self {
        self.items.extend(items);
        self
    }

    /// Set the selected value (for single selection)
    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Set the selected values (for multiple selection)
    pub fn values(mut self, values: Vec<SharedString>) -> Self {
        self.values = values;
        self
    }

    /// Set whether the entire group is disabled
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set the change handler for single selection
    pub fn on_change<F>(mut self, handler: F) -> Self
    where
        F: Fn(&SharedString, &mut Window, &mut App) + 'static,
    {
        self.on_change = Some(Rc::new(handler));
        self
    }

    /// Set the change handler for multiple selection
    pub fn on_multiple_change<F>(mut self, handler: F) -> Self
    where
        F: Fn(&Vec<SharedString>, &mut Window, &mut App) + 'static,
    {
        self.on_multiple_change = Some(Rc::new(handler));
        self
    }
}

impl Default for ToggleGroup {
    #[track_caller]
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for ToggleGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ToggleGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);

        let variant = self.variant;
        let current_value = self.value;
        let current_values = self.values;
        let disabled = self.disabled;
        let size = self.size;
        let on_change = self.on_change;
        let on_multiple_change = self.on_multiple_change;
        let user_style = self.style;
        let id = self.id;
        let id_key = id.to_string();

        div()
            .id(id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Toolbar).label("Toggle group"),
            )
            .tab_group()
            .flex()
            .items_center()
            .gap(px(2.0))
            .p(px(2.0))
            .bg(theme.tokens.muted)
            .rounded(theme.tokens.radius_md)
            .children(
                self.items
                    .into_iter()
                    .enumerate()
                    .map(move |(index, item)| {
                        let is_selected = match variant {
                            ToggleGroupVariant::Single => {
                                current_value.as_ref() == Some(&item.value)
                            }
                            ToggleGroupVariant::Multiple => current_values.contains(&item.value),
                        };
                        let has_handler = match variant {
                            ToggleGroupVariant::Single => on_change.is_some(),
                            ToggleGroupVariant::Multiple => on_multiple_change.is_some(),
                        };
                        let is_disabled = disabled || item.disabled || !has_handler;
                        let value = item.value.clone();
                        let single_handler = on_change.clone();
                        let multiple_handler = on_multiple_change.clone();
                        let selected_values = current_values.clone();
                        let icon = item.icon.clone();
                        let label = item.label.clone();
                        let mut button = Button::new(
                            ElementId::Name(format!("{id_key}-item-{index}").into()),
                            label,
                        )
                        .variant(ButtonVariant::Ghost)
                        .pressed(is_selected)
                        .selected(is_selected)
                        .disabled(is_disabled)
                        .when(has_handler, |this| {
                            this.on_click(move |_, window, cx| match variant {
                                ToggleGroupVariant::Single => {
                                    if let Some(handler) = &single_handler {
                                        handler(&value, window, cx);
                                    }
                                }
                                ToggleGroupVariant::Multiple => {
                                    if let Some(handler) = &multiple_handler {
                                        let next_values = toggled_values(&selected_values, &value);
                                        handler(&next_values, window, cx);
                                    }
                                }
                            })
                        });
                        if let Some(icon) = icon {
                            button = button.icon(IconSource::Named(icon.to_string()));
                        }
                        button.style().refine(
                            &StyleRefinement::default()
                                .h(size.height())
                                .px(size.px_value())
                                .text_size(size.text_size()),
                        );
                        button
                    }),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[cfg(test)]
mod tests {
    use super::toggled_values;
    use kael::SharedString;

    #[test]
    fn multiple_selection_adds_and_removes_values() {
        let alpha = SharedString::from("alpha");
        let beta = SharedString::from("beta");

        assert_eq!(
            toggled_values(std::slice::from_ref(&alpha), &beta),
            vec![alpha.clone(), beta]
        );
        assert!(toggled_values(std::slice::from_ref(&alpha), &alpha).is_empty());
    }
}

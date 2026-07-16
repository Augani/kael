//! Tab navigation component with multiple visual variants.

use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::sync::Arc;

actions!(tabs, [TabNext, TabPrevious, TabFirst, TabLast, TabClose]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabVariant {
    #[default]
    Underline,
    Enclosed,
    Pills,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabsSize {
    Sm,
    #[default]
    Md,
    Lg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabsLayout {
    #[default]
    Hug,
    Fill,
}

impl TabsSize {
    fn padding_x(self) -> Pixels {
        match self {
            Self::Sm => px(10.0),
            Self::Md => px(12.0),
            Self::Lg => px(14.0),
        }
    }

    fn height(self) -> Pixels {
        match self {
            Self::Sm => px(28.0),
            Self::Md => px(32.0),
            Self::Lg => px(36.0),
        }
    }

    fn text_size(self) -> Pixels {
        match self {
            Self::Sm => px(12.0),
            Self::Md => px(14.0),
            Self::Lg => px(15.0),
        }
    }

    fn icon_size(self) -> Pixels {
        match self {
            Self::Sm => px(14.0),
            Self::Md => px(16.0),
            Self::Lg => px(18.0),
        }
    }
}

fn adjacent_enabled_index(indices: &[usize], current: usize, forward: bool) -> usize {
    if indices.is_empty() {
        return current;
    }
    let position = indices
        .iter()
        .position(|candidate| *candidate == current)
        .unwrap_or(0);
    let target = if forward {
        (position + 1) % indices.len()
    } else {
        position.checked_sub(1).unwrap_or(indices.len() - 1)
    };
    indices[target]
}

#[derive(Clone)]
pub struct TabItem<T: Clone> {
    pub id: T,
    pub label: SharedString,
    pub icon: Option<IconSource>,
    pub badge: Option<SharedString>,
    pub disabled: bool,
    pub closeable: bool,
}

impl<T: Clone> TabItem<T> {
    pub fn new(id: impl Into<T>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            badge: None,
            disabled: false,
            closeable: false,
        }
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn closeable(mut self, closeable: bool) -> Self {
        self.closeable = closeable;
        self
    }
}

pub struct TabPanel {
    content: Box<dyn Fn() -> AnyElement + Send + Sync>,
}

impl TabPanel {
    pub fn new<F, E>(render_fn: F) -> Self
    where
        F: Fn() -> E + Send + Sync + 'static,
        E: IntoElement,
    {
        Self {
            content: Box::new(move || render_fn().into_any_element()),
        }
    }

    fn render(&self) -> AnyElement {
        (self.content)()
    }
}

#[derive(IntoElement)]
pub struct Tabs<T: Clone + PartialEq + 'static> {
    id: ElementId,
    tabs: Vec<TabItem<T>>,
    panels: Vec<TabPanel>,
    selected_index: Option<usize>,
    controlled: bool,
    variant: TabVariant,
    size: TabsSize,
    layout: TabsLayout,
    has_divider: bool,
    on_change: Option<Arc<dyn Fn(&usize, &mut Window, &mut App) + Send + Sync + 'static>>,
    on_close: Option<Arc<dyn Fn(&T, &mut Window, &mut App) + Send + Sync + 'static>>,
    style: StyleRefinement,
}

impl<T: Clone + PartialEq + 'static> Default for Tabs<T> {
    #[track_caller]
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + 'static> Tabs<T> {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "tabs:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            tabs: Vec::new(),
            panels: Vec::new(),
            selected_index: Some(0),
            controlled: false,
            variant: TabVariant::default(),
            size: TabsSize::default(),
            layout: TabsLayout::default(),
            has_divider: false,
            on_change: None,
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn tabs(mut self, tabs: Vec<TabItem<T>>) -> Self {
        self.tabs = tabs;
        if self.tabs.is_empty() {
            self.selected_index = None;
        } else if let Some(index) = self.selected_index {
            if index >= self.tabs.len() {
                self.selected_index = Some(self.tabs.len().saturating_sub(1));
            }
        }
        self
    }

    pub fn panels(mut self, panels: Vec<TabPanel>) -> Self {
        self.panels = panels;
        self
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn variant(mut self, variant: TabVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: TabsSize) -> Self {
        self.size = size;
        self
    }

    pub fn layout(mut self, layout: TabsLayout) -> Self {
        self.layout = layout;
        self
    }

    pub fn has_divider(mut self, has_divider: bool) -> Self {
        self.has_divider = has_divider;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasDivider(self, has_divider: bool) -> Self {
        self.has_divider(has_divider)
    }

    pub fn selected_index(mut self, index: usize) -> Self {
        self.selected_index = Some(index.min(self.tabs.len().saturating_sub(1)));
        self.controlled = true;
        self
    }

    pub fn selected_id(mut self, id: T) -> Self {
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
            self.selected_index = Some(index);
            self.controlled = true;
        }
        self
    }

    pub fn on_change<F>(mut self, f: F) -> Self
    where
        F: Fn(&usize, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_change = Some(Arc::new(f));
        self
    }

    pub fn on_close<F>(mut self, f: F) -> Self
    where
        F: Fn(&T, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_close = Some(Arc::new(f));
        self
    }

    pub fn selected_tab_id(&self) -> Option<&T> {
        self.selected_index
            .and_then(|index| self.tabs.get(index))
            .map(|tab| &tab.id)
    }

    fn render_tab_button(
        variant: TabVariant,
        size: TabsSize,
        layout: TabsLayout,
        tab: &TabItem<T>,
        index: usize,
        is_active: bool,
        tab_id: ElementId,
        previous_index: usize,
        next_index: usize,
        first_index: usize,
        last_index: usize,
        theme: &crate::theme::Theme,
        on_change: Option<Arc<dyn Fn(&usize, &mut Window, &mut App) + Send + Sync + 'static>>,
        on_close: Option<Arc<dyn Fn(&T, &mut Window, &mut App) + Send + Sync + 'static>>,
    ) -> impl IntoElement {
        let can_select = !tab.disabled && on_change.is_some();
        let mut state = AccessibilityState::NONE;
        if is_active {
            state |= AccessibilityState::SELECTED;
        }
        if tab.disabled || !can_select {
            state |= AccessibilityState::DISABLED;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Tab)
            .label(tab.label.to_string())
            .states(state);
        if can_select {
            accessibility =
                accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        let base = div()
            .id(tab_id.clone())
            .accessibility(accessibility)
            .flex()
            .items_center()
            .justify_center()
            .relative()
            .gap(px(4.0))
            .h(size.height())
            .px(size.padding_x())
            .text_size(size.text_size())
            .line_height(px(20.0))
            .font_family(theme.tokens.font_family.clone())
            .rounded(theme.tokens.radius_md)
            .transition(theme.tokens.transition_fast)
            .when(layout == TabsLayout::Fill, |this| this.flex_1())
            .cursor(if can_select {
                CursorStyle::PointingHand
            } else {
                CursorStyle::Arrow
            });

        let styled = match variant {
            TabVariant::Underline => base
                .font_weight(if is_active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if tab.disabled {
                    theme.tokens.muted_foreground
                } else if is_active {
                    theme.tokens.foreground
                } else {
                    theme.tokens.muted_foreground
                })
                .border_b_2()
                .border_color(if is_active {
                    theme.tokens.primary
                } else {
                    kael::transparent_black()
                })
                .when(!tab.disabled && !is_active, |div| {
                    div.hover(|style| {
                        style
                            .bg(crate::astryx::overlay_hover(
                                theme.tokens.background.l < 0.5,
                            ))
                            .text_color(theme.tokens.foreground)
                    })
                }),

            TabVariant::Enclosed => base
                .border_1()
                .border_color(if is_active {
                    theme.tokens.border
                } else {
                    kael::transparent_black()
                })
                .rounded_tl(theme.tokens.radius_md)
                .rounded_tr(theme.tokens.radius_md)
                .bg(if is_active {
                    theme.tokens.background
                } else {
                    theme.tokens.muted
                })
                .text_color(if is_active {
                    theme.tokens.foreground
                } else {
                    theme.tokens.muted_foreground
                })
                .when(!tab.disabled && !is_active, |div| {
                    div.hover(|mut style| {
                        style.background = Some(theme.tokens.accent.into());
                        style
                    })
                }),

            TabVariant::Pills => base
                .rounded(theme.tokens.radius_md)
                .bg(if is_active {
                    theme.tokens.primary
                } else if tab.disabled {
                    kael::transparent_black()
                } else {
                    theme.tokens.muted
                })
                .text_color(if is_active {
                    theme.tokens.primary_foreground
                } else if tab.disabled {
                    theme.tokens.muted_foreground
                } else {
                    theme.tokens.foreground
                })
                .when(!tab.disabled && !is_active, |div| {
                    div.hover(|mut style| {
                        style.background = Some(theme.tokens.accent.into());
                        style
                    })
                }),
        };

        let with_icon = styled.when_some(tab.icon.as_ref(), |div, icon| {
            div.child(Icon::new(icon.clone()).size(size.icon_size()).color(
                if is_active && variant == TabVariant::Pills {
                    theme.tokens.primary_foreground
                } else if is_active {
                    theme.tokens.foreground
                } else {
                    theme.tokens.muted_foreground
                },
            ))
        });

        // The tab itself owns the accessible name. Keep the visible label out of
        // the accessibility tree so screen readers do not announce every tab
        // twice (once for the tab node and again for its text child).
        let with_label = with_icon
            .child(div().child(StyledText::new(tab.label.clone()).accessibility_hidden(true)));

        let with_badge = with_label.when_some(tab.badge.as_ref(), |parent, badge| {
            parent.child(
                div()
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(px(10.0))
                    .bg(if is_active && variant == TabVariant::Pills {
                        theme.tokens.primary_foreground.opacity(0.2)
                    } else {
                        theme.tokens.muted
                    })
                    .text_size(px(11.0))
                    .font_family(theme.tokens.font_family.clone())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if is_active && variant == TabVariant::Pills {
                        theme.tokens.primary_foreground
                    } else {
                        theme.tokens.muted_foreground
                    })
                    .child(badge.clone()),
            )
        });

        let with_close = with_badge.when(tab.closeable, |parent| {
            let close_id = ElementId::NamedChild(Box::new(tab_id.clone()), "close".into());
            let can_close = !tab.disabled && on_close.is_some();
            let mut close_accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
                .label(format!("Close {}", tab.label));
            if can_close {
                close_accessibility = close_accessibility
                    .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
            } else {
                close_accessibility = close_accessibility.states(AccessibilityState::DISABLED);
            }
            parent.child(
                div()
                    .id(close_id)
                    .accessibility(close_accessibility)
                    .ml(px(4.0))
                    .p(px(2.0))
                    .rounded(theme.tokens.radius_sm)
                    .when(can_close, |this| {
                        this.focusable()
                            .tab_index(0)
                            .tab_stop(true)
                            .cursor(CursorStyle::PointingHand)
                            .hover(|mut style| {
                                style.background =
                                    Some(if is_active && variant == TabVariant::Pills {
                                        theme.tokens.primary_foreground.opacity(0.2).into()
                                    } else {
                                        theme.tokens.muted.into()
                                    });
                                style
                            })
                            .focus_visible(|style| style.bg(theme.tokens.muted))
                    })
                    .when(!can_close, |this| this.opacity(0.5))
                    .when_some(on_close.clone().filter(|_| can_close), |this, on_close| {
                        let on_key = on_close.clone();
                        let tab_id_for_click = tab.id.clone();
                        let tab_id_for_key = tab.id.clone();
                        this.on_click(move |_, window, cx| {
                            on_close(&tab_id_for_click, window, cx);
                            cx.stop_propagation();
                        })
                        .on_key_down(move |event, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                on_key(&tab_id_for_key, window, cx);
                                cx.stop_propagation();
                                window.prevent_default();
                            }
                        })
                    })
                    .child(Icon::new("x").size(px(12.0)).color(
                        if is_active && variant == TabVariant::Pills {
                            theme.tokens.primary_foreground
                        } else {
                            theme.tokens.muted_foreground
                        },
                    )),
            )
        });

        with_close.when(can_select, |this| {
            let on_click = on_change.clone().unwrap();
            let on_key = on_click.clone();
            this.focusable()
                .tab_index(if is_active { 0 } else { -1 })
                .tab_stop(is_active)
                .focus_visible(|style| style.bg(theme.tokens.muted))
                .on_click(move |_, window, cx| {
                    on_click(&index, window, cx);
                })
                .on_key_down(move |event, window, cx| {
                    let target = match event.keystroke.key.as_str() {
                        "enter" | "space" => Some(index),
                        "arrowleft" => Some(previous_index),
                        "arrowright" => Some(next_index),
                        "home" => Some(first_index),
                        "end" => Some(last_index),
                        _ => None,
                    };
                    if let Some(target) = target {
                        on_key(&target, window, cx);
                        cx.stop_propagation();
                        window.prevent_default();
                    }
                })
        })
    }
}

impl<T: Clone + PartialEq + 'static> Styled for Tabs<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + PartialEq + 'static> RenderOnce for Tabs<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;

        if self.tabs.is_empty() {
            return div()
                .id(self.id)
                .accessibility(AccessibilityAttributes::new(AccessibilityRole::Group).label("Tabs"))
                .child("No tabs")
                .into_any_element();
        }

        let tabs_id = self.id;
        let enabled_indices: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| (!tab.disabled).then_some(index))
            .collect();
        let first_index = enabled_indices.first().copied().unwrap_or(0);
        let last_index = enabled_indices.last().copied().unwrap_or(0);
        let initial_index = self
            .selected_index
            .filter(|index| enabled_indices.contains(index))
            .or_else(|| enabled_indices.first().copied());
        let selected_state = window.use_keyed_state(
            ElementId::NamedChild(Box::new(tabs_id.clone()), "selection".into()),
            cx,
            move |_, _| initial_index,
        );
        if self.controlled && selected_state.read(cx) != &initial_index {
            selected_state.update(cx, |selected, _| *selected = initial_index);
        }
        let selected_index = *selected_state.read(cx);
        let controlled = self.controlled;
        let user_on_change = self.on_change.clone();
        let selection_for_change = selected_state.clone();
        let internal_on_change: Arc<dyn Fn(&usize, &mut Window, &mut App) + Send + Sync + 'static> =
            Arc::new(move |index, window, cx| {
                if !controlled {
                    selection_for_change.update(cx, |selected, cx| {
                        if *selected != Some(*index) {
                            *selected = Some(*index);
                            cx.notify();
                        }
                    });
                }
                if let Some(callback) = user_on_change.as_ref() {
                    callback(index, window, cx);
                }
            });
        let theme = Theme::of(cx);

        let mut tab_list = div()
            .id(ElementId::NamedChild(
                Box::new(tabs_id.clone()),
                "list".into(),
            ))
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::Group).label("Tabs"))
            .tab_group()
            .flex()
            .gap(px(4.0))
            .when(self.layout == TabsLayout::Fill, |div| div.w_full())
            .when(
                self.variant == TabVariant::Underline && self.has_divider,
                |div| div.border_b_1().border_color(theme.tokens.border),
            )
            .when(self.variant == TabVariant::Pills, |div| {
                div.p(px(4.0))
                    .bg(theme.tokens.muted)
                    .rounded(theme.tokens.radius_md)
            });

        for (index, tab) in self.tabs.iter().enumerate() {
            let is_active = Some(index) == selected_index;
            let previous_index = adjacent_enabled_index(&enabled_indices, index, false);
            let next_index = adjacent_enabled_index(&enabled_indices, index, true);
            tab_list = tab_list.child(Self::render_tab_button(
                self.variant,
                self.size,
                self.layout,
                tab,
                index,
                is_active,
                ElementId::NamedChild(Box::new(tabs_id.clone()), format!("tab-{index}").into()),
                previous_index,
                next_index,
                first_index,
                last_index,
                theme,
                Some(internal_on_change.clone()),
                self.on_close.clone(),
            ));
        }

        let tab_list = tab_list;

        let active_panel = self
            .panels
            .get(selected_index.unwrap_or(usize::MAX))
            .map(|panel| panel.render());

        let mut root = div()
            .id(tabs_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group).label("Tabbed content"),
            )
            .flex()
            .flex_col()
            .size_full()
            .gap(px(16.0))
            .child(tab_list);

        if let Some(panel) = active_panel {
            root = root.child(
                div()
                    .id(ElementId::NamedChild(Box::new(tabs_id), "panel".into()))
                    .accessibility(
                        AccessibilityAttributes::new(AccessibilityRole::TabPanel).label(
                            selected_index
                                .and_then(|index| self.tabs.get(index))
                                .map(|tab| tab.label.to_string())
                                .unwrap_or_else(|| "Tab panel".to_string()),
                        ),
                    )
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(div().size_full().child(panel)),
            );
        }

        root.map(|this| {
            let mut div = this;
            div.style().refine(&user_style);
            div.into_any_element()
        })
    }
}

pub fn init_tabs(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("right", TabNext, Some("Tabs")),
        KeyBinding::new("left", TabPrevious, Some("Tabs")),
        KeyBinding::new("home", TabFirst, Some("Tabs")),
        KeyBinding::new("end", TabLast, Some("Tabs")),
        KeyBinding::new("cmd-w", TabClose, Some("Tabs")),
    ]);
}

#[cfg(test)]
mod tests {
    use super::adjacent_enabled_index;

    #[::core::prelude::v1::test]
    fn keyboard_navigation_wraps_across_enabled_tabs() {
        let enabled = [0, 2, 4];
        assert_eq!(adjacent_enabled_index(&enabled, 0, true), 2);
        assert_eq!(adjacent_enabled_index(&enabled, 4, true), 0);
        assert_eq!(adjacent_enabled_index(&enabled, 0, false), 4);
        assert_eq!(adjacent_enabled_index(&[], 3, true), 3);
    }
}

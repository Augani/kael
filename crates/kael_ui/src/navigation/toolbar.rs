//! Toolbar component with icon buttons and grouping.

use crate::{
    astryx,
    components::icon::Icon,
    components::icon_source::IconSource,
    components::section::{SectionDivider, SectionVariant},
    theme::{use_theme, Theme},
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToolbarButtonVariant {
    Default,
    Toggle,
    Dropdown,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToolbarSize {
    Sm,
    Md,
    Lg,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ToolbarOrientation {
    #[default]
    Horizontal,
    Vertical,
}

impl ToolbarSize {
    fn button_size(&self) -> Pixels {
        match self {
            Self::Sm => px(28.0),
            Self::Md => px(32.0),
            Self::Lg => px(36.0),
        }
    }

    fn icon_size(&self) -> Pixels {
        match self {
            Self::Sm => px(14.0),
            Self::Md => px(16.0),
            Self::Lg => px(18.0),
        }
    }
}

#[derive(Clone)]
pub struct ToolbarButton {
    pub id: SharedString,
    pub icon: IconSource,
    pub tooltip: Option<SharedString>,
    pub variant: ToolbarButtonVariant,
    pub pressed: bool,
    pub disabled: bool,
    pub on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

impl ToolbarButton {
    pub fn new(id: impl Into<SharedString>, icon: impl Into<IconSource>) -> Self {
        Self {
            id: id.into(),
            icon: icon.into(),
            tooltip: None,
            variant: ToolbarButtonVariant::Default,
            pressed: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn variant(mut self, variant: ToolbarButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

#[derive(Clone)]
pub enum ToolbarItem {
    Button(ToolbarButton),
    Separator,
    Spacer,
}

#[derive(Clone)]
pub struct ToolbarGroup {
    pub items: Vec<ToolbarItem>,
}

impl ToolbarGroup {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn button(mut self, button: ToolbarButton) -> Self {
        self.items.push(ToolbarItem::Button(button));
        self
    }

    pub fn separator(mut self) -> Self {
        self.items.push(ToolbarItem::Separator);
        self
    }

    pub fn spacer(mut self) -> Self {
        self.items.push(ToolbarItem::Spacer);
        self
    }

    pub fn buttons(mut self, buttons: Vec<ToolbarButton>) -> Self {
        for button in buttons {
            self.items.push(ToolbarItem::Button(button));
        }
        self
    }
}

impl Default for ToolbarGroup {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(IntoElement)]
pub struct Toolbar {
    groups: Vec<ToolbarGroup>,
    start_content: Option<AnyElement>,
    center_content: Option<AnyElement>,
    end_content: Option<AnyElement>,
    label: SharedString,
    size: ToolbarSize,
    gap: Pixels,
    orientation: ToolbarOrientation,
    variant: SectionVariant,
    dividers: Vec<SectionDivider>,
    style: StyleRefinement,
}

pub type ToolbarProps = Toolbar;

impl Toolbar {
    pub fn new() -> Self {
        Self {
            groups: Vec::new(),
            start_content: None,
            center_content: None,
            end_content: None,
            label: "Toolbar".into(),
            size: ToolbarSize::Md,
            gap: px(4.0),
            orientation: ToolbarOrientation::Horizontal,
            variant: SectionVariant::Transparent,
            dividers: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn size(mut self, size: ToolbarSize) -> Self {
        self.size = size;
        self
    }

    pub fn gap(mut self, gap: Pixels) -> Self {
        self.gap = gap;
        self
    }

    pub fn orientation(mut self, orientation: ToolbarOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn variant(mut self, variant: SectionVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn divider(mut self, divider: SectionDivider) -> Self {
        self.dividers.push(divider);
        self
    }

    pub fn dividers(mut self, dividers: Vec<SectionDivider>) -> Self {
        self.dividers = dividers;
        self
    }

    pub fn start_content(mut self, content: impl IntoElement) -> Self {
        self.start_content = Some(content.into_any_element());
        self
    }

    #[allow(non_snake_case)]
    pub fn startContent(self, content: impl IntoElement) -> Self {
        self.start_content(content)
    }

    pub fn center_content(mut self, content: impl IntoElement) -> Self {
        self.center_content = Some(content.into_any_element());
        self
    }

    #[allow(non_snake_case)]
    pub fn centerContent(self, content: impl IntoElement) -> Self {
        self.center_content(content)
    }

    pub fn end_content(mut self, content: impl IntoElement) -> Self {
        self.end_content = Some(content.into_any_element());
        self
    }

    #[allow(non_snake_case)]
    pub fn endContent(self, content: impl IntoElement) -> Self {
        self.end_content(content)
    }

    pub fn group(mut self, group: ToolbarGroup) -> Self {
        self.groups.push(group);
        self
    }

    pub fn groups(mut self, groups: Vec<ToolbarGroup>) -> Self {
        self.groups.extend(groups);
        self
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Toolbar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Toolbar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let button_size = self.size.button_size();
        let icon_size = self.size.icon_size();
        let user_style = self.style;
        let bg = match self.variant {
            SectionVariant::Section => theme.tokens.card,
            SectionVariant::Transparent => transparent_black(),
            SectionVariant::Muted => theme.tokens.muted,
        };
        let has_slots = self.start_content.is_some()
            || self.center_content.is_some()
            || self.end_content.is_some();
        let has_center_content = self.center_content.is_some();
        let start_content = self.start_content;
        let center_content = self.center_content;
        let end_content = self.end_content;
        let dividers = self.dividers.clone();
        let border = theme.tokens.border;

        let group_count = self.groups.len();
        let grouped_content = div().flex().items_center().gap(px(8.0)).children(
            self.groups
                .into_iter()
                .enumerate()
                .map(|(group_idx, group)| {
                    let is_last_group = group_idx == group_count.saturating_sub(1);

                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .children(group.items.into_iter().map(|item| {
                            match item {
                                ToolbarItem::Button(button) => {
                                    render_toolbar_button(button, button_size, icon_size)
                                        .into_any_element()
                                }
                                ToolbarItem::Separator => div()
                                    .w(px(1.0))
                                    .h(button_size * 0.6)
                                    .bg(theme.tokens.border)
                                    .mx(px(4.0))
                                    .into_any_element(),
                                ToolbarItem::Spacer => div().flex_1().into_any_element(),
                            }
                        }))
                        .when(!is_last_group, |this| {
                            this.child(
                                div()
                                    .w(px(1.0))
                                    .h(button_size * 0.6)
                                    .bg(theme.tokens.border)
                                    .mx(px(4.0)),
                            )
                        })
                }),
        );
        let slot_content = if has_center_content {
            div()
                .grid()
                .grid_template_columns(vec![
                    GridTrack::fr(1.0),
                    GridTrack::auto(),
                    GridTrack::fr(1.0),
                ])
                .items_center()
                .gap(self.gap)
                .w_full()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(self.gap)
                        .when_some(start_content, |slot, content| slot.child(content)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(self.gap)
                        .when_some(center_content, |slot, content| slot.child(content)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(self.gap)
                        .when_some(end_content, |slot, content| slot.child(content)),
                )
                .into_any_element()
        } else if has_slots {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(self.gap)
                .w_full()
                .when_some(start_content, |slot, content| {
                    slot.child(div().flex().items_center().gap(self.gap).child(content))
                })
                .when_some(end_content, |slot, content| {
                    slot.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(self.gap)
                            .child(content),
                    )
                })
                .into_any_element()
        } else {
            grouped_content.into_any_element()
        };

        div()
            .flex()
            .items_center()
            .gap(self.gap)
            .px(px(8.0))
            .py(px(8.0))
            .min_h(button_size + px(16.0))
            .bg(bg)
            .when(self.orientation == ToolbarOrientation::Vertical, |this| {
                this.flex_col()
            })
            .when(dividers.contains(&SectionDivider::Top), |this| {
                this.border_t_1().border_color(border)
            })
            .when(dividers.contains(&SectionDivider::Bottom), |this| {
                this.border_b_1().border_color(border)
            })
            .when(dividers.contains(&SectionDivider::Start), |this| {
                this.border_l_1().border_color(border)
            })
            .when(dividers.contains(&SectionDivider::End), |this| {
                this.border_r_1().border_color(border)
            })
            .child(slot_content)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

fn render_toolbar_button(
    button: ToolbarButton,
    button_size: Pixels,
    icon_size: Pixels,
) -> impl IntoElement {
    let theme = use_theme();
    let dark = theme.tokens.background.l < 0.5;

    div()
        .size(button_size)
        .flex()
        .relative()
        .items_center()
        .justify_center()
        .rounded(theme.tokens.radius_md)
        .transition(theme.tokens.transition_fast)
        .cursor(if button.disabled {
            CursorStyle::Arrow
        } else {
            CursorStyle::PointingHand
        })
        .when(button.disabled, |div| div.opacity(0.5))
        .when(button.pressed && !button.disabled, |div| {
            div.bg(astryx::overlay_pressed(dark))
        })
        .when(!button.disabled, |div| {
            div.hover(|style| {
                if button.pressed {
                    style.bg(astryx::overlay_pressed(dark))
                } else {
                    style.bg(astryx::overlay_hover(dark))
                }
            })
        })
        .when_some(
            button.on_click.filter(|_| !button.disabled),
            |div, handler| {
                div.on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                    handler(window, cx);
                })
            },
        )
        .child(
            Icon::new(button.icon)
                .size(icon_size)
                .color(if button.disabled {
                    theme.tokens.muted_foreground
                } else if button.pressed {
                    theme.tokens.foreground
                } else {
                    theme.tokens.muted_foreground
                }),
        )
        .children((button.variant == ToolbarButtonVariant::Dropdown).then(|| {
            div()
                .absolute()
                .bottom(px(2.0))
                .right(px(2.0))
                .child(
                    Icon::new("chevron-down")
                        .size(px(10.0))
                        .color(theme.tokens.foreground),
                )
                .into_any_element()
        }))
}

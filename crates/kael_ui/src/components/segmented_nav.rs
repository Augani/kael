//! Segmented navigation with animated sliding highlight indicator.

use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

use crate::{astryx, components::icon::Icon, theme::Theme};

#[derive(Clone)]
struct SegmentedNavItem {
    id: SharedString,
    label: SharedString,
    icon: Option<SharedString>,
    label_hidden: bool,
    disabled: bool,
}

#[derive(Clone)]
pub struct SegmentedControlItem {
    id: SharedString,
    label: SharedString,
    icon: Option<SharedString>,
    label_hidden: bool,
    disabled: bool,
}

impl SegmentedControlItem {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: value.into(),
            label: label.into(),
            icon: None,
            label_hidden: false,
            disabled: false,
        }
    }

    pub fn value(&self) -> &SharedString {
        &self.id
    }

    pub fn label(&self) -> &SharedString {
        &self.label
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn label_hidden(mut self, hidden: bool) -> Self {
        self.label_hidden = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.label_hidden(hidden)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SegmentedNavSize {
    Sm,
    #[default]
    Md,
    Lg,
}

pub type SegmentedControlSize = SegmentedNavSize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SegmentedControlLayout {
    #[default]
    Hug,
    Fill,
}

impl SegmentedNavSize {
    fn height(&self) -> Pixels {
        match self {
            Self::Sm => px(28.0),
            Self::Md => px(32.0),
            Self::Lg => px(36.0),
        }
    }

    fn item_height(&self) -> Pixels {
        self.height() - px(4.0)
    }

    fn text_size(&self) -> Pixels {
        match self {
            Self::Sm => px(12.0),
            Self::Md | Self::Lg => px(14.0),
        }
    }

    fn padding_x(&self) -> Pixels {
        match self {
            Self::Sm => px(8.0),
            Self::Md => px(12.0),
            Self::Lg => px(16.0),
        }
    }
}

pub struct SegmentedNavState {
    active: SharedString,
    previous_active: Option<SharedString>,
    items: Vec<SegmentedNavItem>,
}

impl SegmentedNavState {
    pub fn new(active: impl Into<SharedString>) -> Self {
        Self {
            active: active.into(),
            previous_active: None,
            items: Vec::new(),
        }
    }

    pub fn set_active(&mut self, id: impl Into<SharedString>, cx: &mut Context<Self>) {
        let new_id = id.into();
        if self.active != new_id {
            self.previous_active = Some(self.active.clone());
            self.active = new_id;
            cx.notify();
        }
    }

    pub fn active(&self) -> &SharedString {
        &self.active
    }

    fn _active_index(&self) -> Option<usize> {
        self.items.iter().position(|item| item.id == self.active)
    }
}

#[derive(IntoElement)]
pub struct SegmentedNav {
    id: ElementId,
    state: Entity<SegmentedNavState>,
    items: Vec<SegmentedNavItem>,
    nav_size: SegmentedNavSize,
    layout: SegmentedControlLayout,
    label: Option<SharedString>,
    disabled: bool,
    on_change: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl SegmentedNav {
    pub fn new(id: impl Into<ElementId>, state: Entity<SegmentedNavState>) -> Self {
        Self {
            id: id.into(),
            state,
            items: Vec::new(),
            nav_size: SegmentedNavSize::default(),
            layout: SegmentedControlLayout::Hug,
            label: None,
            disabled: false,
            on_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn item(mut self, id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        self.items.push(SegmentedNavItem {
            id: id.into(),
            label: label.into(),
            icon: None,
            label_hidden: false,
            disabled: false,
        });
        self
    }

    pub fn control_item(mut self, item: SegmentedControlItem) -> Self {
        self.items.push(SegmentedNavItem {
            id: item.id,
            label: item.label,
            icon: item.icon,
            label_hidden: item.label_hidden,
            disabled: item.disabled,
        });
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn size(mut self, size: SegmentedNavSize) -> Self {
        self.nav_size = size;
        self
    }

    pub fn layout(mut self, layout: SegmentedControlLayout) -> Self {
        self.layout = layout;
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

    pub fn on_change<F>(mut self, handler: F) -> Self
    where
        F: Fn(SharedString, &mut Window, &mut App) + 'static,
    {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for SegmentedNav {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SegmentedNav {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let state = self.state.read(cx);
        let active_id = state.active.clone();

        self.state.update(cx, |state, _| {
            state.items = self.items.clone();
        });

        let tokens = &Theme::of(cx).tokens;
        let card = tokens.card;
        let foreground = tokens.foreground;
        let muted_foreground = tokens.muted_foreground;
        let layout = self.layout;
        let disabled = self.disabled;

        div()
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(2.0))
            .bg(tokens.secondary)
            .rounded(tokens.radius_md)
            .p(px(2.0))
            .h(self.nav_size.height())
            .when(disabled, |this| this.opacity(0.5))
            .children(self.items.iter().enumerate().map(|(idx, item)| {
                let item_id = item.id.clone();
                let is_active = item.id == active_id;
                let item_disabled = disabled || item.disabled;
                let on_change = self.on_change.clone();
                let state = self.state.clone();
                let click_id = item_id.clone();

                div()
                    .id(ElementId::Name(format!("seg-item-{}", idx).into()))
                    .when(layout == SegmentedControlLayout::Fill, |this| this.flex_1())
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(self.nav_size.item_height())
                    .px(self.nav_size.padding_x())
                    .gap(px(4.0))
                    .rounded((tokens.radius_md - px(2.0)).max(px(0.0)))
                    .text_size(self.nav_size.text_size())
                    .line_height(px(20.0))
                    .font_weight(if is_active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if is_active {
                        foreground
                    } else if item_disabled {
                        muted_foreground.opacity(0.7)
                    } else {
                        muted_foreground
                    })
                    .when(is_active, |this| {
                        this.bg(card).shadow(smallvec::smallvec![BoxShadow {
                            color: hsla(0.0, 0.0, 0.0, 0.08),
                            offset: point(px(0.0), px(1.0)),
                            blur_radius: px(2.0),
                            spread_radius: px(0.0),
                            inset: false,
                        }])
                    })
                    .when(!is_active && !item_disabled, |this| {
                        this.hover(|style| style.bg(astryx::overlay_hover(false)))
                    })
                    .when(!item_disabled, |this| {
                        this.cursor_pointer().on_mouse_down(
                            MouseButton::Left,
                            move |_, window, cx| {
                                state.update(cx, |state, cx| {
                                    state.set_active(click_id.clone(), cx);
                                });
                                if let Some(handler) = on_change.as_ref() {
                                    handler(item_id.clone(), window, cx);
                                }
                            },
                        )
                    })
                    .when(item_disabled, |this| this.cursor(CursorStyle::Arrow))
                    .when_some(item.icon.clone(), |this, icon| {
                        this.child(
                            div()
                                .size(match self.nav_size {
                                    SegmentedNavSize::Sm => px(14.0),
                                    SegmentedNavSize::Md => px(16.0),
                                    SegmentedNavSize::Lg => px(18.0),
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .child(Icon::new(icon).size(match self.nav_size {
                                    SegmentedNavSize::Sm => px(14.0),
                                    SegmentedNavSize::Md => px(16.0),
                                    SegmentedNavSize::Lg => px(18.0),
                                })),
                        )
                    })
                    .when(!item.label_hidden, |this| this.child(item.label.clone()))
            }))
            .map(|this| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}

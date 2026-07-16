//! Banner - a full-width, sentiment-tinted announcement bar.

use crate::astryx::Hue;
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum BannerVariant {
    #[default]
    Info,
    Success,
    Warning,
    Error,
    /// Brand-accent announcement.
    Announcement,
}

pub type BannerStatus = BannerVariant;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum BannerContainer {
    #[default]
    Card,
    Section,
}

impl BannerVariant {
    fn default_icon(self) -> &'static str {
        match self {
            BannerVariant::Info => "info",
            BannerVariant::Success => "circle-check",
            BannerVariant::Warning => "triangle-alert",
            BannerVariant::Error => "circle-alert",
            BannerVariant::Announcement => "megaphone",
        }
    }
}

#[derive(IntoElement)]
pub struct Banner {
    id: ElementId,
    variant: BannerVariant,
    message: SharedString,
    description: Option<SharedString>,
    icon: Option<IconSource>,
    show_icon: bool,
    dismissible: bool,
    dismissed: bool,
    expanded: bool,
    container: BannerContainer,
    end_content: Option<AnyElement>,
    children: Vec<AnyElement>,
    on_dismiss: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    action: Option<(SharedString, Rc<dyn Fn(&mut Window, &mut App)>)>,
    style: StyleRefinement,
}

impl Banner {
    #[track_caller]
    pub fn new(message: impl Into<SharedString>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "banner:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            variant: BannerVariant::default(),
            message: message.into(),
            description: None,
            icon: None,
            show_icon: true,
            dismissible: false,
            dismissed: false,
            expanded: false,
            container: BannerContainer::Card,
            end_content: None,
            children: Vec::new(),
            on_dismiss: None,
            action: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn variant(mut self, variant: BannerVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn status(self, status: BannerStatus) -> Self {
        self.variant(status)
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.message = title.into();
        self
    }

    /// Add supporting text below the banner title, matching Astryx's
    /// `description` slot.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    #[track_caller]
    pub fn success(message: impl Into<SharedString>) -> Self {
        Self::new(message).variant(BannerVariant::Success)
    }

    #[track_caller]
    pub fn warning(message: impl Into<SharedString>) -> Self {
        Self::new(message).variant(BannerVariant::Warning)
    }

    #[track_caller]
    pub fn error(message: impl Into<SharedString>) -> Self {
        Self::new(message).variant(BannerVariant::Error)
    }

    #[track_caller]
    pub fn announcement(message: impl Into<SharedString>) -> Self {
        Self::new(message).variant(BannerVariant::Announcement)
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn show_icon(mut self, show: bool) -> Self {
        self.show_icon = show;
        self
    }

    pub fn container(mut self, container: BannerContainer) -> Self {
        self.container = container;
        self
    }

    pub fn default_is_expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    #[allow(non_snake_case)]
    pub fn defaultIsExpanded(self, expanded: bool) -> Self {
        self.default_is_expanded(expanded)
    }

    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self.dismissible = true;
        self
    }

    pub fn dismissible(mut self, dismissible: bool) -> Self {
        self.dismissible = dismissible;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDismissable(self, dismissible: bool) -> Self {
        self.dismissible(dismissible)
    }

    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(handler)));
        self
    }

    pub fn end_content(mut self, content: impl IntoElement) -> Self {
        self.end_content = Some(content.into_any_element());
        self
    }

    #[allow(non_snake_case)]
    pub fn endContent(self, content: impl IntoElement) -> Self {
        self.end_content(content)
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(|child| child.into_any_element()));
        self
    }
}

impl Styled for Banner {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Banner {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let banner_id = self.id.clone();
        let runtime_state = window.use_keyed_state(
            ElementId::NamedChild(Box::new(banner_id.clone()), "runtime-state".into()),
            cx,
            |_, _| (self.expanded, self.dismissed),
        );
        let (expanded, dismissed) = *runtime_state.read(cx);
        if dismissed {
            return div().into_any_element();
        }

        let theme = Theme::of(cx);
        let dark = theme.tokens.background.l < 0.5;
        let hue = match self.variant {
            BannerVariant::Info | BannerVariant::Announcement => Hue::Blue,
            BannerVariant::Success => Hue::Green,
            BannerVariant::Warning => Hue::Yellow,
            BannerVariant::Error => Hue::Red,
        };
        let colors = hue.colors(dark);
        let bg = colors.background;
        let accent = colors.text;
        let title_color = theme.tokens.foreground;
        let description_color = theme.tokens.muted_foreground;
        let user_style = self.style;
        let icon_source = self
            .icon
            .unwrap_or_else(|| IconSource::Named(self.variant.default_icon().into()));
        let has_children = !self.children.is_empty();
        let show_content = has_children && expanded;
        let is_card = self.container == BannerContainer::Card;
        let has_actions =
            self.action.is_some() || self.end_content.is_some() || self.dismissible || has_children;
        let single_line = self.description.is_none() && has_actions;

        div()
            .id(banner_id.clone())
            .accessibility({
                let attributes = AccessibilityAttributes::new(AccessibilityRole::Alert)
                    .label(self.message.to_string());
                if let Some(description) = &self.description {
                    attributes.description(description.to_string())
                } else {
                    attributes
                }
            })
            .flex()
            .flex_col()
            .w_full()
            .font_family(theme.tokens.font_family.clone())
            .child(
                div()
                    .flex()
                    .items_start()
                    .when(single_line, |this| this.items_center())
                    .gap(px(8.0))
                    .px(px(16.0))
                    .py(px(12.0))
                    .when(is_card && !show_content, |this| {
                        this.rounded(theme.tokens.radius_lg)
                    })
                    .when(is_card && show_content, |this| {
                        this.rounded_t(theme.tokens.radius_lg)
                    })
                    .bg(bg)
                    .when(self.show_icon, |this| {
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .flex_shrink_0()
                                .child(Icon::new(icon_source).size(px(16.0)).color(accent)),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(px(0.0))
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .line_height(px(20.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(title_color)
                                    .child(
                                        StyledText::new(self.message.clone())
                                            .accessibility_hidden(true),
                                    ),
                            )
                            .when_some(self.description.clone(), |this, description| {
                                this.child(
                                    div()
                                        .text_size(px(12.0))
                                        .line_height(px(16.0))
                                        .font_weight(FontWeight::NORMAL)
                                        .text_color(description_color)
                                        .child(
                                            StyledText::new(description).accessibility_hidden(true),
                                        ),
                                )
                            }),
                    )
                    .when_some(self.end_content, |this, content| {
                        this.child(
                            div()
                                .ml_auto()
                                .flex()
                                .items_center()
                                .flex_shrink_0()
                                .child(content),
                        )
                    })
                    .when_some(self.action.clone(), |this, (label, handler)| {
                        this.child(
                            Button::new(
                                ElementId::NamedChild(Box::new(banner_id.clone()), "action".into()),
                                label,
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .flex_shrink_0()
                            .on_click(move |_, window, cx| {
                                (handler)(window, cx);
                            }),
                        )
                    })
                    .when(has_children, |this| {
                        this.child(
                            Button::new(
                                ElementId::NamedChild(Box::new(banner_id.clone()), "expand".into()),
                                if expanded { "Collapse" } else { "Expand" },
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Icon)
                            .icon(if expanded {
                                "chevron-up"
                            } else {
                                "chevron-down"
                            })
                            .tooltip(if expanded { "Collapse" } else { "Expand" })
                            .flex_shrink_0()
                            .on_click({
                                let runtime_state = runtime_state.clone();
                                move |_, _, cx| {
                                    runtime_state.update(cx, |state, cx| {
                                        state.0 = !state.0;
                                        cx.notify();
                                    });
                                }
                            }),
                        )
                    })
                    .when(self.dismissible, |this| {
                        let dismiss_handler = self.on_dismiss.clone();
                        this.child(
                            Button::new(
                                ElementId::NamedChild(
                                    Box::new(banner_id.clone()),
                                    "dismiss".into(),
                                ),
                                "Dismiss",
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Icon)
                            .icon("x")
                            .tooltip("Dismiss")
                            .flex_shrink_0()
                            .on_click(move |_, window, cx| {
                                runtime_state.update(cx, |state, cx| {
                                    state.1 = true;
                                    cx.notify();
                                });
                                if let Some(ref handler) = dismiss_handler {
                                    (handler)(window, cx);
                                }
                            }),
                        )
                    }),
            )
            .when(show_content, |this| {
                this.child(
                    div()
                        .bg(theme.tokens.card)
                        .px(px(16.0))
                        .py(px(12.0))
                        .border_l_1()
                        .border_r_1()
                        .border_b_1()
                        .border_color(theme.tokens.border)
                        .when(is_card, |content| content.rounded_b(theme.tokens.radius_lg))
                        .children(self.children),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .into_any_element()
    }
}

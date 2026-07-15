use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::icon::Icon;
use crate::components::icon_button::IconButton;
use crate::components::icon_source::IconSource;
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::{panic::Location, rc::Rc};

/// A custom color set for [`AlertVariant::Custom`], giving an alert any look without forking.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AlertColors {
    /// Background fill.
    pub background: Hsla,
    /// Accent color used for the border and icon.
    pub accent: Hsla,
    /// Title and description text color.
    pub foreground: Hsla,
}

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub enum AlertVariant {
    #[default]
    Info,
    Success,
    Warning,
    Error,
    /// An app-defined color set — build any look without forking the component.
    Custom(AlertColors),
}

impl AlertVariant {
    fn default_icon(&self) -> &'static str {
        match self {
            AlertVariant::Info => "info",
            AlertVariant::Success => "circle-check",
            AlertVariant::Warning => "triangle-alert",
            AlertVariant::Error => "circle-alert",
            AlertVariant::Custom(_) => "info",
        }
    }
}

#[derive(IntoElement)]
pub struct Alert {
    id: ElementId,
    variant: AlertVariant,
    title: Option<SharedString>,
    description: Option<SharedString>,
    icon: Option<IconSource>,
    show_icon: bool,
    dismissible: bool,
    on_dismiss: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    action: Option<(SharedString, Rc<dyn Fn(&mut Window, &mut App)>)>,
    style: StyleRefinement,
}

impl Alert {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "alert:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            variant: AlertVariant::default(),
            title: None,
            description: None,
            icon: None,
            show_icon: true,
            dismissible: false,
            on_dismiss: None,
            action: None,
            style: StyleRefinement::default(),
        }
    }

    #[track_caller]
    pub fn info() -> Self {
        Self::new().variant(AlertVariant::Info)
    }

    #[track_caller]
    pub fn success() -> Self {
        Self::new().variant(AlertVariant::Success)
    }

    #[track_caller]
    pub fn warning() -> Self {
        Self::new().variant(AlertVariant::Warning)
    }

    #[track_caller]
    pub fn error() -> Self {
        Self::new().variant(AlertVariant::Error)
    }

    pub fn variant(mut self, variant: AlertVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    /// Use a fully custom color set, setting the variant to [`AlertVariant::Custom`].
    pub fn colors(mut self, colors: AlertColors) -> Self {
        self.variant = AlertVariant::Custom(colors);
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn show_icon(mut self, show: bool) -> Self {
        self.show_icon = show;
        self
    }

    pub fn dismissible(mut self, dismissible: bool) -> Self {
        self.dismissible = dismissible;
        self
    }

    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self.dismissible = true;
        self
    }

    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(handler)));
        self
    }

    fn get_colors(&self, theme: &crate::theme::Theme) -> (Hsla, Hsla, Hsla) {
        let dark = theme.tokens.background.l < 0.5;
        let hue = |h: crate::astryx::Hue| {
            let c = h.colors(dark);
            (c.background, c.border, c.text)
        };
        match self.variant {
            AlertVariant::Info => hue(crate::astryx::Hue::Blue),
            AlertVariant::Success => hue(crate::astryx::Hue::Green),
            AlertVariant::Warning => hue(crate::astryx::Hue::Yellow),
            AlertVariant::Error => hue(crate::astryx::Hue::Red),
            AlertVariant::Custom(colors) => (colors.background, colors.accent, colors.foreground),
        }
    }
}

impl Default for Alert {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Alert {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Alert {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let (bg_color, border_color, accent_color) = self.get_colors(&theme);
        let user_style = self.style;

        let icon_source = self
            .icon
            .unwrap_or_else(|| IconSource::Named(self.variant.default_icon().into()));

        let has_content = self.title.is_some() || self.description.is_some();
        let alert_id = self.id.clone();
        let visibility = window.use_keyed_state(
            ElementId::NamedChild(Box::new(alert_id.clone()), "visibility".into()),
            cx,
            |_, _| true,
        );
        if !*visibility.read(cx) {
            return div().into_any_element();
        }
        let accessibility_label = self
            .title
            .as_ref()
            .or(self.description.as_ref())
            .map_or_else(|| "Alert".to_string(), ToString::to_string);
        let accessibility =
            AccessibilityAttributes::new(AccessibilityRole::Alert).label(accessibility_label);
        let accessibility = if let Some(description) = self.description.as_ref() {
            accessibility.description(description.to_string())
        } else {
            accessibility
        };
        let description_color = if matches!(self.variant, AlertVariant::Custom(_)) {
            accent_color
        } else {
            theme.tokens.muted_foreground
        };

        div()
            .id(alert_id.clone())
            .accessibility(accessibility)
            .flex()
            .w_full()
            .p(px(16.0))
            .rounded(theme.tokens.radius_md)
            .bg(bg_color)
            .border_1()
            .border_color(border_color.opacity(0.3))
            .gap(px(12.0))
            .when(self.show_icon, |this| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .pt(px(2.0))
                        .child(Icon::new(icon_source).size(px(20.0)).color(accent_color)),
                )
            })
            .when(has_content, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(4.0))
                        .when_some(self.title.clone(), |this, title| {
                            this.child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(accent_color)
                                    .whitespace_normal()
                                    .child(StyledText::new(title).accessibility_hidden(true)),
                            )
                        })
                        .when_some(self.description.clone(), |this, desc| {
                            this.child(
                                div()
                                    .text_sm()
                                    .text_color(description_color)
                                    .whitespace_normal()
                                    .child(StyledText::new(desc).accessibility_hidden(true)),
                            )
                        })
                        .when_some(self.action.clone(), |this, (label, handler)| {
                            this.child(
                                div().mt(px(8.0)).flex().items_start().child(
                                    Button::new(
                                        ElementId::NamedChild(
                                            Box::new(alert_id.clone()),
                                            "action".into(),
                                        ),
                                        label,
                                    )
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm)
                                    .on_click(
                                        move |_, window, cx| {
                                            (handler)(window, cx);
                                        },
                                    ),
                                ),
                            )
                        }),
                )
            })
            .when(self.dismissible, |this| {
                let dismiss_handler = self.on_dismiss.clone();
                let visibility = visibility.clone();
                this.child(
                    IconButton::new("x")
                        .id(ElementId::NamedChild(
                            Box::new(alert_id.clone()),
                            "dismiss".into(),
                        ))
                        .label("Dismiss alert")
                        .variant(ButtonVariant::Ghost)
                        .size(px(32.0))
                        .icon_size(px(16.0))
                        .on_click(move |_, window, cx| {
                            visibility.update(cx, |visible, cx| {
                                *visible = false;
                                cx.notify();
                            });
                            if let Some(ref handler) = dismiss_handler {
                                (handler)(window, cx);
                            }
                        }),
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

pub fn alert() -> Alert {
    Alert::new()
}

#[cfg(test)]
mod tests {
    use super::{Alert, AlertColors, AlertVariant};

    #[test]
    fn colors_sets_custom_alert_variant() {
        let alert = Alert::new().colors(AlertColors {
            background: kael::black(),
            accent: kael::white(),
            foreground: kael::white(),
        });
        assert!(matches!(alert.variant, AlertVariant::Custom(_)));
    }
}

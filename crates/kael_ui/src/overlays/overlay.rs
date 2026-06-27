//! Overlay component - ASTRYX-style scrim and floating content surface.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OverlayTone {
    #[default]
    Scrim,
    Transparent,
    Muted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OverlayScrimMode {
    #[default]
    Dark,
    Light,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OverlayPosition {
    #[default]
    Fill,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OverlayAlign {
    Start,
    #[default]
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OverlayShowOn {
    #[default]
    Always,
    Hover,
    Focus,
}

#[derive(IntoElement)]
pub struct Overlay {
    tone: OverlayTone,
    scrim: OverlayScrimMode,
    position: OverlayPosition,
    align: OverlayAlign,
    show_on: OverlayShowOn,
    center: bool,
    content: Option<AnyElement>,
    on_dismiss: Option<Box<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Overlay {
    pub fn new() -> Self {
        Self {
            tone: OverlayTone::Scrim,
            scrim: OverlayScrimMode::Dark,
            position: OverlayPosition::Fill,
            align: OverlayAlign::Center,
            show_on: OverlayShowOn::Always,
            center: true,
            content: None,
            on_dismiss: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn tone(mut self, tone: OverlayTone) -> Self {
        self.tone = tone;
        self.scrim = match tone {
            OverlayTone::Scrim => OverlayScrimMode::Dark,
            OverlayTone::Transparent => OverlayScrimMode::None,
            OverlayTone::Muted => OverlayScrimMode::Light,
        };
        self
    }

    pub fn scrim(mut self, scrim: OverlayScrimMode) -> Self {
        self.scrim = scrim;
        self.tone = match scrim {
            OverlayScrimMode::Dark => OverlayTone::Scrim,
            OverlayScrimMode::Light => OverlayTone::Muted,
            OverlayScrimMode::None => OverlayTone::Transparent,
        };
        self
    }

    pub fn position(mut self, position: OverlayPosition) -> Self {
        self.position = position;
        self.center = matches!(position, OverlayPosition::Fill);
        self
    }

    pub fn align(mut self, align: OverlayAlign) -> Self {
        self.align = align;
        self
    }

    pub fn show_on(mut self, show_on: OverlayShowOn) -> Self {
        self.show_on = show_on;
        self
    }

    pub fn center(mut self, center: bool) -> Self {
        self.center = center;
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Box::new(handler));
        self
    }
}

impl Default for Overlay {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Overlay {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Overlay {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let bg = match self.tone {
            OverlayTone::Scrim => hsla(0.0, 0.0, 0.0, 0.48),
            OverlayTone::Transparent => transparent_black(),
            OverlayTone::Muted => theme.tokens.background.opacity(0.84),
        };
        let user_style = self.style;
        let position = self.position;
        let align = self.align;
        let show_on = self.show_on;

        div()
            .absolute()
            .inset_0()
            .flex()
            .group("kael-ui-overlay")
            .bg(bg)
            .when(position == OverlayPosition::Fill && self.center, |this| {
                this.items_center()
            })
            .when(position == OverlayPosition::Top, |this| this.items_start())
            .when(position == OverlayPosition::Bottom, |this| this.items_end())
            .when(align == OverlayAlign::Start, |this| this.justify_start())
            .when(align == OverlayAlign::Center, |this| this.justify_center())
            .when(align == OverlayAlign::End, |this| this.justify_end())
            .when(show_on == OverlayShowOn::Hover, |this| {
                this.invisible()
                    .group_hover("kael-ui-overlay", |style| style.visible())
            })
            .when_some(self.on_dismiss, |this, handler| {
                this.on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                    handler(window, cx);
                })
            })
            .when_some(self.content, |this, content| {
                this.child(
                    div()
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, _| {})
                        .child(content),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

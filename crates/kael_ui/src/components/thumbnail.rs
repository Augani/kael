//! Thumbnail component - square media preview with loading and remove states.

use crate::{
    components::{
        icon::Icon,
        skeleton::{Skeleton, SkeletonVariant},
        spinner::{Spinner, SpinnerSize, SpinnerVariant},
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

pub type ThumbnailProps = Thumbnail;

#[derive(IntoElement)]
pub struct Thumbnail {
    src: Option<SharedString>,
    alt: Option<SharedString>,
    label: Option<SharedString>,
    loading: bool,
    disabled: bool,
    on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_remove: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Thumbnail {
    pub fn new() -> Self {
        Self {
            src: None,
            alt: None,
            label: None,
            loading: false,
            disabled: false,
            on_click: None,
            on_remove: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn src(mut self, src: impl Into<SharedString>) -> Self {
        self.src = Some(src.into());
        self
    }

    pub fn alt(mut self, alt: impl Into<SharedString>) -> Self {
        self.alt = Some(alt.into());
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn is_loading(self, loading: bool) -> Self {
        self.loading(loading)
    }

    #[allow(non_snake_case)]
    pub fn isLoading(self, loading: bool) -> Self {
        self.is_loading(loading)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn is_disabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.is_disabled(disabled)
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onClick(self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click(handler)
    }

    pub fn on_remove(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_remove = Some(Rc::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onRemove(self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_remove(handler)
    }
}

impl Default for Thumbnail {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Thumbnail {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Thumbnail {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let has_src = self.src.is_some();
        let show_skeleton = self.loading && !has_src;
        let show_image = has_src && !show_skeleton;
        let show_placeholder = !self.loading && !has_src;
        let is_interactive = self.on_click.is_some() && !self.disabled && !self.loading;
        let on_click = self.on_click;
        let on_remove = self.on_remove;
        let label = self.label;

        div()
            .relative()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(64.0))
            .when(self.disabled, |this| this.opacity(0.5))
            .child(
                div()
                    .relative()
                    .size(px(64.0))
                    .rounded(theme.tokens.radius_md)
                    .overflow_hidden()
                    .bg(theme.tokens.muted)
                    .when(is_interactive, |this| {
                        this.cursor_pointer()
                            .hover(|style| {
                                style.opacity(0.85).shadow(smallvec::smallvec![BoxShadow {
                                    color: black().opacity(0.16),
                                    offset: point(px(0.0), px(6.0)),
                                    blur_radius: px(16.0),
                                    spread_radius: px(0.0),
                                    inset: false,
                                }])
                            })
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                if let Some(handler) = on_click.as_ref() {
                                    handler(window, cx);
                                }
                            })
                    })
                    .when(show_image, |this| {
                        this.child(
                            img(self.src.clone().unwrap())
                                .size_full()
                                .object_fit(ObjectFit::Cover),
                        )
                    })
                    .when(show_skeleton, |this| {
                        this.child(
                            Skeleton::new()
                                .variant(SkeletonVariant::Rect)
                                .size_full()
                                .rounded(theme.tokens.radius_md),
                        )
                    })
                    .when(show_placeholder, |this| {
                        this.child(
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new("image")
                                        .size(px(24.0))
                                        .color(theme.tokens.muted_foreground),
                                ),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(theme.tokens.radius_md)
                            .shadow(smallvec::smallvec![BoxShadow {
                                color: theme.tokens.border,
                                offset: point(px(0.0), px(0.0)),
                                blur_radius: px(0.0),
                                spread_radius: px(1.0),
                                inset: true,
                            }]),
                    )
                    .when(self.loading && has_src, |this| {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(black().opacity(0.32))
                                .child(
                                    Spinner::new()
                                        .size(SpinnerSize::Sm)
                                        .variant(SpinnerVariant::Custom(white())),
                                ),
                        )
                    })
                    .when_some(on_remove.filter(|_| !self.disabled), |this, handler| {
                        this.child(
                            div()
                                .absolute()
                                .top(px(4.0))
                                .right(px(4.0))
                                .size(px(20.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.0))
                                .bg(theme.tokens.secondary)
                                .cursor_pointer()
                                .hover(|style| style.bg(theme.tokens.accent))
                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    cx.stop_propagation();
                                    handler(window, cx);
                                })
                                .child(
                                    Icon::new("x")
                                        .size(px(12.0))
                                        .color(theme.tokens.secondary_foreground),
                                ),
                        )
                    }),
            )
            .when_some(label, |this, label| {
                this.child(
                    div()
                        .absolute()
                        .size(px(1.0))
                        .overflow_hidden()
                        .opacity(0.0)
                        .child(label),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

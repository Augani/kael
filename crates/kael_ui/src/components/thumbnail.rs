//! Thumbnail component - square media preview with loading and remove states.

use crate::{
    components::{
        icon::Icon,
        icon_button::IconButton,
        skeleton::{Skeleton, SkeletonVariant},
        spinner::{Spinner, SpinnerSize, SpinnerVariant},
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;

pub type ThumbnailProps = Thumbnail;

#[derive(IntoElement)]
pub struct Thumbnail {
    id: ElementId,
    src: Option<SharedString>,
    alt: Option<SharedString>,
    label: Option<SharedString>,
    size: Pixels,
    show_label: bool,
    loading: bool,
    loading_animation: bool,
    disabled: bool,
    on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_remove: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Thumbnail {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "thumbnail:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            src: None,
            alt: None,
            label: None,
            size: px(64.0),
            show_label: true,
            loading: false,
            loading_animation: true,
            disabled: false,
            on_click: None,
            on_remove: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn src(mut self, src: impl Into<SharedString>) -> Self {
        let src = src.into();
        self.src = (!src.trim().is_empty()).then_some(src);
        self
    }

    pub fn alt(mut self, alt: impl Into<SharedString>) -> Self {
        let alt = alt.into();
        self.alt = (!alt.trim().is_empty()).then_some(alt);
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.label = (!label.trim().is_empty()).then_some(label);
        self
    }

    /// Sets the square media size. Labels use the same width.
    pub fn size(mut self, size: Pixels) -> Self {
        let size_value = size / px(1.0);
        self.size = if size_value.is_finite() {
            px(size_value.max(1.0))
        } else {
            px(64.0)
        };
        self
    }

    /// Controls whether [`Self::label`] is rendered below the media preview.
    pub fn show_label(mut self, show: bool) -> Self {
        self.show_label = show;
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Control loading motion without changing loading semantics or visuals.
    pub fn loading_animation(mut self, animated: bool) -> Self {
        self.loading_animation = animated;
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
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let user_style = self.style;
        let has_src = self.src.is_some();
        let show_skeleton = self.loading && !has_src;
        let show_image = has_src && !show_skeleton;
        let show_placeholder = !self.loading && !has_src;
        let is_interactive = self.on_click.is_some() && !self.disabled && !self.loading;
        let on_click = self.on_click;
        let on_remove = self.on_remove;
        let label = self.label;
        let size = self.size;
        let show_label = self.show_label;
        let accessibility_label = self
            .alt
            .clone()
            .or_else(|| label.clone())
            .unwrap_or_else(|| "Thumbnail".into());
        let loading_animation = self.loading_animation;
        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let is_focused = focus_handle.is_focused(window);
        let focus_on_mouse = focus_handle.clone();
        let remove_id: ElementId = (self.id.clone(), "remove").into();

        div()
            .id(self.id.clone())
            .relative()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(size)
            .when(self.disabled, |this| this.opacity(0.5))
            .child(
                div()
                    .id((self.id.clone(), "media"))
                    .accessibility({
                        let mut attributes = if is_interactive {
                            AccessibilityAttributes::button(accessibility_label.to_string())
                                .focused(is_focused)
                                .actions(vec![
                                    AccessibilityAction::Focus,
                                    AccessibilityAction::Click,
                                ])
                        } else {
                            AccessibilityAttributes::new(AccessibilityRole::Image)
                                .label(accessibility_label.to_string())
                        };
                        if self.loading {
                            attributes = attributes.busy(true);
                        }
                        if self.disabled {
                            attributes = attributes.disabled(true);
                        }
                        attributes
                    })
                    .relative()
                    .size(size)
                    .rounded(theme.tokens.radius_md)
                    .overflow_hidden()
                    .bg(theme.tokens.muted)
                    .when(is_interactive, |this| {
                        let on_click_for_key = on_click.clone();
                        this.cursor_pointer()
                            .track_focus(&focus_handle.tab_index(0).tab_stop(true))
                            .when(is_focused, |this| {
                                this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                                    theme.tokens.ring,
                                )])
                            })
                            .hover(|style| {
                                style.opacity(0.85).shadow(smallvec::smallvec![BoxShadow {
                                    color: black().opacity(0.16),
                                    offset: point(px(0.0), px(6.0)),
                                    blur_radius: px(16.0),
                                    spread_radius: px(0.0),
                                    inset: false,
                                }])
                            })
                            .on_mouse_down(MouseButton::Left, move |_, window, _cx| {
                                window.focus(&focus_on_mouse);
                            })
                            .when_some(on_click.clone(), |this, handler| {
                                this.on_click(move |_, window, cx| handler(window, cx))
                            })
                            .on_key_down(move |event, window, cx| {
                                if event.keystroke.modifiers.modified()
                                    || !matches!(event.keystroke.key.as_str(), "enter" | "space")
                                {
                                    return;
                                }
                                if let Some(handler) = on_click_for_key.as_ref() {
                                    handler(window, cx);
                                }
                                window.prevent_default();
                                cx.stop_propagation();
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
                                .animated(loading_animation)
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
                                        .variant(SpinnerVariant::Custom(white()))
                                        .decorative(true)
                                        .when(!loading_animation, |spinner| {
                                            spinner.animation_cycles(1)
                                        }),
                                ),
                        )
                    }),
            )
            .when_some(on_remove.filter(|_| !self.disabled), |this, handler| {
                this.child(
                    div().absolute().top(px(4.0)).right(px(4.0)).child(
                        IconButton::new("x")
                            .id(remove_id)
                            .label("Remove thumbnail")
                            .size(px(24.0))
                            .icon_size(px(12.0))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                handler(window, cx);
                            }),
                    ),
                )
            })
            .when_some(label.filter(|_| show_label), |this, label| {
                this.child(
                    div()
                        .accessibility(
                            AccessibilityAttributes::new(AccessibilityRole::StaticText)
                                .hidden(true),
                        )
                        .mt(px(4.0))
                        .w_full()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_center()
                        .text_xs()
                        .text_color(theme.tokens.muted_foreground)
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

#[cfg(test)]
mod tests {
    use super::Thumbnail;
    use kael::px;

    #[test]
    fn invalid_sizes_are_sanitized() {
        assert_eq!(Thumbnail::new().size(px(-10.0)).size, px(1.0));
        assert_eq!(Thumbnail::new().size(px(f32::NAN)).size, px(64.0));
        assert_eq!(Thumbnail::new().size(px(48.0)).size, px(48.0));
    }

    #[test]
    fn empty_metadata_and_loading_motion_are_normalized() {
        let thumbnail = Thumbnail::new()
            .src(" ")
            .alt("")
            .label("\t")
            .loading_animation(false);
        assert!(thumbnail.src.is_none());
        assert!(thumbnail.alt.is_none());
        assert!(thumbnail.label.is_none());
        assert!(!thumbnail.loading_animation);
    }
}

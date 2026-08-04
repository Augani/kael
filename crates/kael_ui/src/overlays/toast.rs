//! Toast notification component with auto-dismiss.

use kael::{prelude::FluentBuilder as _, *};
use smol::Timer;
use std::{panic::Location, rc::Rc, time::Duration};

use crate::animations::easings;
use crate::components::button::{Button, ButtonColors, ButtonSize};
use crate::theme::Theme;

fn sanitize_inset(inset: Pixels) -> Pixels {
    let inset = f32::from(inset);
    px(if inset.is_finite() {
        inset.max(0.0)
    } else {
        0.0
    })
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToastType {
    Info,
    Error,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToastCollisionBehavior {
    Overwrite,
    Ignore,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToastDismissReason {
    Auto,
    Manual,
}

pub type ToastDismissFn = Box<dyn Fn(&mut Window, &mut App)>;
pub type ShowToastFn = Box<dyn Fn(ToastOptions, &mut Window, &mut App) -> ToastDismissFn>;

pub struct ToastOptions {
    pub body: SharedString,
    pub toast_type: ToastType,
    pub is_auto_hide: Option<bool>,
    pub auto_hide_duration: Option<Duration>,
    pub end_content: Option<AnyElement>,
    pub unique_id: Option<SharedString>,
    pub collision_behavior: ToastCollisionBehavior,
    pub on_hide: Option<Box<dyn Fn(ToastDismissReason, &mut Window, &mut App)>>,
}

impl ToastOptions {
    pub fn new(body: impl Into<SharedString>) -> Self {
        Self {
            body: body.into(),
            toast_type: ToastType::Info,
            is_auto_hide: None,
            auto_hide_duration: None,
            end_content: None,
            unique_id: None,
            collision_behavior: ToastCollisionBehavior::Overwrite,
            on_hide: None,
        }
    }

    pub fn toast_type(mut self, toast_type: ToastType) -> Self {
        self.toast_type = toast_type;
        self
    }

    pub fn auto_hide(mut self, is_auto_hide: bool) -> Self {
        self.is_auto_hide = Some(is_auto_hide);
        self
    }

    pub fn auto_hide_duration(mut self, duration: Duration) -> Self {
        self.auto_hide_duration = Some(duration);
        self
    }

    pub fn end_content(mut self, end_content: impl IntoElement) -> Self {
        self.end_content = Some(end_content.into_any_element());
        self
    }

    pub fn unique_id(mut self, unique_id: impl Into<SharedString>) -> Self {
        self.unique_id = Some(unique_id.into());
        self
    }

    pub fn collision_behavior(mut self, behavior: ToastCollisionBehavior) -> Self {
        self.collision_behavior = behavior;
        self
    }

    pub fn on_hide(
        mut self,
        handler: impl Fn(ToastDismissReason, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_hide = Some(Box::new(handler));
        self
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToastVariant {
    Default,
    Success,
    Warning,
    Error,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToastPosition {
    TopStart,
    TopEnd,
    BottomStart,
    BottomEnd,
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl From<ToastType> for ToastVariant {
    fn from(value: ToastType) -> Self {
        match value {
            ToastType::Info => ToastVariant::Default,
            ToastType::Error => ToastVariant::Error,
        }
    }
}

#[derive(IntoElement)]
pub struct Toast {
    id: ElementId,
    toast_type: ToastType,
    body: AnyElement,
    end_content: Option<AnyElement>,
    is_auto_hide: bool,
    auto_hide_duration: Duration,
    is_exiting: bool,
    on_dismiss: Option<Box<dyn Fn(ToastDismissReason, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Toast {
    #[track_caller]
    pub fn new(body: impl IntoElement) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "toast:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            toast_type: ToastType::Info,
            body: body.into_any_element(),
            end_content: None,
            is_auto_hide: true,
            auto_hide_duration: Duration::from_secs(5),
            is_exiting: false,
            on_dismiss: None,
            style: StyleRefinement::default(),
        }
    }

    /// Set a stable identity when multiple toasts originate at one callsite.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn toast_type(mut self, toast_type: ToastType) -> Self {
        self.toast_type = toast_type;
        if toast_type == ToastType::Error {
            self.is_auto_hide = false;
        }
        self
    }

    pub fn type_(self, toast_type: ToastType) -> Self {
        self.toast_type(toast_type)
    }

    pub fn end_content(mut self, content: impl IntoElement) -> Self {
        self.end_content = Some(content.into_any_element());
        self
    }

    pub fn auto_hide(mut self, auto_hide: bool) -> Self {
        self.is_auto_hide = auto_hide;
        self
    }

    pub fn auto_hide_duration(mut self, duration: Duration) -> Self {
        self.auto_hide_duration = duration;
        self
    }

    pub fn exiting(mut self, exiting: bool) -> Self {
        self.is_exiting = exiting;
        self
    }

    pub fn on_dismiss(
        mut self,
        handler: impl Fn(ToastDismissReason, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Box::new(handler));
        self
    }
}

impl Styled for Toast {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Toast {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let toast_id = self.id;
        let dismiss_id = ElementId::NamedChild(Box::new(toast_id.clone()), "dismiss".into());
        let user_style = self.style;
        let is_error = self.toast_type == ToastType::Error;
        let bg = if is_error {
            theme.tokens.destructive
        } else {
            theme.tokens.foreground
        };
        let fg = if is_error {
            theme.tokens.destructive_foreground
        } else {
            theme.tokens.background
        };
        let close_colors = ButtonColors {
            background: transparent_black(),
            foreground: fg,
            border: transparent_black(),
            hover_background: crate::astryx::overlay_hover(bg.l < 0.5),
            hover_foreground: fg,
            has_shadow: false,
            has_border: false,
        };
        let on_dismiss: Option<Rc<dyn Fn(ToastDismissReason, &mut Window, &mut App)>> =
            self.on_dismiss.map(Rc::from);
        if self.is_auto_hide
            && let Some(handler) = on_dismiss.clone()
        {
            let duration = self.auto_hide_duration;
            window
                .spawn(cx, async move |cx| {
                    Timer::after(duration).await;
                    cx.update(|window, cx| handler(ToastDismissReason::Auto, window, cx))
                        .ok();
                })
                .detach();
        }

        div()
            .id(toast_id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Alert).label(if is_error {
                    "Error notification"
                } else {
                    "Notification"
                }),
            )
            .flex()
            .items_start()
            .gap(px(12.0))
            .w(px(400.0))
            .max_w(px(400.0))
            .p(px(16.0))
            .rounded(theme.tokens.radius_lg)
            .bg(bg)
            .text_color(fg)
            .text_size(px(14.0))
            .line_height(px(20.0))
            .font_family(theme.tokens.font_family.clone())
            .shadow(theme.tokens.shadow_md.to_vec())
            .when(self.is_exiting, |this| this.opacity(0.0).mt(px(-8.0)))
            .child(div().flex_1().min_w(px(0.0)).child(self.body))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .when_some(self.end_content, |this, content| this.child(content))
                    .child(
                        Button::new(dismiss_id, "")
                            .colors(close_colors)
                            .size(ButtonSize::Icon)
                            .icon("x")
                            .tooltip("Dismiss notification")
                            .on_click(move |_, window, cx| {
                                if let Some(handler) = on_dismiss.as_ref() {
                                    handler(ToastDismissReason::Manual, window, cx);
                                }
                            }),
                    ),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct ToastViewport {
    manager: Option<Entity<ToastManager>>,
    position: ToastPosition,
    max_visible: usize,
    inset: Edges<Pixels>,
    top_layer: bool,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl ToastViewport {
    pub fn new() -> Self {
        Self {
            manager: None,
            position: ToastPosition::BottomEnd,
            max_visible: 5,
            inset: Edges::all(px(0.0)),
            top_layer: true,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn manager(mut self, manager: Entity<ToastManager>) -> Self {
        self.manager = Some(manager);
        self
    }

    pub fn position(mut self, position: ToastPosition) -> Self {
        self.position = position;
        self
    }

    pub fn max_visible(mut self, max_visible: usize) -> Self {
        self.max_visible = max_visible.max(1);
        self
    }

    pub fn inset(mut self, inset: impl Into<Pixels>) -> Self {
        self.inset = Edges::all(sanitize_inset(inset.into()));
        self
    }

    pub fn top_layer(mut self, top_layer: bool) -> Self {
        self.top_layer = top_layer;
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for ToastViewport {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for ToastViewport {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ToastViewport {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let position = self.position;
        let max_visible = self.max_visible;
        let inset = self.inset;
        let top_layer = self.top_layer;
        let manager = self.manager.clone();

        if let Some(manager) = manager.as_ref() {
            manager.update(cx, |manager, _| {
                manager.position = position;
                manager.max_toasts = max_visible;
                manager.inset = inset;
            });
        }

        div()
            .relative()
            .children(self.children)
            .when_some(manager, |this, manager| {
                this.child(if top_layer {
                    deferred(manager).with_priority(100).into_any_element()
                } else {
                    manager.into_any_element()
                })
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(Clone, Debug)]
pub struct ToastItem {
    pub id: u64,
    pub title: SharedString,
    pub description: Option<SharedString>,
    pub variant: ToastVariant,
    pub duration: Option<Duration>,
    pub style: StyleRefinement,
}

impl ToastItem {
    pub fn new(id: u64, title: impl Into<SharedString>) -> Self {
        Self {
            id,
            title: title.into(),
            description: None,
            variant: ToastVariant::Default,
            duration: Some(Duration::from_secs(5)),
            style: StyleRefinement::default(),
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn variant(mut self, variant: ToastVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    pub fn persistent(mut self) -> Self {
        self.duration = None;
        self
    }
}

impl Styled for ToastItem {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

pub struct ToastManager {
    toasts: Vec<ToastItem>,
    position: ToastPosition,
    max_toasts: usize,
    inset: Edges<Pixels>,
    dismissing: std::collections::HashSet<u64>,
}

impl ToastManager {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            toasts: vec![],
            position: ToastPosition::BottomRight,
            max_toasts: 5,
            inset: Edges::all(px(0.0)),
            dismissing: std::collections::HashSet::new(),
        }
    }

    pub fn position(mut self, position: ToastPosition) -> Self {
        self.position = position;
        self
    }

    pub fn max_toasts(mut self, max: usize) -> Self {
        self.max_toasts = max.max(1);
        self
    }

    pub fn set_position(&mut self, position: ToastPosition, cx: &mut Context<Self>) {
        self.position = position;
        cx.notify();
    }

    pub fn set_max_toasts(&mut self, max: usize, cx: &mut Context<Self>) {
        self.max_toasts = max.max(1);
        cx.notify();
    }

    pub fn add_toast(&mut self, toast: ToastItem, window: &mut Window, cx: &mut Context<Self>) {
        if self.toasts.len() >= self.max_toasts {
            self.toasts.remove(0);
        }

        let id = toast.id;
        let duration = toast.duration;

        self.toasts.push(toast);

        if let Some(duration) = duration {
            cx.spawn_in(window, async move |this, cx| {
                Timer::after(duration).await;
                let _ = this.update(cx, |this, cx| {
                    this.dismissing.insert(id);
                    cx.notify();
                });
                Timer::after(Duration::from_millis(250)).await;
                let _ = this.update(cx, |this, cx| {
                    this.dismiss_toast(id, cx);
                });
            })
            .detach();
        }

        cx.notify();
    }

    pub fn add_toast_no_dismiss(&mut self, toast: ToastItem, cx: &mut Context<Self>) {
        if self.toasts.len() >= self.max_toasts {
            self.toasts.remove(0);
        }

        self.toasts.push(toast);
        cx.notify();
    }

    pub fn dismiss_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        self.toasts.retain(|t| t.id != id);
        self.dismissing.remove(&id);
        cx.notify();
    }

    pub fn dismiss_toast_animated(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.dismissing.contains(&id) {
            return;
        }
        self.dismissing.insert(id);
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            Timer::after(Duration::from_millis(250)).await;
            let _ = this.update(cx, |this, cx| {
                this.dismiss_toast(id, cx);
            });
        })
        .detach();
    }

    pub fn is_dismissing(&self, id: u64) -> bool {
        self.dismissing.contains(&id)
    }

    pub fn clear_all(&mut self, cx: &mut Context<Self>) {
        self.toasts.clear();
        cx.notify();
    }
}

impl Render for ToastManager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let inset = self.inset;
        let animations_enabled = window.animations_enabled();

        if self.toasts.is_empty() {
            return div().into_any_element();
        }

        let (v_pos, h_pos, v_anchor, items_order) = match self.position {
            ToastPosition::TopStart => ("top", "left", "flex_col", false),
            ToastPosition::TopEnd => ("top", "right", "flex_col", false),
            ToastPosition::BottomStart => ("bottom", "left", "flex_col_reverse", true),
            ToastPosition::BottomEnd => ("bottom", "right", "flex_col_reverse", true),
            ToastPosition::TopLeft => ("top", "left", "flex_col", false),
            ToastPosition::TopCenter => ("top", "center", "flex_col", false),
            ToastPosition::TopRight => ("top", "right", "flex_col", false),
            ToastPosition::BottomLeft => ("bottom", "left", "flex_col_reverse", true),
            ToastPosition::BottomCenter => ("bottom", "center", "flex_col_reverse", true),
            ToastPosition::BottomRight => ("bottom", "right", "flex_col_reverse", true),
        };

        let mut container = div()
            .absolute()
            .flex()
            .gap(px(12.0))
            .p(px(16.0))
            .max_w(px(420.0));

        container = match v_pos {
            "top" => container.top(inset.top),
            "bottom" => container.bottom(inset.bottom),
            _ => container,
        };

        container = match h_pos {
            "left" => container.left(inset.left),
            "right" => container.right(inset.right),
            "center" => container.left(inset.left).right(inset.right).mx_auto(),
            _ => container,
        };

        container = match v_anchor {
            "flex_col" => container.flex_col(),
            "flex_col_reverse" => container.flex_col_reverse(),
            _ => container,
        };

        let mut toasts_to_show = self.toasts.clone();
        if items_order {
            toasts_to_show.reverse();
        }

        container
            .children(
                toasts_to_show
                    .into_iter()
                    .map(|toast| {
                        let (bg_color, fg_color) = match toast.variant {
                            ToastVariant::Error => (
                                theme.tokens.destructive,
                                theme.tokens.destructive_foreground,
                            ),
                            ToastVariant::Default
                            | ToastVariant::Success
                            | ToastVariant::Warning => {
                                (theme.tokens.foreground, theme.tokens.background)
                            }
                        };
                        let close_colors = ButtonColors {
                            background: transparent_black(),
                            foreground: fg_color,
                            border: transparent_black(),
                            hover_background: crate::astryx::overlay_hover(bg_color.l < 0.5),
                            hover_foreground: fg_color,
                            has_shadow: false,
                            has_border: false,
                        };

                        let user_style = toast.style.clone();
                        let toast_id = toast.id;
                        let is_dismissing = self.dismissing.contains(&toast_id);
                        let accessibility_title = toast.title.clone();
                        let accessibility_description = toast.description.clone();
                        let accessibility = AccessibilityAttributes::new(AccessibilityRole::Alert)
                            .label(accessibility_title.to_string());
                        let accessibility = if let Some(description) = accessibility_description {
                            accessibility.description(description.to_string())
                        } else {
                            accessibility
                        };

                        div()
                            .id(("toast", toast_id))
                            .accessibility(accessibility)
                            .flex()
                            .items_start()
                            .gap(px(12.0))
                            .w(px(400.0))
                            .max_w(px(400.0))
                            .bg(bg_color)
                            .rounded(theme.tokens.radius_lg)
                            .p(px(16.0))
                            .text_color(fg_color)
                            .font_family(theme.tokens.font_family.clone())
                            .text_size(px(14.0))
                            .line_height(px(20.0))
                            .shadow(theme.tokens.shadow_md.to_vec())
                            .map(|this| {
                                let mut div = this;
                                div.style().refine(&user_style);
                                div
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .child(
                                        div()
                                            .text_size(px(14.0))
                                            .font_family(theme.tokens.font_family.clone())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(fg_color)
                                            .line_height(px(20.0))
                                            .child(
                                                StyledText::new(toast.title)
                                                    .accessibility_hidden(true),
                                            ),
                                    )
                                    .when_some(toast.description, |this, desc| {
                                        this.child(
                                            div()
                                                .text_size(px(14.0))
                                                .font_family(theme.tokens.font_family.clone())
                                                .text_color(fg_color)
                                                .line_height(px(20.0))
                                                .child(
                                                    StyledText::new(desc)
                                                        .accessibility_hidden(true),
                                                ),
                                        )
                                    }),
                            )
                            .child(
                                Button::new(("toast-dismiss", toast_id), "Dismiss notification")
                                    .colors(close_colors)
                                    .size(ButtonSize::Icon)
                                    .icon("x")
                                    .tooltip("Dismiss notification")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.dismiss_toast_animated(toast.id, window, cx);
                                    })),
                            )
                            .map(|toast_element| {
                                if animations_enabled {
                                    toast_element
                                        .with_animation(
                                            ElementId::NamedInteger(
                                                if is_dismissing {
                                                    "toast-exit"
                                                } else {
                                                    "toast-enter"
                                                }
                                                .into(),
                                                toast_id,
                                            ),
                                            Animation::new(Duration::from_millis(
                                                if is_dismissing { 250 } else { 300 },
                                            ))
                                            .with_easing(if is_dismissing {
                                                easings::ease_in_cubic as fn(f32) -> f32
                                            } else {
                                                easings::ease_out_cubic as fn(f32) -> f32
                                            }),
                                            move |el, delta| {
                                                if is_dismissing {
                                                    el.opacity(1.0 - delta).mt(px(8.0 * delta))
                                                } else {
                                                    el.opacity(delta).mt(px(8.0 * (1.0 - delta)))
                                                }
                                            },
                                        )
                                        .into_any_element()
                                } else {
                                    toast_element.into_any_element()
                                }
                            })
                    })
                    .collect::<Vec<_>>(),
            )
            .into_any_element()
    }
}

impl EventEmitter<()> for ToastManager {}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn viewport_and_manager_limits_never_allow_an_empty_capacity() {
        let viewport = ToastViewport::new().max_visible(0).inset(px(f32::NAN));
        assert_eq!(viewport.max_visible, 1);
        assert_eq!(viewport.inset, Edges::all(px(0.0)));

        let cx = TestAppContext::single();
        let manager = cx.update(|cx| cx.new(|cx| ToastManager::new(cx).max_toasts(0)));
        cx.update(|cx| assert_eq!(manager.read(cx).max_toasts, 1));
    }
}

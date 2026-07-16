use crate::components::button::{Button, ButtonVariant};
use crate::components::empty_state::EmptyState;
use crate::components::icon::Icon;
use crate::components::icon_button::IconButton;
use crate::components::icon_source::IconSource;
use crate::components::scrollable::scrollable_vertical;
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::{panic::Location, rc::Rc};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum NotificationVariant {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl NotificationVariant {
    fn icon_name(&self) -> &'static str {
        match self {
            NotificationVariant::Info => "info",
            NotificationVariant::Success => "circle-check",
            NotificationVariant::Warning => "alert-triangle",
            NotificationVariant::Error => "circle-x",
        }
    }

    fn color(&self, theme: &crate::theme::Theme) -> Hsla {
        match self {
            NotificationVariant::Info => theme.tokens.primary,
            NotificationVariant::Success => theme.tokens.success,
            NotificationVariant::Warning => theme.tokens.warning,
            NotificationVariant::Error => theme.tokens.destructive,
        }
    }
}

#[derive(Clone)]
pub struct NotificationAction {
    pub label: SharedString,
    pub handler: Rc<dyn Fn(&mut Window, &mut App)>,
}

#[derive(Clone)]
pub struct NotificationItem {
    pub id: ElementId,
    pub title: SharedString,
    pub message: Option<SharedString>,
    pub timestamp: Option<SharedString>,
    pub group: Option<SharedString>,
    pub variant: NotificationVariant,
    pub read: bool,
    pub icon: Option<IconSource>,
    pub action: Option<NotificationAction>,
}

impl NotificationItem {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            message: None,
            timestamp: None,
            group: None,
            variant: NotificationVariant::default(),
            read: false,
            icon: None,
            action: None,
        }
    }

    pub fn message(mut self, message: impl Into<SharedString>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub fn timestamp(mut self, timestamp: impl Into<SharedString>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }

    /// Sets the heading used when the notification center groups items.
    pub fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn variant(mut self, variant: NotificationVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn read(mut self, read: bool) -> Self {
        self.read = read;
        self
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some(NotificationAction {
            label: label.into(),
            handler: Rc::new(handler),
        });
        self
    }
}

pub struct NotificationCenterState {
    notifications: Vec<NotificationItem>,
}

impl NotificationCenterState {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self {
            notifications: Vec::new(),
        }
    }

    pub fn add(&mut self, notification: NotificationItem, cx: &mut Context<Self>) {
        self.notifications
            .retain(|existing| existing.id != notification.id);
        self.notifications.insert(0, notification);
        cx.notify();
    }

    pub fn remove(&mut self, id: &ElementId, cx: &mut Context<Self>) {
        let old_len = self.notifications.len();
        self.notifications.retain(|n| &n.id != id);
        if self.notifications.len() != old_len {
            cx.notify();
        }
    }

    pub fn mark_read(&mut self, id: &ElementId, cx: &mut Context<Self>) {
        if let Some(notification) = self.notifications.iter_mut().find(|n| &n.id == id) {
            if !notification.read {
                notification.read = true;
                cx.notify();
            }
        }
    }

    pub fn mark_all_read(&mut self, cx: &mut Context<Self>) {
        let had_unread = self
            .notifications
            .iter()
            .any(|notification| !notification.read);
        for notification in &mut self.notifications {
            notification.read = true;
        }
        if had_unread {
            cx.notify();
        }
    }

    pub fn clear_all(&mut self, cx: &mut Context<Self>) {
        if !self.notifications.is_empty() {
            self.notifications.clear();
            cx.notify();
        }
    }

    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }

    pub fn notifications(&self) -> &[NotificationItem] {
        &self.notifications
    }

    pub fn is_empty(&self) -> bool {
        self.notifications.is_empty()
    }
}

impl EventEmitter<()> for NotificationCenterState {}

#[derive(IntoElement)]
pub struct NotificationCenter {
    id: ElementId,
    state: Entity<NotificationCenterState>,
    max_visible: usize,
    show_timestamps: bool,
    group_by_date: bool,
    on_notification_click: Option<Rc<dyn Fn(&NotificationItem, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl NotificationCenter {
    #[track_caller]
    pub fn new(state: Entity<NotificationCenterState>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "notification-center:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            state,
            max_visible: 10,
            show_timestamps: true,
            group_by_date: false,
            on_notification_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn max_visible(mut self, max: usize) -> Self {
        self.max_visible = max.max(1);
        self
    }

    pub fn show_timestamps(mut self, show: bool) -> Self {
        self.show_timestamps = show;
        self
    }

    pub fn group_by_date(mut self, group: bool) -> Self {
        self.group_by_date = group;
        self
    }

    pub fn on_notification_click(
        mut self,
        handler: impl Fn(&NotificationItem, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_notification_click = Some(Rc::new(handler));
        self
    }
}

impl Styled for NotificationCenter {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for NotificationCenter {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let user_style = self.style;
        let state = self.state.read(cx);
        let notifications = state.notifications().to_vec();
        let is_empty = notifications.is_empty();
        let total_count = notifications.len();
        let center_id = self.id.clone();
        let expanded_state = window.use_keyed_state(
            ElementId::NamedChild(Box::new(center_id.clone()), "expanded".into()),
            cx,
            |_, _| false,
        );
        let expanded = *expanded_state.read(cx);
        let show_more = total_count > self.max_visible;
        let visible_limit = if expanded {
            total_count
        } else {
            self.max_visible
        };
        let visible_notifications: Vec<_> = notifications.into_iter().take(visible_limit).collect();
        let mut previous_group: Option<SharedString> = None;
        let visible_notifications: Vec<_> = visible_notifications
            .into_iter()
            .map(|notification| {
                let group = notification
                    .group
                    .clone()
                    .or_else(|| notification.timestamp.clone())
                    .unwrap_or_else(|| "Earlier".into());
                let starts_group = self.group_by_date
                    && previous_group
                        .as_ref()
                        .is_none_or(|previous| previous != &group);
                previous_group = Some(group.clone());
                (notification, starts_group, group)
            })
            .collect();

        let state_entity = self.state.clone();
        let on_click = self.on_notification_click.clone();
        let show_timestamps = self.show_timestamps;

        let shadow_lg = theme.tokens.shadow_lg.clone();

        div()
            .id(center_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Pane).label("Notification center"),
            )
            .flex()
            .flex_col()
            .w(px(380.0))
            .max_w_full()
            .max_h(px(500.0))
            .bg(theme.tokens.card)
            .border_1()
            .border_color(theme.tokens.border)
            .rounded(theme.tokens.radius_lg)
            .shadow(shadow_lg.to_vec())
            .overflow_hidden()
            .map(|mut this| {
                this.style().refine(&user_style);
                this
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.0))
                    .py(px(12.0))
                    .border_b_1()
                    .border_color(theme.tokens.border)
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.tokens.foreground)
                            .font_family(theme.tokens.font_family.clone())
                            .child("Notifications"),
                    )
                    .when(!is_empty, {
                        let state_clone = state_entity.clone();
                        |d| {
                            d.child(
                                Button::new(
                                    ElementId::NamedChild(
                                        Box::new(center_id.clone()),
                                        "mark-all-read".into(),
                                    ),
                                    "Mark all read",
                                )
                                .variant(ButtonVariant::Ghost)
                                .size(crate::components::button::ButtonSize::Sm)
                                .on_click(move |_, _, cx| {
                                    state_clone.update(cx, |state, cx| {
                                        state.mark_all_read(cx);
                                    });
                                }),
                            )
                        }
                    }),
            )
            .when(is_empty, |d| {
                d.child(
                    EmptyState::new(
                        ElementId::NamedChild(Box::new(center_id.clone()), "empty".into()),
                        "No notifications",
                    )
                    .icon("bell-off")
                    .description("You're all caught up!")
                    .size(crate::components::empty_state::EmptyStateSize::Sm)
                    .py(px(32.0)),
                )
            })
            .when(!is_empty, |d| {
                d.child(
                    scrollable_vertical(div().flex().flex_col().children(
                        visible_notifications.into_iter().map(
                            |(notification, starts_group, group)| {
                                let id = notification.id.clone();
                                let row_id = ElementId::NamedChild(
                                    Box::new(center_id.clone()),
                                    format!("item-{id:?}").into(),
                                );
                                let focus_handle = window
                                    .use_keyed_state(row_id.clone(), cx, |_, cx| cx.focus_handle())
                                    .read(cx)
                                    .clone();
                                let focus_on_mouse = focus_handle.clone();
                                let state_for_click = state_entity.clone();
                                let state_for_key = state_entity.clone();
                                let state_for_dismiss = state_entity.clone();
                                let id_for_key = id.clone();
                                let on_click_handler = on_click.clone();
                                let on_key_handler = on_click.clone();
                                let notification_clone = notification.clone();
                                let notification_for_key = notification.clone();
                                let is_read = notification.read;
                                let variant = notification.variant;
                                let variant_color = variant.color(&theme);
                                let mut accessibility_state = AccessibilityState::NONE;
                                if focus_handle.is_focused(window) {
                                    accessibility_state |= AccessibilityState::FOCUSED;
                                }

                                let row = div()
                                    .id(row_id.clone())
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Button)
                                            .label(format!(
                                                "{} notification: {}",
                                                if is_read { "Read" } else { "Unread" },
                                                notification.title
                                            ))
                                            .states(accessibility_state)
                                            .actions(vec![
                                                AccessibilityAction::Focus,
                                                AccessibilityAction::Click,
                                            ]),
                                    )
                                    .track_focus(&focus_handle.tab_index(0).tab_stop(true))
                                    .flex()
                                    .gap(px(12.0))
                                    .px(px(16.0))
                                    .py(px(12.0))
                                    .border_b_1()
                                    .border_color(theme.tokens.border)
                                    .bg(if is_read {
                                        kael::transparent_black()
                                    } else {
                                        theme.tokens.accent.opacity(0.3)
                                    })
                                    .cursor(CursorStyle::PointingHand)
                                    .transition(theme.tokens.transition_fast)
                                    .hover(|style| style.bg(theme.tokens.accent))
                                    .on_click({
                                        let id = id.clone();
                                        move |_, window, cx| {
                                            window.focus(&focus_on_mouse);
                                            state_for_click.update(cx, |state, cx| {
                                                state.mark_read(&id, cx);
                                            });
                                            if let Some(ref handler) = on_click_handler {
                                                handler(&notification_clone, window, cx);
                                            }
                                            window.prevent_default();
                                        }
                                    })
                                    .on_key_down(move |event: &KeyDownEvent, window, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            state_for_key.update(cx, |state, cx| {
                                                state.mark_read(&id_for_key, cx);
                                            });
                                            if let Some(ref handler) = on_key_handler {
                                                handler(&notification_for_key, window, cx);
                                            }
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                    })
                                    .child(
                                        div().flex_shrink_0().mt(px(2.0)).child(
                                            Icon::new(
                                                notification
                                                    .icon
                                                    .clone()
                                                    .unwrap_or_else(|| variant.icon_name().into()),
                                            )
                                            .size(px(18.0))
                                            .color(variant_color),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .gap(px(4.0))
                                            .overflow_hidden()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap(px(8.0))
                                                    .child(
                                                        div()
                                                            .text_size(px(13.0))
                                                            .font_weight(if is_read {
                                                                FontWeight::NORMAL
                                                            } else {
                                                                FontWeight::SEMIBOLD
                                                            })
                                                            .text_color(theme.tokens.foreground)
                                                            .font_family(
                                                                theme.tokens.font_family.clone(),
                                                            )
                                                            .truncate()
                                                            .child(
                                                                StyledText::new(
                                                                    notification.title.clone(),
                                                                )
                                                                .accessibility_hidden(true),
                                                            ),
                                                    )
                                                    .when(
                                                        show_timestamps
                                                            && notification.timestamp.is_some(),
                                                        |d| {
                                                            d.child(
                                                                div()
                                                                    .flex_shrink_0()
                                                                    .text_size(px(11.0))
                                                                    .text_color(
                                                                        theme
                                                                            .tokens
                                                                            .muted_foreground,
                                                                    )
                                                                    .font_family(
                                                                        theme
                                                                            .tokens
                                                                            .font_family
                                                                            .clone(),
                                                                    )
                                                                    .child(
                                                                        notification
                                                                            .timestamp
                                                                            .clone()
                                                                            .unwrap_or_default(),
                                                                    ),
                                                            )
                                                        },
                                                    ),
                                            )
                                            .when_some(notification.message.clone(), |d, msg| {
                                                d.child(
                                                    div()
                                                        .text_size(px(12.0))
                                                        .text_color(theme.tokens.muted_foreground)
                                                        .font_family(
                                                            theme.tokens.font_family.clone(),
                                                        )
                                                        .line_height(relative(1.4))
                                                        .child(msg),
                                                )
                                            })
                                            .when_some(notification.action.clone(), |d, action| {
                                                let handler = action.handler.clone();
                                                d.child(
                                                div().mt(px(4.0)).child(
                                                    Button::new(
                                                        ElementId::NamedChild(
                                                            Box::new(row_id.clone()),
                                                            "action".into(),
                                                        ),
                                                        action.label.clone(),
                                                    )
                                                    .variant(ButtonVariant::Outline)
                                                    .size(crate::components::button::ButtonSize::Sm)
                                                    .on_click(move |_, window, cx| {
                                                        cx.stop_propagation();
                                                        (handler)(window, cx);
                                                    }),
                                                ),
                                            )
                                            }),
                                    )
                                    .child(
                                        IconButton::new("x")
                                            .id(ElementId::NamedChild(
                                                Box::new(row_id),
                                                "dismiss".into(),
                                            ))
                                            .label(format!(
                                                "Dismiss {} notification",
                                                notification.title
                                            ))
                                            .variant(ButtonVariant::Ghost)
                                            .size(px(32.0))
                                            .icon_size(px(14.0))
                                            .on_click({
                                                let id = id.clone();
                                                move |_, _, cx| {
                                                    cx.stop_propagation();
                                                    state_for_dismiss.update(cx, |state, cx| {
                                                        state.remove(&id, cx);
                                                    });
                                                }
                                            }),
                                    );

                                div()
                                    .when(starts_group, |section| {
                                        section.child(
                                            div()
                                                .px(px(16.0))
                                                .py(px(8.0))
                                                .bg(theme.tokens.muted.opacity(0.35))
                                                .text_size(px(11.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.tokens.muted_foreground)
                                                .child(group),
                                        )
                                    })
                                    .child(row)
                            },
                        ),
                    ))
                    .max_h(px(350.0)),
                )
            })
            .when(show_more, |d| {
                let expanded_state = expanded_state.clone();
                let hidden_count = total_count - self.max_visible;
                d.child(
                    div()
                        .px(px(16.0))
                        .py(px(8.0))
                        .border_t_1()
                        .border_color(theme.tokens.border)
                        .flex()
                        .justify_center()
                        .child(
                            Button::new(
                                ElementId::NamedChild(
                                    Box::new(center_id.clone()),
                                    "show-more".into(),
                                ),
                                if expanded {
                                    "Show fewer".to_string()
                                } else {
                                    format!("Show {hidden_count} more")
                                },
                            )
                            .variant(ButtonVariant::Ghost)
                            .size(crate::components::button::ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                expanded_state.update(cx, |expanded, cx| {
                                    *expanded = !*expanded;
                                    cx.notify();
                                });
                            }),
                        ),
                )
            })
            .when(!is_empty, {
                let state_clone = state_entity.clone();
                |d| {
                    d.child(
                        div()
                            .flex()
                            .justify_center()
                            .px(px(16.0))
                            .py(px(8.0))
                            .border_t_1()
                            .border_color(theme.tokens.border)
                            .child(
                                Button::new(
                                    ElementId::NamedChild(
                                        Box::new(center_id.clone()),
                                        "clear-all".into(),
                                    ),
                                    "Clear all",
                                )
                                .variant(ButtonVariant::Ghost)
                                .size(crate::components::button::ButtonSize::Sm)
                                .on_click(move |_, _, cx| {
                                    state_clone.update(cx, |state, cx| {
                                        state.clear_all(cx);
                                    });
                                }),
                            ),
                    )
                }
            })
    }
}

#[derive(IntoElement)]
pub struct NotificationBell {
    id: ElementId,
    state: Entity<NotificationCenterState>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl NotificationBell {
    #[track_caller]
    pub fn new(state: Entity<NotificationCenterState>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "notification-bell:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            state,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl Styled for NotificationBell {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for NotificationBell {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let unread_count = self.state.read(cx).unread_count();
        let on_click = self.on_click.clone();
        let accessible_label = if unread_count == 0 {
            "Notifications".to_string()
        } else {
            format!("Notifications, {unread_count} unread")
        };
        let mut button = Button::new(self.id, "")
            .variant(ButtonVariant::Ghost)
            .size(crate::components::button::ButtonSize::Sm)
            .icon("bell")
            .tooltip(accessible_label);
        if let Some(handler) = on_click {
            button = button.on_click(move |event, window, cx| {
                (handler)(event, window, cx);
            });
        }

        div()
            .relative()
            .w(px(32.0))
            .h(px(32.0))
            .map(|mut this| {
                this.style().refine(&user_style);
                this
            })
            .child(button)
            .when(unread_count > 0, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(-5.0))
                        .right(px(-7.0))
                        .min_w(px(16.0))
                        .h(px(16.0))
                        .px(px(4.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(theme.tokens.destructive)
                        .text_size(px(9.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.tokens.destructive_foreground)
                        .font_family(theme.tokens.font_family.clone())
                        .child(
                            StyledText::new(SharedString::from(if unread_count > 99 {
                                "99+".to_string()
                            } else {
                                unread_count.to_string()
                            }))
                            .accessibility_hidden(true),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{NotificationCenter, NotificationCenterState, NotificationVariant};
    use crate::theme::Theme;
    use kael::{black, AppContext as _, TestAppContext};

    #[test]
    fn success_and_warning_route_through_semantic_tokens() {
        let mut theme = Theme::light();
        theme.tokens.success = black();
        assert_eq!(NotificationVariant::Success.color(&theme), black());
        assert_eq!(
            NotificationVariant::Warning.color(&theme),
            Theme::light().tokens.warning
        );
        assert_eq!(
            NotificationVariant::Error.color(&theme),
            theme.tokens.destructive
        );
    }

    #[test]
    fn notification_center_always_keeps_one_visible_slot() {
        let cx = TestAppContext::single();
        let state = cx.update(|cx| cx.new(NotificationCenterState::new));
        let center = NotificationCenter::new(state).max_visible(0);
        assert_eq!(center.max_visible, 1);
    }
}

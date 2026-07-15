use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};
use std::collections::HashSet;
use std::rc::Rc;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StepperSize {
    Sm,
    Md,
    Lg,
}

impl StepperSize {
    fn indicator_size(&self) -> Pixels {
        match self {
            StepperSize::Sm => px(24.0),
            StepperSize::Md => px(32.0),
            StepperSize::Lg => px(40.0),
        }
    }

    fn icon_size(&self) -> Pixels {
        match self {
            StepperSize::Sm => px(12.0),
            StepperSize::Md => px(16.0),
            StepperSize::Lg => px(20.0),
        }
    }

    fn title_size(&self) -> Pixels {
        match self {
            StepperSize::Sm => px(12.0),
            StepperSize::Md => px(14.0),
            StepperSize::Lg => px(16.0),
        }
    }

    fn description_size(&self) -> Pixels {
        match self {
            StepperSize::Sm => px(10.0),
            StepperSize::Md => px(12.0),
            StepperSize::Lg => px(14.0),
        }
    }

    fn connector_thickness(&self) -> Pixels {
        match self {
            StepperSize::Sm => px(2.0),
            StepperSize::Md => px(2.0),
            StepperSize::Lg => px(3.0),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StepperOrientation {
    Horizontal,
    Vertical,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Current,
    Completed,
    Error,
}

#[derive(Clone, Debug)]
pub struct StepItem {
    pub title: SharedString,
    pub description: Option<SharedString>,
    pub icon: Option<IconSource>,
    pub status: StepStatus,
}

impl StepItem {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            icon: None,
            status: StepStatus::Pending,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn status(mut self, status: StepStatus) -> Self {
        self.status = status;
        self
    }
}

pub struct StepperState {
    steps: Vec<StepItem>,
    current_step: usize,
    completed_steps: HashSet<usize>,
    linear: bool,
    focus_handle: FocusHandle,
}

impl StepperState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            steps: Vec::new(),
            current_step: 0,
            completed_steps: HashSet::new(),
            linear: true,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn with_steps(mut self, steps: Vec<StepItem>) -> Self {
        self.replace_steps(steps);
        self
    }

    pub fn with_linear(mut self, linear: bool) -> Self {
        self.linear = linear;
        self
    }

    pub fn steps(&self) -> &[StepItem] {
        &self.steps
    }

    pub fn set_steps(&mut self, steps: Vec<StepItem>, cx: &mut Context<Self>) {
        self.replace_steps(steps);
        cx.notify();
    }

    pub fn current_step(&self) -> usize {
        self.current_step
    }

    pub fn set_current_step(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.steps.len() && self.is_step_accessible(index) && self.current_step != index
        {
            self.current_step = index;
            cx.notify();
        }
    }

    pub fn is_linear(&self) -> bool {
        self.linear
    }

    pub fn set_linear(&mut self, linear: bool, cx: &mut Context<Self>) {
        if self.linear != linear {
            self.linear = linear;
            cx.notify();
        }
    }

    pub fn next(&mut self, cx: &mut Context<Self>) -> bool {
        if self.current_step + 1 < self.steps.len() {
            self.completed_steps.insert(self.current_step);
            self.current_step += 1;
            cx.notify();
            return true;
        }
        false
    }

    pub fn previous(&mut self, cx: &mut Context<Self>) -> bool {
        if self.current_step > 0 {
            self.current_step -= 1;
            cx.notify();
            return true;
        }
        false
    }

    pub fn go_to(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        if index < self.steps.len() && self.is_step_accessible(index) && self.current_step != index
        {
            self.current_step = index;
            cx.notify();
            return true;
        }
        false
    }

    pub fn mark_completed(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.steps.len() && self.completed_steps.insert(index) {
            cx.notify();
        }
    }

    pub fn unmark_completed(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.completed_steps.remove(&index) {
            cx.notify();
        }
    }

    pub fn is_completed(&self, index: usize) -> bool {
        self.completed_steps.contains(&index)
    }

    pub fn is_step_accessible(&self, index: usize) -> bool {
        if !self.linear {
            return true;
        }

        if index == 0 {
            return true;
        }

        if index <= self.current_step {
            return true;
        }

        for i in 0..index {
            if !self.completed_steps.contains(&i) && i != self.current_step {
                return false;
            }
        }

        index == self.current_step + 1 || self.completed_steps.contains(&(index - 1))
    }

    pub fn set_step_error(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.steps.len() {
            self.steps[index].status = StepStatus::Error;
            cx.notify();
        }
    }

    pub fn clear_step_error(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.steps.len() && self.steps[index].status == StepStatus::Error {
            self.steps[index].status = StepStatus::Pending;
            cx.notify();
        }
    }

    fn replace_steps(&mut self, steps: Vec<StepItem>) {
        self.completed_steps = steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| (step.status == StepStatus::Completed).then_some(index))
            .collect();
        self.current_step = steps
            .iter()
            .position(|step| step.status == StepStatus::Current)
            .or_else(|| {
                steps
                    .iter()
                    .position(|step| step.status != StepStatus::Completed)
            })
            .unwrap_or_else(|| steps.len().saturating_sub(1));
        self.steps = steps;
    }
}

impl Focusable for StepperState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for StepperState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[derive(IntoElement)]
pub struct Stepper {
    state: Entity<StepperState>,
    size: StepperSize,
    orientation: StepperOrientation,
    clickable: bool,
    show_connector: bool,
    accessibility_label: SharedString,
    on_step_change: Option<Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

impl Stepper {
    pub fn new(state: Entity<StepperState>) -> Self {
        Self {
            state,
            size: StepperSize::Md,
            orientation: StepperOrientation::Horizontal,
            clickable: true,
            show_connector: true,
            accessibility_label: "Progress".into(),
            on_step_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn size(mut self, size: StepperSize) -> Self {
        self.size = size;
        self
    }

    pub fn horizontal(mut self) -> Self {
        self.orientation = StepperOrientation::Horizontal;
        self
    }

    pub fn vertical(mut self) -> Self {
        self.orientation = StepperOrientation::Vertical;
        self
    }

    pub fn clickable(mut self, clickable: bool) -> Self {
        self.clickable = clickable;
        self
    }

    pub fn show_connector(mut self, show: bool) -> Self {
        self.show_connector = show;
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = label.into();
        self
    }

    pub fn on_step_change(
        mut self,
        handler: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_step_change = Some(Rc::new(handler));
        self
    }

    fn render_step_indicator(
        &self,
        index: usize,
        status: StepStatus,
        step: &StepItem,
        theme: &crate::theme::Theme,
    ) -> Div {
        let indicator_size = self.size.indicator_size();
        let icon_size = self.size.icon_size();

        let (bg_color, border_color, text_color) = match status {
            StepStatus::Completed => (
                theme.tokens.primary,
                theme.tokens.primary,
                theme.tokens.primary_foreground,
            ),
            StepStatus::Current => (
                theme.tokens.background,
                theme.tokens.primary,
                theme.tokens.primary,
            ),
            StepStatus::Error => (
                theme.tokens.destructive,
                theme.tokens.destructive,
                theme.tokens.destructive_foreground,
            ),
            StepStatus::Pending => (
                theme.tokens.background,
                theme.tokens.border,
                theme.tokens.muted_foreground,
            ),
        };

        div()
            .flex()
            .items_center()
            .justify_center()
            .size(indicator_size)
            .rounded(indicator_size / 2.0)
            .bg(bg_color)
            .border_2()
            .border_color(border_color)
            .flex_shrink_0()
            .child(match (status, &step.icon) {
                (StepStatus::Completed, _) => div()
                    .child(Icon::new("check").size(icon_size).color(text_color))
                    .into_any_element(),
                (StepStatus::Error, _) => div()
                    .child(Icon::new("x").size(icon_size).color(text_color))
                    .into_any_element(),
                (_, Some(icon_source)) => div()
                    .child(
                        Icon::new(icon_source.clone())
                            .size(icon_size)
                            .color(text_color),
                    )
                    .into_any_element(),
                (_, None) => div()
                    .text_size(self.size.title_size())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(text_color)
                    .child(format!("{}", index + 1))
                    .into_any_element(),
            })
    }

    fn render_connector(&self, is_completed: bool, theme: &crate::theme::Theme) -> Div {
        let thickness = self.size.connector_thickness();
        let color = if is_completed {
            theme.tokens.primary
        } else {
            theme.tokens.border
        };

        match self.orientation {
            StepperOrientation::Horizontal => div().flex_1().h(thickness).bg(color).mx(px(8.0)),
            StepperOrientation::Vertical => div()
                .w(thickness)
                .flex_1()
                .min_h(px(24.0))
                .bg(color)
                .my(px(4.0)),
        }
    }

    fn render_horizontal(
        self,
        window: &mut Window,
        theme: crate::theme::Theme,
        steps: Vec<StepItem>,
        current_step: usize,
        completed_steps: HashSet<usize>,
        linear: bool,
        user_style: StyleRefinement,
    ) -> Div {
        let steps_len = steps.len();
        let state = self.state.clone();
        let entity_id = state.entity_id().as_u64();

        div()
            .flex()
            .w_full()
            .map(|this| {
                let mut d = this;
                d.style().refine(&user_style);
                d
            })
            .children(steps.into_iter().enumerate().map(|(index, step)| {
                let status = if step.status == StepStatus::Error {
                    StepStatus::Error
                } else if index == current_step {
                    StepStatus::Current
                } else if step.status == StepStatus::Completed || completed_steps.contains(&index) {
                    StepStatus::Completed
                } else {
                    StepStatus::Pending
                };

                let is_accessible = !linear
                    || index == 0
                    || index <= current_step
                    || index == current_step + 1
                    || (index > 0 && completed_steps.contains(&(index - 1)));

                let is_last = index == steps_len - 1;
                let on_change = self.on_step_change.clone();
                let state_clone = state.clone();
                let supports_step_selection = self.clickable;
                let clickable = supports_step_selection && is_accessible;
                let mut accessibility_state = AccessibilityState::NONE;
                if status == StepStatus::Current {
                    accessibility_state |= AccessibilityState::SELECTED;
                }
                if status == StepStatus::Error {
                    accessibility_state |= AccessibilityState::INVALID;
                }
                if supports_step_selection && !is_accessible {
                    accessibility_state |= AccessibilityState::DISABLED;
                }
                let role = if supports_step_selection {
                    AccessibilityRole::Tab
                } else {
                    AccessibilityRole::ListItem
                };
                let status_description = match status {
                    StepStatus::Pending => "Pending step",
                    StepStatus::Current => "Current step",
                    StepStatus::Completed => "Completed step",
                    StepStatus::Error => "Step has an error",
                };
                let mut accessibility = AccessibilityAttributes::new(role)
                    .label(step.title.to_string())
                    .description(status_description)
                    .states(accessibility_state);
                if let Some(description) = step.description.as_ref() {
                    accessibility =
                        accessibility.description(format!("{status_description}. {description}"));
                }
                if clickable {
                    accessibility = accessibility.actions(vec![AccessibilityAction::Click]);
                }

                div()
                    .id(ElementId::NamedInteger(
                        format!("stepper-step-{entity_id}").into(),
                        index as u64,
                    ))
                    .accessibility(accessibility)
                    .when(clickable, |this| {
                        this.cursor(CursorStyle::PointingHand)
                            .on_click(window.listener_for(
                                &state_clone,
                                move |state, _, window, cx| {
                                    if state.go_to(index, cx) {
                                        if let Some(ref handler) = on_change {
                                            handler(index, window, cx);
                                        }
                                    }
                                },
                            ))
                    })
                    .flex()
                    .flex_1()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.0))
                            .when(!is_accessible, |this| this.opacity(0.5))
                            .child(self.render_step_indicator(index, status, &step, &theme))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(2.0))
                                    .child(
                                        div()
                                            .text_size(self.size.title_size())
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(if status == StepStatus::Current {
                                                theme.tokens.foreground
                                            } else {
                                                theme.tokens.muted_foreground
                                            })
                                            .child(step.title.clone()),
                                    )
                                    .when_some(step.description.clone(), |this, desc| {
                                        this.child(
                                            div()
                                                .text_size(self.size.description_size())
                                                .text_color(theme.tokens.muted_foreground)
                                                .child(desc),
                                        )
                                    }),
                            ),
                    )
                    .when(self.show_connector && !is_last, |this| {
                        this.child(self.render_connector(completed_steps.contains(&index), &theme))
                    })
            }))
    }

    fn render_vertical(
        self,
        window: &mut Window,
        theme: crate::theme::Theme,
        steps: Vec<StepItem>,
        current_step: usize,
        completed_steps: HashSet<usize>,
        linear: bool,
        user_style: StyleRefinement,
    ) -> Div {
        let steps_len = steps.len();
        let state = self.state.clone();
        let entity_id = state.entity_id().as_u64();

        div()
            .flex()
            .flex_col()
            .map(|this| {
                let mut d = this;
                d.style().refine(&user_style);
                d
            })
            .children(steps.into_iter().enumerate().map(|(index, step)| {
                let status = if step.status == StepStatus::Error {
                    StepStatus::Error
                } else if index == current_step {
                    StepStatus::Current
                } else if step.status == StepStatus::Completed || completed_steps.contains(&index) {
                    StepStatus::Completed
                } else {
                    StepStatus::Pending
                };

                let is_accessible = !linear
                    || index == 0
                    || index <= current_step
                    || index == current_step + 1
                    || (index > 0 && completed_steps.contains(&(index - 1)));

                let is_last = index == steps_len - 1;
                let on_change = self.on_step_change.clone();
                let state_clone = state.clone();
                let supports_step_selection = self.clickable;
                let clickable = supports_step_selection && is_accessible;
                let indicator_size = self.size.indicator_size();
                let mut accessibility_state = AccessibilityState::NONE;
                if status == StepStatus::Current {
                    accessibility_state |= AccessibilityState::SELECTED;
                }
                if status == StepStatus::Error {
                    accessibility_state |= AccessibilityState::INVALID;
                }
                if supports_step_selection && !is_accessible {
                    accessibility_state |= AccessibilityState::DISABLED;
                }
                let role = if supports_step_selection {
                    AccessibilityRole::Tab
                } else {
                    AccessibilityRole::ListItem
                };
                let status_description = match status {
                    StepStatus::Pending => "Pending step",
                    StepStatus::Current => "Current step",
                    StepStatus::Completed => "Completed step",
                    StepStatus::Error => "Step has an error",
                };
                let mut accessibility = AccessibilityAttributes::new(role)
                    .label(step.title.to_string())
                    .description(status_description)
                    .states(accessibility_state);
                if let Some(description) = step.description.as_ref() {
                    accessibility =
                        accessibility.description(format!("{status_description}. {description}"));
                }
                if clickable {
                    accessibility = accessibility.actions(vec![AccessibilityAction::Click]);
                }

                div()
                    .id(ElementId::NamedInteger(
                        format!("stepper-step-{entity_id}").into(),
                        index as u64,
                    ))
                    .accessibility(accessibility)
                    .when(clickable, |this| {
                        this.cursor(CursorStyle::PointingHand)
                            .on_click(window.listener_for(
                                &state_clone,
                                move |state, _, window, cx| {
                                    if state.go_to(index, cx) {
                                        if let Some(ref handler) = on_change {
                                            handler(index, window, cx);
                                        }
                                    }
                                },
                            ))
                    })
                    .flex()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                div().when(!is_accessible, |this| this.opacity(0.5)).child(
                                    self.render_step_indicator(index, status, &step, &theme),
                                ),
                            )
                            .when(self.show_connector && !is_last, |this| {
                                this.child(
                                    self.render_connector(completed_steps.contains(&index), &theme),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .pl(px(12.0))
                            .pt(px(4.0))
                            .pb(if is_last { px(0.0) } else { px(24.0) })
                            .min_h(indicator_size + px(8.0))
                            .child(
                                div()
                                    .text_size(self.size.title_size())
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if status == StepStatus::Current {
                                        theme.tokens.foreground
                                    } else {
                                        theme.tokens.muted_foreground
                                    })
                                    .child(step.title.clone()),
                            )
                            .when_some(step.description.clone(), |this, desc| {
                                this.child(
                                    div()
                                        .text_size(self.size.description_size())
                                        .text_color(theme.tokens.muted_foreground)
                                        .mt(px(2.0))
                                        .child(desc),
                                )
                            }),
                    )
            }))
    }
}

impl Styled for Stepper {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Stepper {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let state = self.state.read(cx);
        let steps = state.steps.clone();
        let current_step = state.current_step;
        let completed_steps = state.completed_steps.clone();
        let linear = state.linear;
        let steps_len = steps.len();
        let focus_handle = state.focus_handle.clone();
        let is_focused = focus_handle.is_focused(window);
        let entity_id = self.state.entity_id().as_u64();
        let state_entity = self.state.clone();
        let on_step_change = self.on_step_change.clone();
        let clickable = self.clickable;
        let accessibility_label = self.accessibility_label.clone();
        let user_style = self.style.clone();

        let content = match self.orientation {
            StepperOrientation::Horizontal => self.render_horizontal(
                window,
                theme,
                steps,
                current_step,
                completed_steps,
                linear,
                user_style,
            ),
            StepperOrientation::Vertical => self.render_vertical(
                window,
                theme,
                steps,
                current_step,
                completed_steps,
                linear,
                user_style,
            ),
        };

        let mut group_state = AccessibilityState::NONE;
        if is_focused {
            group_state |= AccessibilityState::FOCUSED;
        }
        div()
            .id(("stepper", entity_id))
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(accessibility_label.to_string())
                    .value(AccessibilityValue::Text(format!(
                        "Step {} of {}",
                        current_step.saturating_add(1).min(steps_len),
                        steps_len
                    )))
                    .states(group_state),
            )
            .when(clickable && steps_len > 0, |this| {
                this.track_focus(&focus_handle.tab_index(0).tab_stop(true))
                    .on_key_down(move |event: &KeyDownEvent, window, cx| {
                        let key = event.keystroke.key.as_str();
                        let handled =
                            matches!(key, "left" | "up" | "right" | "down" | "home" | "end");
                        if handled {
                            state_entity.update(cx, |state, cx| {
                                let changed = match key {
                                    "left" | "up" => state.previous(cx),
                                    "right" | "down" => state.next(cx),
                                    "home" => state.go_to(0, cx),
                                    "end" => state
                                        .steps
                                        .len()
                                        .checked_sub(1)
                                        .is_some_and(|index| state.go_to(index, cx)),
                                    _ => false,
                                };
                                if changed {
                                    if let Some(handler) = on_step_change.as_ref() {
                                        handler(state.current_step, window, cx);
                                    }
                                }
                            });
                            cx.stop_propagation();
                            window.prevent_default();
                        }
                    })
            })
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::{StepItem, StepStatus, StepperState};
    use kael::{AppContext, TestAppContext};

    #[kael::test]
    fn initial_step_statuses_seed_progress_state(cx: &mut TestAppContext) {
        let state = cx.new(|cx| {
            StepperState::new(cx).with_steps(vec![
                StepItem::new("Account").status(StepStatus::Completed),
                StepItem::new("Profile").status(StepStatus::Current),
                StepItem::new("Confirm"),
            ])
        });

        cx.read(|cx| {
            let state = state.read(cx);
            assert_eq!(state.current_step(), 1);
            assert!(state.is_completed(0));
            assert!(state.is_step_accessible(2));
        });
    }

    #[kael::test]
    fn advancing_marks_the_previous_step_once(cx: &mut TestAppContext) {
        let state = cx.new(|cx| {
            StepperState::new(cx).with_steps(vec![StepItem::new("One"), StepItem::new("Two")])
        });

        cx.update(|cx| {
            state.update(cx, |state, cx| {
                assert!(state.next(cx));
                assert!(!state.next(cx));
                assert_eq!(state.current_step(), 1);
                assert!(state.is_completed(0));
            });
        });
    }
}

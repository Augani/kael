//! Drag and drop components with draggable elements and drop zones.

use kael::{prelude::FluentBuilder as _, *};
use std::fmt::Debug;

use crate::theme::{Theme, use_theme};

use std::rc::Rc;

pub struct DragData<T: Clone + Debug> {
    pub data: T,
    pub label: Option<SharedString>,
    pub preview_factory: Option<Rc<dyn Fn() -> AnyElement>>,
    pub position: Point<Pixels>,
}
impl<T: Clone + Debug> Clone for DragData<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            label: self.label.clone(),
            preview_factory: self.preview_factory.clone(),
            position: self.position,
        }
    }
}
impl<T: Clone + Debug> Debug for DragData<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragData")
            .field("has_label", &self.label.is_some())
            .field("has_preview", &self.preview_factory.is_some())
            .finish()
    }
}

impl<T: Clone + Debug> DragData<T> {
    pub fn new(data: T) -> Self {
        Self {
            data,
            label: None,
            preview_factory: None,
            position: Point::default(),
        }
    }

    pub fn with_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_preview<F>(mut self, factory: F) -> Self
    where
        F: Fn() -> AnyElement + 'static,
    {
        self.preview_factory = Some(Rc::new(factory));
        self
    }

    pub fn with_position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl<T: Clone + Debug + 'static> Render for DragData<T> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();

        if let Some(factory) = &self.preview_factory {
            let preview = factory();
            return div()
                .absolute()
                .left(self.position.x)
                .top(self.position.y)
                .child(preview);
        }

        let size = kael::size(px(250.0), px(80.0));

        div()
            .pl(self.position.x - size.width / 2.0)
            .pt(self.position.y - size.height / 2.0)
            .child(
                div()
                    .flex()
                    .justify_center()
                    .items_center()
                    .min_w(size.width)
                    .max_w(px(300.0))
                    .min_h(size.height)
                    .px(px(16.0))
                    .py(px(12.0))
                    .bg(theme.tokens.card.opacity(0.95))
                    .border_1()
                    .border_color(theme.tokens.border)
                    .text_color(theme.tokens.foreground)
                    .font_family(theme.tokens.font_family.clone())
                    .text_size(px(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .rounded(theme.tokens.radius_md)
                    .shadow(smallvec::smallvec![BoxShadow {
                        color: hsla(0.0, 0.0, 0.0, 0.3),
                        offset: point(px(0.0), px(4.0)),
                        blur_radius: px(12.0),
                        spread_radius: px(0.0),
                        inset: false,
                    }])
                    .when_some(self.label.clone(), |this, label| this.child(label))
                    .when(self.label.is_none(), |this| this.child("Dragging...")),
            )
    }
}

/// Shared state that provides a keyboard alternative to pointer drag and drop.
pub struct DragDropKeyboardState<T: Clone + Debug + 'static> {
    grabbed: Option<(ElementId, DragData<T>)>,
}

impl<T: Clone + Debug + 'static> DragDropKeyboardState<T> {
    pub fn new() -> Self {
        Self { grabbed: None }
    }

    pub fn has_grabbed_item(&self) -> bool {
        self.grabbed.is_some()
    }

    pub fn grabbed_data(&self) -> Option<&DragData<T>> {
        self.grabbed.as_ref().map(|(_, data)| data)
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.grabbed.take().is_some() {
            cx.notify();
        }
    }

    fn toggle(&mut self, id: ElementId, data: DragData<T>, cx: &mut Context<Self>) {
        if self
            .grabbed
            .as_ref()
            .is_some_and(|(source_id, _)| *source_id == id)
        {
            self.grabbed = None;
        } else {
            self.grabbed = Some((id, data));
        }
        cx.notify();
    }

    fn take(&mut self, cx: &mut Context<Self>) -> Option<DragData<T>> {
        let data = self.grabbed.take().map(|(_, data)| data);
        if data.is_some() {
            cx.notify();
        }
        data
    }
}

impl<T: Clone + Debug + 'static> Default for DragDropKeyboardState<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Debug + 'static> Render for DragDropKeyboardState<T> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(IntoElement)]
pub struct Draggable<T: Clone + Debug + 'static> {
    id: ElementId,
    base: Stateful<Div>,
    drag_data: DragData<T>,
    keyboard_state: Option<Entity<DragDropKeyboardState<T>>>,
    accessibility_label: SharedString,
    disabled: bool,
    cursor_style: CursorStyle,
    hover_bg: Option<Hsla>,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl<T: Clone + Debug + 'static> Draggable<T> {
    pub fn new(id: impl Into<ElementId>, drag_data: DragData<T>) -> Self {
        let id = id.into();
        let accessibility_label = drag_data
            .label
            .clone()
            .unwrap_or_else(|| "Draggable item".into());
        Self {
            id: id.clone(),
            base: div().id(id),
            drag_data,
            keyboard_state: None,
            accessibility_label,
            disabled: false,
            cursor_style: CursorStyle::PointingHand,
            hover_bg: None,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn keyboard_state(mut self, state: &Entity<DragDropKeyboardState<T>>) -> Self {
        self.keyboard_state = Some(state.clone());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = label.into();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn cursor_style(mut self, cursor: CursorStyle) -> Self {
        self.cursor_style = cursor;
        self
    }

    pub fn hover_bg(mut self, color: Hsla) -> Self {
        self.hover_bg = Some(color);
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn children<I>(mut self, children: impl IntoIterator<Item = I>) -> Self
    where
        I: IntoElement,
    {
        for child in children {
            self.children.push(child.into_any_element());
        }
        self
    }
}

impl<T: Clone + Debug + 'static> Styled for Draggable<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + Debug + 'static> ParentElement for Draggable<T> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl<T: Clone + Debug + 'static> RenderOnce for Draggable<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let drag_data = self.drag_data.clone();
        let drag_data_for_key = self.drag_data.clone();
        let user_style = self.style;
        let keyboard_state = self.keyboard_state.clone();
        let keyboard_enabled = keyboard_state.is_some() && !self.disabled;
        let is_grabbed = keyboard_state.as_ref().is_some_and(|state| {
            state
                .read(cx)
                .grabbed
                .as_ref()
                .is_some_and(|(source_id, _)| *source_id == self.id)
        });
        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let keyboard_state_for_key = keyboard_state.clone();
        let source_id = self.id.clone();
        let mut states = AccessibilityState::NONE;
        if self.disabled {
            states |= AccessibilityState::DISABLED;
        }
        if is_grabbed {
            states |= AccessibilityState::SELECTED;
        }

        self.base
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(self.accessibility_label.to_string())
                    .description(if is_grabbed {
                        "Picked up. Focus a compatible drop zone and press Enter to drop"
                    } else if keyboard_enabled {
                        "Press Enter or Space to pick up"
                    } else {
                        "Draggable item"
                    })
                    .states(states)
                    .actions(if keyboard_enabled {
                        vec![AccessibilityAction::Focus]
                    } else {
                        Vec::new()
                    }),
            )
            .cursor(if self.disabled {
                CursorStyle::Arrow
            } else {
                self.cursor_style
            })
            .when(self.disabled, |this| this.opacity(0.5))
            .when_some(self.hover_bg.filter(|_| !self.disabled), |this, bg| {
                this.hover(move |style| style.bg(bg))
            })
            .when(!self.disabled, |this| {
                this.on_drag(drag_data, |data: &DragData<T>, position, _, cx| {
                    cx.new(|_| data.clone().with_position(position))
                })
            })
            .when(keyboard_enabled, |this| {
                this.track_focus(&focus_handle.tab_index(0).tab_stop(true))
                    .on_key_down(
                        move |event, window, cx| match event.keystroke.key.as_str() {
                            "enter" | "space" => {
                                if let Some(state) = keyboard_state_for_key.as_ref() {
                                    state.update(cx, |state, cx| {
                                        state.toggle(
                                            source_id.clone(),
                                            drag_data_for_key.clone(),
                                            cx,
                                        );
                                    });
                                }
                                cx.stop_propagation();
                                window.prevent_default();
                            }
                            "escape" => {
                                if let Some(state) = keyboard_state_for_key.as_ref() {
                                    state.update(cx, DragDropKeyboardState::clear);
                                }
                                cx.stop_propagation();
                                window.prevent_default();
                            }
                            _ => {}
                        },
                    )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .children(self.children)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropZoneStyle {
    Dashed,
    Solid,
    Filled,
}

#[derive(IntoElement)]
pub struct DropZone<T: Clone + Debug + 'static> {
    id: ElementId,
    base: Stateful<Div>,
    keyboard_state: Option<Entity<DragDropKeyboardState<T>>>,
    accessibility_label: SharedString,
    disabled: bool,
    drop_style: DropZoneStyle,
    active: bool,
    min_height: Option<Pixels>,
    children: Vec<AnyElement>,
    user_style: StyleRefinement,
    on_drop: Option<Rc<dyn Fn(&DragData<T>, &mut Window, &mut App)>>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Clone + Debug + 'static> DropZone<T> {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            base: div().id(id),
            keyboard_state: None,
            accessibility_label: "Drop zone".into(),
            disabled: false,
            drop_style: DropZoneStyle::Dashed,
            active: false,
            min_height: None,
            children: Vec::new(),
            user_style: StyleRefinement::default(),
            on_drop: None,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn keyboard_state(mut self, state: &Entity<DragDropKeyboardState<T>>) -> Self {
        self.keyboard_state = Some(state.clone());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = label.into();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn drop_zone_style(mut self, style: DropZoneStyle) -> Self {
        self.drop_style = style;
        self
    }

    pub fn on_drop<F>(mut self, handler: F) -> Self
    where
        F: Fn(&DragData<T>, &mut Window, &mut App) + 'static,
    {
        self.on_drop = Some(Rc::new(handler));
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn min_h(mut self, height: impl Into<Pixels>) -> Self {
        self.min_height = Some(height.into());
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn children<I>(mut self, children: impl IntoIterator<Item = I>) -> Self
    where
        I: IntoElement,
    {
        for child in children {
            self.children.push(child.into_any_element());
        }
        self
    }
}

impl<T: Clone + Debug + 'static> Styled for DropZone<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.user_style
    }
}

impl<T: Clone + Debug + 'static> InteractiveElement for DropZone<T> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl<T: Clone + Debug + 'static> StatefulInteractiveElement for DropZone<T> {}

impl<T: Clone + Debug + 'static> ParentElement for DropZone<T> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl<T: Clone + Debug + 'static> RenderOnce for DropZone<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let user_style = self.user_style;
        let on_drop = self.on_drop.clone();
        let keyboard_state = self.keyboard_state.clone();
        let keyboard_enabled = keyboard_state.is_some() && on_drop.is_some() && !self.disabled;
        let has_grabbed_item = keyboard_state
            .as_ref()
            .is_some_and(|state| state.read(cx).has_grabbed_item());
        let active = self.active || has_grabbed_item;
        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let keyboard_state_for_key = keyboard_state.clone();
        let on_drop_for_key = on_drop.clone();
        let mut accessibility_state = AccessibilityState::NONE;
        if self.disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }

        let (border_width, border_color, bg_color) = match (self.drop_style, active) {
            (DropZoneStyle::Dashed, false) => {
                (px(2.0), theme.tokens.border, kael::transparent_black())
            }
            (DropZoneStyle::Dashed, true) => (
                px(2.0),
                theme.tokens.primary,
                theme.tokens.primary.opacity(0.05),
            ),
            (DropZoneStyle::Solid, false) => {
                (px(2.0), theme.tokens.border, kael::transparent_black())
            }
            (DropZoneStyle::Solid, true) => (
                px(2.0),
                theme.tokens.primary,
                theme.tokens.primary.opacity(0.1),
            ),
            (DropZoneStyle::Filled, false) => (px(1.0), theme.tokens.border, theme.tokens.muted),
            (DropZoneStyle::Filled, true) => (
                px(2.0),
                theme.tokens.primary,
                theme.tokens.primary.opacity(0.15),
            ),
        };

        self.base
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(self.accessibility_label.to_string())
                    .description(if has_grabbed_item {
                        "Compatible item ready. Press Enter or Space to drop"
                    } else if keyboard_enabled {
                        "Pick up a compatible draggable item first"
                    } else {
                        "Drop zone"
                    })
                    .states(accessibility_state)
                    .actions(if keyboard_enabled {
                        vec![AccessibilityAction::Focus]
                    } else {
                        Vec::new()
                    }),
            )
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .w_full()
            .when_some(self.min_height, |this, h| this.min_h(h))
            .px(px(16.0))
            .py(px(16.0))
            .rounded(theme.tokens.radius_lg)
            .bg(bg_color)
            .border_color(border_color)
            .when(self.drop_style == DropZoneStyle::Dashed, |this| {
                this.border(border_width)
            })
            .when(self.drop_style != DropZoneStyle::Dashed, |this| {
                this.border(border_width)
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .when(!self.disabled, |this| {
                this.can_drop(|value, _, _| value.downcast_ref::<DragData<T>>().is_some())
                    .on_drop(move |data: &DragData<T>, window, cx| {
                        if let Some(on_drop) = &on_drop {
                            on_drop(data, window, cx);
                        }
                    })
            })
            .when(keyboard_enabled, |this| {
                this.track_focus(&focus_handle.tab_index(0).tab_stop(true))
                    .on_key_down(
                        move |event, window, cx| match event.keystroke.key.as_str() {
                            "enter" | "space" if has_grabbed_item => {
                                let data = keyboard_state_for_key.as_ref().and_then(|state| {
                                    state.update(cx, DragDropKeyboardState::take)
                                });
                                if let (Some(data), Some(handler)) =
                                    (data.as_ref(), on_drop_for_key.as_ref())
                                {
                                    handler(data, window, cx);
                                }
                                cx.stop_propagation();
                                window.prevent_default();
                            }
                            "escape" => {
                                if let Some(state) = keyboard_state_for_key.as_ref() {
                                    state.update(cx, DragDropKeyboardState::clear);
                                }
                                cx.stop_propagation();
                                window.prevent_default();
                            }
                            _ => {}
                        },
                    )
            })
            .children(self.children)
    }
}

#[cfg(test)]
mod tests {
    use super::{DragData, DragDropKeyboardState, Draggable, DropZone};
    use kael::{
        AccessibilityAction, AccessibilityActionRequest, AppContext, Context, Entity, IntoElement,
        ParentElement, Render, SharedString, Styled, TestAppContext, Window, div,
    };
    use std::{cell::RefCell, rc::Rc};

    struct DragDropHost {
        state: Entity<DragDropKeyboardState<SharedString>>,
        dropped: Rc<RefCell<Option<SharedString>>>,
    }

    impl Render for DragDropHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let dropped = self.dropped.clone();
            div()
                .flex()
                .child(
                    Draggable::new(
                        "keyboard-drag-source",
                        DragData::new(SharedString::from("Release notes"))
                            .with_label("Release notes card"),
                    )
                    .keyboard_state(&self.state),
                )
                .child(
                    DropZone::new("keyboard-drop-target")
                        .keyboard_state(&self.state)
                        .accessibility_label("Publish queue")
                        .on_drop(move |data, _, _| {
                            *dropped.borrow_mut() = Some(data.data.clone());
                        }),
                )
        }
    }

    #[kael::test]
    fn keyboard_pick_up_and_drop_routes_data(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(|_| DragDropKeyboardState::new());
        let dropped = Rc::new(RefCell::new(None));
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            let dropped = dropped.clone();
            move |_, _| DragDropHost { state, dropped }
        });

        let (source_id, target_id) = window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            let source = tree
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Release notes card"))
                .expect("draggable source should be accessible");
            let target = tree
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Publish queue"))
                .expect("drop target should be accessible");
            assert!(source.actions.contains(&AccessibilityAction::Focus));
            assert!(target.actions.contains(&AccessibilityAction::Focus));
            (source.id, target.id)
        });

        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                source_id,
                AccessibilityAction::Focus,
            ));
        });
        window.run_until_parked();
        window.simulate_keystrokes("enter");
        window.update(|_, cx| assert!(state.read(cx).has_grabbed_item()));

        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                target_id,
                AccessibilityAction::Focus,
            ));
        });
        window.run_until_parked();
        window.simulate_keystrokes("enter");

        window.update(|_, cx| assert!(!state.read(cx).has_grabbed_item()));
        assert_eq!(
            dropped.borrow().as_ref().map(|value| value.as_ref()),
            Some("Release notes")
        );
    }
}

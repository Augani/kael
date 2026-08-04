//! Text field component - Simple text input field.

use crate::styled_ext::StyledExt;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};
use std::ops::Range;
use std::rc::Rc;
use unicode_segmentation::UnicodeSegmentation;

actions!(
    text_field,
    [
        TextFieldBackspace,
        TextFieldDelete,
        TextFieldLeft,
        TextFieldRight,
        TextFieldHome,
        TextFieldEnd,
        TextFieldSelectAll,
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", TextFieldBackspace, Some("TextField")),
        KeyBinding::new("delete", TextFieldDelete, Some("TextField")),
        KeyBinding::new("left", TextFieldLeft, Some("TextField")),
        KeyBinding::new("right", TextFieldRight, Some("TextField")),
        KeyBinding::new("home", TextFieldHome, Some("TextField")),
        KeyBinding::new("end", TextFieldEnd, Some("TextField")),
        KeyBinding::new("cmd-a", TextFieldSelectAll, Some("TextField")),
        KeyBinding::new("ctrl-a", TextFieldSelectAll, Some("TextField")),
    ]);
}

type TextFieldChangeHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TextFieldSize {
    Sm,
    Md,
    Lg,
}

pub struct TextFieldState {
    focus_handle: FocusHandle,
    text: String,
    cursor_position: usize,
    selected_range: Range<usize>,
    marked_range: Option<Range<usize>>,
    on_change: Option<TextFieldChangeHandler>,
}

impl TextFieldState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            text: String::new(),
            cursor_position: 0,
            selected_range: 0..0,
            marked_range: None,
            on_change: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.text = text;
        self.cursor_position = self.text.len();
        self.selected_range = self.cursor_position..self.cursor_position;
        cx.notify();
    }

    fn previous_boundary(&self) -> usize {
        self.text
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < self.cursor_position).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self) -> usize {
        self.text
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > self.cursor_position).then_some(index))
            .unwrap_or(self.text.len())
    }

    fn delete_backward(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.selected_range.is_empty() {
            self.text.replace_range(self.selected_range.clone(), "");
            self.cursor_position = self.selected_range.start;
            self.selected_range = self.cursor_position..self.cursor_position;
            self.marked_range = None;
            cx.notify();
            return true;
        }
        if self.cursor_position == 0 {
            return false;
        }
        let previous = self.previous_boundary();
        self.text.replace_range(previous..self.cursor_position, "");
        self.cursor_position = previous;
        self.selected_range = previous..previous;
        self.marked_range = None;
        cx.notify();
        true
    }

    fn delete_forward(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.selected_range.is_empty() {
            self.text.replace_range(self.selected_range.clone(), "");
            self.cursor_position = self.selected_range.start;
            self.selected_range = self.cursor_position..self.cursor_position;
            self.marked_range = None;
            cx.notify();
            return true;
        }
        if self.cursor_position >= self.text.len() {
            return false;
        }
        let next = self.next_boundary();
        self.text.replace_range(self.cursor_position..next, "");
        self.selected_range = self.cursor_position..self.cursor_position;
        self.marked_range = None;
        cx.notify();
        true
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        self.text
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(self.text.len()))
            .find(|byte_offset| self.text[..*byte_offset].encode_utf16().count() >= offset)
            .unwrap_or(self.text.len())
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len());
        let boundary = self
            .text
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(self.text.len()))
            .take_while(|index| *index <= offset)
            .last()
            .unwrap_or(0);
        self.text[..boundary].encode_utf16().count()
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn notify_change(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handler) = self.on_change.clone() {
            handler(&self.text, window, cx);
        }
    }
}

impl Focusable for TextFieldState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextFieldState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end));
        Some(self.text[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.offset_to_utf16(self.selected_range.start)
                ..self.offset_to_utf16(self.selected_range.end),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .unwrap_or_else(|| {
                if self.selected_range.is_empty() {
                    self.cursor_position..self.cursor_position
                } else {
                    self.selected_range.clone()
                }
            });
        self.text.replace_range(range.clone(), new_text);
        self.cursor_position = range.start + new_text.len();
        self.selected_range = self.cursor_position..self.cursor_position;
        self.marked_range = None;
        self.notify_change(window, cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| {
                if self.selected_range.is_empty() {
                    self.cursor_position..self.cursor_position
                } else {
                    self.selected_range.clone()
                }
            });
        self.text.replace_range(range.clone(), new_text);
        self.cursor_position = range.start + new_text.len();

        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }

        if let Some(sel_range) = new_selected_range_utf16 {
            let selection = self.range_from_utf16(&sel_range);
            self.cursor_position = selection.end;
            self.selected_range = selection;
        } else {
            self.selected_range = self.cursor_position..self.cursor_position;
        }

        self.notify_change(window, cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        None
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Render for TextFieldState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[derive(IntoElement)]
pub struct TextField {
    state: Entity<TextFieldState>,
    size: TextFieldSize,
    placeholder: Option<SharedString>,
    accessibility_label: SharedString,
    disabled: bool,
    invalid: bool,
    on_change: Option<TextFieldChangeHandler>,
    style: StyleRefinement,
}

impl TextField {
    pub fn new(cx: &mut App) -> Self {
        let state = cx.new(TextFieldState::new);
        Self::from_state(state)
    }

    /// Creates a text field backed by caller-owned state.
    ///
    /// Use this constructor when the field is rebuilt during a parent render so
    /// its value, cursor and focus identity persist across frames.
    pub fn from_state(state: Entity<TextFieldState>) -> Self {
        Self {
            state,
            size: TextFieldSize::Md,
            placeholder: None,
            accessibility_label: "Text field".into(),
            disabled: false,
            invalid: false,
            on_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn size(mut self, size: TextFieldSize) -> Self {
        self.size = size;
        self
    }

    pub fn placeholder<T: Into<SharedString>>(mut self, placeholder: T) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = label.into();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn value<T: Into<String>>(self, value: T, cx: &mut App) -> Self {
        self.state.update(cx, |state, cx| {
            state.set_text(value.into(), cx);
        });
        self
    }

    pub fn text(&self, cx: &App) -> String {
        self.state.read(cx).text().to_string()
    }

    pub fn on_change<F: Fn(&str, &mut Window, &mut App) + 'static>(mut self, f: F) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for TextField {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let on_change = self.on_change.clone();
        self.state.update(cx, |state, _| {
            state.on_change = on_change;
        });
        let user_style = self.style;

        let (height, padding_x, text_size) = match self.size {
            TextFieldSize::Sm => (px(28.0), px(10.0), px(13.0)),
            TextFieldSize::Md => (px(32.0), px(12.0), px(14.0)),
            TextFieldSize::Lg => (px(36.0), px(14.0), px(14.0)),
        };

        let focus_handle = self.state.read(cx).focus_handle.clone();
        let is_focused = focus_handle.is_focused(window);
        let focus_handle_for_mouse = focus_handle.clone();
        let state_for_backspace = self.state.clone();
        let state_for_delete = self.state.clone();
        let state_for_left = self.state.clone();
        let state_for_right = self.state.clone();
        let state_for_home = self.state.clone();
        let state_for_end = self.state.clone();
        let state_for_select_all = self.state.clone();
        let on_change_for_backspace = self.on_change.clone();
        let on_change_for_delete = self.on_change.clone();
        let text_content = self.state.read(cx).text().to_string();
        let cursor_position = self.state.read(cx).cursor_position;
        let selected_range = self.state.read(cx).selected_range.clone();
        let disabled = self.disabled;
        let mut accessibility_state = AccessibilityState::NONE;
        if disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        if self.invalid {
            accessibility_state |= AccessibilityState::INVALID;
        }
        if is_focused {
            accessibility_state |= AccessibilityState::FOCUSED;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::TextInput)
            .label(self.accessibility_label.to_string())
            .value(AccessibilityValue::Text(text_content.clone()))
            .states(accessibility_state);
        if let Some(placeholder) = self.placeholder.as_ref() {
            accessibility = accessibility.placeholder(placeholder.to_string());
        }
        if !disabled {
            accessibility = accessibility.actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::SetValue,
            ]);
        }

        let border_color = if self.invalid {
            theme.tokens.destructive
        } else if is_focused {
            theme.tokens.ring
        } else {
            theme.tokens.input
        };

        let mut base = div()
            .id(("text-field", self.state.entity_id()))
            .accessibility(accessibility)
            .key_context("TextField")
            .when(!disabled, |this| {
                this.track_focus(&focus_handle.clone().tab_index(0).tab_stop(true))
                    .on_action(move |_: &TextFieldBackspace, window, cx| {
                        let changed =
                            state_for_backspace.update(cx, |state, cx| state.delete_backward(cx));
                        if changed && let Some(handler) = on_change_for_backspace.as_ref() {
                            let text = state_for_backspace.read(cx).text.clone();
                            handler(&text, window, cx);
                        }
                    })
                    .on_action(move |_: &TextFieldDelete, window, cx| {
                        let changed =
                            state_for_delete.update(cx, |state, cx| state.delete_forward(cx));
                        if changed && let Some(handler) = on_change_for_delete.as_ref() {
                            let text = state_for_delete.read(cx).text.clone();
                            handler(&text, window, cx);
                        }
                    })
                    .on_action(move |_: &TextFieldLeft, _, cx| {
                        state_for_left.update(cx, |state, cx| {
                            let position = state.previous_boundary();
                            if position != state.cursor_position {
                                state.cursor_position = position;
                                state.selected_range = position..position;
                                state.marked_range = None;
                                cx.notify();
                            }
                        });
                    })
                    .on_action(move |_: &TextFieldRight, _, cx| {
                        state_for_right.update(cx, |state, cx| {
                            let position = state.next_boundary();
                            if position != state.cursor_position {
                                state.cursor_position = position;
                                state.selected_range = position..position;
                                state.marked_range = None;
                                cx.notify();
                            }
                        });
                    })
                    .on_action(move |_: &TextFieldHome, _, cx| {
                        state_for_home.update(cx, |state, cx| {
                            if state.cursor_position != 0 {
                                state.cursor_position = 0;
                                state.selected_range = 0..0;
                                state.marked_range = None;
                                cx.notify();
                            }
                        });
                    })
                    .on_action(move |_: &TextFieldEnd, _, cx| {
                        state_for_end.update(cx, |state, cx| {
                            let end = state.text.len();
                            if state.cursor_position != end {
                                state.cursor_position = end;
                                state.selected_range = end..end;
                                state.marked_range = None;
                                cx.notify();
                            }
                        });
                    })
                    .on_action(move |_: &TextFieldSelectAll, _, cx| {
                        state_for_select_all.update(cx, |state, cx| {
                            if !state.text.is_empty() {
                                state.selected_range = 0..state.text.len();
                                state.cursor_position = state.text.len();
                                state.marked_range = None;
                                cx.notify();
                            }
                        });
                    })
            })
            .h(height)
            .px(padding_x)
            .bg(theme.tokens.card)
            .border_1()
            .border_color(border_color)
            .rounded(theme.tokens.radius_md);

        if self.invalid {
            base = base.inset_ring(theme.tokens.destructive.opacity(0.45), px(2.0));
        } else if is_focused {
            base = base.inset_ring(theme.tokens.ring.opacity(0.5), px(2.0));
        }

        if disabled {
            base = base.opacity(0.5);
        }

        let focus_handle_for_input = focus_handle.clone();

        base.map(|this| {
            let mut div = this;
            div.style().refine(&user_style);
            div
        })
        .child(
            div().size_full().flex().items_center().child(
                canvas_with_prepaint(
                    |_bounds, _window, _cx| {},
                    move |bounds, _data, window, cx| {
                        if !disabled {
                            window.handle_input(
                                &focus_handle_for_input,
                                ElementInputHandler::new(bounds, self.state.clone()),
                                cx,
                            );
                        }
                        if !text_content.is_empty() {
                            let text_style = window.text_style();
                            let text_run = TextRun {
                                len: text_content.len(),
                                font: text_style.font(),
                                color: theme.tokens.foreground,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };

                            let shaped = window.text_system().shape_line(
                                text_content.clone().into(),
                                text_size,
                                &[text_run],
                                None,
                            );

                            if is_focused && !disabled && !selected_range.is_empty() {
                                let start_x = shaped.x_for_index(selected_range.start);
                                let end_x = shaped.x_for_index(selected_range.end);
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(bounds.left() + start_x, bounds.top() + px(4.0)),
                                        size(
                                            (end_x - start_x).max(px(1.0)),
                                            (bounds.size.height - px(8.0)).max(px(12.0)),
                                        ),
                                    ),
                                    theme.tokens.primary.opacity(0.2),
                                ));
                            }
                            let _ = shaped.paint(
                                point(bounds.left(), bounds.top()),
                                bounds.size.height,
                                window,
                                cx,
                            );
                            if is_focused && !disabled && selected_range.is_empty() {
                                let cursor_x = shaped.x_for_index(cursor_position);
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(bounds.left() + cursor_x, bounds.top() + px(6.0)),
                                        size(
                                            px(1.5),
                                            (bounds.size.height - px(12.0)).max(px(12.0)),
                                        ),
                                    ),
                                    theme.tokens.foreground,
                                ));
                            }
                        } else if let Some(placeholder) = &self.placeholder {
                            let text_style = window.text_style();
                            let text_run = TextRun {
                                len: placeholder.len(),
                                font: text_style.font(),
                                color: theme.tokens.muted_foreground,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };

                            let shaped = window.text_system().shape_line(
                                placeholder.clone(),
                                text_size,
                                &[text_run],
                                None,
                            );

                            let _ = shaped.paint(
                                point(bounds.left(), bounds.top()),
                                bounds.size.height,
                                window,
                                cx,
                            );
                            if is_focused && !disabled {
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(bounds.left(), bounds.top() + px(6.0)),
                                        size(
                                            px(1.5),
                                            (bounds.size.height - px(12.0)).max(px(12.0)),
                                        ),
                                    ),
                                    theme.tokens.foreground,
                                ));
                            }
                        } else if is_focused && !disabled {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(bounds.left(), bounds.top() + px(6.0)),
                                    size(px(1.5), (bounds.size.height - px(12.0)).max(px(12.0))),
                                ),
                                theme.tokens.foreground,
                            ));
                        }
                    },
                )
                .size_full(),
            ),
        )
        .when(!disabled, |this| {
            this.on_mouse_down(MouseButton::Left, move |_event, window, _cx| {
                window.focus(&focus_handle_for_mouse);
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{TextField, TextFieldState};
    use kael::{AppContext, Context, Entity, IntoElement, Render, TestAppContext, Window};

    struct Host {
        state: Entity<TextFieldState>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            TextField::from_state(self.state.clone()).label("Project name")
        }
    }

    #[kael::test]
    fn deletion_respects_grapheme_boundaries(cx: &mut TestAppContext) {
        let state = cx.new(TextFieldState::new);
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_text("A👨‍👩‍👧‍👦B".into(), cx);
                assert!(state.delete_backward(cx));
                assert_eq!(state.text(), "A👨‍👩‍👧‍👦");
                assert!(state.delete_backward(cx));
                assert_eq!(state.text(), "A");
            });
        });
    }

    #[kael::test]
    fn caller_owned_state_preserves_programmatic_value(cx: &mut TestAppContext) {
        let state = cx.new(TextFieldState::new);
        cx.update(|cx| {
            state.update(cx, |state, cx| state.set_text("Persistent".into(), cx));
            let field = super::TextField::from_state(state.clone());
            assert_eq!(field.text(cx), "Persistent");
        });
    }

    #[kael::test]
    fn live_input_and_navigation_edit_persistent_state(cx: &mut TestAppContext) {
        cx.update(|cx| {
            super::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(TextFieldState::new);
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| Host { state }
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            window.focus(&state.read(cx).focus_handle.clone());
        });
        window.simulate_input("A👨‍👩‍👧‍👦B");
        window.simulate_keystrokes("left backspace");

        assert_eq!(
            window.update(|_, cx| state.read(cx).text().to_string()),
            "AB"
        );

        #[cfg(target_os = "macos")]
        window.simulate_keystrokes("cmd-a");
        #[cfg(not(target_os = "macos"))]
        window.simulate_keystrokes("ctrl-a");
        window.simulate_input("Selected");
        assert_eq!(cx.read(|cx| state.read(cx).text().to_string()), "Selected");
    }
}

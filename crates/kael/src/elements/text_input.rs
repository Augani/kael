use super::local_history::WindowValueHistory;
use crate::{
    AccessibilityAction, AccessibilityActionPayload, AccessibilityId, AccessibilityNode,
    AccessibilityRole, AccessibilityState, AccessibilityValue, Action, App, AppContext, Bounds,
    ClipboardItem, ContentMask, Context, CursorStyle, DispatchPhase, Edges, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, Global,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, KeyBinding,
    KeyContext, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, fill, point,
    px, relative, rgb, rgba, size, util::wrapped_line_end_indices, white,
};
use std::{
    any::TypeId,
    cell::RefCell,
    ops::Range,
    rc::Rc,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;

const TEXT_INPUT_CONTEXT: &str = "TextInput";
const PASSWORD_MASK_TEXT: &str = "•";
const TEXT_INPUT_MERGE_WINDOW: Duration = Duration::from_millis(750);

actions!(
    text_input,
    [
        /// Delete the current selection or the grapheme before the caret.
        Backspace,
        /// Delete the current selection or the grapheme after the caret.
        Delete,
        /// Delete from the caret to the previous word boundary.
        DeleteWordBackward,
        /// Delete from the caret to the next word boundary.
        DeleteWordForward,
        /// Move the caret one grapheme to the left.
        MoveLeft,
        /// Move the caret one grapheme to the right.
        MoveRight,
        /// Move the caret to the closest position on the previous visual line.
        MoveUp,
        /// Move the caret to the closest position on the next visual line.
        MoveDown,
        /// Move the caret to the previous word boundary.
        MoveWordLeft,
        /// Move the caret to the next word boundary.
        MoveWordRight,
        /// Extend the selection one grapheme to the left.
        SelectLeft,
        /// Extend the selection one grapheme to the right.
        SelectRight,
        /// Extend the selection to the closest position on the previous visual line.
        SelectUp,
        /// Extend the selection to the closest position on the next visual line.
        SelectDown,
        /// Extend the selection to the previous word boundary.
        SelectWordLeft,
        /// Extend the selection to the next word boundary.
        SelectWordRight,
        /// Move the caret to the beginning of the field.
        MoveToStart,
        /// Move the caret to the end of the field.
        MoveToEnd,
        /// Extend the selection to the beginning of the field.
        SelectToStart,
        /// Extend the selection to the end of the field.
        SelectToEnd,
        /// Select all text in the field.
        SelectAll,
        /// Paste clipboard text into the field.
        Paste,
        /// Copy the selected text to the clipboard.
        Copy,
        /// Cut the selected text to the clipboard.
        Cut,
        /// Restore the previous edit snapshot.
        Undo,
        /// Reapply the next edit snapshot.
        Redo,
        /// Insert a newline in multiline mode or submit in single-line mode.
        InsertNewline,
        /// Submit the current field value.
        Submit,
        /// Apply the primary Enter behavior for the configured key policy.
        PrimaryEnter,
        /// Apply the Shift+Enter behavior for the configured key policy.
        ShiftEnter,
        /// Apply the Alt+Enter behavior for the configured key policy.
        AltEnter,
        /// Apply the forward Tab behavior for the configured key policy.
        TabForward,
        /// Apply the backward Tab behavior for the configured key policy.
        TabBackward,
        /// Cancel the current field interaction.
        Cancel,
    ]
);

type ChangeListener = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;
type SubmitListener = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;
type FocusListener = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;
type KeyListener = Rc<dyn Fn(TextInputKeyEvent, &mut Window, &mut App)>;
type SelectionListener = Rc<dyn Fn(TextInputSelection, &mut Window, &mut App)>;
type Mask = Rc<dyn InputMask>;

/// Keyboard behavior used by a text input embedded in a canvas editor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextInputKeyPolicy {
    /// Enter inserts a newline in multiline inputs; Tab remains available to focus traversal.
    #[default]
    Multiline,
    /// Enter and Tab commit while Alt+Enter inserts a newline, matching spreadsheet editors.
    Spreadsheet,
}

/// The physical key chord that produced a text-input command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextInputKeyTrigger {
    /// Enter without a text-policy modifier.
    Enter,
    /// Shift+Enter.
    ShiftEnter,
    /// Alt+Enter.
    AltEnter,
    /// The platform command modifier plus Enter.
    CommandEnter,
    /// Tab.
    Tab,
    /// Shift+Tab.
    ShiftTab,
    /// Escape.
    Escape,
}

/// The semantic result of a text-input key command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextInputKeyOutcome {
    /// Insert a hard line break at the selection.
    Newline,
    /// Commit or submit the current value.
    Submit,
    /// Cancel the current canvas edit.
    Cancel,
}

/// A structured canvas-editor command emitted by a text input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextInputKeyEvent {
    /// Current input value after applying the command.
    pub value: SharedString,
    /// Physical key chord that produced the command.
    pub trigger: TextInputKeyTrigger,
    /// Semantic result selected by the input policy.
    pub outcome: TextInputKeyOutcome,
}

/// A bounded UTF-8 selection snapshot emitted by a canvas text input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextInputSelection {
    /// Selected UTF-8 byte range in the controlled value.
    pub range: Range<usize>,
    /// Whether the active caret is at the start of a non-empty selection.
    pub reversed: bool,
    /// UTF-8 byte range currently owned by an input-method composition.
    pub marked_range: Option<Range<usize>>,
}

impl TextInputSelection {
    /// Return the active UTF-8 caret offset.
    #[must_use]
    pub fn caret(&self) -> usize {
        if self.reversed {
            self.range.start
        } else {
            self.range.end
        }
    }

    /// Whether an input-method composition is currently active.
    #[must_use]
    pub const fn is_composing(&self) -> bool {
        self.marked_range.is_some()
    }
}

#[derive(Clone, Debug)]
enum TextInputSelectionRequest {
    Caret(Point<Pixels>),
    Range { range: Range<usize>, reversed: bool },
    All,
}

/// External control handle for a canvas-hosted text input.
///
/// Keep one controller per logical input and pass it to [`TextInput::controller`]
/// from the input's first render.
#[derive(Clone)]
pub struct TextInputController {
    focus_handle: FocusHandle,
    pending_selection: Rc<RefCell<Option<TextInputSelectionRequest>>>,
}

impl TextInputController {
    /// Create a controller with a focus handle allocated by the owning view context.
    pub fn new(focus_handle: FocusHandle) -> Self {
        Self {
            focus_handle,
            pending_selection: Rc::new(RefCell::new(None)),
        }
    }

    /// Return the input's focus handle.
    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    /// Focus the controlled input.
    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    /// Queue caret placement at an absolute canvas point for the next prepaint.
    pub fn place_caret(&self, point: Point<Pixels>, window: &mut Window) {
        self.pending_selection
            .borrow_mut()
            .replace(TextInputSelectionRequest::Caret(point));
        self.focus(window);
        window.refresh();
    }

    /// Queue a UTF-8 byte selection for the next prepaint.
    pub fn select_range(&self, range: Range<usize>, reversed: bool, window: &mut Window) {
        self.pending_selection
            .borrow_mut()
            .replace(TextInputSelectionRequest::Range { range, reversed });
        self.focus(window);
        window.refresh();
    }

    /// Queue selecting the complete value for the next prepaint.
    pub fn select_all(&self, window: &mut Window) {
        self.pending_selection
            .borrow_mut()
            .replace(TextInputSelectionRequest::All);
        self.focus(window);
        window.refresh();
    }
}

impl Focusable for TextInputController {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Clone)]
#[non_exhaustive]
/// A single shaped line and its paint origin for a custom text input renderer.
pub struct TextInputRenderLine {
    /// The shaped line to paint.
    pub line: WrappedLine,
    /// Top-left paint origin for the shaped line.
    pub origin: Point<Pixels>,
}

#[derive(Clone)]
#[non_exhaustive]
/// Snapshot of text input paint state passed to a custom renderer.
pub struct TextInputRenderState {
    /// The underlying field value.
    pub value: SharedString,
    /// The text currently displayed, including masking or placeholder text.
    pub display_text: SharedString,
    /// The configured placeholder text, if any.
    pub placeholder: Option<SharedString>,
    /// Whether the displayed text is currently the placeholder.
    pub showing_placeholder: bool,
    /// Whether the field currently owns keyboard focus.
    pub focused: bool,
    /// Whether the pointer is currently hovering the field hitbox.
    pub hovered: bool,
    /// Whether the field is configured for multiline editing.
    pub multi_line: bool,
    /// Outer field bounds including the border.
    pub outer_bounds: Bounds<Pixels>,
    /// Inner field bounds inside the border.
    pub field_bounds: Bounds<Pixels>,
    /// Clipped text viewport bounds.
    pub text_bounds: Bounds<Pixels>,
    /// Line height used to paint the shaped text.
    pub line_height: Pixels,
    /// Shaped lines and their paint origins.
    pub lines: Vec<TextInputRenderLine>,
    /// Selection rectangles in render space.
    pub selection_bounds: Vec<Bounds<Pixels>>,
    /// Caret rectangle in render space, if visible.
    pub cursor_bounds: Option<Bounds<Pixels>>,
}

impl TextInputRenderState {
    /// Length of the underlying field value in UTF-8 bytes.
    pub fn value_len_bytes(&self) -> usize {
        self.value.len()
    }

    /// Length of the currently displayed text in UTF-8 bytes.
    pub fn display_text_len_bytes(&self) -> usize {
        self.display_text.len()
    }

    /// Length of the configured placeholder text in UTF-8 bytes.
    pub fn placeholder_len_bytes(&self) -> usize {
        self.placeholder
            .as_ref()
            .map(|placeholder| placeholder.len())
            .unwrap_or(0)
    }

    /// Whether a placeholder is configured.
    pub fn has_placeholder(&self) -> bool {
        self.placeholder.is_some()
    }

    /// Whether the underlying field value is empty.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Whether the displayed text differs from the value while not showing a placeholder.
    pub fn is_masked_display(&self) -> bool {
        !self.showing_placeholder && self.value != self.display_text
    }

    /// Number of shaped render lines currently visible to the renderer.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Number of selection rectangles currently visible to the renderer.
    pub fn selection_rect_count(&self) -> usize {
        self.selection_bounds.len()
    }

    /// Whether a non-empty selection is currently visible.
    pub fn has_selection(&self) -> bool {
        !self.selection_bounds.is_empty()
    }

    /// Whether a caret rectangle is currently visible.
    pub fn has_cursor(&self) -> bool {
        self.cursor_bounds.is_some()
    }

    /// Content-safe summary for custom renderers, tests, and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "text input render: value-bytes {}, display-bytes {}, placeholder {}, placeholder-bytes {}, showing-placeholder {}, focused {}, hovered {}, multiline {}, lines {}, selection-rects {}, selection {}, cursor {}, masked-display {}",
            self.value_len_bytes(),
            self.display_text_len_bytes(),
            self.has_placeholder(),
            self.placeholder_len_bytes(),
            self.showing_placeholder,
            self.focused,
            self.hovered,
            self.multi_line,
            self.line_count(),
            self.selection_rect_count(),
            self.has_selection(),
            self.has_cursor(),
            self.is_masked_display()
        )
    }

    /// Paint the shaped text lines using the current window text style.
    pub fn paint_text(&self, window: &mut Window, cx: &mut App) {
        let text_align = window.text_style().text_align;
        window.with_content_mask(
            Some(ContentMask {
                bounds: self.text_bounds,
            }),
            |window| {
                for line in &self.lines {
                    line.line
                        .paint(
                            line.origin,
                            self.line_height,
                            text_align,
                            Some(self.text_bounds),
                            window,
                            cx,
                        )
                        .unwrap();
                }
            },
        );
    }

    /// Paint the current text selection using the provided fill color.
    pub fn paint_selection(&self, color: crate::Hsla, window: &mut Window) {
        if self.selection_bounds.is_empty() {
            return;
        }

        window.with_content_mask(
            Some(ContentMask {
                bounds: self.text_bounds,
            }),
            |window| {
                for selection in &self.selection_bounds {
                    window.paint_quad(fill(*selection, color));
                }
            },
        );
    }

    /// Paint the caret using the provided fill color.
    pub fn paint_cursor(&self, color: crate::Hsla, window: &mut Window) {
        let Some(cursor_bounds) = self.cursor_bounds else {
            return;
        };

        window.with_content_mask(
            Some(ContentMask {
                bounds: self.text_bounds,
            }),
            |window| {
                window.paint_quad(fill(cursor_bounds, color));
            },
        );
    }

    /// Paint the default text, selection, and caret layers.
    pub fn paint_default_contents(&self, window: &mut Window, cx: &mut App) {
        self.paint_selection(rgba(0x3311ff30).into(), window);
        self.paint_text(window, cx);
        self.paint_cursor(crate::blue(), window);
    }
}

type TextInputCustomRenderer = Rc<dyn Fn(TextInputRenderState, &mut Window, &mut App)>;

/// A hook that can normalize a text edit before it is committed.
pub trait InputMask: 'static {
    /// Adjust the proposed text and caret position after an edit.
    fn correct(&self, was: &str, cursor: usize, now: &mut String, new_cursor: &mut usize);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextInputSnapshot {
    content: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
}

#[derive(Debug)]
struct TextInputHistory {
    entries: WindowValueHistory<TextInputSnapshot>,
    merge_group: Option<TextInputMergeGroup>,
}

#[derive(Clone, Debug)]
struct TextInputMergeGroup {
    kind: TextInputMergeKind,
    starting_snapshot: TextInputSnapshot,
    cursor_after: usize,
    recorded_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextInputMergeKind {
    Insert,
    Backspace,
    DeleteForward,
}

impl TextInputHistory {
    fn new(entries: WindowValueHistory<TextInputSnapshot>) -> Self {
        Self {
            entries,
            merge_group: None,
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.merge_group = None;
    }

    fn can_undo(&self) -> bool {
        self.entries.can_undo()
    }

    fn can_redo(&self) -> bool {
        self.entries.can_redo()
    }

    fn record(
        &mut self,
        previous: TextInputSnapshot,
        next: TextInputSnapshot,
        merge_kind: Option<TextInputMergeKind>,
    ) {
        let now = Instant::now();

        if let Some(kind) = merge_kind {
            if self
                .merge_group
                .as_ref()
                .is_some_and(|group| group.can_merge(kind, &previous, now))
            {
                let starting_snapshot = self
                    .merge_group
                    .as_ref()
                    .expect("merge group exists when merge succeeds")
                    .starting_snapshot
                    .clone();
                let replaced = self.entries.replace_last(starting_snapshot, next.clone());
                if replaced {
                    if let Some(group) = self.merge_group.as_mut() {
                        group.cursor_after = next.selected_range.end;
                        group.recorded_at = now;
                    }
                    return;
                }
            }

            self.entries.record(previous.clone(), next.clone());
            self.merge_group = Some(TextInputMergeGroup {
                kind,
                starting_snapshot: previous,
                cursor_after: next.selected_range.end,
                recorded_at: now,
            });
            return;
        }

        self.entries.record(previous, next);
        self.merge_group = None;
    }

    fn undo(&mut self) -> Option<TextInputSnapshot> {
        self.merge_group = None;
        self.entries.undo()
    }

    fn redo(&mut self) -> Option<TextInputSnapshot> {
        self.merge_group = None;
        self.entries.redo()
    }
}

impl TextInputMergeGroup {
    fn can_merge(
        &self,
        kind: TextInputMergeKind,
        previous: &TextInputSnapshot,
        now: Instant,
    ) -> bool {
        if self.kind != kind || now.duration_since(self.recorded_at) > TEXT_INPUT_MERGE_WINDOW {
            return false;
        }

        let cursor = previous.selected_range.end;
        previous.selected_range.is_empty() && cursor == self.cursor_after
    }
}

fn text_input_merge_kind(
    previous: &TextInputSnapshot,
    range: &Range<usize>,
    replacement: &str,
    next: &TextInputSnapshot,
) -> Option<TextInputMergeKind> {
    if previous.marked_range.is_some()
        || next.marked_range.is_some()
        || !previous.selected_range.is_empty()
        || !next.selected_range.is_empty()
        || replacement.contains('\n')
    {
        return None;
    }

    if range.start == range.end && replacement.graphemes(true).count() == 1 {
        return Some(TextInputMergeKind::Insert);
    }

    if replacement.is_empty() && range.start < range.end {
        let cursor = previous.selected_range.end;
        if range.end == cursor {
            return Some(TextInputMergeKind::Backspace);
        }
        if range.start == cursor {
            return Some(TextInputMergeKind::DeleteForward);
        }
    }

    None
}

#[derive(Default)]
struct TextInputBindingsInstalled;

impl Global for TextInputBindingsInstalled {}

/// Construct an editable text field.
#[track_caller]
pub fn text_input(id: impl Into<ElementId>, text: impl Into<SharedString>) -> TextInput {
    TextInput::new(id.into(), text.into())
}

/// A controlled editable text field.
pub struct TextInput {
    element_id: ElementId,
    text: SharedString,
    placeholder: SharedString,
    controller: Option<TextInputController>,
    multi_line: bool,
    max_lines: Option<usize>,
    key_policy: TextInputKeyPolicy,
    content_insets: Edges<Pixels>,
    read_only: bool,
    password: bool,
    mask: Option<Mask>,
    accessibility_label: Option<SharedString>,
    accessibility_description: Option<SharedString>,
    on_change: Option<ChangeListener>,
    on_submit: Option<SubmitListener>,
    on_key: Option<KeyListener>,
    on_selection_change: Option<SelectionListener>,
    on_focus: Option<FocusListener>,
    on_blur: Option<FocusListener>,
    custom_renderer: Option<TextInputCustomRenderer>,
    source_location: &'static core::panic::Location<'static>,
}

impl TextInput {
    #[track_caller]
    fn new(element_id: ElementId, text: SharedString) -> Self {
        Self {
            element_id,
            text,
            placeholder: SharedString::default(),
            controller: None,
            multi_line: false,
            max_lines: None,
            key_policy: TextInputKeyPolicy::Multiline,
            content_insets: uniform_edges(field_padding()),
            read_only: false,
            password: false,
            mask: None,
            accessibility_label: None,
            accessibility_description: None,
            on_change: None,
            on_submit: None,
            on_key: None,
            on_selection_change: None,
            on_focus: None,
            on_blur: None,
            custom_renderer: None,
            source_location: core::panic::Location::caller(),
        }
    }

    /// Set the placeholder text shown when the field is empty.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Attach an externally owned controller for focus and canvas selection placement.
    pub fn controller(mut self, controller: TextInputController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// Enable wrapped multiline editing.
    pub fn multi_line(mut self) -> Self {
        self.multi_line = true;
        self
    }

    /// Limit the visible height of a multiline field.
    pub fn max_lines(mut self, max_lines: usize) -> Self {
        self.max_lines = Some(max_lines.max(1));
        self
    }

    /// Configure editor-specific Enter, Tab, and Escape behavior.
    pub fn key_policy(mut self, policy: TextInputKeyPolicy) -> Self {
        self.key_policy = policy;
        self
    }

    /// Set uniform content padding inside the input border.
    pub fn content_padding(mut self, padding: Pixels) -> Self {
        self.content_insets = uniform_edges(padding);
        self
    }

    /// Set independent content insets inside the input border.
    pub fn content_insets(mut self, insets: Edges<Pixels>) -> Self {
        self.content_insets = insets;
        self
    }

    /// Prevent user and accessibility edits while preserving focus and selection.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// Set the accessible name announced for the input.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    /// Set supplementary accessible help text for the input.
    pub fn accessibility_description(mut self, description: impl Into<SharedString>) -> Self {
        self.accessibility_description = Some(description.into());
        self
    }

    /// Mask the rendered content while keeping the underlying value unchanged.
    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    /// Rewrite proposed edits before they are committed.
    pub fn mask(mut self, mask: impl InputMask) -> Self {
        self.mask = Some(Rc::new(mask));
        self
    }

    /// Register a callback invoked whenever the field content changes.
    pub fn on_change(
        mut self,
        listener: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(listener));
        self
    }

    /// Register a callback invoked when the user submits the field.
    pub fn on_submit(
        mut self,
        listener: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_submit = Some(Rc::new(listener));
        self
    }

    /// Register a structured Enter, Tab, or Escape command callback.
    pub fn on_key(
        mut self,
        listener: impl Fn(TextInputKeyEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_key = Some(Rc::new(listener));
        self
    }

    /// Register a callback for changed UTF-8 selection or composition state.
    pub fn on_selection_change(
        mut self,
        listener: impl Fn(TextInputSelection, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_selection_change = Some(Rc::new(listener));
        self
    }

    /// Register a callback invoked when the field receives keyboard focus.
    pub fn on_focus(
        mut self,
        listener: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_focus = Some(Rc::new(listener));
        self
    }

    /// Register a callback invoked when the field loses keyboard focus.
    pub fn on_blur(
        mut self,
        listener: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_blur = Some(Rc::new(listener));
        self
    }

    /// Render the full text input surface with caller-owned painting.
    pub fn render_with(
        mut self,
        renderer: impl Fn(TextInputRenderState, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.custom_renderer = Some(Rc::new(renderer));
        self
    }

    fn ensure_keybindings(cx: &mut App) {
        if cx.has_global::<TextInputBindingsInstalled>() {
            return;
        }

        cx.bind_keys([
            KeyBinding::new("backspace", Backspace, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("delete", Delete, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new(
                "alt-backspace",
                DeleteWordBackward,
                Some(TEXT_INPUT_CONTEXT),
            ),
            KeyBinding::new(
                "ctrl-backspace",
                DeleteWordBackward,
                Some(TEXT_INPUT_CONTEXT),
            ),
            KeyBinding::new("alt-delete", DeleteWordForward, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("ctrl-delete", DeleteWordForward, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("left", MoveLeft, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("right", MoveRight, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("up", MoveUp, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("down", MoveDown, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("alt-left", MoveWordLeft, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("ctrl-left", MoveWordLeft, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("alt-right", MoveWordRight, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("ctrl-right", MoveWordRight, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-left", SelectLeft, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-right", SelectRight, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-up", SelectUp, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-down", SelectDown, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-alt-left", SelectWordLeft, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-ctrl-left", SelectWordLeft, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-alt-right", SelectWordRight, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new(
                "shift-ctrl-right",
                SelectWordRight,
                Some(TEXT_INPUT_CONTEXT),
            ),
            KeyBinding::new("cmd-left", MoveToStart, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("cmd-right", MoveToEnd, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-cmd-left", SelectToStart, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-cmd-right", SelectToEnd, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("home", MoveToStart, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("end", MoveToEnd, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("secondary-a", SelectAll, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("secondary-v", Paste, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("secondary-c", Copy, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("secondary-x", Cut, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("secondary-z", Undo, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("secondary-shift-z", Redo, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("secondary-y", Redo, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("enter", PrimaryEnter, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-enter", ShiftEnter, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("alt-enter", AltEnter, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("secondary-enter", Submit, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("tab", TabForward, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-tab", TabBackward, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("escape", Cancel, Some(TEXT_INPUT_CONTEXT)),
        ]);

        cx.set_global(TextInputBindingsInstalled);
    }

    fn state(
        &self,
        global_id: &GlobalElementId,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<TextInputState> {
        let current_view = window.current_view();
        let undo_manager = window.undo_manager();
        let (state, is_new) =
            window.with_element_state(global_id, |state: Option<Entity<TextInputState>>, _| {
                if let Some(state) = state {
                    ((state.clone(), false), state)
                } else {
                    let state = cx.new(|cx| {
                        let focus_handle = self
                            .controller
                            .as_ref()
                            .map(TextInputController::focus_handle)
                            .unwrap_or_else(|| cx.focus_handle());
                        let history = TextInputHistory::new(WindowValueHistory::new(
                            undo_manager.clone(),
                            &focus_handle,
                            "Text edit",
                        ));
                        TextInputState::new(focus_handle, history)
                    });
                    cx.observe(&state, move |_, cx| {
                        cx.notify(current_view);
                    })
                    .detach();
                    ((state.clone(), true), state)
                }
            });

        if is_new {
            let focus_handle = state.read(cx).focus_handle.clone();
            let weak_state = state.downgrade();
            window
                .on_focus_in(&focus_handle, cx, move |window, cx| {
                    let _ = weak_state.update(cx, |state, cx| {
                        if let Some(listener) = state.on_focus.clone() {
                            listener(state.content.clone(), window, cx);
                        }
                    });
                })
                .detach();

            let weak_state = state.downgrade();
            window
                .on_focus_out(&focus_handle, cx, move |_, window, cx| {
                    let _ = weak_state.update(cx, |state, cx| {
                        if let Some(listener) = state.on_blur.clone() {
                            listener(state.content.clone(), window, cx);
                        }
                    });
                })
                .detach();
        }

        let text = self.text.clone();
        let placeholder = self.placeholder.clone();
        let multi_line = self.multi_line;
        let max_lines = self.max_lines;
        let key_policy = self.key_policy;
        let read_only = self.read_only;
        let password = self.password;
        let mask = self.mask.clone();
        let accessibility_label = self.accessibility_label.clone();
        let accessibility_description = self.accessibility_description.clone();
        let on_change = self.on_change.clone();
        let on_submit = self.on_submit.clone();
        let on_key = self.on_key.clone();
        let on_selection_change = self.on_selection_change.clone();
        let on_focus = self.on_focus.clone();
        let on_blur = self.on_blur.clone();
        state.update(cx, |state, cx| {
            state.sync_from_props(
                text,
                placeholder,
                multi_line,
                max_lines,
                key_policy,
                read_only,
                password,
                mask,
                accessibility_label,
                accessibility_description,
                on_change,
                on_submit,
                on_key,
                on_selection_change,
                on_focus,
                on_blur,
            );
            state.emit_selection_change(window, cx);
        });
        state
    }
}

impl IntoElement for TextInput {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[doc(hidden)]
#[derive(Clone, Debug)]
struct TextInputParagraphLayout {
    line: WrappedLine,
    start_offset: usize,
}

#[doc(hidden)]
#[derive(Clone, Debug)]
struct TextInputLayout {
    paragraphs: Vec<TextInputParagraphLayout>,
    viewport_bounds: Bounds<Pixels>,
    content_origin: Point<Pixels>,
    content_height: Pixels,
    vertical_scroll: Pixels,
    line_height: Pixels,
    len: usize,
}

impl TextInputLayout {
    fn new(
        paragraphs: Vec<WrappedLine>,
        viewport_bounds: Bounds<Pixels>,
        line_height: Pixels,
        len: usize,
        vertical_scroll: Pixels,
    ) -> Self {
        let paragraph_lengths = paragraphs.iter().map(WrappedLine::len).collect::<Vec<_>>();
        let start_offsets = paragraph_start_offsets(&paragraph_lengths);
        let content_height = paragraphs.iter().fold(px(0.0), |height, paragraph| {
            height + paragraph.layout.size(line_height).height
        });
        let max_vertical_scroll = (content_height - viewport_bounds.size.height).max(px(0.0));
        let vertical_scroll = vertical_scroll.clamp(px(0.0), max_vertical_scroll);
        let paragraphs = paragraphs
            .into_iter()
            .zip(start_offsets)
            .map(|(line, start_offset)| TextInputParagraphLayout { line, start_offset })
            .collect();

        Self {
            paragraphs,
            viewport_bounds,
            content_origin: point(
                viewport_bounds.origin.x,
                viewport_bounds.origin.y - vertical_scroll,
            ),
            content_height,
            vertical_scroll,
            line_height,
            len,
        }
    }

    fn max_vertical_scroll(&self) -> Pixels {
        (self.content_height - self.viewport_bounds.size.height).max(px(0.0))
    }

    fn position_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        let index = index.min(self.len);
        if self.paragraphs.is_empty() {
            return (index == 0).then_some(self.content_origin);
        }

        let mut paragraph_origin = self.content_origin;
        for paragraph in &self.paragraphs {
            let paragraph_end = paragraph.start_offset + paragraph.line.len();
            if index < paragraph.start_offset {
                break;
            }

            if index > paragraph_end {
                paragraph_origin.y += paragraph.line.layout.size(self.line_height).height;
                continue;
            }

            let local_index = index - paragraph.start_offset;
            return paragraph
                .line
                .layout
                .position_for_index(local_index, self.line_height)
                .map(|position| paragraph_origin + position);
        }

        None
    }

    fn index_for_position(&self, position: Point<Pixels>) -> Result<usize, usize> {
        if self.paragraphs.is_empty() {
            return Err(0);
        }

        if position.y < self.content_origin.y {
            return Err(0);
        }
        if position.y > self.content_origin.y + self.content_height {
            return Err(self.len);
        }

        let mut paragraph_origin = self.content_origin;
        for paragraph in &self.paragraphs {
            let paragraph_height = paragraph.line.layout.size(self.line_height).height;
            let paragraph_bottom = paragraph_origin.y + paragraph_height;
            if position.y <= paragraph_bottom {
                let local = position - paragraph_origin;
                return paragraph
                    .line
                    .layout
                    .index_for_position(local, self.line_height)
                    .map(|index| paragraph.start_offset + index)
                    .map_err(|index| paragraph.start_offset + index);
            }

            paragraph_origin.y = paragraph_bottom;
        }

        Err(self.len)
    }

    fn closest_index_for_position(&self, position: Point<Pixels>) -> usize {
        match self.index_for_position(position) {
            Ok(index) | Err(index) => index,
        }
    }

    fn selection_bounds(&self, range: Range<usize>) -> Option<Bounds<Pixels>> {
        let mut rects = self.selection_rects(range).into_iter();
        let first = rects.next()?;
        let mut left = first.origin.x;
        let mut top = first.origin.y;
        let mut right = first.origin.x + first.size.width;
        let mut bottom = first.origin.y + first.size.height;

        for rect in rects {
            left = left.min(rect.origin.x);
            top = top.min(rect.origin.y);
            right = right.max(rect.origin.x + rect.size.width);
            bottom = bottom.max(rect.origin.y + rect.size.height);
        }

        Some(Bounds {
            origin: point(left, top),
            size: size(right - left, bottom - top),
        })
    }

    fn selection_rects(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let start = range.start.min(self.len);
        let end = range.end.min(self.len);
        if start > end {
            return Vec::new();
        }

        if start == end {
            return self
                .position_for_index(start)
                .map_or_else(Vec::new, |origin| {
                    vec![Bounds {
                        origin,
                        size: size(px(1.0), self.line_height),
                    }]
                });
        }

        let mut rects = Vec::new();
        let mut paragraph_origin = self.content_origin;
        for paragraph in &self.paragraphs {
            let paragraph_start = paragraph.start_offset;
            let paragraph_end = paragraph.start_offset + paragraph.line.len();
            let selection_start = start.max(paragraph_start);
            let selection_end = end.min(paragraph_end);

            let mut line_origin = paragraph_origin;
            let mut line_start = 0;
            for line_end in wrapped_line_end_indices(&paragraph.line.layout) {
                let absolute_line_start = paragraph_start + line_start;
                let absolute_line_end = paragraph_start + line_end;
                let segment_start = selection_start.max(absolute_line_start);
                let segment_end = selection_end.min(absolute_line_end);

                if segment_start < segment_end {
                    let start_position = paragraph
                        .line
                        .layout
                        .position_for_index(segment_start - paragraph_start, self.line_height)
                        .unwrap_or_default();
                    let end_position = paragraph
                        .line
                        .layout
                        .position_for_index(segment_end - paragraph_start, self.line_height)
                        .unwrap_or_default();
                    rects.push(Bounds {
                        origin: line_origin + point(start_position.x, px(0.0)),
                        size: size(end_position.x - start_position.x, self.line_height),
                    });
                }

                line_origin.y += self.line_height;
                line_start = line_end;
            }

            paragraph_origin.y += paragraph.line.layout.size(self.line_height).height;
        }

        rects
    }
}

#[doc(hidden)]
pub struct TextInputPrepaintState {
    hitbox: Hitbox,
    layout: TextInputLayout,
    cursor_bounds: Option<Bounds<Pixels>>,
    selection_bounds: Vec<Bounds<Pixels>>,
    text_bounds: Bounds<Pixels>,
}

impl Element for TextInput {
    type RequestLayoutState = ();
    type PrepaintState = TextInputPrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.element_id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        Some(self.source_location)
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        Self::ensure_keybindings(cx);

        if !self.multi_line {
            let mut style = Style::default();
            style.size.width = relative(1.0).into();
            style.size.height = (window.line_height()
                + self.content_insets.top
                + self.content_insets.bottom
                + px(2.0))
            .into();
            return (window.request_layout(style, [], cx), ());
        }

        let global_id = id.expect("text_input always has an element id");
        let state = self.state(global_id, window, cx);
        let content_insets = self.content_insets;
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        let layout_id = window.request_measured_layout(
            style,
            move |known_dimensions, available_space, window, cx| {
                let outer_width = known_dimensions
                    .width
                    .or_else(|| match available_space.width {
                        crate::AvailableSpace::Definite(width) => Some(width),
                        _ => None,
                    });
                let wrap_width =
                    outer_width.map(|width| content_wrap_width(width, &content_insets));
                let line_height = window.line_height();
                let input = state.read(cx);
                let (_, lines) = shape_text_input_lines(&input, wrap_width, false, window);
                let content_width = lines
                    .iter()
                    .fold(px(0.0), |width, line| width.max(line.layout.width().ceil()));
                let total_lines = total_visual_line_count(&lines).max(1);
                let visible_lines = input
                    .max_lines
                    .map_or(total_lines, |max_lines| total_lines.min(max_lines.max(1)));

                size(
                    outer_width.unwrap_or(
                        content_width + content_insets.left + content_insets.right + px(2.0),
                    ),
                    line_height * visible_lines
                        + content_insets.top
                        + content_insets.bottom
                        + px(2.0),
                )
            },
        );

        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let global_id = id.expect("text_input always has an element id");
        let state = self.state(global_id, window, cx);
        let focus_handle = state.read(cx).focus_handle.clone();
        let tab_handle = focus_handle.clone().tab_stop(true).tab_index(0);

        window.set_focus_handle(&focus_handle, cx);
        window.next_frame.tab_stops.insert(&tab_handle);

        let inner_bounds = inset_bounds(bounds, px(1.0));
        let text_bounds = inset_bounds_by_edges(inner_bounds, &self.content_insets);
        let line_height = window.line_height();
        let desired_vertical_scroll = {
            let input = state.read(cx);
            let layout = build_text_input_layout(&input, text_bounds, line_height, window);
            input.target_vertical_scroll(&layout)
        };
        state.update(cx, |input, _| {
            input.vertical_scroll = desired_vertical_scroll;
        });
        let initial_layout = {
            let input = state.read(cx);
            build_text_input_layout(&input, text_bounds, line_height, window)
        };
        if let Some(request) = self
            .controller
            .as_ref()
            .and_then(|controller| controller.pending_selection.borrow_mut().take())
        {
            state.update(cx, |input, cx| {
                input.apply_selection_request(request, &initial_layout);
                input.emit_selection_change(window, cx);
            });
            let requested_vertical_scroll = state.read(cx).target_vertical_scroll(&initial_layout);
            state.update(cx, |input, _| {
                input.vertical_scroll = requested_vertical_scroll;
            });
        }
        let input = state.read(cx);
        let layout = build_text_input_layout(&input, text_bounds, line_height, window);

        let cursor = input.display_offset(input.cursor_offset());
        let selected_range = input.display_range(&input.selected_range);
        let selection_bounds = if input.selected_range.is_empty() {
            Vec::new()
        } else {
            layout.selection_rects(selected_range)
        };
        let cursor_bounds = if input.selected_range.is_empty() && focus_handle.is_focused(window) {
            layout
                .position_for_index(cursor)
                .map(|origin| Bounds::new(origin, size(px(2.0), line_height)))
        } else {
            None
        };
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        TextInputPrepaintState {
            hitbox,
            layout,
            cursor_bounds,
            selection_bounds,
            text_bounds,
        }
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let global_id = id.expect("text_input always has an element id");
        let state = self.state(global_id, window, cx);
        let (focus_handle, can_undo, can_redo, key_policy) = {
            let input = state.read(cx);
            (
                input.focus_handle.clone(),
                !input.read_only && input.history.can_undo(),
                !input.read_only && input.history.can_redo(),
                input.key_policy,
            )
        };

        window.set_key_context(
            KeyContext::parse(TEXT_INPUT_CONTEXT).expect("valid text input context"),
        );
        register_action_handler::<Backspace>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::backspace,
        );
        register_action_handler::<Delete>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::delete,
        );
        register_action_handler::<DeleteWordBackward>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::delete_word_backward,
        );
        register_action_handler::<DeleteWordForward>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::delete_word_forward,
        );
        register_action_handler::<MoveLeft>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::move_left,
        );
        register_action_handler::<MoveRight>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::move_right,
        );
        register_action_handler::<MoveUp>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::move_up,
        );
        register_action_handler::<MoveDown>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::move_down,
        );
        register_action_handler::<MoveWordLeft>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::move_word_left,
        );
        register_action_handler::<MoveWordRight>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::move_word_right,
        );
        register_action_handler::<SelectLeft>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::select_left,
        );
        register_action_handler::<SelectRight>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::select_right,
        );
        register_action_handler::<SelectUp>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::select_up,
        );
        register_action_handler::<SelectDown>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::select_down,
        );
        register_action_handler::<SelectWordLeft>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::select_word_left,
        );
        register_action_handler::<SelectWordRight>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::select_word_right,
        );
        register_action_handler::<MoveToStart>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::move_to_start,
        );
        register_action_handler::<MoveToEnd>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::move_to_end,
        );
        register_action_handler::<SelectToStart>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::select_to_start,
        );
        register_action_handler::<SelectToEnd>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::select_to_end,
        );
        register_action_handler::<SelectAll>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::select_all,
        );
        register_action_handler::<Paste>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::paste,
        );
        register_action_handler::<Copy>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::copy,
        );
        register_action_handler::<Cut>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::cut,
        );
        register_action_handler_when::<Undo>(
            window,
            can_undo,
            state.clone(),
            focus_handle.clone(),
            TextInputState::undo,
        );
        register_action_handler_when::<Redo>(
            window,
            can_redo,
            state.clone(),
            focus_handle.clone(),
            TextInputState::redo,
        );
        register_action_handler::<InsertNewline>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::insert_newline,
        );
        register_action_handler::<Submit>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::submit,
        );
        register_action_handler::<PrimaryEnter>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::primary_enter,
        );
        register_action_handler::<ShiftEnter>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::shift_enter,
        );
        register_action_handler::<AltEnter>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::alt_enter,
        );
        if key_policy == TextInputKeyPolicy::Spreadsheet {
            register_action_handler::<TabForward>(
                window,
                state.clone(),
                focus_handle.clone(),
                TextInputState::tab_forward,
            );
            register_action_handler::<TabBackward>(
                window,
                state.clone(),
                focus_handle.clone(),
                TextInputState::tab_backward,
            );
        }
        register_action_handler::<Cancel>(
            window,
            state.clone(),
            focus_handle.clone(),
            TextInputState::cancel,
        );

        register_mouse_handlers(
            window,
            state.clone(),
            focus_handle.clone(),
            prepaint.hitbox.clone(),
        );

        if prepaint.hitbox.is_hovered(window) {
            window.set_cursor_style(CursorStyle::IBeam, &prepaint.hitbox);
        }

        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(prepaint.text_bounds, state.clone()),
            cx,
        );

        let is_focused = focus_handle.is_focused(window);
        let (
            render_state,
            accessibility_id,
            accessibility_value,
            accessibility_placeholder,
            accessibility_label,
            accessibility_description,
            read_only,
        ) = {
            let input = state.read(cx);
            let showing_placeholder = input.content.is_empty() && !input.placeholder.is_empty();
            (
                TextInputRenderState {
                    value: input.content.clone(),
                    display_text: display_text_for_input(&input, showing_placeholder),
                    placeholder: (!input.placeholder.is_empty())
                        .then_some(input.placeholder.clone()),
                    showing_placeholder,
                    focused: is_focused,
                    hovered: prepaint.hitbox.is_hovered(window),
                    multi_line: input.multi_line,
                    outer_bounds: bounds,
                    field_bounds: inset_bounds(bounds, px(1.0)),
                    text_bounds: prepaint.text_bounds,
                    line_height: prepaint.layout.line_height,
                    lines: text_input_render_lines(&prepaint.layout),
                    selection_bounds: prepaint.selection_bounds.clone(),
                    cursor_bounds: prepaint.cursor_bounds,
                },
                input.accessibility_id,
                input.accessibility_value(),
                (!input.placeholder.is_empty()).then_some(input.placeholder.to_string()),
                input.accessibility_label.as_ref().map(ToString::to_string),
                input
                    .accessibility_description
                    .as_ref()
                    .map(ToString::to_string),
                input.read_only,
            )
        };

        if let Some(renderer) = &self.custom_renderer {
            renderer(render_state, window, cx);
        } else {
            paint_default_text_input(&render_state, window, cx);
        }

        let node = {
            let mut accessibility_state = AccessibilityState::NONE;
            if is_focused {
                accessibility_state |= AccessibilityState::FOCUSED;
            }
            if read_only {
                accessibility_state |= AccessibilityState::READ_ONLY;
            }
            let mut actions = vec![AccessibilityAction::Focus];
            if !read_only {
                actions.push(AccessibilityAction::SetValue);
            }
            let mut node = AccessibilityNode::new(AccessibilityRole::TextInput)
                .with_states(accessibility_state)
                .with_value(AccessibilityValue::Text(accessibility_value))
                .with_actions(actions);
            node.id = accessibility_id;
            node.label = accessibility_label;
            node.description = accessibility_description;
            if let Some(placeholder) = accessibility_placeholder {
                node.placeholder = Some(placeholder);
            }
            node
        };
        register_text_input_accessibility_handlers(
            window,
            cx,
            &node,
            state.clone(),
            focus_handle,
            read_only,
        );
        window.register_accessibility_node_at(node, bounds);

        state.update(cx, |input, _| {
            input.last_layout = Some(prepaint.layout.clone());
        });
    }
}

struct TextInputState {
    focus_handle: FocusHandle,
    accessibility_id: AccessibilityId,
    content: SharedString,
    placeholder: SharedString,
    multi_line: bool,
    max_lines: Option<usize>,
    key_policy: TextInputKeyPolicy,
    read_only: bool,
    vertical_scroll: Pixels,
    preferred_x: Option<Pixels>,
    password: bool,
    mask: Option<Mask>,
    accessibility_label: Option<SharedString>,
    accessibility_description: Option<SharedString>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<TextInputLayout>,
    is_selecting: bool,
    history: TextInputHistory,
    composition_start: Option<TextInputSnapshot>,
    on_change: Option<ChangeListener>,
    on_submit: Option<SubmitListener>,
    on_key: Option<KeyListener>,
    on_selection_change: Option<SelectionListener>,
    last_emitted_selection: Option<TextInputSelection>,
    on_focus: Option<FocusListener>,
    on_blur: Option<FocusListener>,
}

impl TextInputState {
    fn new(focus_handle: FocusHandle, history: TextInputHistory) -> Self {
        Self {
            focus_handle,
            accessibility_id: AccessibilityId::new(),
            content: SharedString::default(),
            placeholder: SharedString::default(),
            multi_line: false,
            max_lines: None,
            key_policy: TextInputKeyPolicy::Multiline,
            read_only: false,
            vertical_scroll: Pixels::ZERO,
            preferred_x: None,
            password: false,
            mask: None,
            accessibility_label: None,
            accessibility_description: None,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            is_selecting: false,
            history,
            composition_start: None,
            on_change: None,
            on_submit: None,
            on_key: None,
            on_selection_change: None,
            last_emitted_selection: None,
            on_focus: None,
            on_blur: None,
        }
    }

    fn sync_from_props(
        &mut self,
        text: SharedString,
        placeholder: SharedString,
        multi_line: bool,
        max_lines: Option<usize>,
        key_policy: TextInputKeyPolicy,
        read_only: bool,
        password: bool,
        mask: Option<Mask>,
        accessibility_label: Option<SharedString>,
        accessibility_description: Option<SharedString>,
        on_change: Option<ChangeListener>,
        on_submit: Option<SubmitListener>,
        on_key: Option<KeyListener>,
        on_selection_change: Option<SelectionListener>,
        on_focus: Option<FocusListener>,
        on_blur: Option<FocusListener>,
    ) {
        if self.content != text {
            self.content = text;
            self.selected_range = clamp_range_to_text(&self.content, self.selected_range.clone());
            self.marked_range = self
                .marked_range
                .clone()
                .map(|range| clamp_range_to_text(&self.content, range))
                .filter(|range| range.start < range.end);
            self.history.clear();
            self.composition_start = None;
        }

        self.placeholder = placeholder;
        self.multi_line = multi_line;
        self.max_lines = max_lines.map(|value| value.max(1));
        self.key_policy = key_policy;
        self.read_only = read_only;
        if !self.multi_line {
            self.vertical_scroll = Pixels::ZERO;
        }
        self.password = password;
        self.mask = mask;
        self.accessibility_label = accessibility_label;
        self.accessibility_description = accessibility_description;
        self.on_change = on_change;
        self.on_submit = on_submit;
        self.on_key = on_key;
        if self.on_selection_change.is_none() && on_selection_change.is_some() {
            self.last_emitted_selection = None;
        }
        self.on_selection_change = on_selection_change;
        self.on_focus = on_focus;
        self.on_blur = on_blur;
    }

    fn target_vertical_scroll(&self, layout: &TextInputLayout) -> Pixels {
        if !self.multi_line {
            return Pixels::ZERO;
        }

        let cursor = self.display_offset(self.cursor_offset());
        let Some(origin) = layout.position_for_index(cursor) else {
            return layout.vertical_scroll;
        };

        reveal_vertical_scroll(
            layout.vertical_scroll,
            layout.viewport_bounds,
            Bounds::new(origin, size(px(2.0), layout.line_height)),
            layout.max_vertical_scroll(),
        )
    }

    fn apply_selection_request(
        &mut self,
        request: TextInputSelectionRequest,
        layout: &TextInputLayout,
    ) {
        self.preferred_x = None;
        match request {
            TextInputSelectionRequest::Caret(point) => {
                let display_offset = layout.closest_index_for_position(point);
                let offset = clamp_offset_to_boundary(
                    &self.content,
                    self.content_offset_for_display_offset(display_offset),
                );
                self.selected_range = offset..offset;
                self.selection_reversed = false;
            }
            TextInputSelectionRequest::Range { range, reversed } => {
                self.selected_range = clamp_range_to_text(&self.content, range);
                self.selection_reversed = reversed && !self.selected_range.is_empty();
            }
            TextInputSelectionRequest::All => {
                self.selected_range = 0..self.content.len();
                self.selection_reversed = false;
            }
        }
    }

    fn snapshot(&self) -> TextInputSnapshot {
        TextInputSnapshot {
            content: self.content.clone(),
            selected_range: self.selected_range.clone(),
            selection_reversed: self.selection_reversed,
            marked_range: self.marked_range.clone(),
        }
    }

    fn selection_snapshot(&self) -> TextInputSelection {
        TextInputSelection {
            range: self.selected_range.clone(),
            reversed: self.selection_reversed,
            marked_range: self.marked_range.clone(),
        }
    }

    fn emit_selection_change(&mut self, window: &mut Window, cx: &mut App) {
        let Some(listener) = self.on_selection_change.clone() else {
            self.last_emitted_selection = None;
            return;
        };
        let selection = self.selection_snapshot();
        if self.last_emitted_selection.as_ref() == Some(&selection) {
            return;
        }
        self.last_emitted_selection = Some(selection.clone());
        listener(selection, window, cx);
    }

    fn restore_snapshot(&mut self, snapshot: TextInputSnapshot) {
        self.content = snapshot.content;
        self.selected_range = snapshot.selected_range;
        self.selection_reversed = snapshot.selection_reversed;
        self.marked_range = snapshot.marked_range;
    }

    fn display_content(&self) -> SharedString {
        if self.password && !self.content.is_empty() {
            masked_display_text(&self.content)
        } else {
            self.content.clone()
        }
    }

    fn accessibility_value(&self) -> String {
        if self.password && !self.content.is_empty() {
            masked_display_text(&self.content).to_string()
        } else {
            self.content.to_string()
        }
    }

    fn display_offset(&self, offset: usize) -> usize {
        if self.password && !self.content.is_empty() {
            masked_display_offset_for_content_offset(&self.content, offset)
        } else {
            offset
        }
    }

    fn display_range(&self, range: &Range<usize>) -> Range<usize> {
        if self.password && !self.content.is_empty() {
            masked_display_range_for_content_range(&self.content, range.clone())
        } else {
            range.clone()
        }
    }

    fn content_offset_for_display_offset(&self, offset: usize) -> usize {
        if self.password && !self.content.is_empty() {
            masked_content_offset_for_display_offset(&self.content, offset)
        } else {
            offset
        }
    }

    fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn move_word_left(&mut self, _: &MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        if self.selected_range.is_empty() {
            self.move_to(
                previous_word_boundary(&self.content, self.cursor_offset()),
                cx,
            );
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn move_word_right(&mut self, _: &MoveWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        if self.selected_range.is_empty() {
            self.move_to(next_word_boundary(&self.content, self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.select_to(
            previous_word_boundary(&self.content, self.cursor_offset()),
            cx,
        );
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.select_to(next_word_boundary(&self.content, self.cursor_offset()), cx);
    }

    fn move_to_start(&mut self, _: &MoveToStart, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.move_to(0, cx);
    }

    fn move_to_end(&mut self, _: &MoveToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.move_to(self.content.len(), cx);
    }

    fn select_to_start(&mut self, _: &SelectToStart, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.select_to(0, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.select_to(self.content.len(), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.preferred_x = None;
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_backward(
        &mut self,
        _: &DeleteWordBackward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(
                previous_word_boundary(&self.content, self.cursor_offset()),
                cx,
            );
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(next_word_boundary(&self.content, self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Ok(Some(item)) = cx.read_from_clipboard()
            && let Some(text) = item.text()
        {
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }

        cx.write_to_clipboard(ClipboardItem::new_string(
            self.content[self.selected_range.clone()].to_string(),
        ));
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selected_range.is_empty() {
            return;
        }

        cx.write_to_clipboard(ClipboardItem::new_string(
            self.content[self.selected_range.clone()].to_string(),
        ));
        self.replace_text_in_range(None, "", window, cx);
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        self.finish_composition_history();
        let Some(snapshot) = self.history.undo() else {
            return;
        };

        self.restore_snapshot(snapshot);
        self.emit_change(window, cx);
        cx.notify();
    }

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let Some(snapshot) = self.history.redo() else {
            return;
        };

        self.restore_snapshot(snapshot);
        self.emit_change(window, cx);
        cx.notify();
    }

    fn insert_newline(&mut self, _: &InsertNewline, window: &mut Window, cx: &mut Context<Self>) {
        if self.multi_line && !self.read_only {
            self.replace_text_in_range(None, "\n", window, cx);
        } else {
            self.emit_submit(window, cx);
        }
    }

    fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        self.emit_submit(window, cx);
        self.emit_key(
            TextInputKeyTrigger::CommandEnter,
            TextInputKeyOutcome::Submit,
            window,
            cx,
        );
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, false, cx);
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, false, cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, true, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, true, cx);
    }

    fn primary_enter(&mut self, _: &PrimaryEnter, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_key_trigger(TextInputKeyTrigger::Enter, window, cx);
    }

    fn shift_enter(&mut self, _: &ShiftEnter, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_key_trigger(TextInputKeyTrigger::ShiftEnter, window, cx);
    }

    fn alt_enter(&mut self, _: &AltEnter, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_key_trigger(TextInputKeyTrigger::AltEnter, window, cx);
    }

    fn tab_forward(&mut self, _: &TabForward, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_key_trigger(TextInputKeyTrigger::Tab, window, cx);
    }

    fn tab_backward(&mut self, _: &TabBackward, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_key_trigger(TextInputKeyTrigger::ShiftTab, window, cx);
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_key_trigger(TextInputKeyTrigger::Escape, window, cx);
    }

    fn apply_key_trigger(
        &mut self,
        trigger: TextInputKeyTrigger,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let outcome = text_input_key_outcome(self.multi_line, self.key_policy, trigger);
        match outcome {
            TextInputKeyOutcome::Newline => {
                if !self.read_only {
                    self.replace_text_in_range(None, "\n", window, cx);
                }
            }
            TextInputKeyOutcome::Submit => self.emit_submit(window, cx),
            TextInputKeyOutcome::Cancel => {}
        }
        self.emit_key(trigger, outcome, window, cx);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        self.preferred_x = None;
        window.focus(&self.focus_handle);

        if event.click_count >= 3 {
            self.select_all(&SelectAll, window, cx);
            return;
        }

        let index = self.index_for_mouse_position(event.position);
        if event.click_count == 2 {
            self.selected_range = self.word_range_at(index);
            self.selection_reversed = false;
            cx.notify();
            return;
        }

        if event.modifiers.shift {
            self.select_to(index, cx);
        } else {
            self.move_to(index, cx);
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.preferred_x = None;
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = clamp_offset_to_boundary(&self.content, offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = clamp_offset_to_boundary(&self.content, offset);
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }

        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }

        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_vertical(&mut self, delta: i32, select: bool, cx: &mut Context<Self>) {
        let Some(layout) = self.last_layout.as_ref() else {
            return;
        };
        let display_cursor = self.display_offset(self.cursor_offset());
        let Some(origin) = layout.position_for_index(display_cursor) else {
            return;
        };
        let preferred_x = self.preferred_x.unwrap_or(origin.x);
        self.preferred_x = Some(preferred_x);
        let target = point(
            preferred_x,
            origin.y + layout.line_height * delta as f32 + layout.line_height * 0.5,
        );
        let display_offset = layout.closest_index_for_position(target);
        let offset = clamp_offset_to_boundary(
            &self.content,
            self.content_offset_for_display_offset(display_offset),
        );
        if select {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn word_range_at(&self, offset: usize) -> Range<usize> {
        if self.content.is_empty() {
            return 0..0;
        }

        let anchor = self.previous_boundary(offset.min(self.content.len()));
        for (start, segment) in self.content.split_word_bound_indices() {
            let end = start + segment.len();
            if start <= anchor && anchor < end {
                return start..end;
            }
        }

        anchor..anchor
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        utf16_offset_to_utf8(&self.content, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        utf8_offset_to_utf16(&self.content, offset)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }

        let Some(layout) = self.last_layout.as_ref() else {
            return 0;
        };

        let display_offset = layout.closest_index_for_position(position);
        let offset = self.content_offset_for_display_offset(display_offset);
        clamp_offset_to_boundary(&self.content, offset)
    }

    fn apply_replacement(
        &mut self,
        range: Range<usize>,
        text: &str,
        selected_range: Option<Range<usize>>,
        marked_range: Option<Range<usize>>,
        record_history: bool,
    ) -> bool {
        let replacement = sanitize_text(text, self.multi_line);
        let original_snapshot = self.snapshot();
        let prior_content = self.content.to_string();
        let prior_cursor = self.cursor_offset();
        let mut next_content =
            self.content[0..range.start].to_owned() + &replacement + &self.content[range.end..];
        let mut next_marked_range = marked_range.map(|range_in_replacement| {
            range.start + range_in_replacement.start..range.start + range_in_replacement.end
        });
        let mut next_selected_range = selected_range
            .map(|range_in_replacement| {
                range.start + range_in_replacement.start..range.start + range_in_replacement.end
            })
            .unwrap_or_else(|| {
                let end = range.start + replacement.len();
                end..end
            });

        if let Some(mask) = self.mask.as_ref() {
            let mut corrected_cursor = next_selected_range.end;
            mask.correct(
                &prior_content,
                prior_cursor,
                &mut next_content,
                &mut corrected_cursor,
            );
            let corrected_cursor = clamp_offset_to_boundary(&next_content, corrected_cursor);
            next_selected_range = corrected_cursor..corrected_cursor;
            next_marked_range = None;
        }

        self.content = next_content.into();
        self.marked_range = next_marked_range
            .map(|range| clamp_range_to_text(&self.content, range))
            .filter(|range| range.start < range.end);
        self.selected_range = clamp_range_to_text(&self.content, next_selected_range);
        self.selection_reversed = false;
        self.preferred_x = None;
        let changed = original_snapshot.content != self.content;
        if changed && record_history {
            let next_snapshot = self.snapshot();
            let merge_kind = if self.mask.is_some() {
                None
            } else {
                text_input_merge_kind(&original_snapshot, &range, &replacement, &next_snapshot)
            };
            self.history
                .record(original_snapshot, next_snapshot, merge_kind);
        }
        changed
    }

    fn finish_composition_history(&mut self) {
        let Some(original_snapshot) = self.composition_start.take() else {
            return;
        };
        let next_snapshot = self.snapshot();
        if original_snapshot != next_snapshot {
            self.history.record(original_snapshot, next_snapshot, None);
        }
    }

    fn emit_change(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(listener) = self.on_change.clone() {
            listener(self.content.clone(), window, cx);
        }
    }

    fn emit_submit(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(listener) = self.on_submit.clone() {
            listener(self.content.clone(), window, cx);
        }
    }

    fn emit_key(
        &self,
        trigger: TextInputKeyTrigger,
        outcome: TextInputKeyOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(listener) = self.on_key.clone() {
            listener(
                TextInputKeyEvent {
                    value: self.content.clone(),
                    trigger,
                    outcome,
                },
                window,
                cx,
            );
        }
    }
}

fn text_input_key_outcome(
    multi_line: bool,
    policy: TextInputKeyPolicy,
    trigger: TextInputKeyTrigger,
) -> TextInputKeyOutcome {
    match trigger {
        TextInputKeyTrigger::Escape => TextInputKeyOutcome::Cancel,
        TextInputKeyTrigger::CommandEnter => TextInputKeyOutcome::Submit,
        TextInputKeyTrigger::AltEnter if multi_line => TextInputKeyOutcome::Newline,
        TextInputKeyTrigger::Enter | TextInputKeyTrigger::ShiftEnter
            if multi_line && policy == TextInputKeyPolicy::Multiline =>
        {
            TextInputKeyOutcome::Newline
        }
        TextInputKeyTrigger::Tab
        | TextInputKeyTrigger::ShiftTab
        | TextInputKeyTrigger::Enter
        | TextInputKeyTrigger::ShiftEnter
        | TextInputKeyTrigger::AltEnter => TextInputKeyOutcome::Submit,
    }
}

impl EntityInputHandler for TextInputState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        adjusted_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked_range.take().is_some() {
            self.finish_composition_history();
            self.emit_selection_change(window, cx);
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let completing_composition =
            self.composition_start.is_some() || self.marked_range.is_some();
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        let changed = self.apply_replacement(range, text, None, None, !completing_composition);
        if completing_composition {
            self.marked_range = None;
            self.finish_composition_history();
        }
        if changed {
            self.emit_change(window, cx);
        }
        self.emit_selection_change(window, cx);
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
        if self.read_only {
            return;
        }
        if self.composition_start.is_none() {
            self.composition_start = Some(self.snapshot());
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        let replacement = sanitize_text(new_text, self.multi_line);
        let selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| utf16_range_to_utf8(&replacement, range_utf16.clone()));
        let marked_range = if replacement.is_empty() {
            None
        } else {
            Some(0..replacement.len())
        };

        let changed =
            self.apply_replacement(range, &replacement, selected_range, marked_range, false);
        if changed {
            self.emit_change(window, cx);
        }
        self.emit_selection_change(window, cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let display_range = self.display_range(&range);
        layout.selection_bounds(display_range)
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if self.content.is_empty() {
            return Some(0);
        }

        let layout = self.last_layout.as_ref()?;
        let display_index = layout.closest_index_for_position(point);
        let utf8_index = self.content_offset_for_display_offset(display_index);
        Some(self.offset_to_utf16(utf8_index))
    }
}

fn register_action_handler<A: Action + 'static>(
    window: &mut Window,
    state: Entity<TextInputState>,
    focus_handle: FocusHandle,
    handler: fn(&mut TextInputState, &A, &mut Window, &mut Context<TextInputState>),
) {
    window.on_action(TypeId::of::<A>(), move |action, phase, window, cx| {
        if phase != DispatchPhase::Bubble || !focus_handle.is_focused(window) {
            return;
        }

        let Some(action) = action.downcast_ref::<A>() else {
            return;
        };

        state.update(cx, |input, cx| {
            handler(input, action, window, cx);
            input.emit_selection_change(window, cx);
        });
        cx.stop_propagation();
    });
}

fn register_action_handler_when<A: Action + 'static>(
    window: &mut Window,
    enabled: bool,
    state: Entity<TextInputState>,
    focus_handle: FocusHandle,
    handler: fn(&mut TextInputState, &A, &mut Window, &mut Context<TextInputState>),
) {
    if enabled {
        register_action_handler(window, state, focus_handle, handler);
    }
}

fn register_text_input_accessibility_handlers(
    window: &mut Window,
    cx: &mut App,
    node: &AccessibilityNode,
    state: Entity<TextInputState>,
    focus_handle: FocusHandle,
    read_only: bool,
) {
    let window_handle = window.window_handle();
    let async_cx = cx.to_async();
    let executor = cx.foreground_executor().clone();
    window.on_accessibility_action(node.id, AccessibilityAction::Focus, move |_| {
        let focus_handle = focus_handle.clone();
        let mut async_cx = async_cx.clone();
        let window_handle = window_handle;
        executor
            .spawn(async move {
                _ = window_handle.update(&mut async_cx, |_, window, _| {
                    window.focus(&focus_handle);
                    window.refresh();
                });
            })
            .detach();
    });

    if read_only {
        return;
    }

    let window_handle = window.window_handle();
    let async_cx = cx.to_async();
    let executor = cx.foreground_executor().clone();
    window.on_accessibility_action(node.id, AccessibilityAction::SetValue, move |request| {
        let Some(AccessibilityActionPayload::Value(value)) = request.payload else {
            return;
        };
        let state = state.clone();
        let window_handle = window_handle;
        let mut async_cx = async_cx.clone();
        executor
            .spawn(async move {
                _ = window_handle.update(&mut async_cx, |_, window, cx| {
                    state.update(cx, |input, cx| {
                        let full_range = 0..input.content.len();
                        let changed = input.apply_replacement(full_range, &value, None, None, true);
                        if changed {
                            input.emit_change(window, cx);
                        }
                        input.emit_selection_change(window, cx);
                        cx.notify();
                    });
                    window.refresh();
                });
            })
            .detach();
    });
}

fn register_mouse_handlers(
    window: &mut Window,
    state: Entity<TextInputState>,
    focus_handle: FocusHandle,
    hitbox: Hitbox,
) {
    let down_state = state.clone();
    let down_hitbox = hitbox;
    let down_focus = focus_handle;
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble
            || event.button != MouseButton::Left
            || !down_hitbox.is_hovered(window)
        {
            return;
        }

        window.focus(&down_focus);
        down_state.update(cx, |input, cx| {
            input.on_mouse_down(event, window, cx);
            input.emit_selection_change(window, cx);
        });
        cx.stop_propagation();
    });

    let move_state = state.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }

        move_state.update(cx, |input, cx| {
            input.on_mouse_move(event, window, cx);
            input.emit_selection_change(window, cx);
        });
    });

    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
            return;
        }

        state.update(cx, |input, cx| {
            input.on_mouse_up(event, window, cx);
            input.emit_selection_change(window, cx);
        });
    });
}

fn text_runs_for_display(
    display_text: &SharedString,
    color: crate::Hsla,
    font: crate::Font,
    marked_range: Option<&Range<usize>>,
) -> Vec<TextRun> {
    let base_run = TextRun {
        len: display_text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    if let Some(marked_range) = marked_range {
        vec![
            TextRun {
                len: marked_range.start,
                ..base_run.clone()
            },
            TextRun {
                len: marked_range.end - marked_range.start,
                underline: Some(UnderlineStyle {
                    color: Some(color),
                    thickness: px(1.0),
                    wavy: false,
                }),
                ..base_run.clone()
            },
            TextRun {
                len: display_text.len().saturating_sub(marked_range.end),
                ..base_run
            },
        ]
        .into_iter()
        .filter(|run| run.len > 0)
        .collect()
    } else {
        vec![base_run]
    }
}

fn field_padding() -> Pixels {
    px(4.0)
}

fn uniform_edges(value: Pixels) -> Edges<Pixels> {
    Edges {
        top: value,
        right: value,
        bottom: value,
        left: value,
    }
}

fn content_wrap_width(outer_width: Pixels, insets: &Edges<Pixels>) -> Pixels {
    (outer_width - insets.left - insets.right - px(2.0)).max(px(0.0))
}

fn inset_bounds(bounds: Bounds<Pixels>, inset: Pixels) -> Bounds<Pixels> {
    Bounds::from_corners(
        point(bounds.left() + inset, bounds.top() + inset),
        point(bounds.right() - inset, bounds.bottom() - inset),
    )
}

fn inset_bounds_by_edges(bounds: Bounds<Pixels>, insets: &Edges<Pixels>) -> Bounds<Pixels> {
    Bounds::from_corners(
        point(bounds.left() + insets.left, bounds.top() + insets.top),
        point(
            bounds.right() - insets.right,
            bounds.bottom() - insets.bottom,
        ),
    )
}

fn display_text_for_input(input: &TextInputState, show_placeholder: bool) -> SharedString {
    if input.content.is_empty() {
        if show_placeholder {
            input.placeholder.clone()
        } else {
            SharedString::default()
        }
    } else {
        input.display_content()
    }
}

fn text_input_render_lines(layout: &TextInputLayout) -> Vec<TextInputRenderLine> {
    let mut origin = layout.content_origin;
    let mut lines = Vec::with_capacity(layout.paragraphs.len());

    for paragraph in &layout.paragraphs {
        lines.push(TextInputRenderLine {
            line: paragraph.line.clone(),
            origin,
        });
        origin.y += paragraph.line.layout.size(layout.line_height).height;
    }

    lines
}

fn paint_default_text_input(
    render_state: &TextInputRenderState,
    window: &mut Window,
    cx: &mut App,
) {
    let border_color: crate::Hsla = if render_state.focused {
        crate::blue()
    } else {
        rgb(0xd0d7de).into()
    };

    window.paint_quad(fill(render_state.outer_bounds, border_color));
    window.paint_quad(fill(render_state.field_bounds, white()));
    render_state.paint_default_contents(window, cx);
}

fn shape_text_input_lines(
    input: &TextInputState,
    wrap_width: Option<Pixels>,
    show_placeholder: bool,
    window: &mut Window,
) -> (usize, Vec<WrappedLine>) {
    let style = window.text_style();
    let display_text = display_text_for_input(input, show_placeholder);
    let display_len = display_text.len();
    let text_color = if show_placeholder && input.content.is_empty() {
        style.color.opacity(0.4)
    } else {
        style.color
    };
    let marked_range = if input.content.is_empty() {
        None
    } else {
        input
            .marked_range
            .as_ref()
            .map(|range| input.display_range(range))
    };
    let runs = text_runs_for_display(
        &display_text,
        text_color,
        style.font(),
        marked_range.as_ref(),
    );
    let font_size = style.font_size.to_pixels(window.rem_size());
    let lines = window
        .text_system()
        .shape_text(display_text, font_size, &runs, wrap_width, None)
        .unwrap_or_default()
        .into_iter()
        .collect();

    (display_len, lines)
}

fn build_text_input_layout(
    input: &TextInputState,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    window: &mut Window,
) -> TextInputLayout {
    let wrap_width = input.multi_line.then_some(bounds.size.width);
    let (display_len, lines) = shape_text_input_lines(input, wrap_width, true, window);
    TextInputLayout::new(
        lines,
        bounds,
        line_height,
        display_len,
        input.vertical_scroll,
    )
}

fn reveal_vertical_scroll(
    current_scroll: Pixels,
    viewport_bounds: Bounds<Pixels>,
    target_bounds: Bounds<Pixels>,
    max_scroll: Pixels,
) -> Pixels {
    let mut scroll = current_scroll.clamp(px(0.0), max_scroll);
    if target_bounds.top() < viewport_bounds.top() {
        scroll -= viewport_bounds.top() - target_bounds.top();
    } else if target_bounds.bottom() > viewport_bounds.bottom() {
        scroll += target_bounds.bottom() - viewport_bounds.bottom();
    }
    scroll.clamp(px(0.0), max_scroll)
}

fn wrapped_visual_line_count(line: &WrappedLine) -> usize {
    line.layout.wrap_boundaries().len() + 1
}

fn total_visual_line_count(lines: &[WrappedLine]) -> usize {
    lines.iter().map(wrapped_visual_line_count).sum()
}

fn paragraph_start_offsets(line_lengths: &[usize]) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(line_lengths.len());
    let mut offset = 0;
    for (ix, line_len) in line_lengths.iter().enumerate() {
        offsets.push(offset);
        offset += line_len;
        if ix + 1 < line_lengths.len() {
            offset += 1;
        }
    }
    offsets
}

fn sanitize_single_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

fn sanitize_multi_line(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if matches!(chars.peek(), Some('\n')) {
                chars.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(ch);
        }
    }

    normalized
}

fn sanitize_text(text: &str, multi_line: bool) -> String {
    if multi_line {
        sanitize_multi_line(text)
    } else {
        sanitize_single_line(text)
    }
}

fn is_word_segment(segment: &str) -> bool {
    segment.chars().any(char::is_alphanumeric)
}

fn previous_word_boundary(text: &str, offset: usize) -> usize {
    let offset = clamp_offset_to_boundary(text, offset);
    if offset == 0 {
        return 0;
    }

    let mut previous_word_start = 0;
    for (start, segment) in text.split_word_bound_indices() {
        let end = start + segment.len();
        if is_word_segment(segment) {
            if offset <= end {
                return start;
            }
            previous_word_start = start;
        } else if offset <= end {
            return previous_word_start;
        }
    }

    previous_word_start
}

fn next_word_boundary(text: &str, offset: usize) -> usize {
    let offset = clamp_offset_to_boundary(text, offset);
    for (start, segment) in text.split_word_bound_indices() {
        let end = start + segment.len();
        if offset < start {
            if is_word_segment(segment) {
                return end;
            }
            continue;
        }

        if start <= offset && offset < end && is_word_segment(segment) {
            return end;
        }
    }

    text.len()
}

fn masked_display_text(text: &str) -> SharedString {
    let mut masked = String::with_capacity(text.len());
    for grapheme in text.graphemes(true) {
        if grapheme == "\n" {
            masked.push('\n');
        } else {
            masked.push_str(PASSWORD_MASK_TEXT);
        }
    }

    masked.into()
}

fn masked_grapheme_display_len(grapheme: &str) -> usize {
    if grapheme == "\n" {
        1
    } else {
        PASSWORD_MASK_TEXT.len()
    }
}

fn masked_display_offset_for_content_offset(text: &str, offset: usize) -> usize {
    let clamped = clamp_offset_to_boundary(text, offset);
    text.grapheme_indices(true)
        .take_while(|(idx, _)| *idx < clamped)
        .fold(0, |display_offset, (_, grapheme)| {
            display_offset + masked_grapheme_display_len(grapheme)
        })
}

fn masked_content_offset_for_display_offset(text: &str, display_offset: usize) -> usize {
    let mut accumulated_display = 0;
    for (start, grapheme) in text.grapheme_indices(true) {
        let next_display = accumulated_display + masked_grapheme_display_len(grapheme);
        let next_content = start + grapheme.len();
        if display_offset < next_display {
            return start;
        }
        if display_offset == next_display {
            return next_content;
        }
        accumulated_display = next_display;
    }

    text.len()
}

fn masked_display_range_for_content_range(text: &str, range: Range<usize>) -> Range<usize> {
    masked_display_offset_for_content_offset(text, range.start)
        ..masked_display_offset_for_content_offset(text, range.end)
}

fn utf16_offset_to_utf8(text: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;

    for ch in text.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += ch.len_utf16();
        utf8_offset += ch.len_utf8();
    }

    utf8_offset
}

fn utf8_offset_to_utf16(text: &str, offset: usize) -> usize {
    let mut utf16_offset = 0;
    let mut utf8_count = 0;

    for ch in text.chars() {
        if utf8_count >= offset {
            break;
        }
        utf8_count += ch.len_utf8();
        utf16_offset += ch.len_utf16();
    }

    utf16_offset
}

fn utf16_range_to_utf8(text: &str, range: Range<usize>) -> Range<usize> {
    utf16_offset_to_utf8(text, range.start)..utf16_offset_to_utf8(text, range.end)
}

fn clamp_range_to_text(text: &str, range: Range<usize>) -> Range<usize> {
    let start = clamp_offset_to_boundary(text, range.start);
    let end = clamp_offset_to_boundary(text, range.end);
    if end < start {
        start..start
    } else {
        start..end
    }
}

fn clamp_offset_to_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }

    if offset == 0 {
        return 0;
    }

    text.grapheme_indices(true)
        .map(|(idx, _)| idx)
        .take_while(|idx| *idx <= offset)
        .last()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Context, ParentElement, Render, Styled, TestAppContext, Undo, div, window::FocusMap,
    };
    use parking_lot::RwLock;
    use slotmap::SlotMap;
    use std::{cell::RefCell, sync::Arc};

    #[derive(Default)]
    struct DigitsMask;

    struct DualTextInputView {
        first: SharedString,
        second: SharedString,
    }

    struct CustomTextInputView {
        value: SharedString,
        captured: Rc<RefCell<Vec<CapturedTextInputRenderState>>>,
    }

    struct ControlledTextInputView {
        value: SharedString,
        controller: TextInputController,
        read_only: bool,
    }

    struct NarrowTextInputView {
        value: SharedString,
        controller: TextInputController,
    }

    struct SelectionTextInputView {
        value: SharedString,
        controller: TextInputController,
        selections: Rc<RefCell<Vec<TextInputSelection>>>,
        text_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
        observe: bool,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct CapturedTextInputRenderState {
        value: SharedString,
        display_text: SharedString,
        summary: String,
        value_len_bytes: usize,
        display_text_len_bytes: usize,
        placeholder_len_bytes: usize,
        has_placeholder: bool,
        is_empty: bool,
        is_masked_display: bool,
        line_count: usize,
        showing_placeholder: bool,
        focused: bool,
        has_cursor: bool,
        has_selection: bool,
        selection_count: usize,
    }

    impl InputMask for DigitsMask {
        fn correct(&self, _was: &str, _cursor: usize, now: &mut String, new_cursor: &mut usize) {
            let digits_before_cursor = now[..(*new_cursor).min(now.len())]
                .chars()
                .filter(char::is_ascii_digit)
                .count();
            now.retain(|ch| ch.is_ascii_digit());
            *new_cursor = digits_before_cursor.min(now.len());
        }
    }

    fn snapshot(text: &str) -> TextInputSnapshot {
        TextInputSnapshot {
            content: text.to_string().into(),
            selected_range: text.len()..text.len(),
            selection_reversed: false,
            marked_range: None,
        }
    }

    fn test_history() -> TextInputHistory {
        let handles: Arc<FocusMap> = Arc::new(RwLock::new(SlotMap::with_key()));
        let focus_handle = FocusHandle::new(&handles);
        let history = WindowValueHistory::new(
            Rc::new(RefCell::new(crate::UndoRedoManager::default())),
            &focus_handle,
            "Text edit",
        );
        TextInputHistory::new(history)
    }

    fn test_input_state() -> TextInputState {
        let handles: Arc<FocusMap> = Arc::new(RwLock::new(SlotMap::with_key()));
        let focus_handle = FocusHandle::new(&handles);
        let history = WindowValueHistory::new(
            Rc::new(RefCell::new(crate::UndoRedoManager::default())),
            &focus_handle,
            "Text edit",
        );
        TextInputState::new(focus_handle, TextInputHistory::new(history))
    }

    impl Render for DualTextInputView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    text_input("first", self.first.clone()).on_change(cx.processor(
                        |this, value, _, cx| {
                            this.first = value;
                            cx.notify();
                        },
                    )),
                )
                .child(
                    text_input("second", self.second.clone()).on_change(cx.processor(
                        |this, value, _, cx| {
                            this.second = value;
                            cx.notify();
                        },
                    )),
                )
        }
    }

    impl Render for CustomTextInputView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let captured = self.captured.clone();

            text_input("custom", self.value.clone())
                .placeholder("Type here")
                .render_with(move |state, window, cx| {
                    captured.borrow_mut().push(CapturedTextInputRenderState {
                        value: state.value.clone(),
                        display_text: state.display_text.clone(),
                        summary: state.to_text(),
                        value_len_bytes: state.value_len_bytes(),
                        display_text_len_bytes: state.display_text_len_bytes(),
                        placeholder_len_bytes: state.placeholder_len_bytes(),
                        has_placeholder: state.has_placeholder(),
                        is_empty: state.is_empty(),
                        is_masked_display: state.is_masked_display(),
                        line_count: state.line_count(),
                        showing_placeholder: state.showing_placeholder,
                        focused: state.focused,
                        has_cursor: state.has_cursor(),
                        has_selection: state.has_selection(),
                        selection_count: state.selection_rect_count(),
                    });

                    window.paint_quad(fill(
                        state.outer_bounds,
                        if state.focused {
                            crate::blue()
                        } else {
                            rgb(0xd0d7de).into()
                        },
                    ));
                    window.paint_quad(fill(state.field_bounds, white()));
                    state.paint_default_contents(window, cx);
                })
                .on_change(cx.processor(|this, value, _, cx| {
                    this.value = value;
                    cx.notify();
                }))
        }
    }

    impl Render for ControlledTextInputView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let input = text_input("controlled", self.value.clone())
                .controller(self.controller.clone())
                .multi_line()
                .accessibility_label("Canvas text")
                .accessibility_description("Editable canvas text")
                .on_change(cx.processor(|this, value, _, cx| {
                    this.value = value;
                    cx.notify();
                }));
            if self.read_only {
                input.read_only()
            } else {
                input
            }
        }
    }

    impl Render for NarrowTextInputView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().w(px(64.0)).child(
                text_input("narrow", self.value.clone())
                    .controller(self.controller.clone())
                    .multi_line()
                    .on_change(cx.processor(|this, value, _, cx| {
                        this.value = value;
                        cx.notify();
                    })),
            )
        }
    }

    impl Render for SelectionTextInputView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let selections = self.selections.clone();
            let text_bounds = self.text_bounds.clone();
            let input = text_input("selection-observer", self.value.clone())
                .controller(self.controller.clone())
                .accessibility_label("Observed text input")
                .render_with(move |state, window, cx| {
                    text_bounds.borrow_mut().replace(state.text_bounds);
                    state.paint_default_contents(window, cx);
                })
                .on_change(cx.processor(|this, value, _, cx| {
                    this.value = value;
                    cx.notify();
                }));
            if self.observe {
                input.on_selection_change(move |selection, _, _| {
                    selections.borrow_mut().push(selection);
                })
            } else {
                input
            }
        }
    }

    fn latest_render_state(
        captured: &Rc<RefCell<Vec<CapturedTextInputRenderState>>>,
    ) -> CapturedTextInputRenderState {
        captured
            .borrow()
            .last()
            .cloned()
            .expect("expected captured render state")
    }

    #[test]
    fn utf16_conversion_round_trips_unicode_offsets() {
        let text = "a🙂ß";
        let utf8 = utf16_offset_to_utf8(text, 3);
        assert_eq!(utf8, "a🙂".len());
        assert_eq!(utf8_offset_to_utf16(text, utf8), 3);
    }

    #[test]
    fn sanitize_single_line_replaces_line_breaks() {
        assert_eq!(sanitize_single_line("a\nb\r\nc"), "a b  c");
    }

    #[test]
    fn sanitize_multi_line_normalizes_line_breaks() {
        assert_eq!(sanitize_multi_line("a\r\nb\rc"), "a\nb\nc");
    }

    #[test]
    fn multiline_builders_configure_internal_state() {
        let input = text_input("message", "")
            .multi_line()
            .max_lines(0)
            .content_insets(Edges {
                top: px(1.0),
                right: px(2.0),
                bottom: px(3.0),
                left: px(4.0),
            });

        assert!(input.multi_line);
        assert_eq!(input.max_lines, Some(1));
        assert_eq!(input.content_insets.left, px(4.0));
        assert_eq!(input.content_insets.bottom, px(3.0));
    }

    #[test]
    fn spreadsheet_and_multiline_key_policies_are_modifier_aware() {
        assert_eq!(
            text_input_key_outcome(
                true,
                TextInputKeyPolicy::Spreadsheet,
                TextInputKeyTrigger::AltEnter,
            ),
            TextInputKeyOutcome::Newline
        );
        for trigger in [
            TextInputKeyTrigger::Enter,
            TextInputKeyTrigger::ShiftEnter,
            TextInputKeyTrigger::Tab,
            TextInputKeyTrigger::ShiftTab,
        ] {
            assert_eq!(
                text_input_key_outcome(true, TextInputKeyPolicy::Spreadsheet, trigger),
                TextInputKeyOutcome::Submit
            );
        }
        assert_eq!(
            text_input_key_outcome(
                true,
                TextInputKeyPolicy::Multiline,
                TextInputKeyTrigger::Enter,
            ),
            TextInputKeyOutcome::Newline
        );
        assert_eq!(
            text_input_key_outcome(
                true,
                TextInputKeyPolicy::Multiline,
                TextInputKeyTrigger::Escape,
            ),
            TextInputKeyOutcome::Cancel
        );
    }

    #[test]
    fn ime_composition_records_one_undo_transaction() {
        let mut state = test_input_state();
        state.content = "start".into();
        state.selected_range = 5..5;
        state.composition_start = Some(state.snapshot());
        assert!(state.apply_replacement(5..5, "k", None, Some(0..1), false));
        assert!(state.apply_replacement(5..6, "かな", None, Some(0..6), false));
        state.marked_range = None;
        state.finish_composition_history();

        assert_eq!(
            state.history.undo().unwrap().content,
            SharedString::from("start")
        );
        assert!(state.history.undo().is_none());
        assert_eq!(
            state.history.redo().unwrap().content,
            SharedString::from("startかな")
        );
    }

    #[test]
    fn selection_snapshot_uses_utf8_bytes_and_reports_composition() {
        let mut state = test_input_state();
        state.content = "a🙂z".into();
        state.selected_range = 1.."a🙂".len();
        state.selection_reversed = true;
        state.marked_range = Some(1.."a🙂".len());

        let selection = state.selection_snapshot();
        assert_eq!(selection.range, 1..5);
        assert_eq!(selection.caret(), 1);
        assert!(selection.is_composing());
    }

    #[crate::test]
    fn rendered_selection_observer_tracks_controller_and_keyboard_once(cx: &mut TestAppContext) {
        let selections = Rc::new(RefCell::new(Vec::new()));
        let captured = selections.clone();
        let (view, mut window) = cx.add_window_view(move |_, cx| SelectionTextInputView {
            value: "a🙂z".into(),
            controller: TextInputController::new(cx.focus_handle()),
            selections: captured,
            text_bounds: Rc::new(RefCell::new(None)),
            observe: true,
        });
        window.update(|window, cx| window.draw(cx).clear());
        let controller = window.update(|_, cx| view.read(cx).controller.clone());
        window.update(|window, _| controller.select_range(1..5, true, window));
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_keystrokes("left");
        window.update(|window, cx| window.draw(cx).clear());

        assert_eq!(
            selections.borrow().as_slice(),
            &[
                TextInputSelection {
                    range: 0..0,
                    reversed: false,
                    marked_range: None,
                },
                TextInputSelection {
                    range: 1..5,
                    reversed: true,
                    marked_range: None,
                },
                TextInputSelection {
                    range: 1..1,
                    reversed: false,
                    marked_range: None,
                },
            ]
        );

        window.update(|window, cx| window.draw(cx).clear());
        assert_eq!(selections.borrow().len(), 3);
    }

    #[crate::test]
    fn observer_attached_to_retained_input_receives_initial_selection(cx: &mut TestAppContext) {
        let selections = Rc::new(RefCell::new(Vec::new()));
        let captured = selections.clone();
        let (view, mut window) = cx.add_window_view(move |_, cx| SelectionTextInputView {
            value: "retained".into(),
            controller: TextInputController::new(cx.focus_handle()),
            selections: captured,
            text_bounds: Rc::new(RefCell::new(None)),
            observe: false,
        });
        window.update(|window, cx| window.draw(cx).clear());
        assert!(selections.borrow().is_empty());

        window.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.observe = true;
                cx.notify();
            });
        });
        window.update(|window, cx| window.draw(cx).clear());
        assert_eq!(
            selections.borrow().as_slice(),
            &[TextInputSelection {
                range: 0..0,
                reversed: false,
                marked_range: None,
            }]
        );
    }

    #[crate::test]
    fn rendered_pointer_selection_emits_one_changed_snapshot(cx: &mut TestAppContext) {
        let selections = Rc::new(RefCell::new(Vec::new()));
        let bounds = Rc::new(RefCell::new(None));
        let captured_selections = selections.clone();
        let captured_bounds = bounds.clone();
        let (_view, mut window) = cx.add_window_view(move |_, cx| SelectionTextInputView {
            value: "abcdef".into(),
            controller: TextInputController::new(cx.focus_handle()),
            selections: captured_selections,
            text_bounds: captured_bounds,
            observe: true,
        });
        window.update(|window, cx| window.draw(cx).clear());
        let bounds = bounds.borrow().expect("text bounds");
        let end = point(
            bounds.origin.x + bounds.size.width - px(2.0),
            bounds.origin.y + bounds.size.height / 2.0,
        );
        window.simulate_mouse_down(end, MouseButton::Left, crate::Modifiers::default());
        window.simulate_mouse_up(end, MouseButton::Left, crate::Modifiers::default());

        assert_eq!(selections.borrow().len(), 2);
        assert_eq!(selections.borrow()[1].range, 6..6);
    }

    #[crate::test]
    fn rendered_ime_mark_and_commit_emit_bounded_composition_transitions(cx: &mut TestAppContext) {
        let selections = Rc::new(RefCell::new(Vec::new()));
        let captured = selections.clone();
        let (view, mut window) = cx.add_window_view(move |_, cx| SelectionTextInputView {
            value: "".into(),
            controller: TextInputController::new(cx.focus_handle()),
            selections: captured,
            text_bounds: Rc::new(RefCell::new(None)),
            observe: true,
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            view.read(cx).controller.focus(window);
            window.draw(cx).clear();
        });

        let mut input_handler = window
            .update(|window, _| window.platform_window.take_input_handler())
            .expect("focused text input handler");
        input_handler.replace_and_mark_text_in_range(None, "かな", Some(2..2));
        window.update(|window, _| window.platform_window.set_input_handler(input_handler));
        assert!(selections.borrow().last().unwrap().is_composing());
        assert_eq!(selections.borrow().last().unwrap().marked_range, Some(0..6));

        window.simulate_input("語");
        let snapshots = selections.borrow();
        assert!(!snapshots.last().unwrap().is_composing());
        assert_eq!(snapshots.last().unwrap().caret(), "語".len());
        assert_eq!(
            snapshots
                .windows(2)
                .filter(|pair| pair[0] == pair[1])
                .count(),
            0,
            "selection notifications must remain deduplicated"
        );
    }

    #[crate::test]
    fn accessibility_set_value_reports_the_resulting_selection_once(cx: &mut TestAppContext) {
        let selections = Rc::new(RefCell::new(Vec::new()));
        let captured = selections.clone();
        let (_view, mut window) = cx.add_window_view(move |_, cx| SelectionTextInputView {
            value: "before".into(),
            controller: TextInputController::new(cx.focus_handle()),
            selections: captured,
            text_bounds: Rc::new(RefCell::new(None)),
            observe: true,
        });
        let node_id = window.update(|window, cx| {
            window.draw(cx).clear();
            window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Observed text input"))
                .expect("observed text input node")
                .id
        });
        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(
                crate::AccessibilityActionRequest::with_payload(
                    node_id,
                    AccessibilityAction::SetValue,
                    AccessibilityActionPayload::Value("after".into()),
                ),
            );
        });
        window.run_until_parked();

        assert_eq!(selections.borrow().last().unwrap().range, 5..5);
        assert_eq!(
            selections
                .borrow()
                .windows(2)
                .filter(|pair| pair[0] == pair[1])
                .count(),
            0
        );
    }

    #[crate::test]
    fn controller_applies_pending_range_before_canvas_input(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, cx| ControlledTextInputView {
            value: "alpha\nbeta".into(),
            controller: TextInputController::new(cx.focus_handle()),
            read_only: false,
        });
        window.update(|window, cx| window.draw(cx).clear());
        let controller = window.update(|_, cx| view.read(cx).controller.clone());
        window.update(|window, _| controller.select_range(0..5, false, window));
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_input("A");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, SharedString::from("A\nbeta"));
        });
    }

    #[crate::test]
    fn vertical_navigation_tracks_preferred_x_across_short_hard_line(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, cx| ControlledTextInputView {
            value: "abcdef\nx\nabcdef".into(),
            controller: TextInputController::new(cx.focus_handle()),
            read_only: false,
        });
        window.update(|window, cx| window.draw(cx).clear());
        let controller = window.update(|_, cx| view.read(cx).controller.clone());
        window.update(|window, _| controller.select_range(5..5, false, window));
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_keystrokes("down down");
        window.simulate_input("X");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(
                view.read(cx).value,
                SharedString::from("abcdef\nx\nabcdeXf")
            );
        });
    }

    #[crate::test]
    fn vertical_navigation_moves_between_wrapped_visual_lines(cx: &mut TestAppContext) {
        let original = "abcdefghijklmnopqrstuvwxyz";
        let (view, mut window) = cx.add_window_view(|_, cx| NarrowTextInputView {
            value: original.into(),
            controller: TextInputController::new(cx.focus_handle()),
        });
        window.update(|window, cx| window.draw(cx).clear());
        let controller = window.update(|_, cx| view.read(cx).controller.clone());
        window.update(|window, _| controller.select_range(1..1, false, window));
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_keystrokes("down");
        window.simulate_input("X");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let value = view.read(cx).value.to_string();
            let inserted_at = value.find('X').expect("inserted text");
            assert!(inserted_at > 1, "down should leave the first visual line");
            assert!(
                inserted_at < original.len(),
                "down should not jump to the end"
            );
        });
    }

    #[crate::test]
    fn accessibility_exposes_metadata_read_only_and_set_value(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, cx| ControlledTextInputView {
            value: "initial".into(),
            controller: TextInputController::new(cx.focus_handle()),
            read_only: false,
        });
        let node_id = window.update(|window, cx| {
            window.draw(cx).clear();
            let node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Canvas text"))
                .expect("text input accessibility node");
            assert_eq!(node.description.as_deref(), Some("Editable canvas text"));
            assert!(!node.states.contains(AccessibilityState::READ_ONLY));
            assert!(node.actions.contains(&AccessibilityAction::Focus));
            assert!(node.actions.contains(&AccessibilityAction::SetValue));
            node.id
        });
        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(
                crate::AccessibilityActionRequest::with_payload(
                    node_id,
                    AccessibilityAction::SetValue,
                    AccessibilityActionPayload::Value("replacement".into()),
                ),
            );
        });
        window.run_until_parked();
        window.update(|_, cx| {
            assert_eq!(view.read(cx).value, SharedString::from("replacement"));
        });

        window.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.read_only = true;
                cx.notify();
            });
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            let node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Canvas text"))
                .expect("read-only text input accessibility node");
            assert!(node.states.contains(AccessibilityState::READ_ONLY));
            assert!(!node.actions.contains(&AccessibilityAction::SetValue));
        });
    }

    #[test]
    fn reveal_vertical_scroll_scrolls_down_to_show_target() {
        let viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(120.0), px(60.0)));
        let target = Bounds::new(point(px(0.0), px(72.0)), size(px(2.0), px(20.0)));

        assert_eq!(
            reveal_vertical_scroll(px(0.0), viewport, target, px(80.0)),
            px(32.0)
        );
    }

    #[test]
    fn reveal_vertical_scroll_scrolls_up_to_show_target() {
        let viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(120.0), px(60.0)));
        let target = Bounds::new(point(px(0.0), px(-8.0)), size(px(2.0), px(20.0)));

        assert_eq!(
            reveal_vertical_scroll(px(24.0), viewport, target, px(80.0)),
            px(16.0)
        );
    }

    #[test]
    fn paragraph_start_offsets_include_hard_line_breaks() {
        assert_eq!(paragraph_start_offsets(&[1, 0, 3]), vec![0, 2, 3]);
    }

    #[test]
    fn input_mask_can_rewrite_text_and_cursor() {
        let mut content = String::from("a1b2");
        let mut cursor = content.len();

        DigitsMask.correct("", 0, &mut content, &mut cursor);

        assert_eq!(content, "12");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn text_input_render_state_summary_detects_masked_display_without_content() {
        let state = TextInputRenderState {
            value: "secret".into(),
            display_text: "••••••".into(),
            placeholder: Some("Password".into()),
            showing_placeholder: false,
            focused: true,
            hovered: true,
            multi_line: false,
            outer_bounds: Bounds::default(),
            field_bounds: Bounds::default(),
            text_bounds: Bounds::default(),
            line_height: px(16.),
            lines: Vec::new(),
            selection_bounds: Vec::new(),
            cursor_bounds: Some(Bounds::default()),
        };

        assert_eq!(state.value_len_bytes(), 6);
        assert_eq!(state.display_text_len_bytes(), 18);
        assert_eq!(state.placeholder_len_bytes(), 8);
        assert!(state.has_placeholder());
        assert!(!state.is_empty());
        assert!(state.is_masked_display());
        assert_eq!(state.line_count(), 0);
        assert!(!state.has_selection());
        assert!(state.has_cursor());
        assert_eq!(
            state.to_text(),
            "text input render: value-bytes 6, display-bytes 18, placeholder true, placeholder-bytes 8, showing-placeholder false, focused true, hovered true, multiline false, lines 0, selection-rects 0, selection false, cursor true, masked-display true"
        );
        assert!(!state.to_text().contains("secret"));
        assert!(!state.to_text().contains("Password"));
    }

    #[test]
    fn word_boundary_helpers_skip_spacing() {
        let text = "hello  world";

        assert_eq!(previous_word_boundary(text, text.len()), 7);
        assert_eq!(previous_word_boundary(text, 7), 0);
        assert_eq!(next_word_boundary(text, 0), 5);
        assert_eq!(next_word_boundary(text, 5), text.len());
    }

    #[test]
    fn text_input_history_undo_redo_round_trips_snapshots() {
        let mut history = test_history();
        history.record(
            snapshot(""),
            snapshot("a"),
            Some(TextInputMergeKind::Insert),
        );
        history.record(
            snapshot("a"),
            snapshot("ab"),
            Some(TextInputMergeKind::Insert),
        );

        let undone = history.undo().unwrap();
        assert_eq!(undone.content, SharedString::from(""));

        let redone = history.redo().unwrap();
        assert_eq!(redone.content, SharedString::from("ab"));
    }

    #[test]
    fn text_input_history_clears_redo_on_new_record() {
        let mut history = test_history();
        history.record(
            snapshot(""),
            snapshot("a"),
            Some(TextInputMergeKind::Insert),
        );
        let _ = history.undo();
        history.record(
            snapshot(""),
            snapshot("x"),
            Some(TextInputMergeKind::Insert),
        );

        assert!(history.redo().is_none());
    }

    #[test]
    fn text_input_history_merges_adjacent_insertions() {
        let mut history = test_history();
        history.record(
            snapshot(""),
            snapshot("a"),
            Some(TextInputMergeKind::Insert),
        );
        history.record(
            snapshot("a"),
            snapshot("ab"),
            Some(TextInputMergeKind::Insert),
        );
        history.record(
            snapshot("ab"),
            snapshot("abc"),
            Some(TextInputMergeKind::Insert),
        );

        let undone = history.undo().unwrap();
        assert_eq!(undone.content, SharedString::from(""));

        let redone = history.redo().unwrap();
        assert_eq!(redone.content, SharedString::from("abc"));
    }

    #[crate::test]
    fn text_input_undo_availability_and_dispatch_follow_focus(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| DualTextInputView {
            first: SharedString::default(),
            second: SharedString::default(),
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(!window.is_action_available(&Undo, cx));
        });
        assert!(!window.cx.update(|app| app.has_undo()));
        assert!(!window.cx.update(|app| app.has_redo()));
        assert_eq!(window.cx.update(|app| app.undo_label()), None);
        assert_eq!(window.cx.update(|app| app.redo_label()), None);

        window.simulate_keystrokes("tab");
        window.simulate_input("a");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let view = view.read(cx);
            assert_eq!(view.first, SharedString::from("a"));
            assert_eq!(view.second, SharedString::default());
            assert!(window.is_action_available(&Undo, cx));
        });
        assert!(window.cx.update(|app| app.has_undo()));
        assert!(!window.cx.update(|app| app.has_redo()));
        assert_eq!(
            window.cx.update(|app| app.undo_label()),
            Some(SharedString::from("Text edit"))
        );
        assert_eq!(window.cx.update(|app| app.redo_label()), None);

        window.simulate_keystrokes("tab");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(!window.is_action_available(&Undo, cx));
        });
        assert!(!window.cx.update(|app| app.has_undo()));
        assert_eq!(window.cx.update(|app| app.undo_label()), None);

        window.simulate_input("x");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let view = view.read(cx);
            assert_eq!(view.first, SharedString::from("a"));
            assert_eq!(view.second, SharedString::from("x"));
            assert!(window.is_action_available(&Undo, cx));
        });
        assert!(window.cx.update(|app| app.has_undo()));
        assert_eq!(
            window.cx.update(|app| app.undo_label()),
            Some(SharedString::from("Text edit"))
        );

        window.simulate_keystrokes("shift-tab");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(window.is_action_available(&Undo, cx));
        });
        assert!(!window.cx.update(|app| app.has_undo()));
        assert_eq!(window.cx.update(|app| app.undo_label()), None);

        window.simulate_input("b");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let view = view.read(cx);
            assert_eq!(view.first, SharedString::from("ab"));
            assert_eq!(view.second, SharedString::from("x"));
            assert!(window.is_action_available(&Undo, cx));
        });
        assert!(window.cx.update(|app| app.has_undo()));
        assert_eq!(
            window.cx.update(|app| app.undo_label()),
            Some(SharedString::from("Text edit"))
        );

        window.simulate_keystrokes("secondary-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let view = view.read(cx);
            assert_eq!(view.first, SharedString::from("a"));
            assert_eq!(view.second, SharedString::from("x"));
        });
        assert!(!window.cx.update(|app| app.has_undo()));
        assert!(window.cx.update(|app| app.has_redo()));
        assert_eq!(window.cx.update(|app| app.undo_label()), None);
        assert_eq!(
            window.cx.update(|app| app.redo_label()),
            Some(SharedString::from("Text edit"))
        );

        window.simulate_keystrokes("tab");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(window.is_action_available(&Undo, cx));
        });
        assert!(window.cx.update(|app| app.has_undo()));
        assert!(!window.cx.update(|app| app.has_redo()));
        assert_eq!(
            window.cx.update(|app| app.undo_label()),
            Some(SharedString::from("Text edit"))
        );
        assert_eq!(window.cx.update(|app| app.redo_label()), None);

        window.simulate_keystrokes("secondary-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let view = view.read(cx);
            assert_eq!(view.first, SharedString::from("a"));
            assert_eq!(view.second, SharedString::default());
        });
        assert!(!window.cx.update(|app| app.has_undo()));
        assert!(window.cx.update(|app| app.has_redo()));
        assert_eq!(window.cx.update(|app| app.undo_label()), None);
        assert_eq!(
            window.cx.update(|app| app.redo_label()),
            Some(SharedString::from("Text edit"))
        );
    }

    #[crate::test]
    fn text_input_render_hook_receives_placeholder_focus_and_selection_state(
        cx: &mut TestAppContext,
    ) {
        let captured = Rc::new(RefCell::new(Vec::new()));
        let captured_for_view = captured.clone();
        let (view, mut window) = cx.add_window_view(|_, _| CustomTextInputView {
            value: SharedString::default(),
            captured: captured_for_view,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let initial = latest_render_state(&captured);
        assert_eq!(initial.value, SharedString::default());
        assert_eq!(initial.display_text, SharedString::from("Type here"));
        assert_eq!(initial.value_len_bytes, 0);
        assert_eq!(initial.display_text_len_bytes, 9);
        assert_eq!(initial.placeholder_len_bytes, 9);
        assert!(initial.has_placeholder);
        assert!(initial.is_empty);
        assert!(!initial.is_masked_display);
        assert_eq!(initial.line_count, 1);
        assert!(initial.showing_placeholder);
        assert!(!initial.focused);
        assert!(!initial.has_cursor);
        assert!(!initial.has_selection);
        assert_eq!(initial.selection_count, 0);
        assert_eq!(
            initial.summary,
            "text input render: value-bytes 0, display-bytes 9, placeholder true, placeholder-bytes 9, showing-placeholder true, focused false, hovered true, multiline false, lines 1, selection-rects 0, selection false, cursor false, masked-display false"
        );
        assert!(!initial.summary.contains("Type here"));

        window.simulate_keystrokes("tab");
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let focused = latest_render_state(&captured);
        assert!(focused.focused);
        assert!(focused.showing_placeholder);
        assert!(focused.has_cursor);

        window.simulate_input("hi");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, SharedString::from("hi"));
        });

        let typed = latest_render_state(&captured);
        assert_eq!(typed.value, SharedString::from("hi"));
        assert_eq!(typed.display_text, SharedString::from("hi"));
        assert_eq!(typed.value_len_bytes, 2);
        assert_eq!(typed.display_text_len_bytes, 2);
        assert!(!typed.is_empty);
        assert!(!typed.showing_placeholder);
        assert!(typed.focused);
        assert!(typed.has_cursor);
        assert_eq!(
            typed.summary,
            "text input render: value-bytes 2, display-bytes 2, placeholder true, placeholder-bytes 9, showing-placeholder false, focused true, hovered true, multiline false, lines 1, selection-rects 0, selection false, cursor true, masked-display false"
        );
        assert!(!typed.summary.contains("hi"));

        window.simulate_keystrokes("secondary-a");
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let selected = latest_render_state(&captured);
        assert_eq!(selected.value, SharedString::from("hi"));
        assert!(!selected.showing_placeholder);
        assert!(selected.focused);
        assert!(!selected.has_cursor);
        assert!(selected.has_selection);
        assert!(selected.selection_count > 0);
        assert!(!selected.summary.contains("hi"));
    }

    #[test]
    fn password_mask_uses_one_mask_glyph_per_grapheme() {
        let text = "a🙂e\u{301}";
        assert_eq!(
            masked_display_text(text).to_string(),
            PASSWORD_MASK_TEXT.repeat(3)
        );
    }

    #[test]
    fn password_mask_offsets_follow_grapheme_boundaries() {
        let text = "a🙂e\u{301}";
        let mask_len = PASSWORD_MASK_TEXT.len();
        let second_boundary = "a".len();
        let third_boundary = "a🙂".len();

        assert_eq!(
            masked_display_offset_for_content_offset(text, second_boundary),
            mask_len
        );
        assert_eq!(
            masked_display_offset_for_content_offset(text, third_boundary),
            mask_len * 2
        );
        assert_eq!(
            masked_content_offset_for_display_offset(text, mask_len),
            second_boundary
        );
        assert_eq!(
            masked_content_offset_for_display_offset(text, mask_len * 2),
            third_boundary
        );
        assert_eq!(
            masked_display_range_for_content_range(text, second_boundary..third_boundary),
            mask_len..mask_len * 2,
        );
    }

    #[test]
    fn password_mask_preserves_line_breaks_and_offsets() {
        let text = "a\n🙂";
        let mask_len = PASSWORD_MASK_TEXT.len();

        assert_eq!(
            masked_display_text(text).to_string(),
            format!("{PASSWORD_MASK_TEXT}\n{PASSWORD_MASK_TEXT}")
        );
        assert_eq!(
            masked_display_offset_for_content_offset(text, 2),
            mask_len + 1
        );
        assert_eq!(
            masked_content_offset_for_display_offset(text, mask_len + 1),
            2
        );
    }

    #[test]
    fn clamp_range_snaps_to_grapheme_boundaries() {
        let text = "a🙂b";
        assert_eq!(clamp_range_to_text(text, 0..3), 0..1);
        assert_eq!(clamp_range_to_text(text, 1..6), 1..6);
    }
}

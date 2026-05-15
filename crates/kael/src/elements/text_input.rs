use super::local_history::WindowValueHistory;
use crate::{
    AccessibilityAction, AccessibilityNode, AccessibilityRole, AccessibilityState,
    AccessibilityValue, Action, App, AppContext, Bounds, ClipboardItem, ContentMask, Context,
    CursorStyle, DispatchPhase, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Global, GlobalElementId, Hitbox, HitboxBehavior,
    InspectorElementId, IntoElement, KeyBinding, KeyContext, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, Style, TextRun, UTF16Selection,
    UnderlineStyle, Window, WrappedLine, WrappedLineLayout, fill, point, px, relative, rgb, rgba,
    size, white,
};
use std::{
    any::TypeId,
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
        /// Move the caret to the previous word boundary.
        MoveWordLeft,
        /// Move the caret to the next word boundary.
        MoveWordRight,
        /// Extend the selection one grapheme to the left.
        SelectLeft,
        /// Extend the selection one grapheme to the right.
        SelectRight,
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
    ]
);

type ChangeListener = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;
type SubmitListener = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;
type Mask = Rc<dyn InputMask>;

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
    multi_line: bool,
    max_lines: Option<usize>,
    password: bool,
    mask: Option<Mask>,
    on_change: Option<ChangeListener>,
    on_submit: Option<SubmitListener>,
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
            multi_line: false,
            max_lines: None,
            password: false,
            mask: None,
            on_change: None,
            on_submit: None,
            custom_renderer: None,
            source_location: core::panic::Location::caller(),
        }
    }

    /// Set the placeholder text shown when the field is empty.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
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
            KeyBinding::new("alt-left", MoveWordLeft, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("ctrl-left", MoveWordLeft, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("alt-right", MoveWordRight, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("ctrl-right", MoveWordRight, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-left", SelectLeft, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("shift-right", SelectRight, Some(TEXT_INPUT_CONTEXT)),
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
            KeyBinding::new("enter", InsertNewline, Some(TEXT_INPUT_CONTEXT)),
            KeyBinding::new("secondary-enter", Submit, Some(TEXT_INPUT_CONTEXT)),
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
        let state =
            window.with_element_state(global_id, |state: Option<Entity<TextInputState>>, _| {
                if let Some(state) = state {
                    (state.clone(), state)
                } else {
                    let state = cx.new(|cx| {
                        let focus_handle = cx.focus_handle();
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
                    (state.clone(), state)
                }
            });

        let text = self.text.clone();
        let placeholder = self.placeholder.clone();
        let multi_line = self.multi_line;
        let max_lines = self.max_lines;
        let password = self.password;
        let mask = self.mask.clone();
        let on_change = self.on_change.clone();
        let on_submit = self.on_submit.clone();
        state.update(cx, |state, _| {
            state.sync_from_props(
                text,
                placeholder,
                multi_line,
                max_lines,
                password,
                mask,
                on_change,
                on_submit,
            );
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
            let padding = field_padding();
            let mut style = Style::default();
            style.size.width = relative(1.0).into();
            style.size.height = (window.line_height() + padding * 2.0 + px(2.0)).into();
            return (window.request_layout(style, [], cx), ());
        }

        let global_id = id.expect("text_input always has an element id");
        let state = self.state(global_id, window, cx);
        let padding = field_padding();
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
                let wrap_width = outer_width.map(content_wrap_width);
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
                    outer_width.unwrap_or(content_width + field_chrome_extent()),
                    line_height * visible_lines + padding * 2.0 + px(2.0),
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

        let padding = field_padding();
        let inner_bounds = inset_bounds(bounds, px(1.0));
        let text_bounds = inset_bounds(inner_bounds, padding);
        let line_height = window.line_height();
        let desired_vertical_scroll = {
            let input = state.read(cx);
            let layout = build_text_input_layout(&input, text_bounds, line_height, window);
            input.target_vertical_scroll(&layout)
        };
        state.update(cx, |input, _| {
            input.vertical_scroll = desired_vertical_scroll;
        });
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
        let (focus_handle, can_undo, can_redo) = {
            let input = state.read(cx);
            (
                input.focus_handle.clone(),
                input.history.can_undo(),
                input.history.can_redo(),
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
        let (render_state, accessibility_value, accessibility_placeholder) = {
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
                input.accessibility_value(),
                (!input.placeholder.is_empty()).then_some(input.placeholder.to_string()),
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
            let mut node = AccessibilityNode::new(AccessibilityRole::TextInput)
                .with_states(accessibility_state)
                .with_value(AccessibilityValue::Text(accessibility_value))
                .with_actions(vec![AccessibilityAction::Focus]);
            if let Some(placeholder) = accessibility_placeholder {
                node.placeholder = Some(placeholder);
            }
            node
        };
        window.register_accessibility_node(node);

        state.update(cx, |input, _| {
            input.last_layout = Some(prepaint.layout.clone());
        });
    }
}

struct TextInputState {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    multi_line: bool,
    max_lines: Option<usize>,
    vertical_scroll: Pixels,
    password: bool,
    mask: Option<Mask>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<TextInputLayout>,
    is_selecting: bool,
    history: TextInputHistory,
    on_change: Option<ChangeListener>,
    on_submit: Option<SubmitListener>,
}

impl TextInputState {
    fn new(focus_handle: FocusHandle, history: TextInputHistory) -> Self {
        Self {
            focus_handle,
            content: SharedString::default(),
            placeholder: SharedString::default(),
            multi_line: false,
            max_lines: None,
            vertical_scroll: Pixels::ZERO,
            password: false,
            mask: None,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            is_selecting: false,
            history,
            on_change: None,
            on_submit: None,
        }
    }

    fn sync_from_props(
        &mut self,
        text: SharedString,
        placeholder: SharedString,
        multi_line: bool,
        max_lines: Option<usize>,
        password: bool,
        mask: Option<Mask>,
        on_change: Option<ChangeListener>,
        on_submit: Option<SubmitListener>,
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
        }

        self.placeholder = placeholder;
        self.multi_line = multi_line;
        self.max_lines = max_lines.map(|value| value.max(1));
        if !self.multi_line {
            self.vertical_scroll = Pixels::ZERO;
        }
        self.password = password;
        self.mask = mask;
        self.on_change = on_change;
        self.on_submit = on_submit;
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

    fn snapshot(&self) -> TextInputSnapshot {
        TextInputSnapshot {
            content: self.content.clone(),
            selected_range: self.selected_range.clone(),
            selection_reversed: self.selection_reversed,
            marked_range: self.marked_range.clone(),
        }
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
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn move_word_left(&mut self, _: &MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
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
        if self.selected_range.is_empty() {
            self.move_to(next_word_boundary(&self.content, self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(
            previous_word_boundary(&self.content, self.cursor_offset()),
            cx,
        );
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(next_word_boundary(&self.content, self.cursor_offset()), cx);
    }

    fn move_to_start(&mut self, _: &MoveToStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn move_to_end(&mut self, _: &MoveToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_to_start(&mut self, _: &SelectToStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
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
        if self.selected_range.is_empty() {
            self.select_to(next_word_boundary(&self.content, self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
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
        if self.selected_range.is_empty() {
            return;
        }

        cx.write_to_clipboard(ClipboardItem::new_string(
            self.content[self.selected_range.clone()].to_string(),
        ));
        self.replace_text_in_range(None, "", window, cx);
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.history.undo() else {
            return;
        };

        self.restore_snapshot(snapshot);
        self.emit_change(window, cx);
        cx.notify();
    }

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.history.redo() else {
            return;
        };

        self.restore_snapshot(snapshot);
        self.emit_change(window, cx);
        cx.notify();
    }

    fn insert_newline(&mut self, _: &InsertNewline, window: &mut Window, cx: &mut Context<Self>) {
        if self.multi_line {
            self.replace_text_in_range(None, "\n", window, cx);
        } else {
            self.emit_submit(window, cx);
        }
    }

    fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        self.emit_submit(window, cx);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
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
        let changed = original_snapshot.content != self.content;
        if changed {
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

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.marked_range.take().is_some() {
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
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        let changed = self.apply_replacement(range, text, None, None);
        if changed {
            self.emit_change(window, cx);
        }
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

        let changed = self.apply_replacement(range, &replacement, selected_range, marked_range);
        if changed {
            self.emit_change(window, cx);
        }
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

        state.update(cx, |input, cx| handler(input, action, window, cx));
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
        down_state.update(cx, |input, cx| input.on_mouse_down(event, window, cx));
        cx.stop_propagation();
    });

    let move_state = state.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }

        move_state.update(cx, |input, cx| input.on_mouse_move(event, window, cx));
    });

    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
            return;
        }

        state.update(cx, |input, cx| input.on_mouse_up(event, window, cx));
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

fn field_chrome_extent() -> Pixels {
    field_padding() * 2.0 + px(2.0)
}

fn content_wrap_width(outer_width: Pixels) -> Pixels {
    (outer_width - field_chrome_extent()).max(px(0.0))
}

fn inset_bounds(bounds: Bounds<Pixels>, inset: Pixels) -> Bounds<Pixels> {
    Bounds::from_corners(
        point(bounds.left() + inset, bounds.top() + inset),
        point(bounds.right() - inset, bounds.bottom() - inset),
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

fn wrapped_line_end_indices(layout: &WrappedLineLayout) -> Vec<usize> {
    let mut indices = Vec::with_capacity(layout.wrap_boundaries().len() + 1);
    for boundary in layout.wrap_boundaries() {
        indices.push(layout.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index);
    }
    indices.push(layout.len());
    indices
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

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct CapturedTextInputRenderState {
        value: SharedString,
        display_text: SharedString,
        showing_placeholder: bool,
        focused: bool,
        has_cursor: bool,
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
                        showing_placeholder: state.showing_placeholder,
                        focused: state.focused,
                        has_cursor: state.cursor_bounds.is_some(),
                        selection_count: state.selection_bounds.len(),
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
        let input = text_input("message", "").multi_line().max_lines(0);

        assert!(input.multi_line);
        assert_eq!(input.max_lines, Some(1));
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
        assert!(initial.showing_placeholder);
        assert!(!initial.focused);
        assert!(!initial.has_cursor);
        assert_eq!(initial.selection_count, 0);

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
        assert!(!typed.showing_placeholder);
        assert!(typed.focused);
        assert!(typed.has_cursor);

        window.simulate_keystrokes("secondary-a");
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let selected = latest_render_state(&captured);
        assert_eq!(selected.value, SharedString::from("hi"));
        assert!(!selected.showing_placeholder);
        assert!(selected.focused);
        assert!(!selected.has_cursor);
        assert!(selected.selection_count > 0);
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

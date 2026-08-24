use core::panic;
use std::{
    cell::{Cell, RefCell},
    mem,
    ops::Range,
    rc::Rc,
    sync::Arc,
};

use crate::colors::Colors;
use crate::{
    AnyElement, App, AvailableSpace, Bounds, CursorStyle, DispatchPhase, Element, ElementId,
    FocusHandle, FontWeight, GlobalElementId, HighlightStyle, Hitbox, HitboxBehavior, Hsla,
    InputHandler, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, Size, TextAlign, TextRun, TextStyle,
    UTF16Selection, UnderlineStyle, WhiteSpace, Window, WrappedLine, WrappedLineLayout, fill,
    point, px, size, util::wrapped_line_end_indices,
};

const INLINE_PLACEHOLDER: &str = "\u{fffc}";

/// Creates a rich text element that supports styled spans, inline elements, and selection.
#[track_caller]
pub fn rich_text() -> RichText {
    RichText {
        segments: Vec::new(),
        layout: RichTextLayout::default(),
        selectable: false,
        selection_color: None,
        element_id: None,
        source_location: panic::Location::caller(),
    }
}

type TextEntityAction = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum TextEntityKind {
    Link(SharedString),
    Mention(SharedString),
    Hashtag(SharedString),
}

#[derive(Clone)]
struct TextEntity {
    kind: TextEntityKind,
    range: Range<usize>,
    on_click: Option<TextEntityAction>,
}

/// A builder-backed rich text element for styled content, entities, and inline children.
pub struct RichText {
    segments: Vec<RichTextSegment>,
    layout: RichTextLayout,
    selectable: bool,
    selection_color: Option<Hsla>,
    element_id: Option<ElementId>,
    source_location: &'static panic::Location<'static>,
}

impl RichText {
    /// Appends plain text to the rich text block.
    pub fn text(mut self, text: impl Into<SharedString>) -> Self {
        self.push_text_segment(text.into(), RichTextSegmentStyle::Plain, None);
        self
    }

    /// Appends text with a highlight style applied to the current text style.
    pub fn styled(
        mut self,
        text: impl Into<SharedString>,
        style: impl Into<HighlightStyle>,
    ) -> Self {
        self.push_text_segment(
            text.into(),
            RichTextSegmentStyle::Highlight(style.into()),
            None,
        );
        self
    }

    /// Appends link text and registers a click handler for that entity range.
    pub fn link<F>(
        mut self,
        text: impl Into<SharedString>,
        target: impl Into<SharedString>,
        on_click: F,
    ) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.push_text_segment(
            text.into(),
            RichTextSegmentStyle::Link,
            Some(PendingTextEntity {
                kind: TextEntityKind::Link(target.into()),
                on_click: Some(Rc::new(on_click)),
            }),
        );
        self
    }

    /// Appends mention text and registers a click handler for that entity range.
    pub fn mention<F>(
        mut self,
        text: impl Into<SharedString>,
        payload: impl Into<SharedString>,
        on_click: F,
    ) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.push_text_segment(
            text.into(),
            RichTextSegmentStyle::Mention,
            Some(PendingTextEntity {
                kind: TextEntityKind::Mention(payload.into()),
                on_click: Some(Rc::new(on_click)),
            }),
        );
        self
    }

    /// Appends hashtag text and registers a click handler for that entity range.
    pub fn hashtag<F>(
        mut self,
        text: impl Into<SharedString>,
        payload: impl Into<SharedString>,
        on_click: F,
    ) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.push_text_segment(
            text.into(),
            RichTextSegmentStyle::Hashtag,
            Some(PendingTextEntity {
                kind: TextEntityKind::Hashtag(payload.into()),
                on_click: Some(Rc::new(on_click)),
            }),
        );
        self
    }

    /// Appends inline code-styled text.
    pub fn code(mut self, text: impl Into<SharedString>) -> Self {
        self.push_text_segment(text.into(), RichTextSegmentStyle::Code, None);
        self
    }

    /// Inserts an inline element aligned within the line box.
    pub fn inline_element(mut self, element: impl IntoElement) -> Self {
        self.segments.push(RichTextSegment::Inline {
            placeholder: INLINE_PLACEHOLDER.into(),
            baseline_offset: None,
            element: Some(element.into_any_element()),
        });
        self
    }

    /// Inserts an inline element using an explicit baseline offset from the element top.
    pub fn inline_element_with_baseline(
        mut self,
        element: impl IntoElement,
        baseline_offset: Pixels,
    ) -> Self {
        self.segments.push(RichTextSegment::Inline {
            placeholder: INLINE_PLACEHOLDER.into(),
            baseline_offset: Some(baseline_offset),
            element: Some(element.into_any_element()),
        });
        self
    }

    /// Enables mouse selection and platform text input geometry for the rich text block.
    pub fn selectable(mut self) -> Self {
        self.selectable = true;
        self
    }

    /// Tracks geometry and selection state through the provided layout handle.
    pub fn track_layout(mut self, layout: &RichTextLayout) -> Self {
        self.layout = layout.clone();
        self
    }

    /// Overrides the selection highlight color used while the element is selected.
    pub fn selection_color(mut self, color: Hsla) -> Self {
        self.selection_color = Some(color);
        self
    }

    /// Sets an explicit persistent element id for stateful selection and focus handling.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.element_id = Some(id.into());
        self
    }

    /// Total number of configured rich text segments.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Number of configured text segments.
    pub fn text_segment_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| matches!(segment, RichTextSegment::Text { .. }))
            .count()
    }

    /// Total UTF-8 byte length across configured text segments.
    pub fn text_len_bytes(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| match segment {
                RichTextSegment::Text { text, .. } => text.len(),
                RichTextSegment::Inline { .. } => 0,
            })
            .sum()
    }

    /// Number of inline element placeholders configured in the rich text block.
    pub fn inline_element_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| matches!(segment, RichTextSegment::Inline { .. }))
            .count()
    }

    /// Number of inline elements with explicit baseline offsets.
    pub fn inline_baseline_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| {
                matches!(
                    segment,
                    RichTextSegment::Inline {
                        baseline_offset: Some(_),
                        ..
                    }
                )
            })
            .count()
    }

    /// Number of highlighted text segments.
    pub fn highlighted_segment_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| {
                matches!(
                    segment,
                    RichTextSegment::Text {
                        style: RichTextSegmentStyle::Highlight(_),
                        ..
                    }
                )
            })
            .count()
    }

    /// Number of inline code text segments.
    pub fn code_segment_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| {
                matches!(
                    segment,
                    RichTextSegment::Text {
                        style: RichTextSegmentStyle::Code,
                        ..
                    }
                )
            })
            .count()
    }

    /// Number of text entity spans across links, mentions, and hashtags.
    pub fn entity_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| {
                matches!(
                    segment,
                    RichTextSegment::Text {
                        entity: Some(_),
                        ..
                    }
                )
            })
            .count()
    }

    /// Number of link entity spans.
    pub fn link_count(&self) -> usize {
        self.entity_kind_count(|kind| matches!(kind, TextEntityKind::Link(_)))
    }

    /// Number of mention entity spans.
    pub fn mention_count(&self) -> usize {
        self.entity_kind_count(|kind| matches!(kind, TextEntityKind::Mention(_)))
    }

    /// Number of hashtag entity spans.
    pub fn hashtag_count(&self) -> usize {
        self.entity_kind_count(|kind| matches!(kind, TextEntityKind::Hashtag(_)))
    }

    /// Number of entity spans with click handlers.
    pub fn click_handler_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| {
                matches!(
                    segment,
                    RichTextSegment::Text {
                        entity: Some(PendingTextEntity {
                            on_click: Some(_),
                            ..
                        }),
                        ..
                    }
                )
            })
            .count()
    }

    /// Whether the rich text block supports mouse selection.
    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// Whether an explicit selection highlight color is configured.
    pub fn has_selection_color(&self) -> bool {
        self.selection_color.is_some()
    }

    /// Whether a persistent element id is configured.
    pub fn has_element_id(&self) -> bool {
        self.element_id.is_some()
    }

    /// Content-safe summary that avoids logging text, URLs, mentions, hashtags, or payloads.
    pub fn to_text(&self) -> String {
        format!(
            "rich text: {} segments, text-segments {}, text-bytes {}, inline {}, baselined-inline {}, highlighted {}, code {}, entities {}, links {}, mentions {}, hashtags {}, click-handlers {}, selectable {}, selection-color {}, element-id {}",
            self.segment_count(),
            self.text_segment_count(),
            self.text_len_bytes(),
            self.inline_element_count(),
            self.inline_baseline_count(),
            self.highlighted_segment_count(),
            self.code_segment_count(),
            self.entity_count(),
            self.link_count(),
            self.mention_count(),
            self.hashtag_count(),
            self.click_handler_count(),
            self.is_selectable(),
            self.has_selection_color(),
            self.has_element_id()
        )
    }

    /// Finalizes the builder and returns the rich text element.
    pub fn build(self) -> Self {
        self
    }

    fn entity_kind_count(&self, is_match: impl Fn(&TextEntityKind) -> bool) -> usize {
        self.segments
            .iter()
            .filter(|segment| {
                matches!(
                    segment,
                    RichTextSegment::Text {
                        entity: Some(PendingTextEntity { kind, .. }),
                        ..
                    } if is_match(kind)
                )
            })
            .count()
    }

    fn push_text_segment(
        &mut self,
        text: SharedString,
        style: RichTextSegmentStyle,
        entity: Option<PendingTextEntity>,
    ) {
        if text.is_empty() {
            return;
        }

        self.segments.push(RichTextSegment::Text {
            text,
            style,
            entity,
        });
    }

    fn resolved_element_id(&self) -> ElementId {
        self.element_id
            .clone()
            .unwrap_or_else(|| ElementId::CodeLocation(*self.source_location))
    }

    fn take_segments(&mut self) -> (Vec<RichTextSegmentSnapshot>, Vec<InlineElementState>) {
        let mut snapshots = Vec::with_capacity(self.segments.len());
        let mut inline_elements = Vec::new();

        for segment in &mut self.segments {
            match segment {
                RichTextSegment::Text {
                    text,
                    style,
                    entity,
                } => {
                    snapshots.push(RichTextSegmentSnapshot::Text {
                        text: text.clone(),
                        style: style.clone(),
                        entity: entity.clone(),
                    });
                }
                RichTextSegment::Inline {
                    placeholder,
                    baseline_offset,
                    element,
                } => {
                    snapshots.push(RichTextSegmentSnapshot::Inline {
                        placeholder: placeholder.clone(),
                        baseline_offset: *baseline_offset,
                    });

                    if let Some(element) = element.take() {
                        inline_elements.push(InlineElementState {
                            element,
                            baseline_offset: *baseline_offset,
                            size: Size::default(),
                            paragraph_ix: 0,
                            global_offset: 0,
                            local_offset: 0,
                        });
                    }
                }
            }
        }

        (snapshots, inline_elements)
    }
}

impl IntoElement for RichText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for RichText {
    type RequestLayoutState = RichTextRequestState;
    type PrepaintState = RichTextPrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.resolved_element_id())
    }

    fn source_location(&self) -> Option<&'static panic::Location<'static>> {
        Some(self.source_location)
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let (segments, inline_elements) = self.take_segments();
        let request_state = RichTextRequestState::new(inline_elements);
        let base_line_height = window.line_height();
        {
            let mut inline_elements = request_state.inline_elements.borrow_mut();
            measure_inline_elements(&mut inline_elements, base_line_height, window, cx);
        }
        let layout_id = self.layout.layout(
            segments,
            request_state.inline_elements.clone(),
            request_state.entities.clone(),
            window,
            cx,
        );
        (layout_id, request_state)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.layout.prepaint(bounds);

        if let Some(global_id) = id {
            window.with_element_state::<RichTextState, _>(global_id, |state, window| {
                let mut state =
                    state.unwrap_or_else(|| RichTextState::new(&self.layout, self.selectable, cx));
                self.layout.bind_selection_state(state.selection.clone());

                if self.selectable
                    && let Some(focus_handle) = state.focus_handle.as_ref()
                {
                    window.set_focus_handle(focus_handle, cx);
                }

                ((), state)
            });
        }

        let inline_positions = self.layout.inline_positions();
        {
            let mut inline_elements = request_layout.inline_elements.borrow_mut();
            for placement in inline_positions {
                if let Some(inline) = inline_elements.get_mut(placement.child_ix) {
                    let origin = bounds.origin + placement.origin;
                    inline.element.prepaint_at(origin, window, cx);
                }
            }
        }

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        RichTextPrepaintState {
            hitbox,
            request_state: request_layout.clone(),
        }
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let default_selection_color: Hsla = Colors::for_appearance(window).selected.into();
        let selection_color = self
            .selection_color
            .unwrap_or_else(|| default_selection_color.opacity(0.24));
        let text_align = window.text_style().text_align;
        let entities = request_layout.entities.borrow().clone();
        let hitbox = prepaint.hitbox.clone();

        if let Some(global_id) = id {
            window.with_element_state::<RichTextState, _>(global_id, |state, window| {
                let mut state =
                    state.unwrap_or_else(|| RichTextState::new(&self.layout, self.selectable, cx));
                self.layout.bind_selection_state(state.selection.clone());

                if let Some(focus_handle) = state.focus_handle.as_ref() {
                    if self.selectable && focus_handle.is_focused(window) {
                        window.handle_input(
                            focus_handle,
                            RichTextInputHandler::new(self.layout.clone()),
                            cx,
                        );
                    }
                }

                if self.selectable || !entities.is_empty() {
                    let layout = self.layout.clone();
                    let selection = state.selection.clone();
                    let mouse_down_index = state.mouse_down_index.clone();
                    let focus_handle = state.focus_handle.clone();
                    let selectable = self.selectable;
                    let entities_for_down = entities.clone();
                    let hitbox_for_down = hitbox.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _cx| {
                        if phase != DispatchPhase::Bubble
                            || event.button != MouseButton::Left
                            || !hitbox_for_down.is_hovered(window)
                        {
                            return;
                        }

                        let index = layout.closest_index_for_position(event.position);
                        mouse_down_index.set(Some(index));

                        if selectable {
                            selection.borrow_mut().set(index, index);
                            if let Some(focus_handle) = focus_handle.as_ref() {
                                window.focus(focus_handle);
                            }
                            window.refresh();
                        } else if entity_at(&entities_for_down, index).is_some() {
                            window.refresh();
                        }
                    });

                    let layout = self.layout.clone();
                    let selection = state.selection.clone();
                    let mouse_down_index = state.mouse_down_index.clone();
                    let hitbox_for_move = hitbox.clone();
                    let cursor_hitbox = hitbox.clone();
                    let entities_for_move = entities.clone();
                    let selectable = self.selectable;
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _cx| {
                        if phase != DispatchPhase::Bubble {
                            return;
                        }

                        if event.dragging() {
                            if selectable && let Some(anchor) = mouse_down_index.get() {
                                let head = layout.closest_index_for_position(event.position);
                                selection.borrow_mut().set(anchor, head);
                                window.refresh();
                            }
                            return;
                        }

                        if !hitbox_for_move.is_hovered(window) {
                            return;
                        }

                        let index = layout.closest_index_for_position(event.position);
                        if let Some(entity) = entity_at(&entities_for_move, index) {
                            if entity_supports_pointer(entity) {
                                window.set_cursor_style(CursorStyle::PointingHand, &cursor_hitbox);
                            }
                        } else if selectable {
                            window.set_cursor_style(CursorStyle::IBeam, &cursor_hitbox);
                        }
                    });

                    let layout = self.layout.clone();
                    let selection = state.selection.clone();
                    let mouse_down_index = state.mouse_down_index.clone();
                    let entities_for_up = entities.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                            return;
                        }

                        let Some(anchor) = mouse_down_index.take() else {
                            return;
                        };

                        let head = layout.closest_index_for_position(event.position);
                        if selectable {
                            selection.borrow_mut().set(anchor, head);
                            window.refresh();
                        }

                        if anchor == head
                            && let Some(entity) = entity_at(&entities_for_up, head)
                            && entity_supports_pointer(entity)
                            && let Some(on_click) = entity.on_click.as_ref()
                        {
                            on_click(window, cx);
                        }
                    });
                }

                ((), state)
            });
        }

        self.layout.paint_backgrounds(text_align, window, cx);
        self.layout.paint_selection(selection_color, window, cx);
        self.layout.paint_text(text_align, window, cx);

        let mut inline_elements = prepaint.request_state.inline_elements.borrow_mut();
        for inline in inline_elements.iter_mut() {
            inline.element.paint(window, cx);
        }
    }
}

/// A tracked layout handle for rich text geometry and selection queries.
#[derive(Clone)]
pub struct RichTextLayout {
    inner: Rc<RefCell<Option<RichTextLayoutInner>>>,
    selection_state: Rc<RefCell<Rc<RefCell<RichTextSelectionState>>>>,
}

impl Default for RichTextLayout {
    fn default() -> Self {
        let selection = Rc::new(RefCell::new(RichTextSelectionState::default()));
        Self {
            inner: Rc::new(RefCell::new(None)),
            selection_state: Rc::new(RefCell::new(selection)),
        }
    }
}

impl RichTextLayout {
    /// Returns the full text content as a UTF-8 string.
    pub fn text(&self) -> String {
        self.inner
            .borrow()
            .as_ref()
            .map(|inner| inner.text.to_string())
            .unwrap_or_default()
    }

    /// Returns the wrapped text content with line breaks inserted at wrap boundaries.
    pub fn wrapped_text(&self) -> String {
        let inner = self.inner.borrow();
        let Some(inner) = inner.as_ref() else {
            return String::new();
        };

        let mut wrapped_lines = Vec::new();
        for paragraph in &inner.paragraphs {
            let mut start = 0;
            for end in wrapped_line_end_indices(&paragraph.line.layout) {
                wrapped_lines.push(paragraph.line.text[start..end].to_string());
                start = end;
            }
        }

        wrapped_lines.join("\n")
    }

    /// Returns the effective line height used by the current layout.
    pub fn line_height(&self) -> Pixels {
        self.inner
            .borrow()
            .as_ref()
            .map(|inner| inner.line_height)
            .unwrap_or_default()
    }

    /// Returns the current non-empty selection as a UTF-8 byte range.
    pub fn selected_range(&self) -> Option<Range<usize>> {
        self.selection_handle().borrow().normalized_range()
    }

    /// Returns the currently selected text, if the selection is non-empty.
    pub fn selected_text(&self) -> Option<String> {
        let range = self.selected_range()?;
        let text = self.text();
        text.get(range).map(ToOwned::to_owned)
    }

    /// Returns the bounds of the current non-empty selection.
    pub fn selected_bounds(&self) -> Option<Bounds<Pixels>> {
        let range = self.selected_range()?;
        self.selection_bounds(range)
    }

    /// Returns the bounds for an arbitrary UTF-8 byte range in the rich text layout.
    pub fn selection_bounds(&self, range: Range<usize>) -> Option<Bounds<Pixels>> {
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

    /// Returns the absolute position for the given UTF-8 byte index.
    pub fn position_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        let inner = self.inner.borrow();
        let inner = inner.as_ref()?;
        let bounds = inner.bounds?;
        let mut paragraph_origin = bounds.origin;

        for paragraph in &inner.paragraphs {
            let paragraph_end = paragraph.start_offset + paragraph.line.len();
            if index < paragraph.start_offset {
                break;
            }

            if index > paragraph_end {
                paragraph_origin.y += paragraph.line.layout.size(inner.line_height).height;
                continue;
            }

            let local_index = index - paragraph.start_offset;
            return paragraph
                .line
                .layout
                .position_for_index(local_index, inner.line_height)
                .map(|position| paragraph_origin + position);
        }

        None
    }

    /// Returns the UTF-8 byte index at a point, or the nearest edge index when outside the layout.
    pub fn index_for_position(&self, position: Point<Pixels>) -> Result<usize, usize> {
        let inner = self.inner.borrow();
        let Some(inner) = inner.as_ref() else {
            return Err(0);
        };
        let Some(bounds) = inner.bounds else {
            return Err(0);
        };

        if position.y < bounds.origin.y {
            return Err(0);
        }

        let mut paragraph_origin = bounds.origin;
        for paragraph in &inner.paragraphs {
            let paragraph_height = paragraph.line.layout.size(inner.line_height).height;
            let paragraph_bottom = paragraph_origin.y + paragraph_height;
            if position.y <= paragraph_bottom {
                let local = position - paragraph_origin;
                return paragraph
                    .line
                    .layout
                    .index_for_position(local, inner.line_height)
                    .map(|index| paragraph.start_offset + index)
                    .map_err(|index| paragraph.start_offset + index);
            }

            paragraph_origin.y = paragraph_bottom;
        }

        Err(inner.len)
    }

    /// Returns the closest UTF-8 byte index for the provided point.
    pub fn closest_index_for_position(&self, position: Point<Pixels>) -> usize {
        match self.index_for_position(position) {
            Ok(index) | Err(index) => index,
        }
    }

    fn layout(
        &self,
        segments: Vec<RichTextSegmentSnapshot>,
        inline_elements: Rc<RefCell<Vec<InlineElementState>>>,
        entities: Rc<RefCell<Vec<TextEntity>>>,
        window: &mut Window,
        _cx: &mut App,
    ) -> LayoutId {
        let layout = self.clone();
        window.request_measured_layout(
            Default::default(),
            move |known_dimensions, available_space, window, _cx| {
                let base_style = window.text_style();
                let colors = Colors::for_appearance(window);
                let wrap_width = if matches!(base_style.white_space, WhiteSpace::Normal) {
                    known_dimensions
                        .width
                        .or_else(|| match available_space.width {
                            AvailableSpace::Definite(width) => Some(width),
                            _ => None,
                        })
                } else {
                    None
                };
                let font_size = window.ui_length_in_pixels(base_style.font_size);
                let line_height = window
                    .ui_definite_length_in_pixels(base_style.line_height, font_size)
                    .round();
                let mut inline_elements = inline_elements.borrow_mut();
                let effective_line_height =
                    inline_elements.iter().fold(line_height, |height, inline| {
                        height.max(inline.size.height.ceil())
                    });
                let document =
                    materialize_document(&segments, &mut inline_elements, &base_style, &colors);
                *entities.borrow_mut() = document.entities.clone();

                let mut paragraphs = Vec::with_capacity(document.paragraphs.len());
                let mut inline_positions = Vec::new();
                let mut content_size = size(px(0.), px(0.));

                for (paragraph_ix, paragraph) in document.paragraphs.iter().enumerate() {
                    let shaped = window.text_system().shape_line(
                        paragraph.text.clone(),
                        font_size,
                        &paragraph.runs,
                        None,
                    );
                    let mut unwrapped_layout = clone_line_layout(&shaped.layout);
                    adjust_inline_positions(&mut unwrapped_layout, paragraph_ix, &inline_elements);
                    let wrap_boundaries = match wrap_width {
                        Some(width) => unwrapped_layout.wrap_boundaries_for_text(
                            paragraph.text.as_ref(),
                            width,
                            base_style.line_clamp,
                        ),
                        None => Default::default(),
                    };
                    let wrapped_layout = WrappedLineLayout {
                        unwrapped_layout: Arc::new(unwrapped_layout),
                        wrap_boundaries,
                        wrap_width,
                    };
                    let wrapped_line = WrappedLine {
                        layout: Arc::new(wrapped_layout),
                        text: paragraph.text.clone(),
                        decoration_runs: shaped.decoration_runs.clone(),
                    };

                    let paragraph_origin_y = content_size.height;
                    for (child_ix, inline) in inline_elements.iter().enumerate() {
                        if inline.paragraph_ix != paragraph_ix {
                            continue;
                        }

                        let local_position = wrapped_line
                            .layout
                            .position_for_index(inline.local_offset, effective_line_height)
                            .unwrap_or_default();
                        let line_baseline = local_position.y + wrapped_line.layout.ascent();
                        let local_top = match inline.baseline_offset {
                            Some(baseline_offset) => line_baseline - baseline_offset,
                            None => {
                                local_position.y + (effective_line_height - inline.size.height) / 2.
                            }
                        }
                        .max(px(0.));

                        inline_positions.push(RichInlinePlacement {
                            child_ix,
                            origin: point(local_position.x, paragraph_origin_y + local_top),
                        });
                    }

                    let paragraph_size = wrapped_line.layout.size(effective_line_height);
                    content_size.width = content_size.width.max(paragraph_size.width);
                    content_size.height += paragraph_size.height;
                    paragraphs.push(RichTextParagraphLayout {
                        start_offset: paragraph.start_offset,
                        line: wrapped_line,
                    });
                }

                let len = document.text.len();
                *layout.inner.borrow_mut() = Some(RichTextLayoutInner {
                    text: document.text,
                    len,
                    line_height: effective_line_height,
                    paragraphs,
                    inline_positions,
                    bounds: None,
                });

                content_size
            },
        )
    }

    fn prepaint(&self, bounds: Bounds<Pixels>) {
        if let Some(inner) = self.inner.borrow_mut().as_mut() {
            inner.bounds = Some(bounds);
        }
    }

    fn paint_backgrounds(&self, align: TextAlign, window: &mut Window, cx: &mut App) {
        let inner = self.inner.borrow();
        let Some(inner) = inner.as_ref() else {
            return;
        };
        let Some(bounds) = inner.bounds else {
            return;
        };

        let mut paragraph_origin = bounds.origin;
        for paragraph in &inner.paragraphs {
            let _ = paragraph.line.paint_background(
                paragraph_origin,
                inner.line_height,
                align,
                Some(bounds),
                window,
                cx,
            );
            paragraph_origin.y += paragraph.line.layout.size(inner.line_height).height;
        }
    }

    fn paint_selection(&self, color: Hsla, window: &mut Window, _cx: &mut App) {
        let Some(range) = self.selected_range() else {
            return;
        };

        for rect in self.selection_rects(range) {
            window.paint_quad(fill(rect, color));
        }
    }

    fn paint_text(&self, align: TextAlign, window: &mut Window, cx: &mut App) {
        let inner = self.inner.borrow();
        let Some(inner) = inner.as_ref() else {
            return;
        };
        let Some(bounds) = inner.bounds else {
            return;
        };

        let mut paragraph_origin = bounds.origin;
        for paragraph in &inner.paragraphs {
            let _ = paragraph.line.paint(
                paragraph_origin,
                inner.line_height,
                align,
                Some(bounds),
                window,
                cx,
            );
            paragraph_origin.y += paragraph.line.layout.size(inner.line_height).height;
        }
    }

    fn inline_positions(&self) -> Vec<RichInlinePlacement> {
        self.inner
            .borrow()
            .as_ref()
            .map(|inner| inner.inline_positions.clone())
            .unwrap_or_default()
    }

    fn selection_rects(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let inner = self.inner.borrow();
        let Some(inner) = inner.as_ref() else {
            return Vec::new();
        };
        let Some(bounds) = inner.bounds else {
            return Vec::new();
        };

        let start = range.start.min(inner.len);
        let end = range.end.min(inner.len);
        if start > end {
            return Vec::new();
        }

        if start == end {
            return self
                .position_for_index(start)
                .map_or_else(Vec::new, |origin| {
                    vec![Bounds {
                        origin,
                        size: size(px(1.), inner.line_height),
                    }]
                });
        }

        let mut rects = Vec::new();
        let mut paragraph_origin = bounds.origin;
        for paragraph in &inner.paragraphs {
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
                        .position_for_index(segment_start - paragraph_start, inner.line_height)
                        .unwrap_or_default();
                    let end_position = paragraph
                        .line
                        .layout
                        .position_for_index(segment_end - paragraph_start, inner.line_height)
                        .unwrap_or_default();
                    rects.push(Bounds {
                        origin: line_origin + point(start_position.x, px(0.)),
                        size: size(end_position.x - start_position.x, inner.line_height),
                    });
                }

                line_origin.y += inner.line_height;
                line_start = line_end;
            }

            paragraph_origin.y += paragraph.line.layout.size(inner.line_height).height;
        }

        rects
    }

    fn bind_selection_state(&self, selection: Rc<RefCell<RichTextSelectionState>>) {
        *self.selection_state.borrow_mut() = selection;
    }

    fn selection_handle(&self) -> Rc<RefCell<RichTextSelectionState>> {
        self.selection_state.borrow().clone()
    }
}

struct RichTextLayoutInner {
    text: SharedString,
    len: usize,
    line_height: Pixels,
    paragraphs: Vec<RichTextParagraphLayout>,
    inline_positions: Vec<RichInlinePlacement>,
    bounds: Option<Bounds<Pixels>>,
}

struct RichTextParagraphLayout {
    start_offset: usize,
    line: WrappedLine,
}

#[derive(Clone)]
struct RichInlinePlacement {
    child_ix: usize,
    origin: Point<Pixels>,
}

#[doc(hidden)]
#[derive(Clone)]
pub struct RichTextRequestState {
    inline_elements: Rc<RefCell<Vec<InlineElementState>>>,
    entities: Rc<RefCell<Vec<TextEntity>>>,
}

impl RichTextRequestState {
    fn new(inline_elements: Vec<InlineElementState>) -> Self {
        Self {
            inline_elements: Rc::new(RefCell::new(inline_elements)),
            entities: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

#[doc(hidden)]
pub struct RichTextPrepaintState {
    hitbox: Hitbox,
    request_state: RichTextRequestState,
}

struct RichTextState {
    focus_handle: Option<FocusHandle>,
    mouse_down_index: Rc<Cell<Option<usize>>>,
    selection: Rc<RefCell<RichTextSelectionState>>,
}

impl RichTextState {
    fn new(layout: &RichTextLayout, selectable: bool, cx: &mut App) -> Self {
        Self {
            focus_handle: selectable.then(|| cx.focus_handle()),
            mouse_down_index: Rc::new(Cell::new(None)),
            selection: layout.selection_handle(),
        }
    }
}

#[derive(Clone, Default)]
struct RichTextSelectionState {
    anchor: Option<usize>,
    head: Option<usize>,
}

impl RichTextSelectionState {
    fn set(&mut self, anchor: usize, head: usize) {
        self.anchor = Some(anchor);
        self.head = Some(head);
    }

    fn normalized_range(&self) -> Option<Range<usize>> {
        let (Some(anchor), Some(head)) = (self.anchor, self.head) else {
            return None;
        };

        if anchor == head {
            return None;
        }

        Some(anchor.min(head)..anchor.max(head))
    }

    fn active_range(&self) -> Range<usize> {
        let anchor = self.anchor.unwrap_or(0);
        let head = self.head.unwrap_or(anchor);
        anchor.min(head)..anchor.max(head)
    }

    fn reversed(&self) -> bool {
        matches!((self.anchor, self.head), (Some(anchor), Some(head)) if head < anchor)
    }
}

enum RichTextSegment {
    Text {
        text: SharedString,
        style: RichTextSegmentStyle,
        entity: Option<PendingTextEntity>,
    },
    Inline {
        placeholder: SharedString,
        baseline_offset: Option<Pixels>,
        element: Option<AnyElement>,
    },
}

enum RichTextSegmentSnapshot {
    Text {
        text: SharedString,
        style: RichTextSegmentStyle,
        entity: Option<PendingTextEntity>,
    },
    Inline {
        placeholder: SharedString,
        baseline_offset: Option<Pixels>,
    },
}

#[derive(Clone)]
struct PendingTextEntity {
    kind: TextEntityKind,
    on_click: Option<TextEntityAction>,
}

#[derive(Clone)]
enum RichTextSegmentStyle {
    Plain,
    Highlight(HighlightStyle),
    Link,
    Mention,
    Hashtag,
    Code,
}

impl RichTextSegmentStyle {
    fn resolve(&self, base_style: &TextStyle, colors: &Colors) -> TextStyle {
        let mut style = base_style.clone();
        match self {
            RichTextSegmentStyle::Plain => {}
            RichTextSegmentStyle::Highlight(highlight) => {
                style = style.highlight(*highlight);
            }
            RichTextSegmentStyle::Link => {
                let accent: Hsla = colors.selected.into();
                style.color = accent;
                style.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(accent),
                    wavy: false,
                });
            }
            RichTextSegmentStyle::Mention | RichTextSegmentStyle::Hashtag => {
                style.color = colors.selected.into();
                style.font_weight = FontWeight::MEDIUM;
            }
            RichTextSegmentStyle::Code => {
                let code_background: Hsla = colors.selected.into();
                style.font_family = ".ZedMono".into();
                style.font_weight = FontWeight::MEDIUM;
                style.background_color = Some(code_background.opacity(0.14));
            }
        }

        style
    }
}

struct InlineElementState {
    element: AnyElement,
    baseline_offset: Option<Pixels>,
    size: Size<Pixels>,
    paragraph_ix: usize,
    global_offset: usize,
    local_offset: usize,
}

struct MaterializedDocument {
    text: SharedString,
    paragraphs: Vec<MaterializedParagraph>,
    entities: Vec<TextEntity>,
}

struct MaterializedParagraph {
    start_offset: usize,
    text: SharedString,
    runs: Vec<TextRun>,
}

fn materialize_document(
    segments: &[RichTextSegmentSnapshot],
    inline_elements: &mut [InlineElementState],
    base_style: &TextStyle,
    colors: &Colors,
) -> MaterializedDocument {
    let mut paragraphs = Vec::new();
    let mut entities = Vec::new();
    let mut paragraph_text = String::new();
    let mut paragraph_runs = Vec::new();
    let mut paragraph_start_offset = 0;
    let mut inline_ix = 0;
    let mut placeholder_style = base_style.clone();
    placeholder_style.color.fade_out(1.);
    placeholder_style.background_color = None;
    placeholder_style.underline = None;
    placeholder_style.strikethrough = None;

    let finalize_paragraph = |paragraphs: &mut Vec<MaterializedParagraph>,
                              paragraph_start_offset: usize,
                              paragraph_text: &mut String,
                              paragraph_runs: &mut Vec<TextRun>,
                              base_style: &TextStyle| {
        let mut runs = mem::take(paragraph_runs);
        if runs.is_empty() {
            runs.push(base_style.to_run(0));
        }

        paragraphs.push(MaterializedParagraph {
            start_offset: paragraph_start_offset,
            text: SharedString::from(mem::take(paragraph_text)),
            runs,
        });
    };

    for segment in segments {
        match segment {
            RichTextSegmentSnapshot::Text {
                text,
                style,
                entity,
            } => {
                let resolved_style = style.resolve(base_style, colors);
                let mut remaining = text.as_ref();
                loop {
                    if let Some(newline_ix) = remaining.find('\n') {
                        let fragment = &remaining[..newline_ix];
                        append_text_fragment(
                            fragment,
                            &resolved_style,
                            entity.as_ref(),
                            paragraph_start_offset,
                            paragraph_text.len(),
                            &mut paragraph_text,
                            &mut paragraph_runs,
                            &mut entities,
                        );
                        finalize_paragraph(
                            &mut paragraphs,
                            paragraph_start_offset,
                            &mut paragraph_text,
                            &mut paragraph_runs,
                            base_style,
                        );
                        paragraph_start_offset += paragraphs
                            .last()
                            .map(|paragraph| paragraph.text.len() + 1)
                            .unwrap_or(1);
                        remaining = &remaining[newline_ix + 1..];
                    } else {
                        append_text_fragment(
                            remaining,
                            &resolved_style,
                            entity.as_ref(),
                            paragraph_start_offset,
                            paragraph_text.len(),
                            &mut paragraph_text,
                            &mut paragraph_runs,
                            &mut entities,
                        );
                        break;
                    }
                }
            }
            RichTextSegmentSnapshot::Inline {
                placeholder,
                baseline_offset,
            } => {
                if let Some(inline) = inline_elements.get_mut(inline_ix) {
                    inline.paragraph_ix = paragraphs.len();
                    inline.global_offset = paragraph_start_offset + paragraph_text.len();
                    inline.local_offset = paragraph_text.len();
                    inline.baseline_offset = *baseline_offset;
                }

                paragraph_text.push_str(placeholder);
                paragraph_runs.push(placeholder_style.to_run(placeholder.len()));
                inline_ix += 1;
            }
        }
    }

    finalize_paragraph(
        &mut paragraphs,
        paragraph_start_offset,
        &mut paragraph_text,
        &mut paragraph_runs,
        base_style,
    );

    let mut full_text = String::new();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        if index > 0 {
            full_text.push('\n');
        }
        full_text.push_str(paragraph.text.as_ref());
    }

    MaterializedDocument {
        text: full_text.into(),
        paragraphs,
        entities,
    }
}

fn append_text_fragment(
    fragment: &str,
    style: &TextStyle,
    entity: Option<&PendingTextEntity>,
    paragraph_start_offset: usize,
    paragraph_len: usize,
    paragraph_text: &mut String,
    paragraph_runs: &mut Vec<TextRun>,
    entities: &mut Vec<TextEntity>,
) {
    if fragment.is_empty() {
        return;
    }

    let start = paragraph_start_offset + paragraph_len;
    let end = start + fragment.len();
    paragraph_text.push_str(fragment);
    paragraph_runs.push(style.to_run(fragment.len()));

    if let Some(entity) = entity {
        entities.push(TextEntity {
            kind: entity.kind.clone(),
            range: start..end,
            on_click: entity.on_click.clone(),
        });
    }
}

fn measure_inline_elements(
    inline_elements: &mut [InlineElementState],
    line_height: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> Pixels {
    let mut effective_line_height = line_height;
    for inline in inline_elements.iter_mut() {
        inline.size = inline
            .element
            .layout_as_root(AvailableSpace::min_size(), window, cx);
        effective_line_height = effective_line_height.max(inline.size.height.ceil());
    }
    effective_line_height
}

fn clone_line_layout(layout: &Arc<crate::LineLayout>) -> crate::LineLayout {
    crate::LineLayout {
        font_size: layout.font_size,
        width: layout.width,
        ascent: layout.ascent,
        descent: layout.descent,
        runs: layout.runs.clone(),
        len: layout.len,
    }
}

fn adjust_inline_positions(
    line_layout: &mut crate::LineLayout,
    paragraph_ix: usize,
    inline_elements: &[InlineElementState],
) {
    for inline in inline_elements
        .iter()
        .filter(|inline| inline.paragraph_ix == paragraph_ix)
    {
        let placeholder_width =
            glyph_width_for_index(line_layout, inline.local_offset).unwrap_or_default();
        let delta = inline.size.width - placeholder_width;
        if delta == px(0.) {
            continue;
        }

        for run in &mut line_layout.runs {
            for glyph in &mut run.glyphs {
                if glyph.index > inline.local_offset {
                    glyph.position.x += delta;
                }
            }
        }
        line_layout.width += delta;
    }
}

fn glyph_width_for_index(line_layout: &crate::LineLayout, index: usize) -> Option<Pixels> {
    for (run_ix, run) in line_layout.runs.iter().enumerate() {
        for (glyph_ix, glyph) in run.glyphs.iter().enumerate() {
            if glyph.index == index {
                let next_x = run
                    .glyphs
                    .get(glyph_ix + 1)
                    .map(|next| next.position.x)
                    .or_else(|| {
                        line_layout
                            .runs
                            .get(run_ix + 1)
                            .and_then(|next_run| next_run.glyphs.first())
                            .map(|next| next.position.x)
                    })
                    .unwrap_or(line_layout.width);
                return Some(next_x - glyph.position.x);
            }
        }
    }

    None
}

fn entity_at(entities: &[TextEntity], index: usize) -> Option<&TextEntity> {
    entities
        .iter()
        .find(|entity| entity.range.start <= index && index < entity.range.end)
}

fn entity_supports_pointer(entity: &TextEntity) -> bool {
    matches!(
        entity.kind,
        TextEntityKind::Link(_) | TextEntityKind::Mention(_) | TextEntityKind::Hashtag(_)
    )
}

struct RichTextInputHandler {
    layout: RichTextLayout,
}

impl RichTextInputHandler {
    fn new(layout: RichTextLayout) -> Self {
        Self { layout }
    }
}

impl InputHandler for RichTextInputHandler {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<UTF16Selection> {
        let text = self.layout.text();
        let selection = self.layout.selection_handle();
        let selection = selection.borrow();
        let range = selection.active_range();
        Some(UTF16Selection {
            range: utf8_range_to_utf16(&text, range),
            reversed: selection.reversed(),
        })
    }

    fn marked_text_range(&mut self, _window: &mut Window, _cx: &mut App) -> Option<Range<usize>> {
        None
    }

    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<String> {
        let text = self.layout.text();
        let (utf8_range, actual_utf16_range) = clamp_utf16_range(&text, range_utf16);
        *adjusted_range = Some(actual_utf16_range);
        text.get(utf8_range).map(ToOwned::to_owned)
    }

    fn replace_text_in_range(
        &mut self,
        _replacement_range: Option<Range<usize>>,
        _text: &str,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        _new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut App) {}

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let text = self.layout.text();
        let (utf8_range, _) = clamp_utf16_range(&text, range_utf16);
        self.layout.selection_bounds(utf8_range)
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<usize> {
        let text = self.layout.text();
        let utf8_index = self
            .layout
            .closest_index_for_position(point)
            .min(text.len());
        Some(utf8_to_utf16_index(&text, utf8_index))
    }
}

fn clamp_utf16_range(text: &str, range_utf16: Range<usize>) -> (Range<usize>, Range<usize>) {
    let utf16_len = text.encode_utf16().count();
    let start = range_utf16.start.min(utf16_len);
    let end = range_utf16.end.min(utf16_len);
    let normalized = start.min(end)..start.max(end);
    let utf8_range =
        utf16_to_utf8_index(text, normalized.start)..utf16_to_utf8_index(text, normalized.end);
    (utf8_range, normalized)
}

fn utf8_range_to_utf16(text: &str, range: Range<usize>) -> Range<usize> {
    utf8_to_utf16_index(text, range.start)..utf8_to_utf16_index(text, range.end)
}

fn utf8_to_utf16_index(text: &str, utf8_index: usize) -> usize {
    let mut utf16_index = 0;
    for (byte_ix, ch) in text.char_indices() {
        if byte_ix >= utf8_index {
            break;
        }
        utf16_index += ch.len_utf16();
    }
    utf16_index
}

fn utf16_to_utf8_index(text: &str, utf16_index: usize) -> usize {
    let mut utf16_offset = 0;
    for (byte_ix, ch) in text.char_indices() {
        if utf16_offset >= utf16_index {
            return byte_ix;
        }

        utf16_offset += ch.len_utf16();
        if utf16_offset >= utf16_index {
            return byte_ix + ch.len_utf8();
        }
    }

    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Context, InteractiveElement, Modifiers, ParentElement, Render, Styled, TestAppContext,
        Window, div, point, px, rgb,
    };

    #[test]
    fn rich_text_builder_summaries_are_content_safe() {
        let block = rich_text()
            .id("audit-rich-text")
            .selectable()
            .selection_color(rgb(0x2255ff).into())
            .text("Hello ")
            .styled("important", HighlightStyle::default())
            .link("docs", "https://example.com/private", |_, _| {})
            .mention("@ada", "user-123", |_, _| {})
            .hashtag("#launch", "launch-plan", |_, _| {})
            .code("secret_token")
            .inline_element(div())
            .inline_element_with_baseline(div(), px(8.));

        assert_eq!(block.segment_count(), 8);
        assert_eq!(block.text_segment_count(), 6);
        assert_eq!(block.text_len_bytes(), 42);
        assert_eq!(block.inline_element_count(), 2);
        assert_eq!(block.inline_baseline_count(), 1);
        assert_eq!(block.highlighted_segment_count(), 1);
        assert_eq!(block.code_segment_count(), 1);
        assert_eq!(block.entity_count(), 3);
        assert_eq!(block.link_count(), 1);
        assert_eq!(block.mention_count(), 1);
        assert_eq!(block.hashtag_count(), 1);
        assert_eq!(block.click_handler_count(), 3);
        assert!(block.is_selectable());
        assert!(block.has_selection_color());
        assert!(block.has_element_id());
        assert_eq!(
            block.to_text(),
            "rich text: 8 segments, text-segments 6, text-bytes 42, inline 2, baselined-inline 1, highlighted 1, code 1, entities 3, links 1, mentions 1, hashtags 1, click-handlers 3, selectable true, selection-color true, element-id true"
        );
        assert!(!block.to_text().contains("Hello"));
        assert!(!block.to_text().contains("important"));
        assert!(!block.to_text().contains("https://example.com/private"));
        assert!(!block.to_text().contains("user-123"));
        assert!(!block.to_text().contains("launch-plan"));
        assert!(!block.to_text().contains("secret_token"));
    }

    struct LinkRoot {
        layout: RichTextLayout,
        clicks: Rc<Cell<usize>>,
    }

    impl Render for LinkRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                rich_text()
                    .track_layout(&self.layout)
                    .text("Open ")
                    .link("docs", "docs", {
                        let clicks = self.clicks.clone();
                        move |_, _| clicks.set(clicks.get() + 1)
                    })
                    .build(),
            )
        }
    }

    struct SelectionRoot {
        layout: RichTextLayout,
    }

    impl Render for SelectionRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                rich_text()
                    .track_layout(&self.layout)
                    .selectable()
                    .text("Hello world")
                    .build(),
            )
        }
    }

    struct InlineRoot {
        layout: RichTextLayout,
    }

    impl Render for InlineRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div().w(px(64.)).child(
                    rich_text()
                        .track_layout(&self.layout)
                        .text("wrap ")
                        .inline_element(
                            div()
                                .w(px(40.))
                                .h(px(18.))
                                .bg(rgb(0x4e89ff))
                                .debug_selector(|| "inline-chip".to_string()),
                        )
                        .text(" tail")
                        .build(),
                ),
            )
        }
    }

    #[kael::test]
    fn rich_text_dispatches_link_clicks(cx: &mut TestAppContext) {
        let clicks = Rc::new(Cell::new(0));
        let (view, window) = cx.add_window_view(|_, _| LinkRoot {
            layout: RichTextLayout::default(),
            clicks: clicks.clone(),
        });
        let layout = window.update(|window, cx| {
            window.draw(cx).clear();
            view.read(cx).layout.clone()
        });
        let click_point =
            layout.position_for_index(6).unwrap() + point(px(1.), layout.line_height() / 2.);

        window.simulate_click(click_point, Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(clicks.get(), 1);
    }

    #[kael::test]
    fn rich_text_tracks_drag_selection(cx: &mut TestAppContext) {
        let (view, window) = cx.add_window_view(|_, _| SelectionRoot {
            layout: RichTextLayout::default(),
        });
        let layout = window.update(|window, cx| {
            window.draw(cx).clear();
            view.read(cx).layout.clone()
        });
        let start =
            layout.position_for_index(1).unwrap() + point(px(0.), layout.line_height() / 2.);
        let end = layout.position_for_index(5).unwrap() + point(px(0.), layout.line_height() / 2.);

        window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(layout.selected_range(), Some(1..5));
        assert_eq!(layout.selected_text().as_deref(), Some("ello"));
        assert!(layout.selected_bounds().is_some());
    }

    #[kael::test]
    fn rich_text_wraps_inline_elements(cx: &mut TestAppContext) {
        let (view, window) = cx.add_window_view(|_, _| InlineRoot {
            layout: RichTextLayout::default(),
        });
        let layout = window.update(|window, cx| {
            window.draw(cx).clear();
            view.read(cx).layout.clone()
        });

        let inline_bounds = window.debug_bounds("inline-chip").unwrap();
        assert!(layout.wrapped_text().contains('\n'));
        assert!(inline_bounds.origin.y > px(0.));
    }
}

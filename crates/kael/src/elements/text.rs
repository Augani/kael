use crate::{
    ActiveTooltip, AnyView, App, Bounds, DispatchPhase, Element, ElementId, GlobalElementId,
    HighlightStyle, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, Size, TextOverflow,
    TextRun, TextStyle, TooltipId, WhiteSpace, Window, WrappedLine, WrappedLineLayout, px,
    register_tooltip_mouse_handlers, set_tooltip_on_window,
};
use anyhow::Context as _;
use smallvec::SmallVec;
use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use util::ResultExt;

impl Element for &'static str {
    type RequestLayoutState = TextLayout;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut state = TextLayout::default();
        let layout_id = state.layout(SharedString::from(*self), None, window, cx);
        (layout_id, state)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        text_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        text_layout.prepaint(bounds, self)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        text_layout: &mut TextLayout,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        text_layout.paint(self, window, cx);
        let mut node = crate::AccessibilityNode::new(crate::AccessibilityRole::StaticText)
            .with_label(self.to_string());
        node.id = window.next_anonymous_accessibility_id();
        window.register_accessibility_node_at(node, bounds);
    }
}

impl IntoElement for &'static str {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl IntoElement for String {
    type Element = SharedString;

    fn into_element(self) -> Self::Element {
        self.into()
    }
}

impl Element for SharedString {
    type RequestLayoutState = TextLayout;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut state = TextLayout::default();
        let layout_id = state.layout(self.clone(), None, window, cx);
        (layout_id, state)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        text_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        text_layout.prepaint(bounds, self.as_ref())
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        text_layout: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        text_layout.paint(self.as_ref(), window, cx);
        let mut node = crate::AccessibilityNode::new(crate::AccessibilityRole::StaticText)
            .with_label(self.to_string());
        node.id = window.next_anonymous_accessibility_id();
        window.register_accessibility_node_at(node, bounds);
    }
}

impl IntoElement for SharedString {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Renders text with runs of different styles.
///
/// Callers are responsible for setting the correct style for each run.
/// For text with a uniform style, you can usually avoid calling this constructor
/// and just pass text directly.
pub struct StyledText {
    text: SharedString,
    runs: Option<Vec<TextRun>>,
    delayed_highlights: Option<Vec<(Range<usize>, HighlightStyle)>>,
    layout: TextLayout,
    accessibility_hidden: bool,
}

impl StyledText {
    /// Construct a new styled text element from the given string.
    pub fn new(text: impl Into<SharedString>) -> Self {
        StyledText {
            text: text.into(),
            runs: None,
            delayed_highlights: None,
            layout: TextLayout::default(),
            accessibility_hidden: false,
        }
    }

    /// Get the layout for this element. This can be used to map indices to pixels and vice versa.
    pub fn layout(&self) -> &TextLayout {
        &self.layout
    }

    /// Set the styling attributes for the given text, as well as
    /// as any ranges of text that have had their style customized.
    pub fn with_default_highlights(
        mut self,
        default_style: &TextStyle,
        highlights: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> Self {
        debug_assert!(
            self.delayed_highlights.is_none(),
            "Can't use `with_default_highlights` and `with_highlights`"
        );
        let runs = Self::compute_runs(&self.text, default_style, highlights);
        self.runs = Some(runs);
        self
    }

    /// Set the styling attributes for the given text, as well as
    /// as any ranges of text that have had their style customized.
    pub fn with_highlights(
        mut self,
        highlights: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> Self {
        debug_assert!(
            self.runs.is_none(),
            "Can't use `with_highlights` and `with_default_highlights`"
        );
        self.delayed_highlights = Some(highlights.into_iter().collect::<Vec<_>>());
        self
    }

    fn compute_runs(
        text: &str,
        default_style: &TextStyle,
        highlights: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> Vec<TextRun> {
        let mut runs = Vec::new();
        let mut ix = 0;
        for (range, highlight) in highlights {
            if ix < range.start {
                runs.push(default_style.clone().to_run(range.start - ix));
            }
            runs.push(
                default_style
                    .clone()
                    .highlight(highlight)
                    .to_run(range.len()),
            );
            ix = range.end;
        }
        if ix < text.len() {
            runs.push(default_style.to_run(text.len() - ix));
        }
        runs
    }

    /// Set the text runs for this piece of text.
    pub fn with_runs(mut self, runs: Vec<TextRun>) -> Self {
        self.runs = Some(runs);
        self
    }

    /// Hide this visual text run from the accessibility tree.
    ///
    /// Use this only when an ancestor exposes an equivalent semantic label,
    /// such as an animated digit ticker whose rolling columns contain every
    /// possible digit.
    pub fn accessibility_hidden(mut self, hidden: bool) -> Self {
        self.accessibility_hidden = hidden;
        self
    }
}

impl Element for StyledText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let runs = self.runs.take().or_else(|| {
            self.delayed_highlights.take().map(|delayed_highlights| {
                Self::compute_runs(&self.text, &window.text_style(), delayed_highlights)
            })
        });

        let layout_id = self.layout.layout(self.text.clone(), runs, window, cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        self.layout.prepaint(bounds, &self.text)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.layout.paint(&self.text, window, cx);
        if !self.accessibility_hidden {
            let mut node = crate::AccessibilityNode::new(crate::AccessibilityRole::StaticText)
                .with_label(self.text.to_string());
            node.id = window.next_anonymous_accessibility_id();
            window.register_accessibility_node_at(node, bounds);
        }
    }
}

impl IntoElement for StyledText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// The Layout for TextElement. This can be used to map indices to pixels and vice versa.
#[derive(Default, Clone)]
pub struct TextLayout(Rc<RefCell<Option<TextLayoutInner>>>);

struct TextLayoutInner {
    len: usize,
    lines: SmallVec<[WrappedLine; 1]>,
    line_height: Pixels,
    wrap_width: Option<Pixels>,
    size: Option<Size<Pixels>>,
    bounds: Option<Bounds<Pixels>>,
}

impl TextLayout {
    fn layout(
        &self,
        text: SharedString,
        runs: Option<Vec<TextRun>>,
        window: &mut Window,
        _: &mut App,
    ) -> LayoutId {
        let text_style = window.text_style();
        let font_size = window.ui_length_in_pixels(text_style.font_size);
        let line_height = window.ui_definite_length_in_pixels(text_style.line_height, font_size);

        let mut runs = if let Some(runs) = runs {
            runs
        } else {
            vec![text_style.to_run(text.len())]
        };

        window.request_measured_layout(Default::default(), {
            let element_state = self.clone();

            move |known_dimensions, available_space, window, cx| {
                let wrap_width = if text_style.white_space == WhiteSpace::Normal {
                    known_dimensions.width.or(match available_space.width {
                        crate::AvailableSpace::Definite(x) => Some(x),
                        _ => None,
                    })
                } else {
                    None
                };

                let (truncate_width, truncation_suffix) =
                    if let Some(text_overflow) = text_style.text_overflow.clone() {
                        let width = known_dimensions.width.or(match available_space.width {
                            crate::AvailableSpace::Definite(x) => match text_style.line_clamp {
                                Some(max_lines) => Some(x * max_lines),
                                None => Some(x),
                            },
                            _ => None,
                        });

                        match text_overflow {
                            TextOverflow::Truncate(s) => (width, s),
                        }
                    } else {
                        (None, "".into())
                    };

                if let Some(text_layout) = element_state.0.borrow().as_ref()
                    && text_layout.size.is_some()
                    && (wrap_width.is_none() || wrap_width == text_layout.wrap_width)
                {
                    return text_layout.size.unwrap();
                }

                let mut line_wrapper = cx.text_system().line_wrapper(text_style.font(), font_size);
                let text = if let Some(truncate_width) = truncate_width {
                    line_wrapper.truncate_line(
                        text.clone(),
                        truncate_width,
                        &truncation_suffix,
                        &mut runs,
                    )
                } else {
                    text.clone()
                };
                let len = text.len();

                let Some(lines) = window
                    .text_system()
                    .shape_text(
                        text,
                        font_size,
                        &runs,
                        wrap_width,            // Wrap if we know the width.
                        text_style.line_clamp, // Limit the number of lines if line_clamp is set.
                    )
                    .log_err()
                else {
                    element_state.0.borrow_mut().replace(TextLayoutInner {
                        lines: Default::default(),
                        len: 0,
                        line_height,
                        wrap_width,
                        size: Some(Size::default()),
                        bounds: None,
                    });
                    return Size::default();
                };

                let mut size: Size<Pixels> = Size::default();
                for line in &lines {
                    let line_size = line.size(line_height);
                    size.height += line_size.height;
                    size.width = size.width.max(line_size.width).ceil();
                }

                element_state.0.borrow_mut().replace(TextLayoutInner {
                    lines,
                    len,
                    line_height,
                    wrap_width,
                    size: Some(size),
                    bounds: None,
                });

                size
            }
        })
    }

    fn prepaint(&self, bounds: Bounds<Pixels>, text: &str) {
        let mut element_state = self.0.borrow_mut();
        let element_state = element_state
            .as_mut()
            .with_context(|| format!("measurement has not been performed on {text}"))
            .unwrap();
        element_state.bounds = Some(bounds);
    }

    fn paint(&self, text: &str, window: &mut Window, cx: &mut App) {
        let element_state = self.0.borrow();
        let element_state = element_state
            .as_ref()
            .with_context(|| format!("measurement has not been performed on {text}"))
            .unwrap();
        let bounds = element_state
            .bounds
            .with_context(|| format!("prepaint has not been performed on {text}"))
            .unwrap();

        let line_height = element_state.line_height;
        let mut line_origin = bounds.origin;
        let text_style = window.text_style();
        for line in &element_state.lines {
            line.paint_background(
                line_origin,
                line_height,
                text_style.text_align,
                Some(bounds),
                window,
                cx,
            )
            .log_err();
            line.paint(
                line_origin,
                line_height,
                text_style.text_align,
                Some(bounds),
                window,
                cx,
            )
            .log_err();
            line_origin.y += line.size(line_height).height;
        }
    }

    /// Get the byte index into the input of the pixel position.
    pub fn index_for_position(&self, mut position: Point<Pixels>) -> Result<usize, usize> {
        let element_state = self.0.borrow();
        let element_state = element_state
            .as_ref()
            .expect("measurement has not been performed");
        let bounds = element_state
            .bounds
            .expect("prepaint has not been performed");

        if position.y < bounds.top() {
            return Err(0);
        }

        let line_height = element_state.line_height;
        let mut line_origin = bounds.origin;
        let mut line_start_ix = 0;
        for line in &element_state.lines {
            let line_bottom = line_origin.y + line.size(line_height).height;
            if position.y > line_bottom {
                line_origin.y = line_bottom;
                line_start_ix += line.len() + 1;
            } else {
                let position_within_line = position - line_origin;
                match line.index_for_position(position_within_line, line_height) {
                    Ok(index_within_line) => return Ok(line_start_ix + index_within_line),
                    Err(index_within_line) => return Err(line_start_ix + index_within_line),
                }
            }
        }

        Err(line_start_ix.saturating_sub(1))
    }

    /// Get the pixel position for the given byte index.
    pub fn position_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        let element_state = self.0.borrow();
        let element_state = element_state
            .as_ref()
            .expect("measurement has not been performed");
        let bounds = element_state
            .bounds
            .expect("prepaint has not been performed");
        let line_height = element_state.line_height;

        let mut line_origin = bounds.origin;
        let mut line_start_ix = 0;

        for line in &element_state.lines {
            let line_end_ix = line_start_ix + line.len();
            if index < line_start_ix {
                break;
            } else if index > line_end_ix {
                line_origin.y += line.size(line_height).height;
                line_start_ix = line_end_ix + 1;
                continue;
            } else {
                let ix_within_line = index - line_start_ix;
                return Some(line_origin + line.position_for_index(ix_within_line, line_height)?);
            }
        }

        None
    }

    /// Returns the visual bounds that contain a UTF-8 byte range.
    ///
    /// A wrapped range is represented by the smallest rectangle containing its
    /// first and last line. This is suitable for accessibility hit testing,
    /// where a single semantic node must describe the entire text span.
    pub fn bounds_for_range(&self, range: Range<usize>) -> Option<Bounds<Pixels>> {
        if range.start >= range.end || range.end > self.len() {
            return None;
        }
        let start = self.position_for_index(range.start)?;
        let end = self.position_for_index(range.end)?;
        let line_height = self.line_height();

        if start.y == end.y {
            return Some(Bounds {
                origin: start,
                size: Size {
                    width: (end.x - start.x).max(px(1.0)),
                    height: line_height,
                },
            });
        }

        let layout_bounds = self.bounds();
        Some(Bounds::from_corners(
            Point {
                x: layout_bounds.left().min(start.x),
                y: start.y,
            },
            Point {
                x: layout_bounds.right().max(end.x),
                y: end.y + line_height,
            },
        ))
    }

    /// Retrieve the layout for the line containing the given byte index.
    pub fn line_layout_for_index(&self, index: usize) -> Option<Arc<WrappedLineLayout>> {
        let element_state = self.0.borrow();
        let element_state = element_state
            .as_ref()
            .expect("measurement has not been performed");
        let bounds = element_state
            .bounds
            .expect("prepaint has not been performed");
        let line_height = element_state.line_height;

        let mut line_origin = bounds.origin;
        let mut line_start_ix = 0;

        for line in &element_state.lines {
            let line_end_ix = line_start_ix + line.len();
            if index < line_start_ix {
                break;
            } else if index > line_end_ix {
                line_origin.y += line.size(line_height).height;
                line_start_ix = line_end_ix + 1;
                continue;
            } else {
                return Some(line.layout.clone());
            }
        }

        None
    }

    /// The bounds of this layout.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().as_ref().unwrap().bounds.unwrap()
    }

    /// The line height for this layout.
    pub fn line_height(&self) -> Pixels {
        self.0.borrow().as_ref().unwrap().line_height
    }

    /// The UTF-8 length of the underlying text.
    pub fn len(&self) -> usize {
        self.0.borrow().as_ref().unwrap().len
    }

    /// The text for this layout.
    pub fn text(&self) -> String {
        self.0
            .borrow()
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .map(|s| s.text.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The text for this layout (with soft-wraps as newlines)
    pub fn wrapped_text(&self) -> String {
        let mut lines = Vec::new();
        for wrapped in self.0.borrow().as_ref().unwrap().lines.iter() {
            let mut seen = 0;
            for boundary in wrapped.layout.wrap_boundaries.iter() {
                let index = wrapped.layout.unwrapped_layout.runs[boundary.run_ix].glyphs
                    [boundary.glyph_ix]
                    .index;

                lines.push(wrapped.text[seen..index].to_string());
                seen = index;
            }
            lines.push(wrapped.text[seen..].to_string());
        }

        lines.join("\n")
    }
}

/// A text element that can be interacted with.
pub struct InteractiveText {
    element_id: ElementId,
    text: StyledText,
    click_listener:
        Option<Rc<dyn Fn(&[Range<usize>], InteractiveTextClickEvent, &mut Window, &mut App)>>,
    hover_listener: Option<Box<dyn Fn(Option<usize>, MouseMoveEvent, &mut Window, &mut App)>>,
    tooltip_builder: Option<Rc<dyn Fn(usize, &mut Window, &mut App) -> Option<AnyView>>>,
    tooltip_id: Option<TooltipId>,
    clickable_ranges: Vec<Range<usize>>,
}

struct InteractiveTextClickEvent {
    mouse_down_index: usize,
    mouse_up_index: usize,
}

#[doc(hidden)]
#[derive(Default)]
pub struct InteractiveTextState {
    mouse_down_index: Rc<Cell<Option<usize>>>,
    hovered_index: Rc<Cell<Option<usize>>>,
    active_tooltip: Rc<RefCell<Option<ActiveTooltip>>>,
    accessibility_ids: Vec<crate::AccessibilityId>,
}

/// InteractiveTest is a wrapper around StyledText that adds mouse interactions.
impl InteractiveText {
    /// Creates a new InteractiveText from the given text.
    pub fn new(id: impl Into<ElementId>, text: StyledText) -> Self {
        Self {
            element_id: id.into(),
            text,
            click_listener: None,
            hover_listener: None,
            tooltip_builder: None,
            tooltip_id: None,
            clickable_ranges: Vec::new(),
        }
    }

    /// on_click is called when the user clicks on one of the given ranges, passing the index of
    /// the clicked range.
    pub fn on_click(
        mut self,
        ranges: Vec<Range<usize>>,
        listener: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.click_listener = Some(Rc::new(move |ranges, event, window, cx| {
            for (range_ix, range) in ranges.iter().enumerate() {
                if range.contains(&event.mouse_down_index) && range.contains(&event.mouse_up_index)
                {
                    listener(range_ix, window, cx);
                }
            }
        }));
        self.clickable_ranges = ranges;
        self
    }

    /// on_hover is called when the mouse moves over a character within the text, passing the
    /// index of the hovered character, or None if the mouse leaves the text.
    pub fn on_hover(
        mut self,
        listener: impl Fn(Option<usize>, MouseMoveEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.hover_listener = Some(Box::new(listener));
        self
    }

    /// tooltip lets you specify a tooltip for a given character index in the string.
    pub fn tooltip(
        mut self,
        builder: impl Fn(usize, &mut Window, &mut App) -> Option<AnyView> + 'static,
    ) -> Self {
        self.tooltip_builder = Some(Rc::new(builder));
        self
    }
}

#[derive(Debug, PartialEq, Eq)]
struct InteractiveTextAccessibilitySegment {
    label: String,
    range: Range<usize>,
    clickable_range_index: Option<usize>,
}

fn interactive_text_accessibility_segments(
    text: &str,
    ranges: &[Range<usize>],
) -> Vec<InteractiveTextAccessibilitySegment> {
    let mut ranges: Vec<_> = ranges
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, range)| {
            range.start < range.end
                && range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end)
        })
        .collect();
    ranges.sort_by_key(|(_, range)| (range.start, range.end));

    let mut segments = Vec::new();
    let mut cursor = 0;
    for (range_index, range) in ranges {
        if range.start < cursor {
            continue;
        }
        if cursor < range.start {
            segments.push(InteractiveTextAccessibilitySegment {
                label: text[cursor..range.start].to_string(),
                range: cursor..range.start,
                clickable_range_index: None,
            });
        }
        segments.push(InteractiveTextAccessibilitySegment {
            label: text[range.clone()].to_string(),
            range: range.clone(),
            clickable_range_index: Some(range_index),
        });
        cursor = range.end;
    }
    if cursor < text.len() {
        segments.push(InteractiveTextAccessibilitySegment {
            label: text[cursor..].to_string(),
            range: cursor..text.len(),
            clickable_range_index: None,
        });
    }
    if segments.is_empty() && !text.is_empty() {
        segments.push(InteractiveTextAccessibilitySegment {
            label: text.to_string(),
            range: 0..text.len(),
            clickable_range_index: None,
        });
    }
    segments
}

impl Element for InteractiveText {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.element_id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.text.request_layout(None, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        window.with_optional_element_state::<InteractiveTextState, _>(
            global_id,
            |interactive_state, window| {
                let mut interactive_state = interactive_state
                    .map(|interactive_state| interactive_state.unwrap_or_default());

                if let Some(interactive_state) = interactive_state.as_mut() {
                    if self.tooltip_builder.is_some() {
                        self.tooltip_id =
                            set_tooltip_on_window(&interactive_state.active_tooltip, window);
                    } else {
                        // If there is no longer a tooltip builder, remove the active tooltip.
                        interactive_state.active_tooltip.take();
                    }
                }

                self.text
                    .prepaint(None, inspector_id, bounds, state, window, cx);
                let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                (hitbox, interactive_state)
            },
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let current_view = window.current_view();
        let text_layout = self.text.layout().clone();
        window.with_element_state::<InteractiveTextState, _>(
            global_id.unwrap(),
            |interactive_state, window| {
                let mut interactive_state = interactive_state.unwrap_or_default();
                if let Some(click_listener) = self.click_listener.clone() {
                    let mouse_position = window.mouse_position();
                    if let Ok(ix) = text_layout.index_for_position(mouse_position)
                        && self
                            .clickable_ranges
                            .iter()
                            .any(|range| range.contains(&ix))
                    {
                        window.set_cursor_style(crate::CursorStyle::PointingHand, hitbox)
                    }

                    let text_layout = text_layout.clone();
                    let mouse_down = interactive_state.mouse_down_index.clone();
                    let mouse_down_hitbox = hitbox.clone();
                    let mouse_down_layout = text_layout.clone();
                    let pending_mouse_down = mouse_down.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                        if phase == DispatchPhase::Bubble
                            && mouse_down_hitbox.is_hovered(window)
                            && let Ok(mouse_down_index) =
                                mouse_down_layout.index_for_position(event.position)
                        {
                            pending_mouse_down.set(Some(mouse_down_index));
                            window.refresh();
                        }
                    });

                    let mouse_up_hitbox = hitbox.clone();
                    let clickable_ranges = self.clickable_ranges.clone();
                    window.on_mouse_event(
                        move |event: &MouseUpEvent, phase, window: &mut Window, cx| {
                            if phase != DispatchPhase::Bubble {
                                return;
                            }
                            let Some(mouse_down_index) = mouse_down.take() else {
                                return;
                            };
                            if mouse_up_hitbox.is_hovered(window)
                                && let Ok(mouse_up_index) =
                                    text_layout.index_for_position(event.position)
                            {
                                click_listener(
                                    &clickable_ranges,
                                    InteractiveTextClickEvent {
                                        mouse_down_index,
                                        mouse_up_index,
                                    },
                                    window,
                                    cx,
                                );
                            }
                            window.refresh();
                        },
                    );
                }

                window.on_mouse_event({
                    let mut hover_listener = self.hover_listener.take();
                    let hitbox = hitbox.clone();
                    let text_layout = text_layout.clone();
                    let hovered_index = interactive_state.hovered_index.clone();
                    move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                            let current = hovered_index.get();
                            let updated = text_layout.index_for_position(event.position).ok();
                            if current != updated {
                                hovered_index.set(updated);
                                if let Some(hover_listener) = hover_listener.as_ref() {
                                    hover_listener(updated, event.clone(), window, cx);
                                }
                                cx.notify(current_view);
                            }
                        }
                    }
                });

                if let Some(tooltip_builder) = self.tooltip_builder.clone() {
                    let active_tooltip = interactive_state.active_tooltip.clone();
                    let build_tooltip = Rc::new({
                        let tooltip_is_hoverable = false;
                        let text_layout = text_layout.clone();
                        move |window: &mut Window, cx: &mut App| {
                            text_layout
                                .index_for_position(window.mouse_position())
                                .ok()
                                .and_then(|position| tooltip_builder(position, window, cx))
                                .map(|view| (view, tooltip_is_hoverable))
                        }
                    });

                    // Use bounds instead of testing hitbox since this is called during prepaint.
                    let check_is_hovered_during_prepaint = Rc::new({
                        let source_bounds = hitbox.bounds;
                        let text_layout = text_layout.clone();
                        let pending_mouse_down = interactive_state.mouse_down_index.clone();
                        move |window: &Window, _cx: &App| {
                            text_layout
                                .index_for_position(window.mouse_position())
                                .is_ok()
                                && source_bounds.contains(&window.mouse_position())
                                && pending_mouse_down.get().is_none()
                        }
                    });

                    let check_is_hovered = Rc::new({
                        let hitbox = hitbox.clone();
                        let text_layout = text_layout.clone();
                        let pending_mouse_down = interactive_state.mouse_down_index.clone();
                        move |window: &Window, _cx: &App| {
                            text_layout
                                .index_for_position(window.mouse_position())
                                .is_ok()
                                && hitbox.is_hovered(window)
                                && pending_mouse_down.get().is_none()
                        }
                    });

                    register_tooltip_mouse_handlers(
                        &active_tooltip,
                        self.tooltip_id,
                        build_tooltip,
                        check_is_hovered,
                        check_is_hovered_during_prepaint,
                        Duration::from_millis(500),
                        Duration::ZERO,
                        None,
                        window,
                    );
                }

                self.text.layout.paint(&self.text.text, window, cx);

                let segments = interactive_text_accessibility_segments(
                    self.text.text.as_ref(),
                    &self.clickable_ranges,
                );
                interactive_state
                    .accessibility_ids
                    .resize_with(segments.len(), crate::AccessibilityId::new);
                interactive_state.accessibility_ids.truncate(segments.len());

                for (segment_index, segment) in segments.into_iter().enumerate() {
                    let node_id = interactive_state.accessibility_ids[segment_index];
                    let accessibility_bounds = text_layout
                        .bounds_for_range(segment.range.clone())
                        .unwrap_or(bounds);
                    let mut node =
                        crate::AccessibilityNode::new(if segment.clickable_range_index.is_some() {
                            crate::AccessibilityRole::Link
                        } else {
                            crate::AccessibilityRole::StaticText
                        })
                        .with_label(segment.label);
                    node.id = node_id;
                    if segment.clickable_range_index.is_some() {
                        node.actions = vec![crate::AccessibilityAction::Click];
                    }
                    window.register_accessibility_node_at(node, accessibility_bounds);

                    if let (Some(range_index), Some(click_listener)) =
                        (segment.clickable_range_index, self.click_listener.clone())
                    {
                        let clickable_ranges = self.clickable_ranges.clone();
                        let Some(range) = clickable_ranges.get(range_index).cloned() else {
                            continue;
                        };
                        let window_handle = window.window_handle();
                        let mut async_cx = cx.to_async();
                        let executor = cx.foreground_executor().clone();
                        window.on_accessibility_action(
                            node_id,
                            crate::AccessibilityAction::Click,
                            move |_| {
                                let click_listener = click_listener.clone();
                                let clickable_ranges = clickable_ranges.clone();
                                let range = range.clone();
                                let mut async_cx = async_cx.clone();
                                executor
                                    .spawn(async move {
                                        _ = window_handle.update(&mut async_cx, |_, window, cx| {
                                            click_listener(
                                                &clickable_ranges,
                                                InteractiveTextClickEvent {
                                                    mouse_down_index: range.start,
                                                    mouse_up_index: range.start,
                                                },
                                                window,
                                                cx,
                                            );
                                            window.refresh();
                                        });
                                    })
                                    .detach();
                            },
                        );
                    }
                }

                ((), interactive_state)
            },
        );
    }
}

impl IntoElement for InteractiveText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod interactive_text_accessibility_tests {
    use super::*;
    use crate::{Context, ParentElement as _, Render, Styled as _, TestAppContext, div};

    #[test]
    fn segments_preserve_text_and_link_boundaries() {
        assert_eq!(
            interactive_text_accessibility_segments("Before Docs after", &[7..11]),
            vec![
                InteractiveTextAccessibilitySegment {
                    label: "Before ".into(),
                    range: 0..7,
                    clickable_range_index: None,
                },
                InteractiveTextAccessibilitySegment {
                    label: "Docs".into(),
                    range: 7..11,
                    clickable_range_index: Some(0),
                },
                InteractiveTextAccessibilitySegment {
                    label: " after".into(),
                    range: 11..17,
                    clickable_range_index: None,
                },
            ]
        );
        assert_eq!(
            interactive_text_accessibility_segments("éDocs", &[1..5]),
            vec![InteractiveTextAccessibilitySegment {
                label: "éDocs".into(),
                range: 0..6,
                clickable_range_index: None,
            }]
        );
    }

    struct AccessibleInteractiveText {
        clicks: Rc<Cell<usize>>,
    }

    impl Render for AccessibleInteractiveText {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let clicks = self.clicks.clone();
            div().size_full().child(
                InteractiveText::new(
                    "accessible-interactive-text",
                    StyledText::new("Before Docs after"),
                )
                .on_click(vec![7..11], move |_, _, _| clicks.set(clicks.get() + 1)),
            )
        }
    }

    #[crate::test]
    fn links_are_distinct_and_route_accessibility_clicks(cx: &mut TestAppContext) {
        let clicks = Rc::new(Cell::new(0));
        let (_view, mut window) = cx.add_window_view({
            let clicks = clicks.clone();
            move |_, _| AccessibleInteractiveText { clicks }
        });

        let (link_id, link_bounds) = window.update(|window, cx| {
            window.draw(cx).clear();
            let link = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| {
                    node.role == crate::AccessibilityRole::Link
                        && node.label.as_deref() == Some("Docs")
                })
                .expect("interactive range should be exposed as a link");
            assert_eq!(
                window
                    .accessibility_tree()
                    .nodes
                    .values()
                    .filter(|node| node.label.as_deref() == Some("Before Docs after"))
                    .count(),
                0,
                "the full label must not be registered a second time"
            );
            assert!(link.actions.contains(&crate::AccessibilityAction::Click));
            (
                link.id,
                link.bounds
                    .expect("the link should have glyph-range bounds"),
            )
        });

        window.simulate_click(
            Point {
                x: px((link_bounds.x + link_bounds.width / 2.0) as f32),
                y: px((link_bounds.y + link_bounds.height / 2.0) as f32),
            },
            crate::Modifiers::default(),
        );
        assert_eq!(
            clicks.get(),
            1,
            "a same-frame pointer click should activate"
        );

        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(crate::AccessibilityActionRequest::new(
                link_id,
                crate::AccessibilityAction::Click,
            ));
        });
        window.run_until_parked();
        assert_eq!(clicks.get(), 2);
    }
}

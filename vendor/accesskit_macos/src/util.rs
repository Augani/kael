// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use accesskit::{Color, Point, Rect, TextDirection};
use accesskit_consumer::{Node, TextPosition, TextRange};
use objc2::encode::{Encoding, RefEncode};
use objc2::{msg_send, rc::Id, runtime::AnyObject};
use objc2_app_kit::*;
use objc2_foundation::{NSPoint, NSRange, NSRect, NSSize};

pub(crate) fn from_ns_range<'a>(node: &'a Node<'a>, ns_range: NSRange) -> Option<TextRange<'a>> {
    let end_index = ns_range.location.checked_add(ns_range.length)?;
    let pos = node.text_position_from_global_utf16_index(ns_range.location)?;
    if pos.to_global_utf16_index() != ns_range.location {
        return None;
    }
    let mut range = pos.to_degenerate_range();
    if ns_range.length > 0 {
        let end = node.text_position_from_global_utf16_index(end_index)?;
        if end.to_global_utf16_index() != end_index {
            return None;
        }
        range.set_end(end);
    }
    Some(range)
}

// Native reads use UTF-16 indices, which can address individual scalars inside
// a shaped atomic character. Setter conversion above deliberately requires an
// exact AccessKit position; reads must neither reject nor round these scalars.
pub(crate) fn traverse_ns_range(
    node: &Node,
    ns_range: NSRange,
    mut visit: impl FnMut(&Node, &str),
) -> Option<()> {
    fn byte_index(text: &str, target: usize) -> Option<usize> {
        let mut offset = 0;
        for (byte, character) in text.char_indices() {
            if offset == target {
                return Some(byte);
            }
            offset += character.len_utf16();
            if offset > target {
                return None;
            }
        }
        (offset == target).then_some(text.len())
    }

    let end = ns_range.location.checked_add(ns_range.length)?;
    if end > node.document_range().end().to_global_utf16_index() {
        return None;
    }
    let mut offset = 0;
    let invalid = node.document_range().traverse_text(|run, text| {
        let next = offset + text.encode_utf16().count();
        if offset <= end && next >= ns_range.location {
            let start = ns_range.location.saturating_sub(offset);
            let local_end = end.saturating_sub(offset).min(next - offset);
            let (Some(start), Some(local_end)) =
                (byte_index(text, start), byte_index(text, local_end))
            else {
                return Some(());
            };
            if start < local_end {
                visit(run, &text[start..local_end]);
            }
        }
        offset = next;
        None
    });
    invalid.is_none().then_some(())
}

pub(crate) fn to_ns_range(range: &TextRange) -> NSRange {
    let start = range.start().to_global_utf16_index();
    let end = range.end().to_global_utf16_index();
    NSRange::from(start..end)
}

pub(crate) fn to_ns_range_for_character(pos: &TextPosition) -> NSRange {
    let mut range = pos.to_degenerate_range();
    if !pos.is_document_end() {
        range.set_end(pos.forward_to_character_end());
    }
    to_ns_range(&range)
}

pub(crate) fn from_ns_point(view: &NSView, node: &Node, point: NSPoint) -> Option<Point> {
    let window = view.window()?;
    let point = window.convertPointFromScreen(point);
    let point = view.convertPoint_fromView(point, None);
    // AccessKit coordinates are in physical (DPI-dependent) pixels, but
    // macOS provides logical (DPI-independent) coordinates here.
    let factor = window.backingScaleFactor();
    let point = Point::new(
        point.x * factor,
        if view.isFlipped() {
            point.y * factor
        } else {
            let view_bounds = view.bounds();
            (view_bounds.size.height - point.y) * factor
        },
    );
    Some(node.transform().inverse() * point)
}

pub(crate) fn to_ns_rect(view: &NSView, rect: Rect) -> NSRect {
    let Some(window) = view.window() else {
        return NSRect::ZERO;
    };
    // AccessKit coordinates are in physical (DPI-dependent)
    // pixels, but macOS expects logical (DPI-independent)
    // coordinates here.
    let factor = window.backingScaleFactor();
    let rect = NSRect {
        origin: NSPoint {
            x: rect.x0 / factor,
            y: if view.isFlipped() {
                rect.y0 / factor
            } else {
                let view_bounds = view.bounds();
                view_bounds.size.height - rect.y1 / factor
            },
        },
        size: NSSize {
            width: rect.width() / factor,
            height: rect.height() / factor,
        },
    };
    let rect = view.convertRect_toView(rect, None);
    window.convertRectToScreen(rect)
}

// Native requests may scan retained runs, but do not allocate or rebuild text
// metadata on caret paints. Only mounted, shaped glyphs intersecting the owner
// viewport contribute; offscreen runs have no invented physical geometry.
pub(crate) fn visible_character_range(node: &Node) -> NSRange {
    let Some(clip) = node.bounding_box() else {
        return NSRange::new(0, 0);
    };
    let mut offset = 0;
    let mut first = None;
    let mut last = 0;
    node.document_range().traverse_text::<_, ()>(|run, text| {
        let data = run.data();
        if let (Some(bounds), Some(positions), Some(widths), Some(direction)) = (
            run.raw_bounds(),
            data.character_positions(),
            data.character_widths(),
            run.text_direction(),
        ) {
            let mut byte_offset = 0;
            let mut local_utf16 = 0;
            for (index, bytes) in data.character_lengths().iter().enumerate() {
                let end = byte_offset + usize::from(*bytes);
                let Some(character) = text.get(byte_offset..end) else {
                    break;
                };
                let units = character.encode_utf16().count();
                if let (Some(position), Some(width)) = (positions.get(index), widths.get(index)) {
                    let start = f64::from(*position);
                    let end = start + f64::from(*width);
                    let mut rect = bounds;
                    match direction {
                        TextDirection::LeftToRight => {
                            rect.x0 = bounds.x0 + start;
                            rect.x1 = bounds.x0 + end;
                        }
                        TextDirection::RightToLeft => {
                            rect.x0 = bounds.x1 - end;
                            rect.x1 = bounds.x1 - start;
                        }
                        TextDirection::TopToBottom => {
                            rect.y0 = bounds.y0 + start;
                            rect.y1 = bounds.y0 + end;
                        }
                        TextDirection::BottomToTop => {
                            rect.y0 = bounds.y1 - end;
                            rect.y1 = bounds.y1 - start;
                        }
                    }
                    let rect = run.transform().transform_rect_bbox(rect);
                    let visible = !rect.intersect(clip).is_empty()
                        // A newline or combining unit may have zero advance;
                        // its real caret position is visible without fabricating
                        // an area for its native bounding rectangle.
                        || (rect.width() == 0.0 && rect.height() > 0.0 && rect.x0 >= clip.x0 && rect.x0 <= clip.x1 && rect.y0 < clip.y1 && rect.y1 > clip.y0)
                        || (rect.height() == 0.0 && rect.width() > 0.0 && rect.y0 >= clip.y0 && rect.y0 <= clip.y1 && rect.x0 < clip.x1 && rect.x1 > clip.x0);
                    if visible {
                        first.get_or_insert(offset + local_utf16);
                        last = offset + local_utf16 + units;
                    }
                }
                local_utf16 += units;
                byte_offset = end;
            }
        }
        offset += text.encode_utf16().count();
        None
    });
    first.map_or(NSRange::new(0, 0), |first| NSRange::from(first..last))
}

fn color_channel_to_f64(channel: u8) -> f64 {
    (channel as f64) / 255.0
}

// TODO: can be removed after updating objc2 to 0.6 which has proper `CGColor` support
#[repr(C)]
struct CGColor {
    _private: [u8; 0],
}

unsafe impl RefEncode for CGColor {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGColor", &[]));
}

pub(crate) fn to_color_attribute(color: Color) -> Id<AnyObject> {
    let ns_color = unsafe {
        NSColor::colorWithSRGBRed_green_blue_alpha(
            color_channel_to_f64(color.red),
            color_channel_to_f64(color.green),
            color_channel_to_f64(color.blue),
            color_channel_to_f64(color.alpha),
        )
    };
    let cg_color: *const CGColor = unsafe { msg_send![&ns_color, CGColor] };
    unsafe { Id::retain(cg_color as *mut AnyObject).unwrap() }
}

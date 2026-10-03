// Copyright 2026 The Kael contributors. All rights reserved.
// Licensed under Apache-2.0 OR MIT. See PATCHES.md for upstream provenance.
//! Portable query model. No full-document copy is needed for search.
use accesskit::{Rect, TextDirection};
use accesskit_consumer::{Node, TextRange};
use std::collections::VecDeque;

/// Streaming KMP, with UTF-16 offsets relative to the supplied range. The
/// comparison must be an equivalence relation preserving UTF-16 scalar length.
/// Matches respect the consumer's atomic text units (including CRLF). `accept`
/// may apply an additional caller filter without indexing the document.
pub(crate) fn find_text(
    range: &TextRange<'_>,
    needle: &str,
    backward: bool,
    mut equal: impl FnMut(char, char) -> bool,
    mut accept: impl FnMut(usize, usize) -> bool,
) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let length = needle.encode_utf16().count();
    if length > range.end().to_global_utf16_index() - range.start().to_global_utf16_index() {
        return None;
    }
    let pattern: Vec<char> = needle.chars().collect();
    let mut prefix = vec![0; pattern.len()];
    let mut matched = 0;
    for i in 1..pattern.len() {
        while matched > 0 && !equal(pattern[i], pattern[matched]) {
            matched = prefix[matched - 1];
        }
        if equal(pattern[i], pattern[matched]) {
            matched += 1;
        }
        prefix[i] = matched;
    }
    let mut offset = 0;
    matched = 0;
    let mut result = None;
    let mut boundaries = VecDeque::with_capacity(pattern.len());
    range.traverse_text(|run, text| {
        // Consumer traversal returns a complete-unit substring of this value.
        // Find its starting unit without copying the document or constructing
        // an index for every matching candidate (which would be quadratic).
        let full = run.data().value().unwrap();
        let slice_start = (text.as_ptr() as usize).checked_sub(full.as_ptr() as usize)?;
        let slice_end = slice_start + text.len();
        let mut byte = 0;
        for &unit_length in run.data().character_lengths() {
            let end = byte + usize::from(unit_length);
            if byte >= slice_start && end <= slice_end {
                let unit = &full[byte..end];
                for (index, ch) in unit.char_indices() {
                    if boundaries.len() == pattern.len() {
                        boundaries.pop_front();
                    }
                    boundaries.push_back(index == 0);
                    offset += ch.len_utf16();
                    while matched > 0 && !equal(ch, pattern[matched]) {
                        matched = prefix[matched - 1];
                    }
                    if equal(ch, pattern[matched]) {
                        matched += 1;
                    }
                    if matched == pattern.len() {
                        let start = offset - length;
                        let atomic = boundaries.front() == Some(&true)
                            && index + ch.len_utf8() == unit.len();
                        if atomic && accept(start, offset) {
                            result = Some((start, offset));
                            if !backward {
                                return Some(());
                            }
                        }
                        matched = prefix[matched - 1];
                    }
                }
            }
            byte = end;
            if byte >= slice_end {
                break;
            }
        }
        None
    });
    result
}

pub(crate) fn viewport(node: Node<'_>) -> Option<Rect> {
    if node.is_hidden() {
        return None;
    }
    let mut result = node.bounding_box()?;
    let mut parent = node.parent();
    while let Some(ancestor) = parent {
        if ancestor.clips_children() {
            result = result.intersect(ancestor.bounding_box()?);
        }
        parent = ancestor.parent();
    }
    (result.width() > 0.0 && result.height() > 0.0).then_some(result)
}

/// Visible character spans, including partial glyphs and zero-width marks.
/// Missing geometry is unknown visibility, never evidence that the full text
/// document is visible. Offscreen document runs still remain searchable.
pub(crate) fn visible_ranges<'a>(node: &'a Node<'a>) -> Vec<TextRange<'a>> {
    let document = node.document_range();
    let Some(clip) = viewport(*node) else {
        return vec![node.document_start().to_degenerate_range()];
    };
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut global = 0;
    document.traverse_text(|run, text| {
        let start = global;
        global += text.encode_utf16().count();
        if run.is_hidden() {
            return None::<()>;
        }
        let (Some(bounds), Some(positions), Some(widths), Some(direction)) = (
            run.data().bounds(),
            run.data().character_positions(),
            run.data().character_widths(),
            run.text_direction(),
        ) else {
            return None;
        };
        let lengths = run.data().character_lengths();
        if positions.len() != lengths.len() || widths.len() != lengths.len() {
            return None;
        }
        let mut bytes = 0;
        let mut utf16 = start;
        for (i, len) in lengths.iter().enumerate() {
            let end_bytes = bytes + usize::from(*len);
            let unit = text.get(bytes..end_bytes)?;
            let end_utf16 = utf16 + unit.encode_utf16().count();
            let pos = f64::from(positions[i]);
            let width = f64::from(widths[i]);
            if !pos.is_finite() || !width.is_finite() || width < 0.0 {
                return None;
            }
            let mut glyph = bounds;
            match direction {
                TextDirection::LeftToRight => {
                    glyph.x0 = bounds.x0 + pos;
                    glyph.x1 = glyph.x0 + width;
                }
                TextDirection::RightToLeft => {
                    glyph.x1 = bounds.x1 - pos;
                    glyph.x0 = glyph.x1 - width;
                }
                TextDirection::TopToBottom => {
                    glyph.y0 = bounds.y0 + pos;
                    glyph.y1 = glyph.y0 + width;
                }
                TextDirection::BottomToTop => {
                    glyph.y1 = bounds.y1 - pos;
                    glyph.y0 = glyph.y1 - width;
                }
            }
            let glyph = run.transform().transform_rect_bbox(glyph);
            // A combining mark/newline can have zero advance. Include it only
            // when its baseline position and perpendicular extent are visible.
            let visible = glyph.x1 > clip.x0
                && glyph.x0 < clip.x1
                && glyph.y1 > clip.y0
                && glyph.y0 < clip.y1;
            if visible {
                if let Some(last) = spans.last_mut().filter(|last| last.1 == utf16) {
                    last.1 = end_utf16;
                } else {
                    spans.push((utf16, end_utf16));
                }
            }
            bytes = end_bytes;
            utf16 = end_utf16;
        }
        None
    });
    let result: Vec<_> = spans
        .into_iter()
        .filter_map(|(start, end)| {
            let mut range = document;
            range.set_start(node.text_position_from_global_utf16_index(start)?);
            range.set_end(node.text_position_from_global_utf16_index(end)?);
            Some(range)
        })
        .collect();
    if result.is_empty() {
        vec![node.document_start().to_degenerate_range()]
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::{Node as Data, NodeId, Role, Tree as TreeData, TreeUpdate};
    use accesskit_consumer::Tree;
    fn tree(runs: &[(&str, Option<Rect>, Option<TextDirection>)], clip: Rect) -> Tree {
        let root_id = NodeId(1);
        let mut root = Data::new(Role::MultilineTextInput);
        root.set_bounds(clip);
        root.set_clips_children();
        root.set_children(
            (0..runs.len())
                .map(|i| NodeId(i as u64 + 2))
                .collect::<Vec<_>>(),
        );
        let mut nodes = vec![(root_id, root)];
        for (i, (value, bounds, direction)) in runs.iter().enumerate() {
            let mut run = Data::new(Role::TextRun);
            run.set_value(*value);
            let mut chars = value.chars().peekable();
            let mut lengths = Vec::new();
            while let Some(ch) = chars.next() {
                let mut length = ch.len_utf8() as u8;
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                    length += 1;
                }
                lengths.push(length);
            }
            let unit_count = lengths.len();
            run.set_character_lengths(lengths);
            if let Some(bounds) = bounds {
                run.set_bounds(*bounds);
            }
            if let Some(direction) = direction {
                run.set_text_direction(*direction);
                run.set_character_positions(
                    (0..unit_count).map(|i| i as f32 * 10.0).collect::<Vec<_>>(),
                );
                run.set_character_widths(vec![10.0; unit_count]);
            }
            nodes.push((NodeId(i as u64 + 2), run));
        }
        Tree::new(
            TreeUpdate {
                nodes,
                tree: Some(TreeData::new(root_id)),
                focus: root_id,
                tree_id: accesskit::TreeId::ROOT,
            },
            true,
        )
    }
    #[test]
    fn unicode_search_crosses_runs_and_retains_last_overlapping_match() {
        let tree = tree(
            &[("日本", None, None), ("語 👩🏽‍💻 café ababa", None, None)],
            Rect::new(0., 0., 100., 100.),
        );
        let node = tree.state().root();
        let range = node.document_range();
        assert_eq!(
            find_text(&range, "日本語 👩🏽‍💻", false, |a, b| a == b, |_, _| true),
            Some((0, 11))
        );
        let start = range.text().find("ababa").unwrap();
        let utf16 = range.text()[..start].encode_utf16().count();
        assert_eq!(
            find_text(&range, "aba", true, |a, b| a == b, |_, _| true),
            Some((utf16 + 2, utf16 + 5))
        );
        assert_eq!(
            find_text(&range, "absent", false, |a, b| a == b, |_, _| true),
            None
        );
        assert_eq!(
            find_text(
                &range,
                "CAFÉ",
                false,
                |a, b| a.eq_ignore_ascii_case(&b),
                |_, _| true
            )
            .map(|(s, e)| e - s),
            Some(5)
        );
    }
    #[test]
    fn horizontal_partial_visibility_preserves_disjoint_text_spans() {
        let tree = tree(
            &[
                (
                    "abcd",
                    Some(Rect::new(0., 0., 40., 10.)),
                    Some(TextDirection::LeftToRight),
                ),
                ("hidden", None, None),
                (
                    "日本",
                    Some(Rect::new(0., 10., 20., 20.)),
                    Some(TextDirection::RightToLeft),
                ),
            ],
            Rect::new(15., 0., 25., 20.),
        );
        let node = tree.state().root();
        let texts: Vec<_> = visible_ranges(&node).iter().map(TextRange::text).collect();
        assert_eq!(texts, vec!["bc", "日"]);
    }
    #[test]
    fn missing_or_offscreen_geometry_returns_one_degenerate_range() {
        let tree = tree(
            &[
                (
                    "offscreen",
                    Some(Rect::new(0., 50., 100., 60.)),
                    Some(TextDirection::LeftToRight),
                ),
                ("unknown", None, None),
            ],
            Rect::new(0., 0., 20., 20.),
        );
        let node = tree.state().root();
        let ranges = visible_ranges(&node);
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].is_degenerate());
    }
    #[test]
    fn search_does_not_split_crlf_or_surrogate_units() {
        let tree = tree(&[("a\r\nb\nc👩", None, None)], Rect::new(0., 0., 20., 20.));
        let node = tree.state().root();
        let range = node.document_range();
        assert_eq!(
            find_text(&range, "\n", false, |a, b| a == b, |_, _| true),
            Some((4, 5))
        );
        assert_eq!(
            find_text(&range, "\r\n", false, |a, b| a == b, |_, _| true),
            Some((1, 3))
        );
        assert_eq!(
            find_text(&range, "👩", false, |a, b| a == b, |_, _| true),
            Some((6, 8))
        );
    }
    #[test]
    fn backwards_repeated_matches_remain_linear() {
        let text = "a".repeat(100_000);
        let tree = tree(&[(&text, None, None)], Rect::new(0., 0., 20., 20.));
        let node = tree.state().root();
        let range = node.document_range();
        let mut comparisons = 0;
        assert_eq!(
            find_text(
                &range,
                &"a".repeat(255),
                true,
                |a, b| {
                    comparisons += 1;
                    a == b
                },
                |_, _| true
            ),
            Some((99_745, 100_000))
        );
        assert!(
            comparisons < 400_000,
            "search comparisons grew beyond the linear bound: {comparisons}"
        );
    }
    #[test]
    fn search_uses_subset_and_rejected_atomic_boundaries_do_not_hide_later_match() {
        let tree = tree(&[("aab aab", None, None)], Rect::new(0., 0., 20., 20.));
        let node = tree.state().root();
        let mut range = node.document_range();
        range.set_start(node.text_position_from_global_utf16_index(1).unwrap());
        assert_eq!(
            find_text(&range, "ab", false, |a, b| a == b, |s, _| s > 0),
            Some((4, 6))
        );
    }
}

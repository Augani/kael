// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

#![allow(non_upper_case_globals)]

use accesskit::{Action, ActionData, ActionRequest, ScrollHint, VerticalOffset};
use accesskit_consumer::{
    Node, TextPosition as Position, TextRange as Range, Tree, TreeState, WeakTextRange as WeakRange,
};
use std::sync::{Arc, RwLock, Weak};
use windows::{
    Win32::{
        System::{Com::*, Variant::*},
        UI::Accessibility::*,
    },
    core::*,
};

use crate::{
    context::Context,
    nullable_text_range::{NullableTextRange, NullableTextRange_Impl},
    text_queries,
    util::*,
};

fn upgrade_range<'a>(weak: &WeakRange, tree_state: &'a TreeState) -> Result<Range<'a>> {
    if let Some(range) = weak.upgrade(tree_state) {
        Ok(range)
    } else {
        Err(element_not_available())
    }
}

fn upgrade_range_node<'a>(weak: &WeakRange, tree_state: &'a TreeState) -> Result<Node<'a>> {
    if let Some(node) = weak.upgrade_node(tree_state) {
        Ok(node)
    } else {
        Err(element_not_available())
    }
}

fn weak_comparable_position_from_endpoint(
    range: &WeakRange,
    endpoint: TextPatternRangeEndpoint,
) -> Result<&(Vec<usize>, usize)> {
    match endpoint {
        TextPatternRangeEndpoint_Start => Ok(range.start_comparable()),
        TextPatternRangeEndpoint_End => Ok(range.end_comparable()),
        _ => Err(invalid_arg()),
    }
}

fn position_from_endpoint<'a>(
    range: &Range<'a>,
    endpoint: TextPatternRangeEndpoint,
) -> Result<Position<'a>> {
    match endpoint {
        TextPatternRangeEndpoint_Start => Ok(range.start()),
        TextPatternRangeEndpoint_End => Ok(range.end()),
        _ => Err(invalid_arg()),
    }
}

fn set_endpoint_position<'a>(
    range: &mut Range<'a>,
    endpoint: TextPatternRangeEndpoint,
    pos: Position<'a>,
) -> Result<()> {
    match endpoint {
        TextPatternRangeEndpoint_Start => {
            range.set_start(pos);
        }
        TextPatternRangeEndpoint_End => {
            range.set_end(pos);
        }
        _ => {
            return Err(invalid_arg());
        }
    }
    Ok(())
}

fn back_to_unit_start(start: Position, unit: TextUnit) -> Result<Position> {
    match unit {
        TextUnit_Character => {
            // If we get here, this position is at the start of a non-degenerate
            // range, so it's always at the start of a character.
            debug_assert!(!start.is_document_end());
            Ok(start)
        }
        TextUnit_Format => {
            if start.is_format_start() {
                Ok(start)
            } else {
                Ok(start.backward_to_format_start())
            }
        }
        TextUnit_Word => {
            if start.is_word_start() {
                Ok(start)
            } else {
                Ok(start.backward_to_word_start())
            }
        }
        TextUnit_Line => {
            if start.is_line_start() {
                Ok(start)
            } else {
                Ok(start.backward_to_line_start())
            }
        }
        TextUnit_Paragraph => {
            if start.is_paragraph_start() {
                Ok(start)
            } else {
                Ok(start.backward_to_paragraph_start())
            }
        }
        TextUnit_Page => {
            if start.is_page_start() {
                Ok(start)
            } else {
                Ok(start.backward_to_page_start())
            }
        }
        TextUnit_Document => {
            if start.is_document_start() {
                Ok(start)
            } else {
                Ok(start.document_start())
            }
        }
        _ => Err(invalid_arg()),
    }
}

fn move_forward_to_start(pos: Position, unit: TextUnit) -> Result<Position> {
    match unit {
        TextUnit_Character => Ok(pos.forward_to_character_start()),
        TextUnit_Format => Ok(pos.forward_to_format_start()),
        TextUnit_Word => Ok(pos.forward_to_word_start()),
        TextUnit_Line => Ok(pos.forward_to_line_start()),
        TextUnit_Paragraph => Ok(pos.forward_to_paragraph_start()),
        TextUnit_Page => Ok(pos.forward_to_page_start()),
        TextUnit_Document => Ok(pos.document_end()),
        _ => Err(invalid_arg()),
    }
}

fn move_forward_to_end(pos: Position, unit: TextUnit) -> Result<Position> {
    match unit {
        TextUnit_Character => Ok(pos.forward_to_character_end()),
        TextUnit_Format => Ok(pos.forward_to_format_end()),
        TextUnit_Word => Ok(pos.forward_to_word_end()),
        TextUnit_Line => Ok(pos.forward_to_line_end()),
        TextUnit_Paragraph => Ok(pos.forward_to_paragraph_end()),
        TextUnit_Page => Ok(pos.forward_to_page_end()),
        TextUnit_Document => Ok(pos.document_end()),
        _ => Err(invalid_arg()),
    }
}

fn move_backward(pos: Position, unit: TextUnit) -> Result<Position> {
    match unit {
        TextUnit_Character => Ok(pos.backward_to_character_start()),
        TextUnit_Format => Ok(pos.backward_to_format_start()),
        TextUnit_Word => Ok(pos.backward_to_word_start()),
        TextUnit_Line => Ok(pos.backward_to_line_start()),
        TextUnit_Paragraph => Ok(pos.backward_to_paragraph_start()),
        TextUnit_Page => Ok(pos.backward_to_page_start()),
        TextUnit_Document => Ok(pos.document_start()),
        _ => Err(invalid_arg()),
    }
}

fn move_position(
    mut pos: Position,
    unit: TextUnit,
    to_end: bool,
    count: i32,
) -> Result<(Position, i32)> {
    let forward = count > 0;
    let count = count.abs();
    let mut moved = 0i32;
    for _ in 0..count {
        let at_end = if forward {
            pos.is_document_end()
        } else {
            pos.is_document_start()
        };
        if at_end {
            break;
        }
        pos = if forward {
            if to_end {
                move_forward_to_end(pos, unit)
            } else {
                move_forward_to_start(pos, unit)
            }
        } else {
            move_backward(pos, unit)
        }?;
        moved += 1;
    }
    if !forward {
        moved = -moved;
    }
    Ok((pos, moved))
}

#[implement(NullableTextRange)]
pub(crate) struct PlatformRange {
    context: Weak<Context>,
    state: RwLock<WeakRange>,
}

impl PlatformRange {
    pub(crate) fn new(context: &Weak<Context>, range: Range) -> Self {
        Self {
            context: context.clone(),
            state: RwLock::new(range.downgrade()),
        }
    }

    pub(crate) fn into_provider(self) -> ITextRangeProvider {
        let nullable: NullableTextRange = self.into();
        nullable.cast().expect("identical UIA interface IID")
    }

    fn upgrade_context(&self) -> Result<Arc<Context>> {
        upgrade(&self.context)
    }

    fn with_tree_state_and_context<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&TreeState, &Context) -> Result<T>,
    {
        let context = self.upgrade_context()?;
        let tree = context.read_tree();
        f(tree.state(), &context)
    }

    fn with_tree_state<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&TreeState) -> Result<T>,
    {
        self.with_tree_state_and_context(|state, _| f(state))
    }

    fn upgrade_node<'a>(&self, tree_state: &'a TreeState) -> Result<Node<'a>> {
        let state = self.state.read().unwrap();
        upgrade_range_node(&state, tree_state)
    }

    fn upgrade_for_read<'a>(&self, tree_state: &'a TreeState) -> Result<Range<'a>> {
        let state = self.state.read().unwrap();
        upgrade_range(&state, tree_state)
    }

    fn read_with_context<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(Range, &Context) -> Result<T>,
    {
        self.with_tree_state_and_context(|tree_state, context| {
            let range = self.upgrade_for_read(tree_state)?;
            f(range, context)
        })
    }

    fn read<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(Range) -> Result<T>,
    {
        self.read_with_context(|range, _| f(range))
    }

    fn write<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Range) -> Result<T>,
    {
        self.with_tree_state(|tree_state| {
            let mut state = self.state.write().unwrap();
            let mut range = upgrade_range(&state, tree_state)?;
            let result = f(&mut range);
            *state = range.downgrade();
            result
        })
    }

    fn do_action<F>(&self, f: F) -> Result<()>
    where
        for<'a> F: FnOnce(Range<'a>, &Tree) -> ActionRequest,
    {
        let context = self.upgrade_context()?;
        let tree = context.read_tree();
        let range = self.upgrade_for_read(tree.state())?;
        if range.node().is_disabled() {
            return Err(element_not_enabled());
        }
        let request = f(range, &tree);
        drop(tree);
        context.do_action(request);
        Ok(())
    }

    fn require_same_context(&self, other: &PlatformRange) -> Result<()> {
        if self.context.ptr_eq(&other.context) {
            Ok(())
        } else {
            Err(invalid_arg())
        }
    }
}

impl Clone for PlatformRange {
    fn clone(&self) -> Self {
        PlatformRange {
            context: self.context.clone(),
            state: RwLock::new(self.state.read().unwrap().clone()),
        }
    }
}

// Safely reject foreign range implementations rather than reinterpret-casting
// their COM allocation as an AccessKit object.

#[allow(non_snake_case)]
impl ITextRangeProvider_Impl for PlatformRange_Impl {
    fn Clone(&self) -> Result<ITextRangeProvider> {
        Ok(self.this.clone().into_provider())
    }

    fn Compare(&self, other: Ref<ITextRangeProvider>) -> Result<BOOL> {
        let other = &required_param(&other)?
            .cast_object_ref::<PlatformRange>()?
            .this;
        Ok((self.context.ptr_eq(&other.context)
            && *self.state.read().unwrap() == *other.state.read().unwrap())
        .into())
    }

    fn CompareEndpoints(
        &self,
        endpoint: TextPatternRangeEndpoint,
        other: Ref<ITextRangeProvider>,
        other_endpoint: TextPatternRangeEndpoint,
    ) -> Result<i32> {
        let other = &required_param(&other)?
            .cast_object_ref::<PlatformRange>()?
            .this;
        if std::ptr::eq(other as *const _, &self.this as *const _) {
            // Comparing endpoints within the same range can be done
            // safely without upgrading the range. This allows ATs
            // to determine whether an old range is degenerate even if
            // that range is no longer valid.
            let state = self.state.read().unwrap();
            let other_state = other.state.read().unwrap();
            let pos = weak_comparable_position_from_endpoint(&state, endpoint)?;
            let other_pos = weak_comparable_position_from_endpoint(&other_state, other_endpoint)?;
            let result = pos.cmp(other_pos);
            return Ok(result as i32);
        }
        self.require_same_context(other)?;
        self.with_tree_state(|tree_state| {
            let range = self.upgrade_for_read(tree_state)?;
            let other_range = other.upgrade_for_read(tree_state)?;
            if range.node().id() != other_range.node().id() {
                return Err(invalid_arg());
            }
            let pos = position_from_endpoint(&range, endpoint)?;
            let other_pos = position_from_endpoint(&other_range, other_endpoint)?;
            let result = pos.partial_cmp(&other_pos).unwrap();
            Ok(result as i32)
        })
    }

    fn ExpandToEnclosingUnit(&self, unit: TextUnit) -> Result<()> {
        if unit == TextUnit_Document {
            // Handle document as a special case so we can get to a document
            // range even if the current endpoints are now invalid.
            // Based on observed behavior, Narrator needs this ability.
            return self.with_tree_state(|tree_state| {
                let mut state = self.state.write().unwrap();
                let node = upgrade_range_node(&state, tree_state)?;
                *state = node.document_range().downgrade();
                Ok(())
            });
        }
        self.write(|range| {
            let start = range.start();
            if unit == TextUnit_Character && start.is_document_end() {
                // We know from experimentation that some Windows ATs
                // expect ExpandToEnclosingUnit(TextUnit_Character)
                // to do nothing if the range is degenerate at the end
                // of the document.
                return Ok(());
            }
            let start = back_to_unit_start(start, unit)?;
            range.set_start(start);
            if !start.is_document_end() {
                let end = move_forward_to_end(start, unit)?;
                range.set_end(end);
            }
            Ok(())
        })
    }

    fn FindAttribute(
        &self,
        _id: UIA_TEXTATTRIBUTE_ID,
        _value: &VARIANT,
        _backward: BOOL,
    ) -> Result<ITextRangeProvider> {
        // Variable rich-text attributes are not implemented. Return an actual
        // failure rather than S_OK with an uninitialized output interface.
        Err(not_implemented())
    }

    fn FindText(
        &self,
        text: &BSTR,
        backward: BOOL,
        ignore_case: BOOL,
    ) -> Result<ITextRangeProvider> {
        // The nullable ABI entry overrides this generated entry point.
        self.find_text_nullable(text, backward, ignore_case)?
            .ok_or_else(not_implemented)
    }

    fn GetAttributeValue(&self, id: UIA_TEXTATTRIBUTE_ID) -> Result<VARIANT> {
        self.read(|range| match id {
            UIA_IsReadOnlyAttributeId => {
                // TBD: do we ever want to support mixed read-only/editable text?
                let value = range.node().is_read_only();
                Ok(value.into())
            }
            UIA_CaretPositionAttributeId => {
                let mut value = CaretPosition_Unknown;
                if range.is_degenerate() {
                    let pos = range.start();
                    if pos.is_line_start() {
                        value = CaretPosition_BeginningOfLine;
                    } else if pos.is_line_end() {
                        value = CaretPosition_EndOfLine;
                    }
                }
                Ok(value.0.into())
            }
            UIA_CultureAttributeId => Ok(Variant::from(range.language().map(LocaleName)).into()),
            UIA_FontNameAttributeId => {
                let mut buffer = StringBuffer::acquire();
                Ok(
                    Variant::from(range.font_family().map(|s| StrWrapper::new(s, &mut buffer)))
                        .into(),
                )
            }
            UIA_FontSizeAttributeId => {
                Ok(Variant::from(range.font_size().map(|value| value as f64)).into())
            }
            UIA_FontWeightAttributeId => {
                Ok(Variant::from(range.font_weight().map(|value| value as i32)).into())
            }
            UIA_IsItalicAttributeId => Ok(Variant::from(range.is_italic()).into()),
            UIA_BackgroundColorAttributeId => Ok(Variant::from(range.background_color()).into()),
            UIA_ForegroundColorAttributeId => Ok(Variant::from(range.foreground_color()).into()),
            UIA_OverlineStyleAttributeId => {
                Ok(Variant::from(range.overline().map(|d| d.style)).into())
            }
            UIA_OverlineColorAttributeId => {
                Ok(Variant::from(range.overline().map(|d| d.color)).into())
            }
            UIA_StrikethroughStyleAttributeId => {
                Ok(Variant::from(range.strikethrough().map(|d| d.style)).into())
            }
            UIA_StrikethroughColorAttributeId => {
                Ok(Variant::from(range.strikethrough().map(|d| d.color)).into())
            }
            UIA_UnderlineStyleAttributeId => {
                Ok(Variant::from(range.underline().map(|d| d.style)).into())
            }
            UIA_UnderlineColorAttributeId => {
                Ok(Variant::from(range.underline().map(|d| d.color)).into())
            }
            UIA_HorizontalTextAlignmentAttributeId => Ok(Variant::from(range.text_align()).into()),
            UIA_IsSubscriptAttributeId => Ok(Variant::from(
                range
                    .vertical_offset()
                    .map(|o| o == VerticalOffset::Subscript),
            )
            .into()),
            UIA_IsSuperscriptAttributeId => Ok(Variant::from(
                range
                    .vertical_offset()
                    .map(|o| o == VerticalOffset::Superscript),
            )
            .into()),
            // TODO: implement more attributes
            _ => {
                let value = unsafe { UiaGetReservedNotSupportedValue() }.unwrap();
                Ok(value.into())
            }
        })
    }

    fn GetBoundingRectangles(&self) -> Result<*mut SAFEARRAY> {
        self.read_with_context(|range, context| {
            let clip = text_queries::viewport(*range.node());
            let rects = range.bounding_boxes();
            let client_top_left = context.client_top_left();
            let mut result = Vec::<f64>::with_capacity(rects.len() * 4);
            for rect in rects {
                let Some(clip) = clip else {
                    continue;
                };
                let rect = rect.intersect(clip);
                if rect.width() <= 0.0 || rect.height() <= 0.0 {
                    continue;
                }
                result.push(rect.x0 + client_top_left.x);
                result.push(rect.y0 + client_top_left.y);
                result.push(rect.width());
                result.push(rect.height());
            }
            Ok(safe_array_from_f64_slice(&result))
        })
    }

    fn GetEnclosingElement(&self) -> Result<IRawElementProviderSimple> {
        // Revisit this if we eventually support embedded objects.
        let context = self.upgrade_context()?;
        let tree = context.read_tree();
        let id = self.upgrade_node(tree.state())?.id();
        Ok(context.get_or_create_platform_node(id).into_interface())
    }

    fn GetText(&self, _max_length: i32) -> Result<BSTR> {
        // The Microsoft docs imply that the provider isn't _required_
        // to truncate text at the max length, so we just ignore it.
        self.read(|range| {
            let mut buffer = StringBuffer::acquire();
            let mut result = WideString::new(&mut buffer);
            range.write_text(&mut result).unwrap();
            Ok(result.into())
        })
    }

    fn Move(&self, unit: TextUnit, count: i32) -> Result<i32> {
        self.write(|range| {
            let degenerate = range.is_degenerate();
            let start = range.start();
            let start = if degenerate {
                start
            } else {
                back_to_unit_start(start, unit)?
            };
            let (start, moved) = move_position(start, unit, false, count)?;
            if moved != 0 {
                range.set_start(start);
                let end = if degenerate || start.is_document_end() {
                    start
                } else {
                    move_forward_to_end(start, unit)?
                };
                range.set_end(end);
            }
            Ok(moved)
        })
    }

    fn MoveEndpointByUnit(
        &self,
        endpoint: TextPatternRangeEndpoint,
        unit: TextUnit,
        count: i32,
    ) -> Result<i32> {
        self.write(|range| {
            let pos = position_from_endpoint(range, endpoint)?;
            let (pos, moved) =
                move_position(pos, unit, endpoint == TextPatternRangeEndpoint_End, count)?;
            set_endpoint_position(range, endpoint, pos)?;
            Ok(moved)
        })
    }

    fn MoveEndpointByRange(
        &self,
        endpoint: TextPatternRangeEndpoint,
        other: Ref<ITextRangeProvider>,
        other_endpoint: TextPatternRangeEndpoint,
    ) -> Result<()> {
        let other = &required_param(&other)?
            .cast_object_ref::<PlatformRange>()?
            .this;
        self.require_same_context(other)?;
        // We have to obtain the tree state and ranges manually to avoid
        // lifetime issues, and work with the two locks in a specific order
        // to avoid deadlock.
        self.with_tree_state(|tree_state| {
            let other_range = other.upgrade_for_read(tree_state)?;
            let mut state = self.state.write().unwrap();
            let mut range = upgrade_range(&state, tree_state)?;
            if range.node().id() != other_range.node().id() {
                return Err(invalid_arg());
            }
            let pos = position_from_endpoint(&other_range, other_endpoint)?;
            set_endpoint_position(&mut range, endpoint, pos)?;
            *state = range.downgrade();
            Ok(())
        })
    }

    fn Select(&self) -> Result<()> {
        self.do_action(|range, tree| {
            let (target_node, target_tree) = tree.state().locate_node(range.node().id()).unwrap();
            ActionRequest {
                action: Action::SetTextSelection,
                target_tree,
                target_node,
                data: Some(ActionData::SetTextSelection(range.to_text_selection())),
            }
        })
    }

    fn AddToSelection(&self) -> Result<()> {
        // AccessKit doesn't support multiple text selections.
        Err(invalid_operation())
    }

    fn RemoveFromSelection(&self) -> Result<()> {
        // AccessKit doesn't support multiple text selections.
        Err(invalid_operation())
    }

    fn ScrollIntoView(&self, align_to_top: BOOL) -> Result<()> {
        self.do_action(|range, tree| {
            let position = if align_to_top.into() {
                range.start()
            } else {
                range.end()
            };
            let (target_node, target_tree) = tree
                .state()
                .locate_node(position.inner_node().id())
                .unwrap();
            ActionRequest {
                action: Action::ScrollIntoView,
                target_tree,
                target_node,
                data: Some(ActionData::ScrollHint(if align_to_top.into() {
                    ScrollHint::TopEdge
                } else {
                    ScrollHint::BottomEdge
                })),
            }
        })
    }

    fn GetChildren(&self) -> Result<*mut SAFEARRAY> {
        // We don't support embedded objects in text.
        Ok(safe_array_from_com_slice(&[]))
    }
}

// Ensures that `PlatformRange` is actually safe to use in the free-threaded
// manner that we advertise via `ProviderOptions`.
#[test]
fn platform_range_impl_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PlatformRange>();
}

#[allow(non_snake_case)]
impl NullableTextRange_Impl for PlatformRange_Impl {
    fn find_text_nullable(
        &self,
        text: &BSTR,
        backward: BOOL,
        ignore_case: BOOL,
    ) -> Result<Option<ITextRangeProvider>> {
        let needle = String::from_utf16(text).map_err(|_| invalid_arg())?;
        if needle.is_empty() {
            return Ok(None);
        }
        self.read(|range| {
            let root = range.node();
            let base = range.start().to_global_utf16_index();
            let equal = |a: char, b: char| {
                if !ignore_case.as_bool() {
                    return a == b;
                }
                if a.len_utf16() != b.len_utf16() {
                    return false;
                }
                let mut left = [0u16; 2];
                let mut right = [0u16; 2];
                let left = a.encode_utf16(&mut left);
                let right = b.encode_utf16(&mut right);
                (unsafe { windows::Win32::Globalization::CompareStringOrdinal(left, right, true) })
                    == windows::Win32::Globalization::CSTR_EQUAL
            };
            let found =
                text_queries::find_text(&range, &needle, backward.as_bool(), equal, |_, _| true);
            let Some((start, end)) = found else {
                return Ok(None);
            };
            let mut found = range;
            found.set_start(
                root.text_position_from_global_utf16_index(base + start)
                    .ok_or_else(invalid_arg)?,
            );
            found.set_end(
                root.text_position_from_global_utf16_index(base + end)
                    .ok_or_else(invalid_arg)?,
            );
            Ok(Some(
                PlatformRange::new(&self.context, found).into_provider(),
            ))
        })
    }
}

#[cfg(test)]
mod kael_text_tests {
    use super::*;
    use crate::{context::ActionHandlerNoMut, window_handle::WindowHandle};
    use accesskit::{Node as Data, NodeId, Role, Tree as TreeData, TreeId, TreeUpdate};
    struct NoActions;
    impl ActionHandlerNoMut for NoActions {
        fn do_action(&self, _: ActionRequest) {
            panic!("unexpected native text mutation");
        }
    }
    fn context(readonly: bool) -> Arc<Context> {
        let mut root = Data::new(Role::MultilineTextInput);
        root.set_children([NodeId(2), NodeId(3)]);
        if readonly {
            root.set_read_only();
            root.add_action(Action::SetValue);
        }
        let mut first = Data::new(Role::TextRun);
        first.set_value("日本語 👩🏽‍💻 café ");
        first.set_character_lengths(
            "日本語 👩🏽‍💻 café "
                .chars()
                .map(|ch| ch.len_utf8() as u8)
                .collect::<Vec<_>>(),
        );
        let mut last = Data::new(Role::TextRun);
        last.set_value("日本語 👩🏽‍💻 café");
        last.set_character_lengths(
            "日本語 👩🏽‍💻 café"
                .chars()
                .map(|ch| ch.len_utf8() as u8)
                .collect::<Vec<_>>(),
        );
        Context::new(
            WindowHandle(Default::default()),
            Tree::new(
                TreeUpdate {
                    nodes: vec![(NodeId(1), root), (NodeId(2), first), (NodeId(3), last)],
                    tree: Some(TreeData::new(NodeId(1))),
                    tree_id: TreeId::ROOT,
                    focus: NodeId(1),
                },
                true,
            ),
            Arc::new(NoActions),
            false,
        )
    }
    fn document(context: &Arc<Context>) -> ITextRangeProvider {
        let tree = context.read_tree();
        let node = tree.state().root();
        PlatformRange::new(&Arc::downgrade(context), node.document_range()).into_provider()
    }
    #[test]
    fn native_find_text_unicode_forward_backward_case_and_nullable_abi() -> Result<()> {
        let context = context(false);
        let range = document(&context);
        let needle = BSTR::from("日本語 👩🏽‍💻 café");
        let first = unsafe { range.FindText(&needle, false, false) }?;
        let last = unsafe { range.FindText(&needle, true, false) }?;
        assert_eq!(
            unsafe { first.GetText(-1) }?.to_string(),
            needle.to_string()
        );
        assert!(
            unsafe {
                first.CompareEndpoints(
                    TextPatternRangeEndpoint_Start,
                    &last,
                    TextPatternRangeEndpoint_Start,
                )
            }? < 0
        );
        assert_eq!(
            unsafe {
                range
                    .FindText(&BSTR::from("CAFÉ"), false, true)?
                    .GetText(-1)
            }?
            .to_string(),
            "café"
        );
        let missing = BSTR::from("not present");
        let mut output = core::ptr::dangling_mut::<core::ffi::c_void>();
        let result = unsafe {
            (range.vtable().FindText)(
                range.as_raw(),
                missing.as_ptr().cast_mut().cast(),
                false.into(),
                false.into(),
                &mut output,
            )
        };
        assert_eq!(result, HRESULT(0));
        assert!(
            output.is_null(),
            "S_OK no-match must initialize NULL, not preserve a caller sentinel"
        );
        let result = unsafe {
            (range.vtable().FindText)(
                range.as_raw(),
                missing.as_ptr().cast_mut().cast(),
                false.into(),
                false.into(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(result, HRESULT(0x80004003u32 as i32));
        let malformed = BSTR::from_wide(&[0xD800]);
        let result = unsafe {
            (range.vtable().FindText)(
                range.as_raw(),
                malformed.as_ptr().cast_mut().cast(),
                false.into(),
                false.into(),
                &mut output,
            )
        };
        assert_eq!(result, HRESULT(0x80070057u32 as i32));
        assert!(output.is_null());
        Ok(())
    }
    #[test]
    fn native_visible_ranges_unknown_geometry_is_a_nonnull_degenerate_array() -> Result<()> {
        let context = context(false);
        let id = context.read_tree().state().root_id();
        let provider = context
            .get_or_create_platform_node(id)
            .to_interface::<ITextProvider>();
        let array = unsafe { provider.GetVisibleRanges() }?;
        assert!(!array.is_null());
        assert_eq!(
            unsafe { windows::Win32::System::Ole::SafeArrayGetDim(array) },
            1
        );
        assert_eq!(
            unsafe { windows::Win32::System::Ole::SafeArrayGetLBound(array, 1) }?,
            0
        );
        assert_eq!(
            unsafe { windows::Win32::System::Ole::SafeArrayGetUBound(array, 1) }?,
            0
        );
        unsafe { windows::Win32::System::Ole::SafeArrayDestroy(array) }?;
        Ok(())
    }
    #[test]
    fn native_com_range_rejects_foreign_owner() -> Result<()> {
        let first_context = context(false);
        let second_context = context(false);
        let first = document(&first_context);
        let other = document(&second_context);
        assert_eq!(
            unsafe {
                first.CompareEndpoints(
                    TextPatternRangeEndpoint_Start,
                    &other,
                    TextPatternRangeEndpoint_Start,
                )
            }
            .unwrap_err()
            .code(),
            invalid_arg().code()
        );
        Ok(())
    }
    #[test]
    fn native_readonly_value_mutation_is_rejected_before_action_dispatch() -> Result<()> {
        let context = context(true);
        let id = context.read_tree().state().root_id();
        let provider = context
            .get_or_create_platform_node(id)
            .to_interface::<IValueProvider>();
        assert_eq!(
            unsafe { provider.SetValue(w!("forbidden")) }
                .unwrap_err()
                .code(),
            invalid_operation().code()
        );
        Ok(())
    }
}

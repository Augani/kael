//! Tree navigation component with hierarchical data support.

use crate::components::button::ButtonVariant;
use crate::components::icon::Icon;
use crate::components::icon_button::IconButton;
use crate::components::icon_source::IconSource;
use crate::theme::Theme;
use kael::{prelude::*, *};
use std::collections::HashSet;
use std::hash::Hash;
use std::panic::Location;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
pub struct TreeNode<T: Clone> {
    pub id: T,
    pub label: SharedString,
    pub children: Vec<TreeNode<T>>,
    pub icon: Option<IconSource>,
    pub icon_color: Option<Hsla>,
    pub disabled: bool,
    pub has_lazy_children: bool,
}

impl<T: Clone> TreeNode<T> {
    pub fn new(id: T, label: impl Into<SharedString>) -> Self {
        Self {
            id,
            label: label.into(),
            children: Vec::new(),
            icon: None,
            icon_color: None,
            disabled: false,
            has_lazy_children: false,
        }
    }

    pub fn with_icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn with_icon_color(mut self, color: Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }

    pub fn with_children(mut self, children: Vec<TreeNode<T>>) -> Self {
        self.children = children;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_lazy_children(mut self, has_lazy: bool) -> Self {
        self.has_lazy_children = has_lazy;
        self
    }
}

// Retain only the row payload. Cloning a TreeNode here would recursively clone
// every descendant once per ancestor, even when the branch is collapsed.
pub(super) struct FlatTreeNode<T: Clone> {
    pub(super) node_id: T,
    pub(super) label: SharedString,
    pub(super) icon: Option<IconSource>,
    pub(super) icon_color: Option<Hsla>,
    pub(super) disabled: bool,
    pub(super) has_children: bool,
    pub(super) level: usize,
    pub(super) match_ranges: Vec<(usize, usize)>,
}

impl<T: Clone> FlatTreeNode<T> {
    fn new(node: &TreeNode<T>, level: usize, match_ranges: Vec<(usize, usize)>) -> Self {
        Self {
            node_id: node.id.clone(),
            label: node.label.clone(),
            icon: node.icon.clone(),
            icon_color: node.icon_color,
            disabled: node.disabled,
            has_children: !node.children.is_empty() || node.has_lazy_children,
            level,
            match_ranges,
        }
    }
}

pub(super) struct FilteredNode<'a, T: Clone> {
    node: &'a TreeNode<T>,
    match_ranges: Vec<(usize, usize)>,
    children: Vec<FilteredNode<'a, T>>,
}

pub(super) fn filter_tree<'a, T: Clone>(
    nodes: &'a [TreeNode<T>],
    filter: &str,
) -> Vec<FilteredNode<'a, T>> {
    fn visit<'a, T: Clone>(
        nodes: &'a [TreeNode<T>],
        filter_lower: &str,
    ) -> Vec<FilteredNode<'a, T>> {
        let mut filtered = Vec::new();
        for node in nodes {
            let (matches, match_ranges) = find_label_matches(&node.label, filter_lower);
            let children = visit(&node.children, filter_lower);
            if filter_lower.is_empty() || matches || !children.is_empty() {
                filtered.push(FilteredNode {
                    node,
                    match_ranges,
                    children,
                });
            }
        }
        filtered
    }

    visit(nodes, &filter.to_lowercase())
}

// Lowercasing can expand a character (for example, İ becomes i + ◌̇).
// Highlight ranges must still index characters in the original label.
fn find_label_matches(label: &str, filter_lower: &str) -> (bool, Vec<(usize, usize)>) {
    let label_lower = label.to_lowercase();
    let (matches, ranges) = find_matches(&label_lower, filter_lower);
    if !matches || label_lower.chars().count() == label.chars().count() {
        return (matches, ranges);
    }

    let original_indices: Vec<usize> = label
        .chars()
        .enumerate()
        .flat_map(|(index, character)| character.to_lowercase().map(move |_| index))
        .collect();
    let mut mapped: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        let range = (original_indices[start], original_indices[end - 1] + 1);
        if let Some(previous) = mapped.last_mut().filter(|previous| range.0 <= previous.1) {
            previous.1 = previous.1.max(range.1);
        } else {
            mapped.push(range);
        }
    }
    (true, mapped)
}

fn find_matches(text: &str, filter: &str) -> (bool, Vec<(usize, usize)>) {
    if filter.is_empty() {
        return (false, Vec::new());
    }

    let mut match_ranges = Vec::new();
    let mut start_byte = 0;
    let mut start_char_offset = 0;
    let filter_char_count = filter.chars().count();

    while let Some(pos) = text[start_byte..].find(filter) {
        let absolute_byte = start_byte + pos;
        let start_char = start_char_offset + text[start_byte..absolute_byte].chars().count();
        let end_char = start_char + filter_char_count;
        let overlaps_previous = match_ranges
            .last()
            .is_some_and(|(_, previous_end)| start_char <= *previous_end);
        if overlaps_previous {
            if let Some((_, previous_end)) = match_ranges.last_mut() {
                *previous_end = (*previous_end).max(end_char);
            }
        } else {
            match_ranges.push((start_char, end_char));
        }
        let advance = text[absolute_byte..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
        start_byte = absolute_byte + advance;
        start_char_offset = start_char + 1;
    }

    if !match_ranges.is_empty() {
        return (true, match_ranges);
    }

    let filter_chars: Vec<char> = filter.chars().collect();
    let mut filter_idx = 0;
    let mut current_match_start = None;
    let mut fuzzy_ranges = Vec::new();

    for (text_idx, text_char) in text.chars().enumerate() {
        if filter_idx < filter_chars.len() && text_char == filter_chars[filter_idx] {
            if current_match_start.is_none() {
                current_match_start = Some(text_idx);
            }
            filter_idx += 1;

            if filter_idx == filter_chars.len() {
                if let Some(start) = current_match_start {
                    fuzzy_ranges.push((start, text_idx + 1));
                }
                return (true, fuzzy_ranges);
            }
        }
    }

    (false, Vec::new())
}

pub(super) fn flatten_filtered_tree<T: Clone + Eq + Hash>(
    filtered_nodes: &[FilteredNode<'_, T>],
    expanded_ids: &HashSet<T>,
    level: usize,
    auto_expand_matches: bool,
) -> Vec<FlatTreeNode<T>> {
    let mut flat = Vec::new();
    let mut stack = vec![(filtered_nodes.iter(), level)];
    while let Some((siblings, level)) = stack.last_mut() {
        let Some(filtered_node) = siblings.next() else {
            stack.pop();
            continue;
        };
        let next_level = *level + 1;
        flat.push(FlatTreeNode::new(
            filtered_node.node,
            *level,
            filtered_node.match_ranges.clone(),
        ));
        if expanded_ids.contains(&filtered_node.node.id)
            || (auto_expand_matches && !filtered_node.children.is_empty())
        {
            stack.push((filtered_node.children.iter(), next_level));
        }
    }
    flat
}

pub(super) fn flatten_tree<T: Clone + Eq + Hash>(
    nodes: &[TreeNode<T>],
    expanded_ids: &HashSet<T>,
    level: usize,
) -> Vec<FlatTreeNode<T>> {
    let mut flat = Vec::new();
    // Keep one iterator per expanded ancestor, avoiding both recursive vectors
    // and traversal of collapsed descendants.
    let mut stack = vec![(nodes.iter(), level)];
    while let Some((siblings, level)) = stack.last_mut() {
        let Some(node) = siblings.next() else {
            stack.pop();
            continue;
        };
        let next_level = *level + 1;
        flat.push(FlatTreeNode::new(node, *level, Vec::new()));
        if !node.children.is_empty() && expanded_ids.contains(&node.id) {
            stack.push((node.children.iter(), next_level));
        }
    }
    flat
}

pub(super) fn parent_indices<T: Clone>(nodes: &[FlatTreeNode<T>]) -> Vec<Option<usize>> {
    let mut parents = Vec::with_capacity(nodes.len());
    let mut ancestors: Vec<usize> = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        while ancestors
            .last()
            .is_some_and(|ancestor| nodes[*ancestor].level >= node.level)
        {
            ancestors.pop();
        }
        parents.push(ancestors.last().copied());
        ancestors.push(index);
    }
    parents
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TreeListDensity {
    Compact,
    #[default]
    Balanced,
    Spacious,
}

impl TreeListDensity {
    pub(super) fn row_height(self) -> Pixels {
        match self {
            Self::Compact => px(28.0),
            Self::Balanced => px(36.0),
            Self::Spacious => px(44.0),
        }
    }

    pub(super) fn text_size(self) -> Pixels {
        px(14.0)
    }

    pub(super) fn indent(self) -> f32 {
        match self {
            Self::Compact => 14.0,
            Self::Balanced => 16.0,
            Self::Spacious => 20.0,
        }
    }
}

#[derive(IntoElement)]
pub struct TreeList<T: Clone + PartialEq + Eq + Hash + 'static> {
    id: ElementId,
    nodes: Arc<[TreeNode<T>]>,
    header: Option<AnyElement>,
    density: TreeListDensity,
    selected_id: Option<T>,
    expanded_ids: Vec<T>,
    filter: Option<String>,
    auto_expand_matches: bool,
    highlight_matches: bool,
    on_select: Option<Arc<dyn Fn(&T, &mut Window, &mut App) + Send + Sync + 'static>>,
    on_toggle: Option<Arc<dyn Fn(&T, bool, &mut Window, &mut App) + Send + Sync + 'static>>,
    on_right_click:
        Option<Arc<dyn Fn(&T, &MouseDownEvent, &mut Window, &mut App) + Send + Sync + 'static>>,
    style: StyleRefinement,
}

impl<T: Clone + PartialEq + Eq + Hash + 'static> Default for TreeList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + Eq + Hash + 'static> TreeList<T> {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "tree-list-{}-{}-{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            nodes: Arc::from([]),
            header: None,
            density: TreeListDensity::Balanced,
            selected_id: None,
            expanded_ids: Vec::new(),
            filter: None,
            auto_expand_matches: false,
            highlight_matches: true,
            on_select: None,
            on_toggle: None,
            on_right_click: None,
            style: StyleRefinement::default(),
        }
    }

    /// Set a stable id when multiple tree lists are rendered from the same callsite.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    /// Set the tree model. Use [`Self::shared_nodes`] to reuse a model across renders.
    pub fn nodes(mut self, nodes: Vec<TreeNode<T>>) -> Self {
        self.nodes = nodes.into();
        self
    }

    /// Reuse an immutable tree model without cloning its descendants each render.
    ///
    /// Replace the shared model when its nodes change. Expanded and selected IDs
    /// remain independently controlled by the application.
    pub fn shared_nodes(mut self, nodes: Arc<[TreeNode<T>]>) -> Self {
        self.nodes = nodes;
        self
    }

    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
    }

    pub fn density(mut self, density: TreeListDensity) -> Self {
        self.density = density;
        self
    }

    pub fn selected_id(mut self, id: T) -> Self {
        self.selected_id = Some(id);
        self
    }

    pub fn expanded_ids(mut self, ids: Vec<T>) -> Self {
        self.expanded_ids = ids;
        self
    }

    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        let filter_str = filter.into();
        self.filter = if filter_str.is_empty() {
            None
        } else {
            Some(filter_str)
        };
        self
    }

    pub fn auto_expand_matches(mut self, auto_expand: bool) -> Self {
        self.auto_expand_matches = auto_expand;
        self
    }

    pub fn highlight_matches(mut self, highlight: bool) -> Self {
        self.highlight_matches = highlight;
        self
    }

    pub fn on_select<F>(mut self, f: F) -> Self
    where
        F: Fn(&T, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_select = Some(Arc::new(f));
        self
    }

    pub fn on_toggle<F>(mut self, f: F) -> Self
    where
        F: Fn(&T, bool, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_toggle = Some(Arc::new(f));
        self
    }

    pub fn on_right_click<F>(mut self, f: F) -> Self
    where
        F: Fn(&T, &MouseDownEvent, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_right_click = Some(Arc::new(f));
        self
    }

    pub(super) fn render_highlighted_text(
        text: &str,
        match_ranges: &[(usize, usize)],
        highlight_color: Hsla,
        highlight_matches: bool,
        accessibility_hidden: bool,
    ) -> impl IntoElement + use<T> {
        if match_ranges.is_empty() || !highlight_matches {
            return div()
                .child(StyledText::new(text.to_string()).accessibility_hidden(accessibility_hidden))
                .into_any_element();
        }

        let mut parts = Vec::new();
        let mut last_end = 0;
        let text_chars: Vec<char> = text.chars().collect();

        let mut sorted_ranges = match_ranges.to_vec();
        sorted_ranges.sort_by_key(|r| r.0);

        for (start, end) in sorted_ranges {
            if last_end < start {
                let part: String = text_chars[last_end..start].iter().collect();
                parts.push((part, false));
            }

            let highlighted: String = text_chars[start..end.min(text_chars.len())]
                .iter()
                .collect();
            parts.push((highlighted, true));
            last_end = end.min(text_chars.len());
        }

        if last_end < text_chars.len() {
            let part: String = text_chars[last_end..].iter().collect();
            parts.push((part, false));
        }

        div()
            .flex()
            .children(parts.into_iter().map(|(text, is_match)| {
                if is_match {
                    div()
                        .bg(highlight_color)
                        .rounded_sm()
                        .px(px(1.0))
                        .child(StyledText::new(text).accessibility_hidden(accessibility_hidden))
                        .into_any_element()
                } else {
                    div()
                        .child(StyledText::new(text).accessibility_hidden(accessibility_hidden))
                        .into_any_element()
                }
            }))
            .into_any_element()
    }
}

impl<T: Clone + PartialEq + Eq + Hash + 'static> Styled for TreeList<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + PartialEq + Eq + Hash + 'static> RenderOnce for TreeList<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let expanded_set: HashSet<T> = self.expanded_ids.into_iter().collect();

        let flat_nodes = if let Some(ref filter) = self.filter {
            let filtered = filter_tree(&self.nodes, filter);
            flatten_filtered_tree(&filtered, &expanded_set, 0, self.auto_expand_matches)
        } else {
            flatten_tree(&self.nodes, &expanded_set, 0)
        };

        let total_items = flat_nodes.len();
        let density = self.density;
        let row_height = density.row_height();
        let text_size = density.text_size();
        let indent_step = density.indent();
        let flat_nodes_rc = Rc::new(flat_nodes);
        let tree_id = self.id;
        let row_focus_handles: Rc<Vec<FocusHandle>> = Rc::new(
            (0..total_items)
                .map(|index| {
                    let row_id = ElementId::NamedChild(
                        Box::new(tree_id.clone()),
                        format!("row-{index}").into(),
                    );
                    window
                        .use_keyed_state(row_id, cx, |_, cx| cx.focus_handle())
                        .read(cx)
                        .clone()
                })
                .collect(),
        );
        let parent_indices = Rc::new(parent_indices(&flat_nodes_rc));
        let selected_id = self.selected_id;
        let expanded_ids_rc = Rc::new(expanded_set);
        let on_select = self.on_select;
        let on_toggle = self.on_toggle;
        let on_right_click = self.on_right_click;
        let highlight_matches = self.highlight_matches;
        let user_style = self.style;
        let header = self.header;
        let theme = Theme::of(cx);
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);

        div()
            .id(tree_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Tree).label("Tree navigation"),
            )
            .flex()
            .flex_col()
            .w_full()
            .bg(theme.tokens.background)
            .when_some(header, |this, header| {
                this.child(div().mb(px(8.0)).child(header))
            })
            .map(|mut this| {
                this.style().refine(&user_style);
                this
            })
            .child(
                div()
                    .w_full()
                    .children(
                        flat_nodes_rc
                            .iter()
                            .enumerate()
                            .map(|(row_index, flat_node)| {
                                let is_selected = selected_id.as_ref() == Some(&flat_node.node_id);
                                let is_expanded = expanded_ids_rc.contains(&flat_node.node_id);
                                let has_children = flat_node.has_children;
                                let indent = px((flat_node.level as f32) * indent_step);
                                let focus_handle = row_focus_handles[row_index].clone();
                                let focus_on_mouse = focus_handle.clone();
                                let is_focused = focus_handle.is_focused(window);
                                let mut accessibility_state = AccessibilityState::NONE;
                                if is_selected {
                                    accessibility_state |= AccessibilityState::SELECTED;
                                }
                                if is_focused {
                                    accessibility_state |= AccessibilityState::FOCUSED;
                                }
                                if flat_node.disabled {
                                    accessibility_state |= AccessibilityState::DISABLED;
                                }
                                if has_children {
                                    accessibility_state |= if is_expanded {
                                        AccessibilityState::EXPANDED
                                    } else {
                                        AccessibilityState::COLLAPSED
                                    };
                                }
                                let mut accessibility =
                                    AccessibilityAttributes::new(AccessibilityRole::TreeItem)
                                        .label(flat_node.label.to_string())
                                        .states(accessibility_state);
                                if !flat_node.disabled {
                                    let mut actions = vec![
                                        AccessibilityAction::Focus,
                                        AccessibilityAction::Click,
                                    ];
                                    if has_children {
                                        actions.push(if is_expanded {
                                            AccessibilityAction::Collapse
                                        } else {
                                            AccessibilityAction::Expand
                                        });
                                    }
                                    accessibility = accessibility.actions(actions);
                                }

                                div()
                                    .id(ElementId::NamedChild(
                                        Box::new(tree_id.clone()),
                                        format!("item-{row_index}").into(),
                                    ))
                                    .accessibility(accessibility)
                                    .when(!flat_node.disabled, |this| {
                                        this.track_focus(&focus_handle.tab_index(0).tab_stop(true))
                                    })
                                    .w_full()
                                    .h(row_height)
                                    .flex()
                                    .items_center()
                                    .px(px(8.0))
                                    .pl(indent + px(8.0))
                                    .rounded(theme.tokens.radius_sm)
                                    .transition(theme.tokens.transition_fast)
                                    .cursor(if flat_node.disabled {
                                        CursorStyle::Arrow
                                    } else {
                                        CursorStyle::PointingHand
                                    })
                                    .bg(if is_selected {
                                        theme.tokens.accent
                                    } else {
                                        kael::transparent_black()
                                    })
                                    .text_color(if is_selected {
                                        theme.tokens.foreground
                                    } else if flat_node.disabled {
                                        theme.tokens.muted_foreground
                                    } else {
                                        theme.tokens.foreground
                                    })
                                    .when(!flat_node.disabled && !is_selected, |div| {
                                        div.hover(move |mut style| {
                                            style.background = Some(overlay_hover.into());
                                            style
                                        })
                                    })
                                    .when(!flat_node.disabled, {
                                        let on_select = on_select.clone();
                                        let node_id = flat_node.node_id.clone();

                                        move |this| {
                                            this.on_mouse_down(
                                                MouseButton::Left,
                                                move |_, window, cx| {
                                                    window.focus(&focus_on_mouse);
                                                    if let Some(on_select) = on_select.clone() {
                                                        on_select(&node_id, window, cx);
                                                    }
                                                },
                                            )
                                        }
                                    })
                                    .when(!flat_node.disabled, {
                                        let node_id = flat_node.node_id.clone();
                                        let on_select = on_select.clone();
                                        let on_toggle = on_toggle.clone();
                                        let focus_handles = row_focus_handles.clone();
                                        let parent_indices = parent_indices.clone();
                                        move |this| {
                                            this.on_key_down(move |event, window, cx| {
                                                if event.keystroke.modifiers.modified() {
                                                    return;
                                                }
                                                let handled = match event.keystroke.key.as_str() {
                                                    "up" if row_index > 0 => {
                                                        window.focus(&focus_handles[row_index - 1]);
                                                        true
                                                    }
                                                    "down"
                                                        if row_index + 1 < focus_handles.len() =>
                                                    {
                                                        window.focus(&focus_handles[row_index + 1]);
                                                        true
                                                    }
                                                    "home" if !focus_handles.is_empty() => {
                                                        window.focus(&focus_handles[0]);
                                                        true
                                                    }
                                                    "end" if !focus_handles.is_empty() => {
                                                        window.focus(
                                                            &focus_handles[focus_handles.len() - 1],
                                                        );
                                                        true
                                                    }
                                                    "right" if has_children && !is_expanded => {
                                                        if let Some(handler) = on_toggle.as_ref() {
                                                            handler(&node_id, true, window, cx);
                                                        }
                                                        true
                                                    }
                                                    "left" if has_children && is_expanded => {
                                                        if let Some(handler) = on_toggle.as_ref() {
                                                            handler(&node_id, false, window, cx);
                                                        }
                                                        true
                                                    }
                                                    "left" => {
                                                        if let Some(parent) =
                                                            parent_indices[row_index]
                                                        {
                                                            window.focus(&focus_handles[parent]);
                                                            true
                                                        } else {
                                                            false
                                                        }
                                                    }
                                                    "enter" | "space" => {
                                                        if let Some(handler) = on_select.as_ref() {
                                                            handler(&node_id, window, cx);
                                                        }
                                                        true
                                                    }
                                                    _ => false,
                                                };
                                                if handled {
                                                    cx.stop_propagation();
                                                    window.prevent_default();
                                                }
                                            })
                                        }
                                    })
                                    .when(!flat_node.disabled, {
                                        let on_right_click = on_right_click.clone();
                                        let node_id = flat_node.node_id.clone();

                                        move |this| {
                                            this.on_mouse_down(
                                                MouseButton::Right,
                                                move |event, window, cx| {
                                                    if let Some(on_right_click) =
                                                        on_right_click.clone()
                                                    {
                                                        on_right_click(&node_id, event, window, cx);
                                                    }
                                                },
                                            )
                                        }
                                    })
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(if has_children {
                                                let node_id = flat_node.node_id.clone();
                                                let on_toggle = on_toggle.clone();
                                                IconButton::new(if is_expanded {
                                                    "chevron-down"
                                                } else {
                                                    "chevron-right"
                                                })
                                                .id(ElementId::NamedChild(
                                                    Box::new(tree_id.clone()),
                                                    format!("toggle-{row_index}").into(),
                                                ))
                                                .label(if is_expanded {
                                                    format!("Collapse {}", flat_node.label)
                                                } else {
                                                    format!("Expand {}", flat_node.label)
                                                })
                                                .variant(ButtonVariant::Ghost)
                                                .size(px(24.0))
                                                .icon_size(px(12.0))
                                                .tab_stop(false)
                                                .disabled(flat_node.disabled)
                                                .on_click(move |_, window, cx| {
                                                    if let Some(handler) = on_toggle.as_ref() {
                                                        handler(&node_id, !is_expanded, window, cx);
                                                    }
                                                })
                                                .into_any_element()
                                            } else {
                                                div().w(px(24.0)).h(px(24.0)).into_any_element()
                                            })
                                            .children(flat_node.icon.as_ref().map(|icon| {
                                                Icon::new(icon.clone()).size(px(16.0)).color(
                                                    if flat_node.disabled {
                                                        theme.tokens.muted_foreground
                                                    } else {
                                                        flat_node.icon_color.unwrap_or(
                                                            theme.tokens.muted_foreground,
                                                        )
                                                    },
                                                )
                                            }))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.0))
                                                    .overflow_hidden()
                                                    .text_ellipsis()
                                                    .whitespace_nowrap()
                                                    .text_size(text_size)
                                                    .line_height(px(20.0))
                                                    .font_family(theme.tokens.font_family.clone())
                                                    .font_weight(if is_selected {
                                                        FontWeight::MEDIUM
                                                    } else {
                                                        FontWeight::NORMAL
                                                    })
                                                    .child({
                                                        let ranges = &flat_node.match_ranges;

                                                        if !ranges.is_empty() && highlight_matches {
                                                            Self::render_highlighted_text(
                                                                &flat_node.label,
                                                                ranges,
                                                                if is_selected {
                                                                    theme
                                                                        .tokens
                                                                        .accent_foreground
                                                                        .opacity(0.3)
                                                                } else {
                                                                    theme.tokens.accent.opacity(0.3)
                                                                },
                                                                highlight_matches,
                                                                false,
                                                            )
                                                            .into_any_element()
                                                        } else {
                                                            div()
                                                                .child(flat_node.label.clone())
                                                                .into_any_element()
                                                        }
                                                    }),
                                            ),
                                    )
                            }),
                    ),
            )
    }
}

#[derive(Clone)]
pub struct ListItem<T: Clone> {
    pub id: T,
    pub label: SharedString,
    pub icon: Option<IconSource>,
    pub badge: Option<SharedString>,
    pub disabled: bool,
}

impl<T: Clone> ListItem<T> {
    pub fn new(id: T, label: impl Into<SharedString>) -> Self {
        Self {
            id,
            label: label.into(),
            icon: None,
            badge: None,
            disabled: false,
        }
    }

    pub fn with_icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn with_badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(IntoElement)]
pub struct List<T: Clone + PartialEq + 'static> {
    items: Vec<ListItem<T>>,
    selected_id: Option<T>,
    on_select: Option<Arc<dyn Fn(&T, &mut Window, &mut App) + Send + Sync + 'static>>,
}

impl<T: Clone + PartialEq + 'static> Default for List<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + 'static> List<T> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected_id: None,
            on_select: None,
        }
    }

    pub fn items(mut self, items: Vec<ListItem<T>>) -> Self {
        self.items = items;
        self
    }

    pub fn selected_id(mut self, id: T) -> Self {
        self.selected_id = Some(id);
        self
    }

    pub fn on_select<F>(mut self, f: F) -> Self
    where
        F: Fn(&T, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_select = Some(Arc::new(f));
        self
    }

    fn is_selected(&self, item_id: &T) -> bool {
        self.selected_id.as_ref() == Some(item_id)
    }

    fn render_item(
        &self,
        item: &ListItem<T>,
        theme: &crate::theme::Theme,
    ) -> impl IntoElement + use<T> {
        let is_selected = self.is_selected(&item.id);

        let base = div()
            .flex()
            .items_center()
            .w_full()
            .px(px(12.0))
            .py(px(8.0))
            .rounded(theme.tokens.radius_sm)
            .transition(theme.tokens.transition_fast)
            .cursor(if item.disabled {
                CursorStyle::Arrow
            } else {
                CursorStyle::PointingHand
            });

        let styled = base
            .bg(if is_selected {
                theme.tokens.accent
            } else {
                kael::transparent_black()
            })
            .text_color(if is_selected {
                theme.tokens.accent_foreground
            } else if item.disabled {
                theme.tokens.muted_foreground
            } else {
                theme.tokens.primary
            })
            .when(!item.disabled && !is_selected, |div| {
                div.hover(|mut style| {
                    style.background = Some(theme.tokens.accent.opacity(0.5).into());
                    style
                })
            });

        let element = if let Some(icon) = item.icon.as_ref() {
            styled.child(
                div()
                    .mr(px(10.0))
                    .child(
                        Icon::new(icon.clone())
                            .size(px(18.0))
                            .color(if is_selected {
                                theme.tokens.accent_foreground
                            } else if item.disabled {
                                theme.tokens.muted_foreground
                            } else {
                                theme.tokens.primary
                            }),
                    ),
            )
        } else {
            styled
        };

        let with_label = element.child(
            div()
                .flex_1()
                .text_size(px(14.0))
                .font_family(theme.tokens.font_family.clone())
                .font_weight(if is_selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .child(item.label.clone()),
        );

        let with_badge = with_label.when_some(item.badge.as_ref(), |parent, badge| {
            parent.child(
                div()
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(theme.tokens.radius_sm)
                    .bg(if is_selected {
                        theme.tokens.accent_foreground.opacity(0.2)
                    } else {
                        theme.tokens.muted
                    })
                    .text_size(px(11.0))
                    .font_family(theme.tokens.font_family.clone())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if is_selected {
                        theme.tokens.accent_foreground
                    } else {
                        theme.tokens.muted_foreground
                    })
                    .child(badge.clone()),
            )
        });

        with_badge.when(!item.disabled, |this| {
            let on_select = self.on_select.clone();
            let item_id = item.id.clone();

            this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if let Some(on_select) = on_select.clone() {
                    on_select(&item_id, window, cx);
                }
            })
        })
    }
}

impl<T: Clone + PartialEq + 'static> RenderOnce for List<T> {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);

        div()
            .flex()
            .flex_col()
            .w_full()
            .bg(theme.tokens.background)
            .children(
                self.items
                    .iter()
                    .map(|item| self.render_item(item, theme).into_any_element()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::hash::{Hash, Hasher};

    #[derive(Debug)]
    struct CountedId {
        value: usize,
        clones: Rc<Cell<usize>>,
    }

    impl Clone for CountedId {
        fn clone(&self) -> Self {
            self.clones.set(self.clones.get() + 1);
            Self {
                value: self.value,
                clones: self.clones.clone(),
            }
        }
    }

    impl PartialEq for CountedId {
        fn eq(&self, other: &Self) -> bool {
            self.value == other.value
        }
    }

    impl Eq for CountedId {}

    impl Hash for CountedId {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.value.hash(state);
        }
    }

    fn counted_chain(length: usize, clones: &Rc<Cell<usize>>) -> Vec<TreeNode<CountedId>> {
        let mut nodes = Vec::new();
        for value in (0..length).rev() {
            nodes = vec![
                TreeNode::new(
                    CountedId {
                        value,
                        clones: clones.clone(),
                    },
                    "matching node",
                )
                .with_children(nodes),
            ];
        }
        nodes
    }

    #[::core::prelude::v1::test]
    fn substring_match_ranges_use_character_offsets() {
        assert_eq!(find_matches("éclair", "cl"), (true, vec![(1, 3)]));
        assert_eq!(find_matches("東京駅", "京"), (true, vec![(1, 2)]));
    }

    #[::core::prelude::v1::test]
    fn overlapping_substring_matches_are_merged() {
        assert_eq!(find_matches("aaaa", "aa"), (true, vec![(0, 4)]));
    }

    #[::core::prelude::v1::test]
    fn fuzzy_matches_keep_character_offsets() {
        assert_eq!(find_matches("résumé", "ré"), (true, vec![(0, 2)]));
        assert_eq!(find_matches("résumé", "rs"), (true, vec![(0, 3)]));
    }

    #[::core::prelude::v1::test]
    fn long_repeated_labels_merge_ranges_without_recounting_prefixes() {
        let text = "é".repeat(100_000);
        assert_eq!(find_matches(&text, "éé"), (true, vec![(0, 100_000)]));
    }

    #[::core::prelude::v1::test]
    fn lowercase_expansion_maps_highlights_to_original_characters() {
        assert_eq!(find_label_matches("İstanbul", "s"), (true, vec![(1, 2)]));
        assert_eq!(find_label_matches("İstanbul", "i"), (true, vec![(0, 1)]));
        assert_eq!(find_label_matches("İstanbul", "l"), (true, vec![(7, 8)]));
        assert_eq!(find_label_matches("İİ", "i"), (true, vec![(0, 2)]));
        assert_eq!(find_label_matches("İstanbul", "il"), (true, vec![(0, 8)]));
    }

    #[::core::prelude::v1::test]
    fn collapsed_branches_clone_only_the_visible_row_id() {
        let clones = Rc::new(Cell::new(0));
        let nodes = counted_chain(128, &clones);
        let flat = flatten_tree(&nodes, &HashSet::new(), 0);

        assert_eq!(flat.len(), 1);
        assert_eq!(clones.get(), 1);
        assert!(flat[0].has_children);
        assert_eq!(flat[0].node_id.value, 0);
    }

    #[::core::prelude::v1::test]
    fn expanded_chain_clones_each_row_once() {
        let clones = Rc::new(Cell::new(0));
        let nodes = counted_chain(128, &clones);
        #[allow(
            clippy::mutable_key_type,
            reason = "CountedId Hash and Eq use only immutable value; the Cell measures clones"
        )]
        let expanded = (0..128)
            .map(|value| CountedId {
                value,
                clones: clones.clone(),
            })
            .collect();
        let flat = flatten_tree(&nodes, &expanded, 0);

        assert_eq!(flat.len(), 128);
        assert_eq!(clones.get(), 128);
        assert_eq!(flat.last().unwrap().level, 127);
        assert!(!flat.last().unwrap().has_children);
    }

    #[::core::prelude::v1::test]
    fn filtering_borrows_models_and_clones_only_displayed_row_ids() {
        let clones = Rc::new(Cell::new(0));
        let nodes = counted_chain(128, &clones);
        let filtered = filter_tree(&nodes, "MATCH");

        assert!(std::ptr::eq(filtered[0].node, &nodes[0]));
        assert_eq!(clones.get(), 0);
        let flat = flatten_filtered_tree(&filtered, &HashSet::new(), 0, true);
        assert_eq!(flat.len(), 128);
        assert_eq!(clones.get(), 128);
        assert!(flat.iter().all(|node| node.match_ranges == vec![(0, 5)]));
    }

    #[::core::prelude::v1::test]
    fn filtered_tree_retains_ancestors_and_original_expandability() {
        let nodes = vec![TreeNode::new(0, "root").with_children(vec![
            TreeNode::new(1, "branch").with_children(vec![TreeNode::new(2, "needle")]),
            TreeNode::new(3, "hidden"),
        ])];
        let filtered = filter_tree(&nodes, "needle");
        let collapsed = flatten_filtered_tree(&filtered, &HashSet::new(), 0, false);
        assert_eq!(collapsed.len(), 1);
        assert!(collapsed[0].has_children);

        let expanded = flatten_filtered_tree(&filtered, &HashSet::new(), 0, true);
        assert_eq!(
            expanded.iter().map(|node| node.node_id).collect::<Vec<_>>(),
            vec![0, 1, 2],
        );
        assert_eq!(parent_indices(&expanded), vec![None, Some(0), Some(1)]);

        // A matched branch still expands even when all its children were filtered out.
        let root_match = filter_tree(&nodes, "root");
        let root_row = flatten_filtered_tree(&root_match, &HashSet::new(), 0, true);
        assert_eq!(root_row.len(), 1);
        assert!(root_row[0].has_children);
    }

    #[::core::prelude::v1::test]
    fn parent_indexing_handles_wide_siblings_and_nested_roots() {
        let nodes = vec![
            TreeNode::new(0, "root").with_children(vec![
                TreeNode::new(1, "first").with_children(vec![TreeNode::new(2, "nested")]),
                TreeNode::new(3, "second"),
                TreeNode::new(4, "third"),
            ]),
            TreeNode::new(5, "other root").with_children(vec![TreeNode::new(6, "other child")]),
        ];
        let flat = flatten_tree(&nodes, &HashSet::from([0, 1, 5]), 0);
        assert_eq!(
            parent_indices(&flat),
            vec![None, Some(0), Some(1), Some(0), Some(0), None, Some(5)],
        );

        let wide = vec![
            TreeNode::new(0, "root")
                .with_children((1..10_001).map(|id| TreeNode::new(id, "child")).collect()),
        ];
        let flat = flatten_tree(&wide, &HashSet::from([0]), 0);
        let parents = parent_indices(&flat);
        assert_eq!(parents.len(), 10_001);
        assert_eq!(parents[0], None);
        assert!(parents[1..].iter().all(|parent| *parent == Some(0)));
    }

    #[::core::prelude::v1::test]
    fn shared_nodes_reuse_the_original_allocation() {
        let nodes: Arc<[TreeNode<usize>]> = vec![TreeNode::new(0, "root")].into();
        let tree = TreeList::new().shared_nodes(nodes.clone());
        assert!(Arc::ptr_eq(&tree.nodes, &nodes));
        assert_eq!(Arc::strong_count(&nodes), 2);
    }
}

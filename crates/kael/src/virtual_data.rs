use std::collections::BTreeSet;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

const MAX_SELECTION_ITEMS: usize = 100_000;
const MAX_COLLECTION_CHANGES: usize = 100_000;
const MAX_TABLE_COLUMNS: usize = 1_024;
const MAX_COLUMN_ID_BYTES: usize = 256;
const MAX_COLUMN_LABEL_BYTES: usize = 4_096;
const MAX_TREE_NODES: usize = 100_000;
const MAX_TREE_DEPTH: usize = 256;

/// How a selection model handles user interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionMode {
    /// Only one item may be selected at a time.
    Single,
    /// Multiple items may be toggled independently.
    Multi,
    /// A contiguous range of items may be selected via anchor/pivot.
    Range,
}

/// Tracks selected indices within a virtualized collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "SelectionSerde")]
pub struct Selection {
    mode: SelectionMode,
    selected: BTreeSet<usize>,
    anchor: Option<usize>,
    pivot: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionSerde {
    mode: SelectionMode,
    selected: BTreeSet<usize>,
    anchor: Option<usize>,
    pivot: Option<usize>,
}

impl TryFrom<SelectionSerde> for Selection {
    type Error = String;

    fn try_from(value: SelectionSerde) -> std::result::Result<Self, Self::Error> {
        if value.selected.len() > MAX_SELECTION_ITEMS {
            return Err("selection capacity exceeded".into());
        }
        if value.mode == SelectionMode::Single && value.selected.len() > 1 {
            return Err("single selection contains multiple indices".into());
        }
        Ok(Self {
            mode: value.mode,
            selected: value.selected,
            anchor: value.anchor,
            pivot: value.pivot,
        })
    }
}

impl Selection {
    /// Create a new empty selection with the given mode.
    pub fn new(mode: SelectionMode) -> Self {
        Self {
            mode,
            selected: BTreeSet::new(),
            anchor: None,
            pivot: None,
        }
    }

    /// Select a single index. In [`SelectionMode::Single`] mode this clears
    /// any previous selection first.
    pub fn select(&mut self, index: usize) {
        let _ = self.try_select(index);
    }

    /// Select an index, returning an error if the selection capacity is exhausted.
    pub fn try_select(&mut self, index: usize) -> Result<()> {
        if self.mode != SelectionMode::Single
            && self.selected.len() >= MAX_SELECTION_ITEMS
            && !self.selected.contains(&index)
        {
            return Err(anyhow!("selection capacity exceeded"));
        }
        if self.mode == SelectionMode::Single {
            self.selected.clear();
        }
        self.selected.insert(index);
        self.anchor = Some(index);
        self.pivot = Some(index);
        Ok(())
    }

    /// Toggle the presence of `index` in the selection.
    /// Only meaningful in [`SelectionMode::Multi`] mode; otherwise behaves
    /// like [`select`](Self::select).
    pub fn toggle(&mut self, index: usize) {
        if self.mode != SelectionMode::Multi {
            self.select(index);
            return;
        }
        if self.selected.contains(&index) {
            self.selected.remove(&index);
        } else {
            if self.selected.len() >= MAX_SELECTION_ITEMS {
                return;
            }
            self.selected.insert(index);
        }
        self.anchor = Some(index);
    }

    /// Extend the selection from the current anchor to `index`, replacing
    /// any previous range. Requires [`SelectionMode::Range`] or
    /// [`SelectionMode::Multi`].
    pub fn extend_to(&mut self, index: usize) {
        let _ = self.try_extend_to(index);
    }

    /// Extend the selection, returning an error when the requested range is too large.
    pub fn try_extend_to(&mut self, index: usize) -> Result<()> {
        if self.mode == SelectionMode::Single {
            return self.try_select(index);
        }
        let anchor = self.anchor.unwrap_or(0);

        let start = anchor.min(index);
        let end = anchor.max(index);
        let range_len = end
            .checked_sub(start)
            .and_then(|len| len.checked_add(1))
            .ok_or_else(|| anyhow!("selection range overflow"))?;
        if range_len > MAX_SELECTION_ITEMS {
            return Err(anyhow!("selection range exceeds capacity"));
        }

        if self.selected.len() > MAX_SELECTION_ITEMS {
            return Err(anyhow!("selection exceeds capacity"));
        }
        let mut selected = self.selected.clone();
        if let Some(prev_pivot) = self.pivot {
            let old_start = anchor.min(prev_pivot);
            let old_end = anchor.max(prev_pivot);
            selected.retain(|selected| *selected < old_start || *selected > old_end);
        }

        for i in start..=end {
            selected.insert(i);
        }
        if selected.len() > MAX_SELECTION_ITEMS {
            return Err(anyhow!("selection capacity exceeded"));
        }
        self.selected = selected;
        self.pivot = Some(index);
        Ok(())
    }

    /// Select all indices in `0..count`.
    pub fn select_all(&mut self, count: usize) {
        let _ = self.try_select_all(count);
    }

    /// Select all indices, returning an error when `count` exceeds capacity.
    pub fn try_select_all(&mut self, count: usize) -> Result<()> {
        if count > MAX_SELECTION_ITEMS {
            return Err(anyhow!("selection count exceeds capacity"));
        }
        self.selected = (0..count).collect();
        Ok(())
    }

    /// Remove all selected indices.
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.pivot = None;
    }

    /// Returns `true` if the given index is currently selected.
    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    /// Returns a reference to the set of all selected indices.
    pub fn selected_indices(&self) -> &BTreeSet<usize> {
        &self.selected
    }

    /// Returns the number of currently selected items.
    pub fn count(&self) -> usize {
        self.selected.len()
    }
}

/// Trait for data sources that back virtualized list and table views.
///
/// Implementors provide indexed access to items without requiring the full
/// collection to reside in memory.
pub trait VirtualDataSource: Send + Sync {
    /// The element type stored in this data source.
    type Item;

    /// Total number of items available.
    fn len(&self) -> usize;

    /// Returns `true` when the data source contains no items.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Retrieve the item at `index`, or `None` if out of bounds.
    fn item_at(&self, index: usize) -> Option<&Self::Item>;
}

/// A single change within a [`CollectionDiff`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollectionChange {
    /// An item was inserted at the given index.
    Insert {
        /// Index where the new item was placed.
        index: usize,
    },
    /// The item at the given index was removed.
    Remove {
        /// Index that was removed.
        index: usize,
    },
    /// The item at the given index was updated in place.
    Update {
        /// Index that was modified.
        index: usize,
    },
    /// An item moved from one position to another.
    Move {
        /// Original index.
        from: usize,
        /// Destination index.
        to: usize,
    },
    /// The entire collection was replaced.
    Reset,
}

/// An ordered list of [`CollectionChange`]s describing how a data source
/// transitioned between two states.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(try_from = "CollectionDiffSerde")]
pub struct CollectionDiff {
    changes: Vec<CollectionChange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionDiffSerde {
    changes: Vec<CollectionChange>,
}

impl TryFrom<CollectionDiffSerde> for CollectionDiff {
    type Error = String;

    fn try_from(value: CollectionDiffSerde) -> std::result::Result<Self, Self::Error> {
        if value.changes.len() > MAX_COLLECTION_CHANGES {
            return Err("collection diff capacity exceeded".into());
        }
        Ok(Self {
            changes: value.changes,
        })
    }
}

impl CollectionDiff {
    /// Create an empty diff.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a change to this diff.
    pub fn push(&mut self, change: CollectionChange) {
        let _ = self.try_push(change);
    }

    /// Append a change, returning an error if the diff is at capacity.
    pub fn try_push(&mut self, change: CollectionChange) -> Result<()> {
        if self.changes.len() >= MAX_COLLECTION_CHANGES {
            return Err(anyhow!("collection diff capacity exceeded"));
        }
        self.changes.push(change);
        Ok(())
    }

    /// Returns a slice of all recorded changes.
    pub fn changes(&self) -> &[CollectionChange] {
        &self.changes
    }

    /// Returns `true` if no changes have been recorded.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Returns the number of recorded changes.
    pub fn len(&self) -> usize {
        self.changes.len()
    }
}

/// Tracks which portion of a virtualized collection is currently visible,
/// with optional prefetch margins.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "VisibleRangeSerde")]
pub struct VisibleRange {
    start: usize,
    end: usize,
    prefetch: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VisibleRangeSerde {
    start: usize,
    end: usize,
    prefetch: usize,
}

impl TryFrom<VisibleRangeSerde> for VisibleRange {
    type Error = String;

    fn try_from(value: VisibleRangeSerde) -> std::result::Result<Self, Self::Error> {
        if value.end < value.start {
            return Err("visible range ends before it starts".into());
        }
        Ok(Self {
            start: value.start,
            end: value.end,
            prefetch: value.prefetch,
        })
    }
}

impl VisibleRange {
    /// Create a new visible range.
    ///
    /// `start` is the first visible index, `end` is exclusive, and `prefetch`
    /// is the number of extra items to keep loaded before and after the
    /// visible window.
    pub fn new(start: usize, end: usize, prefetch: usize) -> Self {
        Self {
            start,
            end: end.max(start),
            prefetch,
        }
    }

    /// Returns `true` if `index` falls within the visible window
    /// (excluding prefetch).
    pub fn contains(&self, index: usize) -> bool {
        index >= self.start && index < self.end
    }

    /// Returns the expanded range `(start, end)` including prefetch margins,
    /// clamped so the start is never below zero.
    pub fn prefetch_range(&self) -> (usize, usize) {
        let start = self.start.saturating_sub(self.prefetch);
        let end = self.end.saturating_add(self.prefetch);
        (start, end)
    }

    /// Number of items in the visible window (excluding prefetch).
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Returns `true` if the visible window has zero length.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Update the visible window boundaries.
    pub fn set_range(&mut self, start: usize, end: usize) {
        self.start = start;
        self.end = end.max(start);
    }
}

/// Sort direction for a table column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    /// Smallest values first.
    Ascending,
    /// Largest values first.
    Descending,
}

/// Describes a single column in a [`VirtualTableModel`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnDescriptor {
    /// Unique identifier for this column.
    pub id: String,
    /// Human-readable header label.
    pub label: String,
    /// Current width in logical pixels.
    pub width: f32,
    /// Minimum allowed width in logical pixels.
    pub min_width: f32,
    /// Maximum allowed width in logical pixels.
    pub max_width: f32,
    /// Whether the user may drag-resize this column.
    pub resizable: bool,
    /// Whether clicking the header sorts by this column.
    pub sortable: bool,
}

/// Current sort state of a [`VirtualTableModel`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSort {
    /// The id of the column being sorted.
    pub column_id: String,
    /// The direction of the sort.
    pub direction: SortDirection,
}

/// Data model for a virtualized table with columns, selection, sorting,
/// and visible-range tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "VirtualTableModelSerde")]
pub struct VirtualTableModel {
    columns: Vec<ColumnDescriptor>,
    row_count: usize,
    sort: Option<TableSort>,
    selection: Selection,
    visible_range: VisibleRange,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VirtualTableModelSerde {
    columns: Vec<ColumnDescriptor>,
    row_count: usize,
    sort: Option<TableSort>,
    selection: Selection,
    visible_range: VisibleRange,
}

impl TryFrom<VirtualTableModelSerde> for VirtualTableModel {
    type Error = String;

    fn try_from(value: VirtualTableModelSerde) -> std::result::Result<Self, Self::Error> {
        let mut table =
            Self::try_new(value.columns, value.row_count).map_err(|error| error.to_string())?;
        table
            .try_set_sort(value.sort)
            .map_err(|error| error.to_string())?;
        table.selection = value.selection;
        table.visible_range = value.visible_range;
        Ok(table)
    }
}

impl VirtualTableModel {
    /// Create a new table model with the given columns and row count.
    pub fn new(columns: Vec<ColumnDescriptor>, row_count: usize) -> Self {
        Self::try_new(columns, row_count).unwrap_or_else(|_| Self {
            columns: Vec::new(),
            row_count,
            sort: None,
            selection: Selection::new(SelectionMode::Single),
            visible_range: VisibleRange::new(0, 0, 0),
        })
    }

    /// Create a validated table model.
    pub fn try_new(columns: Vec<ColumnDescriptor>, row_count: usize) -> Result<Self> {
        if columns.len() > MAX_TABLE_COLUMNS {
            return Err(anyhow!("table column capacity exceeded"));
        }
        let mut ids = BTreeSet::new();
        for column in &columns {
            validate_column(column)?;
            if !ids.insert(column.id.as_str()) {
                return Err(anyhow!("duplicate table column id"));
            }
        }
        Ok(Self {
            columns,
            row_count,
            sort: None,
            selection: Selection::new(SelectionMode::Single),
            visible_range: VisibleRange::new(0, 0, 0),
        })
    }

    /// Returns a slice of all column descriptors.
    pub fn columns(&self) -> &[ColumnDescriptor] {
        &self.columns
    }

    /// Total number of rows.
    pub fn row_count(&self) -> usize {
        self.row_count
    }

    /// Update the total row count.
    pub fn set_row_count(&mut self, count: usize) {
        self.row_count = count;
    }

    /// Returns the current sort state, if any.
    pub fn sort(&self) -> Option<&TableSort> {
        self.sort.as_ref()
    }

    /// Set or clear the sort state.
    pub fn set_sort(&mut self, sort: Option<TableSort>) {
        if self.try_set_sort(sort).is_err() {
            self.sort = None;
        }
    }

    /// Set a validated sort state.
    pub fn try_set_sort(&mut self, sort: Option<TableSort>) -> Result<()> {
        if let Some(sort) = &sort {
            if sort.column_id.is_empty() || sort.column_id.len() > MAX_COLUMN_ID_BYTES {
                return Err(anyhow!("invalid sort column id"));
            }
            let column = self
                .columns
                .iter()
                .find(|column| column.id == sort.column_id)
                .ok_or_else(|| anyhow!("sort column not found"))?;
            if !column.sortable {
                return Err(anyhow!("column is not sortable"));
            }
        }
        self.sort = sort;
        Ok(())
    }

    /// Returns a shared reference to the row selection.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Returns a mutable reference to the row selection.
    pub fn selection_mut(&mut self) -> &mut Selection {
        &mut self.selection
    }

    /// Returns a shared reference to the visible range.
    pub fn visible_range(&self) -> &VisibleRange {
        &self.visible_range
    }

    /// Returns a mutable reference to the visible range.
    pub fn visible_range_mut(&mut self) -> &mut VisibleRange {
        &mut self.visible_range
    }

    /// Resize the column identified by `id` to `width`, clamped to the
    /// column's `[min_width, max_width]` bounds. Returns an error if no
    /// column with the given id exists or the column is not resizable.
    pub fn resize_column(&mut self, id: &str, width: f32) -> Result<()> {
        if id.is_empty() || id.len() > MAX_COLUMN_ID_BYTES || !width.is_finite() {
            return Err(anyhow!("invalid column resize request"));
        }
        let col = self
            .columns
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| anyhow!("column not found"))?;

        if !col.resizable {
            return Err(anyhow!("column is not resizable"));
        }

        validate_column(col)?;

        col.width = width.clamp(col.min_width, col.max_width);
        Ok(())
    }
}

fn validate_column(column: &ColumnDescriptor) -> Result<()> {
    if column.id.is_empty()
        || column.id.len() > MAX_COLUMN_ID_BYTES
        || column.id.chars().any(char::is_control)
        || column.label.len() > MAX_COLUMN_LABEL_BYTES
        || column
            .label
            .chars()
            .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(anyhow!("invalid table column text"));
    }
    if !column.width.is_finite()
        || !column.min_width.is_finite()
        || !column.max_width.is_finite()
        || column.min_width < 0.0
        || column.max_width < column.min_width
        || column.width < column.min_width
        || column.width > column.max_width
    {
        return Err(anyhow!("invalid table column width bounds"));
    }
    Ok(())
}

/// A node in a tree structure used for virtualized tree views.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeNode<T> {
    /// The data payload for this node.
    pub data: T,
    /// Child nodes.
    pub children: Vec<TreeNode<T>>,
    /// Whether this node's children are visible.
    pub expanded: bool,
    /// Nesting depth (0 for roots).
    pub depth: usize,
}

/// A read-only view of one row produced by flattening a [`TreeModel`].
#[derive(Debug, Clone)]
pub struct FlattenedTreeItem<'a, T> {
    /// Reference to the node's data.
    pub data: &'a T,
    /// Nesting depth.
    pub depth: usize,
    /// Whether this node is expanded.
    pub expanded: bool,
    /// Whether this node has any children.
    pub has_children: bool,
    /// Position in the flattened list.
    pub index: usize,
}

/// A forest of [`TreeNode`]s with expand/collapse tracking and
/// efficient flattening for virtualized rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeModel<T> {
    roots: Vec<TreeNode<T>>,
}

impl<T> Default for TreeModel<T> {
    fn default() -> Self {
        Self { roots: Vec::new() }
    }
}

impl<T> TreeModel<T> {
    /// Create an empty tree model.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a root-level node.
    pub fn add_root(&mut self, node: TreeNode<T>) {
        let _ = self.try_add_root(node);
    }

    /// Append a validated root node within the tree's count and depth limits.
    pub fn try_add_root(&mut self, node: TreeNode<T>) -> Result<()> {
        let existing = count_tree_nodes(&self.roots)
            .ok_or_else(|| anyhow!("existing tree exceeds capacity"))?;
        let incoming = validate_tree_node(&node)?;
        if existing
            .checked_add(incoming)
            .map_or(true, |count| count > MAX_TREE_NODES)
        {
            return Err(anyhow!("tree node capacity exceeded"));
        }
        self.roots.push(node);
        Ok(())
    }

    /// Returns a slice of the root nodes.
    pub fn roots(&self) -> &[TreeNode<T>] {
        &self.roots
    }

    /// Flatten the tree into a linear sequence of visible items.
    /// Only expanded nodes have their children included.
    pub fn flatten(&self) -> Vec<FlattenedTreeItem<'_, T>> {
        let mut items = Vec::new();
        let mut pending: Vec<_> = self.roots.iter().take(MAX_TREE_NODES).rev().collect();
        while let Some(node) = pending.pop() {
            if items.len() >= MAX_TREE_NODES {
                break;
            }
            let index = items.len();
            items.push(FlattenedTreeItem {
                data: &node.data,
                depth: node.depth,
                expanded: node.expanded,
                has_children: !node.children.is_empty(),
                index,
            });
            if node.expanded {
                let remaining = MAX_TREE_NODES.saturating_sub(pending.len());
                pending.extend(node.children.iter().rev().take(remaining));
            }
        }
        items
    }

    /// Toggle the expanded state of the node at `path` where each element
    /// is a child index at the corresponding depth. Returns the new
    /// expanded state, or `false` if the path is invalid.
    pub fn toggle_expanded(&mut self, path: &[usize]) -> bool {
        if path.is_empty() || path.len() > MAX_TREE_DEPTH {
            return false;
        }
        let mut nodes = self.roots.as_mut_slice();
        for (depth, index) in path.iter().copied().enumerate() {
            let Some(node) = nodes.get_mut(index) else {
                return false;
            };
            if depth + 1 == path.len() {
                node.expanded = !node.expanded;
                return node.expanded;
            }
            nodes = node.children.as_mut_slice();
        }
        false
    }

    /// Count the total number of currently visible (flattened) nodes.
    pub fn total_visible_count(&self) -> usize {
        let mut count = 0usize;
        let mut pending: Vec<_> = self.roots.iter().take(MAX_TREE_NODES).collect();
        while let Some(node) = pending.pop() {
            count = count.saturating_add(1);
            if count >= MAX_TREE_NODES {
                return MAX_TREE_NODES;
            }
            if node.expanded {
                let remaining = MAX_TREE_NODES.saturating_sub(pending.len());
                pending.extend(node.children.iter().take(remaining));
            }
        }
        count
    }
}

fn count_tree_nodes<T>(roots: &[TreeNode<T>]) -> Option<usize> {
    if roots.len() > MAX_TREE_NODES {
        return None;
    }
    let mut count = 0usize;
    let mut pending: Vec<_> = roots.iter().collect();
    while let Some(node) = pending.pop() {
        count = count.checked_add(1)?;
        if count > MAX_TREE_NODES {
            return None;
        }
        if node.children.len() > MAX_TREE_NODES.saturating_sub(pending.len()) {
            return None;
        }
        pending.extend(&node.children);
    }
    Some(count)
}

fn validate_tree_node<T>(root: &TreeNode<T>) -> Result<usize> {
    let mut count = 0usize;
    let mut pending = vec![(root, 0usize)];
    while let Some((node, expected_depth)) = pending.pop() {
        if expected_depth >= MAX_TREE_DEPTH || node.depth != expected_depth {
            return Err(anyhow!("invalid tree depth"));
        }
        count = count
            .checked_add(1)
            .ok_or_else(|| anyhow!("tree node count overflow"))?;
        if count > MAX_TREE_NODES {
            return Err(anyhow!("tree node capacity exceeded"));
        }
        let child_depth = expected_depth
            .checked_add(1)
            .ok_or_else(|| anyhow!("tree depth overflow"))?;
        if node.children.len() > MAX_TREE_NODES.saturating_sub(pending.len()) {
            return Err(anyhow!("tree node capacity exceeded"));
        }
        pending.extend(node.children.iter().map(|child| (child, child_depth)));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod selection_tests {
        use super::*;

        #[test]
        fn single_mode_replaces_previous() {
            let mut sel = Selection::new(SelectionMode::Single);
            sel.select(3);
            sel.select(5);
            assert!(!sel.is_selected(3));
            assert!(sel.is_selected(5));
            assert_eq!(sel.count(), 1);
        }

        #[test]
        fn multi_mode_keeps_all() {
            let mut sel = Selection::new(SelectionMode::Multi);
            sel.select(1);
            sel.select(3);
            assert!(sel.is_selected(1));
            assert!(sel.is_selected(3));
            assert_eq!(sel.count(), 2);
        }

        #[test]
        fn toggle_adds_and_removes() {
            let mut sel = Selection::new(SelectionMode::Multi);
            sel.toggle(2);
            assert!(sel.is_selected(2));
            sel.toggle(2);
            assert!(!sel.is_selected(2));
            assert_eq!(sel.count(), 0);
        }

        #[test]
        fn toggle_in_single_mode_acts_like_select() {
            let mut sel = Selection::new(SelectionMode::Single);
            sel.toggle(1);
            assert!(sel.is_selected(1));
            sel.toggle(3);
            assert!(!sel.is_selected(1));
            assert!(sel.is_selected(3));
        }

        #[test]
        fn extend_to_creates_range() {
            let mut sel = Selection::new(SelectionMode::Range);
            sel.select(2);
            sel.extend_to(5);
            for i in 2..=5 {
                assert!(sel.is_selected(i), "expected {} to be selected", i);
            }
            assert!(!sel.is_selected(1));
            assert!(!sel.is_selected(6));
        }

        #[test]
        fn extend_to_replaces_old_range() {
            let mut sel = Selection::new(SelectionMode::Range);
            sel.select(5);
            sel.extend_to(8);
            assert_eq!(sel.count(), 4);

            sel.extend_to(3);
            for i in 3..=5 {
                assert!(sel.is_selected(i), "expected {} selected", i);
            }
            assert!(!sel.is_selected(6));
            assert!(!sel.is_selected(8));
        }

        #[test]
        fn extend_to_in_single_mode_selects_endpoint() {
            let mut sel = Selection::new(SelectionMode::Single);
            sel.select(2);
            sel.extend_to(7);
            assert_eq!(sel.count(), 1);
            assert!(sel.is_selected(7));
        }

        #[test]
        fn select_all_and_clear() {
            let mut sel = Selection::new(SelectionMode::Multi);
            sel.select_all(10);
            assert_eq!(sel.count(), 10);
            sel.clear();
            assert_eq!(sel.count(), 0);
        }

        #[test]
        fn selected_indices_returns_ordered_set() {
            let mut sel = Selection::new(SelectionMode::Multi);
            sel.select(5);
            sel.select(1);
            sel.select(3);
            let indices: Vec<_> = sel.selected_indices().iter().copied().collect();
            assert_eq!(indices, vec![1, 3, 5]);
        }

        #[test]
        fn empty_selection_defaults() {
            let sel = Selection::new(SelectionMode::Single);
            assert_eq!(sel.count(), 0);
            assert!(!sel.is_selected(0));
            assert!(sel.selected_indices().is_empty());
        }

        #[test]
        fn oversized_range_and_select_all_fail_without_mutation() {
            let mut sel = Selection::new(SelectionMode::Range);
            sel.select(7);
            assert!(sel.try_extend_to(usize::MAX).is_err());
            assert_eq!(sel.selected_indices(), &BTreeSet::from([7]));
            assert!(sel.try_select_all(MAX_SELECTION_ITEMS + 1).is_err());
            assert_eq!(sel.selected_indices(), &BTreeSet::from([7]));
        }

        #[test]
        fn malformed_serialized_selection_is_rejected() {
            let json = r#"{
                "mode":"Single","selected":[1,2],"anchor":1,"pivot":2
            }"#;
            assert!(serde_json::from_str::<Selection>(json).is_err());
        }
    }

    mod collection_diff_tests {
        use super::*;

        #[test]
        fn new_diff_is_empty() {
            let diff = CollectionDiff::new();
            assert!(diff.is_empty());
            assert_eq!(diff.len(), 0);
        }

        #[test]
        fn push_and_query() {
            let mut diff = CollectionDiff::new();
            diff.push(CollectionChange::Insert { index: 0 });
            diff.push(CollectionChange::Remove { index: 5 });
            diff.push(CollectionChange::Update { index: 3 });
            diff.push(CollectionChange::Move { from: 1, to: 4 });
            diff.push(CollectionChange::Reset);
            assert_eq!(diff.len(), 5);
            assert!(!diff.is_empty());
            assert_eq!(diff.changes()[0], CollectionChange::Insert { index: 0 });
        }

        #[test]
        fn diff_capacity_is_checked() {
            let mut diff = CollectionDiff {
                changes: vec![CollectionChange::Reset; MAX_COLLECTION_CHANGES],
            };
            assert!(diff.try_push(CollectionChange::Reset).is_err());
            assert_eq!(diff.len(), MAX_COLLECTION_CHANGES);
        }
    }

    mod visible_range_tests {
        use super::*;

        #[test]
        fn contains_checks_exclusive_end() {
            let vr = VisibleRange::new(10, 20, 5);
            assert!(!vr.contains(9));
            assert!(vr.contains(10));
            assert!(vr.contains(19));
            assert!(!vr.contains(20));
        }

        #[test]
        fn prefetch_range_expands_both_directions() {
            let vr = VisibleRange::new(10, 20, 5);
            assert_eq!(vr.prefetch_range(), (5, 25));
        }

        #[test]
        fn prefetch_clamps_at_zero() {
            let vr = VisibleRange::new(2, 8, 10);
            assert_eq!(vr.prefetch_range(), (0, 18));
        }

        #[test]
        fn len_and_is_empty() {
            let vr = VisibleRange::new(5, 5, 0);
            assert_eq!(vr.len(), 0);
            assert!(vr.is_empty());

            let vr2 = VisibleRange::new(0, 10, 0);
            assert_eq!(vr2.len(), 10);
            assert!(!vr2.is_empty());
        }

        #[test]
        fn set_range_updates() {
            let mut vr = VisibleRange::new(0, 10, 3);
            vr.set_range(20, 30);
            assert!(vr.contains(25));
            assert!(!vr.contains(10));
        }

        #[test]
        fn end_clamped_to_start() {
            let vr = VisibleRange::new(10, 5, 0);
            assert_eq!(vr.len(), 0);
            assert!(vr.is_empty());
        }

        #[test]
        fn serialized_range_must_be_ordered_and_known() {
            assert!(
                serde_json::from_str::<VisibleRange>(r#"{"start":10,"end":2,"prefetch":0}"#)
                    .is_err()
            );
            assert!(
                serde_json::from_str::<VisibleRange>(
                    r#"{"start":0,"end":2,"prefetch":0,"extra":1}"#
                )
                .is_err()
            );
        }
    }

    mod virtual_table_tests {
        use super::*;

        fn sample_columns() -> Vec<ColumnDescriptor> {
            vec![
                ColumnDescriptor {
                    id: "name".into(),
                    label: "Name".into(),
                    width: 200.0,
                    min_width: 50.0,
                    max_width: 500.0,
                    resizable: true,
                    sortable: true,
                },
                ColumnDescriptor {
                    id: "size".into(),
                    label: "Size".into(),
                    width: 100.0,
                    min_width: 60.0,
                    max_width: 200.0,
                    resizable: false,
                    sortable: true,
                },
            ]
        }

        #[test]
        fn basic_construction() {
            let table = VirtualTableModel::new(sample_columns(), 100);
            assert_eq!(table.columns().len(), 2);
            assert_eq!(table.row_count(), 100);
            assert!(table.sort().is_none());
        }

        #[test]
        fn set_and_get_sort() {
            let mut table = VirtualTableModel::new(sample_columns(), 50);
            table.set_sort(Some(TableSort {
                column_id: "name".into(),
                direction: SortDirection::Ascending,
            }));
            let sort = table.sort().unwrap();
            assert_eq!(sort.column_id, "name");
            assert_eq!(sort.direction, SortDirection::Ascending);
        }

        #[test]
        fn resize_column_clamps() {
            let mut table = VirtualTableModel::new(sample_columns(), 10);
            table.resize_column("name", 1000.0).unwrap();
            assert_eq!(table.columns()[0].width, 500.0);

            table.resize_column("name", 10.0).unwrap();
            assert_eq!(table.columns()[0].width, 50.0);
        }

        #[test]
        fn resize_nonexistent_column_errors() {
            let mut table = VirtualTableModel::new(sample_columns(), 10);
            assert!(table.resize_column("missing", 100.0).is_err());
        }

        #[test]
        fn resize_non_resizable_errors() {
            let mut table = VirtualTableModel::new(sample_columns(), 10);
            assert!(table.resize_column("size", 100.0).is_err());
        }

        #[test]
        fn selection_and_visible_range_access() {
            let mut table = VirtualTableModel::new(sample_columns(), 100);
            table.selection_mut().select(5);
            assert!(table.selection().is_selected(5));

            table.visible_range_mut().set_range(10, 30);
            assert!(table.visible_range().contains(20));
        }

        #[test]
        fn set_row_count() {
            let mut table = VirtualTableModel::new(sample_columns(), 0);
            table.set_row_count(999);
            assert_eq!(table.row_count(), 999);
        }

        #[test]
        fn invalid_columns_sorts_and_resize_values_are_rejected() {
            let mut duplicate = sample_columns();
            duplicate[1].id = duplicate[0].id.clone();
            assert!(VirtualTableModel::try_new(duplicate, 1).is_err());

            let mut invalid = sample_columns();
            invalid[0].min_width = 500.0;
            invalid[0].max_width = 50.0;
            assert!(VirtualTableModel::try_new(invalid, 1).is_err());

            let mut table = VirtualTableModel::new(sample_columns(), 1);
            assert!(table.resize_column("name", f32::NAN).is_err());
            assert!(
                table
                    .try_set_sort(Some(TableSort {
                        column_id: "missing".into(),
                        direction: SortDirection::Ascending,
                    }))
                    .is_err()
            );
            assert!(table.sort().is_none());
        }

        #[test]
        fn serialized_table_is_validated() {
            let table = VirtualTableModel::new(sample_columns(), 10);
            let mut value = serde_json::to_value(table).unwrap();
            value["columns"][0]["min_width"] = serde_json::json!(900.0);
            assert!(serde_json::from_value::<VirtualTableModel>(value).is_err());
        }
    }

    mod tree_model_tests {
        use super::*;

        fn sample_tree() -> TreeModel<&'static str> {
            let mut tree = TreeModel::new();
            tree.add_root(TreeNode {
                data: "root1",
                children: vec![
                    TreeNode {
                        data: "child1",
                        children: vec![],
                        expanded: false,
                        depth: 1,
                    },
                    TreeNode {
                        data: "child2",
                        children: vec![TreeNode {
                            data: "grandchild",
                            children: vec![],
                            expanded: false,
                            depth: 2,
                        }],
                        expanded: true,
                        depth: 1,
                    },
                ],
                expanded: true,
                depth: 0,
            });
            tree.add_root(TreeNode {
                data: "root2",
                children: vec![],
                expanded: false,
                depth: 0,
            });
            tree
        }

        #[test]
        fn flatten_respects_expanded() {
            let tree = sample_tree();
            let flat = tree.flatten();
            let labels: Vec<_> = flat.iter().map(|f| *f.data).collect();
            assert_eq!(
                labels,
                vec!["root1", "child1", "child2", "grandchild", "root2"]
            );
        }

        #[test]
        fn flatten_indices_are_sequential() {
            let tree = sample_tree();
            let flat = tree.flatten();
            for (i, item) in flat.iter().enumerate() {
                assert_eq!(item.index, i);
            }
        }

        #[test]
        fn flatten_has_children_flag() {
            let tree = sample_tree();
            let flat = tree.flatten();
            assert!(flat[0].has_children);
            assert!(!flat[1].has_children);
            assert!(flat[2].has_children);
            assert!(!flat[3].has_children);
            assert!(!flat[4].has_children);
        }

        #[test]
        fn total_visible_count_matches_flatten() {
            let tree = sample_tree();
            assert_eq!(tree.total_visible_count(), tree.flatten().len());
        }

        #[test]
        fn toggle_expanded_collapses_node() {
            let mut tree = sample_tree();
            let new_state = tree.toggle_expanded(&[0]);
            assert!(!new_state);
            let flat = tree.flatten();
            let labels: Vec<_> = flat.iter().map(|f| *f.data).collect();
            assert_eq!(labels, vec!["root1", "root2"]);
        }

        #[test]
        fn toggle_expanded_on_child() {
            let mut tree = sample_tree();
            let new_state = tree.toggle_expanded(&[0, 1]);
            assert!(!new_state);
            let flat = tree.flatten();
            let labels: Vec<_> = flat.iter().map(|f| *f.data).collect();
            assert_eq!(labels, vec!["root1", "child1", "child2", "root2"]);
        }

        #[test]
        fn toggle_expanded_invalid_path_returns_false() {
            let mut tree = sample_tree();
            assert!(!tree.toggle_expanded(&[99]));
            assert!(!tree.toggle_expanded(&[]));
        }

        #[test]
        fn empty_tree() {
            let tree: TreeModel<i32> = TreeModel::new();
            assert!(tree.roots().is_empty());
            assert!(tree.flatten().is_empty());
            assert_eq!(tree.total_visible_count(), 0);
        }

        #[test]
        fn excessive_or_inconsistent_tree_depth_is_rejected() {
            let mut node = TreeNode {
                data: 0,
                children: Vec::new(),
                expanded: true,
                depth: MAX_TREE_DEPTH,
            };
            for depth in (0..MAX_TREE_DEPTH).rev() {
                node = TreeNode {
                    data: depth,
                    children: vec![node],
                    expanded: true,
                    depth,
                };
            }
            let mut tree = TreeModel::new();
            assert!(tree.try_add_root(node).is_err());
            assert!(tree.roots().is_empty());

            assert!(
                tree.try_add_root(TreeNode {
                    data: 1,
                    children: vec![TreeNode {
                        data: 2,
                        children: Vec::new(),
                        expanded: false,
                        depth: 7,
                    }],
                    expanded: true,
                    depth: 0,
                })
                .is_err()
            );
            assert!(!tree.toggle_expanded(&vec![0; MAX_TREE_DEPTH + 1]));
        }
    }

    mod virtual_data_source_tests {
        use super::*;

        struct VecSource(Vec<String>);

        impl VirtualDataSource for VecSource {
            type Item = String;

            fn len(&self) -> usize {
                self.0.len()
            }

            fn item_at(&self, index: usize) -> Option<&String> {
                self.0.get(index)
            }
        }

        #[test]
        fn basic_source() {
            let src = VecSource(vec!["a".into(), "b".into(), "c".into()]);
            assert_eq!(src.len(), 3);
            assert!(!src.is_empty());
            assert_eq!(src.item_at(0).unwrap(), "a");
            assert_eq!(src.item_at(2).unwrap(), "c");
            assert!(src.item_at(3).is_none());
        }

        #[test]
        fn empty_source() {
            let src = VecSource(vec![]);
            assert_eq!(src.len(), 0);
            assert!(src.is_empty());
            assert!(src.item_at(0).is_none());
        }
    }
}

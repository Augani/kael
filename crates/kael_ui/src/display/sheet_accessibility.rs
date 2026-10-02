//! Bounded semantic metadata for a logical sheet coordinate space.
use super::virtual_sheet_grid::{SheetCellPosition, VirtualSheetGridError};
use kael::*;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Cached cell semantics are bounded independently of the source tile cache.
pub const VIRTUAL_SHEET_ACCESSIBILITY_CACHE_CELLS: usize = 1_024;
pub(crate) const CELL_PREVIEW_BYTES: usize = 4_096;

#[derive(Clone, Copy)]
pub(crate) struct SheetAccessibilityIds {
    range: AccessibilityIdRange,
    rows: usize,
    columns: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SheetSemanticTarget {
    Root,
    HeaderRow,
    Header(usize),
    Row(usize),
    Cell(SheetCellPosition),
}
impl SheetAccessibilityIds {
    pub fn new(rows: usize, columns: usize) -> Result<Self, VirtualSheetGridError> {
        let count = (rows as u64)
            .checked_mul(columns as u64 + 1)
            .and_then(|cells| cells.checked_add(columns as u64 + 2))
            .ok_or(VirtualSheetGridError::AllocationFailed)?;
        Ok(Self {
            range: AccessibilityId::reserve_range(count)
                .ok_or(VirtualSheetGridError::AllocationFailed)?,
            rows,
            columns,
        })
    }
    pub fn root(self) -> AccessibilityId {
        self.range.get(0).unwrap()
    }
    pub fn header_row(self) -> AccessibilityId {
        self.range.get(1).unwrap()
    }
    pub fn header(self, column: usize) -> Option<AccessibilityId> {
        (column < self.columns)
            .then(|| self.range.get(column as u64 + 2))
            .flatten()
    }
    pub fn row(self, row: usize) -> Option<AccessibilityId> {
        (row < self.rows)
            .then(|| {
                self.range
                    .get(2 + self.columns as u64 + row as u64 * (self.columns as u64 + 1))
            })
            .flatten()
    }
    pub fn cell(self, position: SheetCellPosition) -> Option<AccessibilityId> {
        (position.row < self.rows && position.column < self.columns)
            .then(|| {
                self.range.get(
                    3 + self.columns as u64
                        + position.row as u64 * (self.columns as u64 + 1)
                        + position.column as u64,
                )
            })
            .flatten()
    }
    pub fn resolve(self, id: AccessibilityId) -> Option<SheetSemanticTarget> {
        let offset = id.0.checked_sub(self.root().0)?;
        if self.range.get(offset) != Some(id) {
            return None;
        }
        match offset {
            0 => Some(SheetSemanticTarget::Root),
            1 => Some(SheetSemanticTarget::HeaderRow),
            offset if offset < self.columns as u64 + 2 => {
                Some(SheetSemanticTarget::Header(offset as usize - 2))
            }
            offset => {
                let relative = offset - self.columns as u64 - 2;
                let row = (relative / (self.columns as u64 + 1)) as usize;
                let column = (relative % (self.columns as u64 + 1)) as usize;
                if column == 0 {
                    Some(SheetSemanticTarget::Row(row))
                } else {
                    Some(SheetSemanticTarget::Cell(SheetCellPosition::new(
                        row,
                        column - 1,
                    )))
                }
            }
        }
    }
}

pub(crate) fn cell_node(
    ids: SheetAccessibilityIds,
    position: SheetCellPosition,
    value: Option<&str>,
    header: Option<&str>,
) -> AccessibilityNode {
    let column = header
        .map(str::to_owned)
        .unwrap_or_else(|| super::virtual_sheet_grid::column_label(position.column));
    let mut node = AccessibilityNode::new(AccessibilityRole::Cell)
        .with_label(format!("{column}, row {}", position.row + 1))
        .with_actions(vec![
            AccessibilityAction::Focus,
            AccessibilityAction::Click,
            AccessibilityAction::ScrollToVisible,
            AccessibilityAction::SetValue,
        ]);
    node.id = ids.cell(position).unwrap();
    node.parent = ids.row(position.row);
    node.row_index = Some(position.row + 2);
    node.column_index = Some(position.column + 1);
    if let Some(value) = value {
        let mut end = value.len().min(CELL_PREVIEW_BYTES);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        let mut preview = value[..end].to_owned();
        if end < value.len() {
            preview.push('…');
            node.description = Some(
                "Long value preview. Enter edit mode to read or select the complete cell value."
                    .into(),
            );
        }
        node.value = Some(AccessibilityValue::Text(preview));
    } else {
        node.states |= AccessibilityState::BUSY;
        node.description = Some("Value has not been loaded. Reveal this cell to fetch it.".into());
    }
    node
}
pub(crate) fn row_node(ids: SheetAccessibilityIds, row: usize) -> AccessibilityNode {
    let mut node = AccessibilityNode::new(AccessibilityRole::Row)
        .with_label(format!("Row {}", row + 1))
        .with_actions(vec![
            AccessibilityAction::Focus,
            AccessibilityAction::Click,
            AccessibilityAction::ScrollToVisible,
        ]);
    node.id = ids.row(row).unwrap();
    node.parent = Some(ids.root());
    node.row_index = Some(row + 2);
    node
}

pub(crate) fn prepare_snapshot(
    ids: SheetAccessibilityIds,
    headers: Arc<std::collections::HashMap<usize, SharedString>>,
    cells: Vec<(SheetCellPosition, SharedString)>,
    executor: &BackgroundExecutor,
) -> Arc<AccessibilitySnapshot> {
    let mut root = AccessibilityNode::new(AccessibilityRole::Grid)
        .with_label("Spreadsheet grid")
        .with_actions(vec![AccessibilityAction::Focus]);
    root.id = ids.root();
    root.row_count = Some(ids.rows + 1);
    root.column_count = Some(ids.columns);
    let mut header_row = AccessibilityNode::new(AccessibilityRole::Row);
    header_row.id = ids.header_row();
    header_row.parent = Some(root.id);
    header_row.row_index = Some(1);
    let mut nodes = Vec::with_capacity(ids.columns + cells.len() * 2 + 2);
    for column in 0..ids.columns {
        let label = headers
            .get(&column)
            .map(|header| header.to_string())
            .unwrap_or_else(|| super::virtual_sheet_grid::column_label(column));
        let mut header = AccessibilityNode::new(AccessibilityRole::ColumnHeader)
            .with_label(label)
            .with_actions(vec![AccessibilityAction::ScrollToVisible]);
        header.id = ids.header(column).unwrap();
        header.parent = Some(header_row.id);
        header.column_index = Some(column + 1);
        header.row_index = Some(1);
        header_row.children.push(header.id);
        nodes.push(header);
    }
    root.children.push(header_row.id);
    nodes.push(header_row);
    let mut rows = BTreeMap::<usize, AccessibilityNode>::new();
    for (position, value) in cells {
        let cell = cell_node(
            ids,
            position,
            Some(&value),
            headers.get(&position.column).map(|label| label.as_ref()),
        );
        rows.entry(position.row)
            .or_insert_with(|| row_node(ids, position.row))
            .children
            .push(cell.id);
        nodes.push(cell);
    }
    for row in rows.into_values() {
        root.children.push(row.id);
        nodes.push(row);
    }
    nodes.push(root);
    AccessibilitySnapshot::with_reclaim_executor(ids.root(), nodes, executor)
        .expect("checked coordinate IDs and row-major bounded cells form a valid subtree")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn maximum_sheet_coordinate_ids_need_constant_storage_and_never_alias_new_models() {
        let first = SheetAccessibilityIds::new(1_000_000, 16_384).unwrap();
        let last = SheetCellPosition::new(999_999, 16_383);
        let id = first.cell(last).unwrap();
        assert_eq!(first.resolve(id), Some(SheetSemanticTarget::Cell(last)));
        assert_eq!(
            first.resolve(first.row(999_999).unwrap()),
            Some(SheetSemanticTarget::Row(999_999))
        );
        assert!(first.cell(SheetCellPosition::new(1_000_000, 0)).is_none());
        assert!(first.header(16_384).is_none());
        let next = SheetAccessibilityIds::new(1_000_000, 16_384).unwrap();
        assert_eq!(next.resolve(id), None);
        assert_ne!(next.cell(last), Some(id));
        assert!(std::mem::size_of::<SheetAccessibilityIds>() <= 32);
    }
}

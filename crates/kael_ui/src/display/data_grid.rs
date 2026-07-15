use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};
use std::collections::HashMap;
use std::{panic::Location, rc::Rc};

#[derive(Clone, Debug, PartialEq)]
pub enum CellEditor {
    Text,
    Number,
    Checkbox,
    Custom,
}

impl CellEditor {
    /// Stable editor key for content-safe diagnostics.
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Number => "number",
            Self::Checkbox => "checkbox",
            Self::Custom => "custom",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kael::{div, px, IntoElement, TestAppContext};

    #[derive(Clone)]
    struct PrivateGridRow {
        name: String,
        amount: String,
        enabled: bool,
    }

    fn private_grid_columns() -> Vec<GridColumnDef<PrivateGridRow>> {
        vec![
            GridColumnDef::new(
                "private-name",
                "Secret Account",
                |row: &PrivateGridRow, _| div().child(row.name.clone()).into_any_element(),
                |row: &PrivateGridRow| row.name.clone(),
            )
            .width(px(240.0))
            .min_width(px(120.0))
            .sortable(true)
            .editable(true)
            .value_setter(|row, value| row.name = value.to_string()),
            GridColumnDef::new(
                "private-amount",
                "Private Amount",
                |row: &PrivateGridRow, _| div().child(row.amount.clone()).into_any_element(),
                |row: &PrivateGridRow| row.amount.clone(),
            )
            .max_width(px(320.0))
            .resizable(false)
            .sortable(true)
            .editor(CellEditor::Number),
            GridColumnDef::new(
                "private-enabled",
                "Secret Enabled",
                |row: &PrivateGridRow, _| div().child(row.enabled.to_string()).into_any_element(),
                |row: &PrivateGridRow| row.enabled.to_string(),
            )
            .editable(true)
            .editor(CellEditor::Checkbox),
        ]
    }

    #[::core::prelude::v1::test]
    fn data_grid_column_summary_is_content_safe() {
        let column = GridColumnDef::new(
            "private-name",
            "Secret Account",
            |row: &PrivateGridRow, _| div().child(row.name.clone()).into_any_element(),
            |row: &PrivateGridRow| row.name.clone(),
        )
        .width(px(240.0))
        .min_width(px(120.0))
        .max_width(px(360.0))
        .sortable(true)
        .editable(true)
        .editor(CellEditor::Custom)
        .value_setter(|row, value| row.name = value.to_string());

        assert_eq!(CellEditor::Custom.to_text(), "custom");
        assert_eq!(column.id_len_bytes(), "private-name".len());
        assert_eq!(column.header_len_bytes(), "Secret Account".len());
        assert_eq!(column.width_class(), "wide");
        assert!(column.has_min_width());
        assert!(column.has_max_width());
        assert!(column.has_value_setter());
        assert_eq!(column.editor_key(), "custom");

        let summary = column.to_text();
        assert!(summary.contains("sortable=true"));
        assert!(summary.contains("editable=true"));
        assert!(summary.contains("editor=custom"));
        assert!(!summary.contains("private-name"));
        assert!(!summary.contains("Secret Account"));
        assert!(!summary.contains("240"));
        assert!(!summary.contains("360"));
    }

    #[::core::prelude::v1::test]
    fn data_grid_state_summary_is_content_safe() {
        let mut state = DataGridState::new(
            vec![
                PrivateGridRow {
                    name: "Acme Confidential".to_string(),
                    amount: "$42,000".to_string(),
                    enabled: true,
                },
                PrivateGridRow {
                    name: "Zenith Private".to_string(),
                    amount: "$7,000".to_string(),
                    enabled: false,
                },
            ],
            private_grid_columns(),
        );

        state.sort_by_column("private-name");
        state.start_editing(CellPosition { row: 0, col: 0 });
        state.selected_cells.push(CellPosition { row: 1, col: 2 });
        state.resize_column("private-name", px(280.0));

        assert_eq!(GridSortDirection::Ascending.to_text(), "ascending");
        assert_eq!(state.row_count(), 2);
        assert_eq!(state.column_count(), 3);
        assert!(state.has_editing_cell());
        assert_eq!(state.edit_value_len_bytes(), "Acme Confidential".len());
        assert_eq!(state.selected_cell_count(), 1);
        assert!(state.has_sort());
        assert_eq!(state.sort_column_len_bytes(), "private-name".len());
        assert_eq!(state.sort_direction_key(), "ascending");
        assert_eq!(state.sortable_column_count(), 2);
        assert_eq!(state.editable_column_count(), 2);
        assert_eq!(state.resizable_column_count(), 2);
        assert_eq!(state.value_setter_count(), 1);

        let summary = state.to_text();
        assert!(summary.contains("rows=2"));
        assert!(summary.contains("columns=3"));
        assert!(summary.contains("editing_cell=true"));
        assert!(summary.contains("selected_cells=1"));
        assert!(!summary.contains("Acme Confidential"));
        assert!(!summary.contains("Zenith Private"));
        assert!(!summary.contains("private-name"));
        assert!(!summary.contains("Secret Account"));
        assert!(!summary.contains("$42,000"));
        assert!(!summary.contains("280"));
    }

    #[::core::prelude::v1::test]
    fn data_grid_summary_is_content_safe() {
        let cx = TestAppContext::single();
        let state = cx.update(|cx| {
            cx.new(|_| {
                DataGridState::new(
                    vec![PrivateGridRow {
                        name: "Delta Hidden".to_string(),
                        amount: "$99,000".to_string(),
                        enabled: true,
                    }],
                    private_grid_columns(),
                )
            })
        });

        let grid = DataGrid::new(state.clone())
            .striped(true)
            .bordered(false)
            .compact(true);

        cx.update(|cx| {
            state.update(cx, |state, _| {
                state.start_editing(CellPosition { row: 0, col: 0 });
            });

            let summary = grid.to_text(cx);
            assert!(summary.contains("data_grid("));
            assert!(summary.contains("rows=1"));
            assert!(summary.contains("striped=true"));
            assert!(summary.contains("bordered=false"));
            assert!(summary.contains("compact=true"));
            assert!(!summary.contains("Delta Hidden"));
            assert!(!summary.contains("$99,000"));
            assert!(!summary.contains("private-name"));
            assert!(!summary.contains("Secret Account"));
        });
    }

    #[::core::prelude::v1::test]
    fn data_grid_selection_and_typed_sorting_are_consistent() {
        let mut state = DataGridState::new(
            vec![
                PrivateGridRow {
                    name: "Ten".into(),
                    amount: "10".into(),
                    enabled: true,
                },
                PrivateGridRow {
                    name: "Two".into(),
                    amount: "2".into(),
                    enabled: false,
                },
            ],
            private_grid_columns(),
        );

        state.sort_by_column("private-amount");
        assert_eq!(state.data()[0].amount, "2");
        state.sort_by_column("private-amount");
        assert_eq!(state.data()[0].amount, "10");

        state.select_cell(CellPosition { row: 0, col: 0 });
        state.move_selection(1, 2);
        assert_eq!(state.selected_cells(), &[CellPosition { row: 1, col: 2 }]);

        state.start_editing(CellPosition { row: 1, col: 1 });
        assert!(!state.has_editing_cell());
        state.select_cell(CellPosition { row: 99, col: 99 });
        assert_eq!(state.selected_cells(), &[CellPosition { row: 1, col: 2 }]);
    }

    #[::core::prelude::v1::test]
    fn sorting_preserves_selected_and_edited_records() {
        let mut state = DataGridState::new(
            vec![
                PrivateGridRow {
                    name: "Bravo".into(),
                    amount: "20".into(),
                    enabled: true,
                },
                PrivateGridRow {
                    name: "Alpha".into(),
                    amount: "10".into(),
                    enabled: false,
                },
            ],
            private_grid_columns(),
        );
        state.select_cell(CellPosition { row: 0, col: 2 });
        state.start_editing(CellPosition { row: 0, col: 0 });

        state.sort_by_column("private-name");

        assert_eq!(state.data()[1].name, "Bravo");
        assert_eq!(state.selected_cells(), &[CellPosition { row: 1, col: 2 }]);
        assert_eq!(state.editing_cell, Some(CellPosition { row: 1, col: 0 }));

        state.sort_by_column("private-enabled");
        assert_eq!(state.sort_column, Some(SharedString::from("private-name")));
        state.sort_by_column("missing");
        assert_eq!(state.sort_column, Some(SharedString::from("private-name")));
    }

    #[::core::prelude::v1::test]
    fn grid_widths_and_replacement_data_are_normalized() {
        let columns = vec![GridColumnDef::new(
            "name",
            "Name",
            |row: &PrivateGridRow, _| div().child(row.name.clone()).into_any_element(),
            |row: &PrivateGridRow| row.name.clone(),
        )
        .width(px(f32::NAN))
        .min_width(px(120.0))
        .max_width(px(200.0))];
        let mut state = DataGridState::new(
            vec![PrivateGridRow {
                name: "Old".into(),
                amount: "1".into(),
                enabled: true,
            }],
            columns,
        );
        assert_eq!(state.column_widths["name"], px(150.0));
        state.resize_column("name", px(40.0));
        assert_eq!(state.column_widths["name"], px(120.0));
        state.resize_column("name", px(400.0));
        assert_eq!(state.column_widths["name"], px(200.0));
        state.resize_column("name", px(f32::INFINITY));
        assert_eq!(state.column_widths["name"], px(200.0));

        state.select_cell(CellPosition { row: 0, col: 0 });
        state.set_data(vec![PrivateGridRow {
            name: "New".into(),
            amount: "2".into(),
            enabled: false,
        }]);
        assert!(state.selected_cells().is_empty());
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellPosition {
    pub row: usize,
    pub col: usize,
}

impl CellPosition {
    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "cell_position(row_present={}, col_present=true)",
            self.row != usize::MAX
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum GridSortDirection {
    Ascending,
    Descending,
    None,
}

impl GridSortDirection {
    /// Stable sort direction key for content-safe diagnostics.
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::Ascending => "ascending",
            Self::Descending => "descending",
            Self::None => "none",
        }
    }
}

pub struct GridColumnDef<T: 'static> {
    pub id: SharedString,
    pub header: SharedString,
    pub width: Pixels,
    pub min_width: Option<Pixels>,
    pub max_width: Option<Pixels>,
    pub resizable: bool,
    pub sortable: bool,
    pub editable: bool,
    pub editor: CellEditor,
    pub cell_renderer: Rc<dyn Fn(&T, usize) -> AnyElement>,
    pub value_getter: Rc<dyn Fn(&T) -> String>,
    pub value_setter: Option<Rc<dyn Fn(&mut T, &str)>>,
}

impl<T: 'static> GridColumnDef<T> {
    pub fn new<S: Into<SharedString>>(
        id: S,
        header: S,
        renderer: impl Fn(&T, usize) -> AnyElement + 'static,
        getter: impl Fn(&T) -> String + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            header: header.into(),
            width: px(150.0),
            min_width: None,
            max_width: None,
            resizable: true,
            sortable: false,
            editable: false,
            editor: CellEditor::Text,
            cell_renderer: Rc::new(renderer),
            value_getter: Rc::new(getter),
            value_setter: None,
        }
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = valid_grid_width(width).unwrap_or(px(150.0));
        self
    }

    pub fn min_width(mut self, width: Pixels) -> Self {
        self.min_width = valid_grid_width(width);
        self
    }

    pub fn max_width(mut self, width: Pixels) -> Self {
        self.max_width = valid_grid_width(width);
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn sortable(mut self, sortable: bool) -> Self {
        self.sortable = sortable;
        self
    }

    pub fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }

    pub fn editor(mut self, editor: CellEditor) -> Self {
        self.editor = editor;
        self
    }

    pub fn value_setter(mut self, setter: impl Fn(&mut T, &str) + 'static) -> Self {
        self.value_setter = Some(Rc::new(setter));
        self
    }

    /// Returns the column id length without exposing the id.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Returns the header length without exposing header text.
    pub fn header_len_bytes(&self) -> usize {
        self.header.len()
    }

    /// Coarse width class for content-safe diagnostics.
    pub fn width_class(&self) -> &'static str {
        let width: f32 = self.width.into();
        if width <= 96.0 {
            "compact"
        } else if width <= 180.0 {
            "standard"
        } else if width <= 320.0 {
            "wide"
        } else {
            "extra_wide"
        }
    }

    /// Returns true when a minimum width is configured.
    pub fn has_min_width(&self) -> bool {
        self.min_width.is_some()
    }

    /// Returns true when a maximum width is configured.
    pub fn has_max_width(&self) -> bool {
        self.max_width.is_some()
    }

    /// Returns true when a value setter is configured.
    pub fn has_value_setter(&self) -> bool {
        self.value_setter.is_some()
    }

    /// Stable editor key for content-safe diagnostics.
    pub fn editor_key(&self) -> &'static str {
        self.editor.to_text()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "grid_column(id_len_bytes={}, header_len_bytes={}, width_class={}, min_width={}, max_width={}, resizable={}, sortable={}, editable={}, editor={}, value_setter={})",
            self.id_len_bytes(),
            self.header_len_bytes(),
            self.width_class(),
            self.has_min_width(),
            self.has_max_width(),
            self.resizable,
            self.sortable,
            self.editable,
            self.editor_key(),
            self.has_value_setter()
        )
    }
}

fn valid_grid_width(width: Pixels) -> Option<Pixels> {
    let value = f32::from(width);
    (value.is_finite() && value > 0.0).then_some(width)
}

fn clamped_grid_width<T>(column: &GridColumnDef<T>, width: Pixels) -> Pixels {
    let minimum = column.min_width.and_then(valid_grid_width);
    let maximum = column.max_width.and_then(valid_grid_width).map(|maximum| {
        if let Some(minimum) = minimum {
            maximum.max(minimum)
        } else {
            maximum
        }
    });
    let mut width = valid_grid_width(width).unwrap_or_else(|| {
        valid_grid_width(column.width)
            .or(minimum)
            .unwrap_or(px(150.0))
    });
    if let Some(minimum) = minimum {
        width = width.max(minimum);
    }
    if let Some(maximum) = maximum {
        width = width.min(maximum);
    }
    width
}

#[allow(dead_code)]
pub struct DataGridState<T: 'static> {
    data: Vec<T>,
    columns: Vec<GridColumnDef<T>>,
    editing_cell: Option<CellPosition>,
    edit_value: String,
    selected_cells: Vec<CellPosition>,
    column_widths: HashMap<SharedString, Pixels>,
    sort_column: Option<SharedString>,
    sort_direction: GridSortDirection,
    scroll_handle: ScrollHandle,
    focus_handle: Option<FocusHandle>,
    resizing_column: Option<usize>,
    resize_start_x: f32,
    resize_start_width: Pixels,
}

impl<T: 'static> DataGridState<T> {
    pub fn new(data: Vec<T>, columns: Vec<GridColumnDef<T>>) -> Self {
        let column_widths = columns
            .iter()
            .map(|column| (column.id.clone(), clamped_grid_width(column, column.width)))
            .collect();
        Self {
            data,
            columns,
            editing_cell: None,
            edit_value: String::new(),
            selected_cells: Vec::new(),
            column_widths,
            sort_column: None,
            sort_direction: GridSortDirection::None,
            scroll_handle: ScrollHandle::new(),
            focus_handle: None,
            resizing_column: None,
            resize_start_x: 0.0,
            resize_start_width: px(0.0),
        }
    }

    pub fn data(&self) -> &[T] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut Vec<T> {
        &mut self.data
    }

    pub fn start_editing(&mut self, pos: CellPosition) {
        if pos.col >= self.columns.len() || pos.row >= self.data.len() {
            return;
        }
        if !self.columns[pos.col].editable || self.columns[pos.col].value_setter.is_none() {
            return;
        }
        self.edit_value = (self.columns[pos.col].value_getter)(&self.data[pos.row]);
        self.editing_cell = Some(pos);
    }

    pub fn commit_edit(&mut self) {
        if let Some(pos) = self.editing_cell.take() {
            if let Some(col) = self.columns.get(pos.col) {
                if let Some(ref setter) = col.value_setter {
                    if let Some(row) = self.data.get_mut(pos.row) {
                        setter(row, &self.edit_value);
                    }
                }
            }
            self.edit_value.clear();
        }
    }

    pub fn cancel_edit(&mut self) {
        self.editing_cell = None;
        self.edit_value.clear();
    }

    pub fn move_edit_next(&mut self) {
        let current = match self.editing_cell.take() {
            Some(pos) => pos,
            None => return,
        };
        if let Some(col) = self.columns.get(current.col) {
            if let Some(ref setter) = col.value_setter {
                if let Some(row) = self.data.get_mut(current.row) {
                    setter(row, &self.edit_value);
                }
            }
        }
        self.edit_value.clear();

        let num_cols = self.columns.len();
        let num_rows = self.data.len();
        let mut row = current.row;
        let mut col = current.col + 1;
        while row < num_rows {
            while col < num_cols {
                if self.columns[col].editable && self.columns[col].value_setter.is_some() {
                    self.start_editing(CellPosition { row, col });
                    return;
                }
                col += 1;
            }
            col = 0;
            row += 1;
        }
    }

    pub fn sort_by_column(&mut self, col_id: &str) {
        let col_id_shared: SharedString = SharedString::from(col_id.to_string());
        let Some(col_idx) = self
            .columns
            .iter()
            .position(|column| column.id == col_id_shared && column.sortable)
        else {
            return;
        };

        if self.sort_column.as_ref() == Some(&col_id_shared) {
            self.sort_direction = match self.sort_direction {
                GridSortDirection::Ascending => GridSortDirection::Descending,
                GridSortDirection::Descending => GridSortDirection::Ascending,
                GridSortDirection::None => GridSortDirection::Ascending,
            };
        } else {
            self.sort_column = Some(col_id_shared.clone());
            self.sort_direction = GridSortDirection::Ascending;
        }

        self.apply_sort(col_idx);
    }

    fn apply_sort(&mut self, col_idx: usize) {
        let getter = self.columns[col_idx].value_getter.clone();
        let editor = self.columns[col_idx].editor.clone();
        let ascending = self.sort_direction == GridSortDirection::Ascending;
        let mut indexed_data: Vec<(usize, T)> = std::mem::take(&mut self.data)
            .into_iter()
            .enumerate()
            .collect();
        indexed_data.sort_by(|(_, a), (_, b)| {
            let va = getter(a);
            let vb = getter(b);
            let ordering = match editor {
                CellEditor::Number => match (va.parse::<f64>(), vb.parse::<f64>()) {
                    (Ok(a), Ok(b)) => a.total_cmp(&b),
                    _ => va.cmp(&vb),
                },
                CellEditor::Checkbox => va
                    .parse::<bool>()
                    .unwrap_or(false)
                    .cmp(&vb.parse::<bool>().unwrap_or(false)),
                CellEditor::Text | CellEditor::Custom => va.cmp(&vb),
            };
            if ascending {
                ordering
            } else {
                ordering.reverse()
            }
        });

        let mut row_map = vec![0; indexed_data.len()];
        for (new_index, (old_index, _)) in indexed_data.iter().enumerate() {
            row_map[*old_index] = new_index;
        }
        for position in &mut self.selected_cells {
            if let Some(new_row) = row_map.get(position.row) {
                position.row = *new_row;
            }
        }
        if let Some(position) = &mut self.editing_cell {
            if let Some(new_row) = row_map.get(position.row) {
                position.row = *new_row;
            }
        }
        self.data = indexed_data.into_iter().map(|(_, row)| row).collect();
    }

    pub fn select_cell(&mut self, pos: CellPosition) {
        if pos.row >= self.data.len() || pos.col >= self.columns.len() {
            return;
        }
        self.selected_cells.clear();
        self.selected_cells.push(pos);
    }

    pub fn selected_cells(&self) -> &[CellPosition] {
        &self.selected_cells
    }

    pub fn is_cell_selected(&self, pos: &CellPosition) -> bool {
        self.selected_cells.contains(pos)
    }

    pub fn move_selection(&mut self, row_delta: isize, col_delta: isize) {
        if self.data.is_empty() || self.columns.is_empty() {
            return;
        }
        let current = self
            .selected_cells
            .first()
            .cloned()
            .unwrap_or(CellPosition { row: 0, col: 0 });
        let row = current
            .row
            .saturating_add_signed(row_delta)
            .min(self.data.len() - 1);
        let col = current
            .col
            .saturating_add_signed(col_delta)
            .min(self.columns.len() - 1);
        self.select_cell(CellPosition { row, col });
    }

    pub fn toggle_checkbox(&mut self, pos: CellPosition) {
        let Some(column) = self.columns.get(pos.col) else {
            return;
        };
        if !column.editable || column.editor != CellEditor::Checkbox {
            return;
        }
        let Some(setter) = column.value_setter.clone() else {
            return;
        };
        let getter = column.value_getter.clone();
        let Some(row) = self.data.get_mut(pos.row) else {
            return;
        };
        let next = !getter(row).parse::<bool>().unwrap_or(false);
        setter(row, if next { "true" } else { "false" });
    }

    pub fn resize_column(&mut self, col_id: &str, width: Pixels) {
        let Some(width) = valid_grid_width(width) else {
            return;
        };
        let Some(column) = self.columns.iter().find(|column| column.id == col_id) else {
            return;
        };
        self.column_widths
            .insert(column.id.clone(), clamped_grid_width(column, width));
    }

    pub fn set_data(&mut self, data: Vec<T>) {
        self.data = data;
        self.editing_cell = None;
        self.edit_value.clear();
        self.selected_cells.clear();

        if let Some(column_id) = self.sort_column.clone() {
            if let Some(column_index) = self
                .columns
                .iter()
                .position(|column| column.id == column_id && column.sortable)
            {
                self.apply_sort(column_index);
            }
        }
    }

    /// Returns the row count.
    pub fn row_count(&self) -> usize {
        self.data.len()
    }

    /// Returns the column count.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Returns true when a cell is currently being edited.
    pub fn has_editing_cell(&self) -> bool {
        self.editing_cell.is_some()
    }

    /// Returns the current edit buffer length without exposing its value.
    pub fn edit_value_len_bytes(&self) -> usize {
        self.edit_value.len()
    }

    /// Returns the number of selected cells.
    pub fn selected_cell_count(&self) -> usize {
        self.selected_cells.len()
    }

    /// Returns true when a sort column is configured.
    pub fn has_sort(&self) -> bool {
        self.sort_column.is_some()
    }

    /// Returns the sort column id length without exposing the id.
    pub fn sort_column_len_bytes(&self) -> usize {
        self.sort_column.as_ref().map_or(0, |column| column.len())
    }

    /// Stable sort direction key.
    pub fn sort_direction_key(&self) -> &'static str {
        self.sort_direction.to_text()
    }

    /// Counts sortable columns.
    pub fn sortable_column_count(&self) -> usize {
        self.columns.iter().filter(|column| column.sortable).count()
    }

    /// Counts editable columns.
    pub fn editable_column_count(&self) -> usize {
        self.columns.iter().filter(|column| column.editable).count()
    }

    /// Counts resizable columns.
    pub fn resizable_column_count(&self) -> usize {
        self.columns
            .iter()
            .filter(|column| column.resizable)
            .count()
    }

    /// Counts columns with value setters.
    pub fn value_setter_count(&self) -> usize {
        self.columns
            .iter()
            .filter(|column| column.value_setter.is_some())
            .count()
    }

    /// Returns true when a column resize is in progress.
    pub fn has_resizing_column(&self) -> bool {
        self.resizing_column.is_some()
    }

    /// Returns true when a focus handle has been created.
    pub fn has_focus_handle(&self) -> bool {
        self.focus_handle.is_some()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "data_grid_state(rows={}, columns={}, editing_cell={}, edit_value_len_bytes={}, selected_cells={}, has_sort={}, sort_column_len_bytes={}, sort_direction={}, sortable_columns={}, editable_columns={}, resizable_columns={}, value_setters={}, resizing_column={}, focus_handle={})",
            self.row_count(),
            self.column_count(),
            self.has_editing_cell(),
            self.edit_value_len_bytes(),
            self.selected_cell_count(),
            self.has_sort(),
            self.sort_column_len_bytes(),
            self.sort_direction_key(),
            self.sortable_column_count(),
            self.editable_column_count(),
            self.resizable_column_count(),
            self.value_setter_count(),
            self.has_resizing_column(),
            self.has_focus_handle()
        )
    }
}

#[derive(IntoElement)]
pub struct DataGrid<T: 'static> {
    id: ElementId,
    state: Entity<DataGridState<T>>,
    striped: bool,
    bordered: bool,
    compact: bool,
    empty_message: SharedString,
    style: StyleRefinement,
}

impl<T: 'static> DataGrid<T> {
    #[track_caller]
    pub fn new(state: Entity<DataGridState<T>>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "data-grid:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            state,
            striped: false,
            bordered: true,
            compact: false,
            empty_message: "No rows to display".into(),
            style: StyleRefinement::default(),
        }
    }

    /// Overrides the stable identity used to scope focus and child element ids.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn striped(mut self, striped: bool) -> Self {
        self.striped = striped;
        self
    }

    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Sets the message announced and displayed when the grid has no rows.
    pub fn empty_message(mut self, message: impl Into<SharedString>) -> Self {
        self.empty_message = message.into();
        self
    }

    /// Returns true when striped row styling is enabled.
    pub fn is_striped(&self) -> bool {
        self.striped
    }

    /// Returns true when borders are enabled.
    pub fn is_bordered(&self) -> bool {
        self.bordered
    }

    /// Returns true when compact density is enabled.
    pub fn is_compact(&self) -> bool {
        self.compact
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self, cx: &App) -> String {
        let state = self.state.read(cx);
        format!(
            "data_grid({}, striped={}, bordered={}, compact={})",
            state.to_text(),
            self.is_striped(),
            self.is_bordered(),
            self.is_compact()
        )
    }
}

impl<T: 'static> Styled for DataGrid<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[allow(dead_code)]
struct ColSnapshot {
    id: SharedString,
    header: SharedString,
    width: Pixels,
    min_width: Option<Pixels>,
    max_width: Option<Pixels>,
    resizable: bool,
    sortable: bool,
    editable: bool,
    editor: CellEditor,
}

impl<T: 'static> RenderOnce for DataGrid<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let state_entity = self.state.clone();
        let striped = self.striped;
        let bordered = self.bordered;
        let compact = self.compact;
        let empty_message = self.empty_message;
        let user_style = self.style;
        let grid_id = self.id;

        let (cell_px, cell_py) = if compact {
            (px(10.0), px(6.0))
        } else {
            (px(16.0), px(12.0))
        };

        let focus_handle = state_entity.update(cx, |s, scx| {
            if s.focus_handle.is_none() {
                s.focus_handle = Some(scx.focus_handle());
            }
            s.focus_handle.clone().unwrap()
        });

        let state = state_entity.read(cx);
        let num_rows = state.data.len();
        let num_cols = state.columns.len();
        let editing = state.editing_cell.clone();
        let edit_val = state.edit_value.clone();
        let sort_col = state.sort_column.clone();
        let sort_dir = state.sort_direction.clone();
        let selected_cells = state.selected_cells.clone();

        let col_infos: Vec<ColSnapshot> = state
            .columns
            .iter()
            .map(|c| ColSnapshot {
                id: c.id.clone(),
                header: c.header.clone(),
                width: state.column_widths.get(&c.id).copied().unwrap_or(c.width),
                min_width: c.min_width,
                max_width: c.max_width,
                resizable: c.resizable,
                sortable: c.sortable,
                editable: c.editable,
                editor: c.editor.clone(),
            })
            .collect();

        let mut all_cells: Vec<Vec<AnyElement>> = Vec::with_capacity(num_rows);
        for row_idx in 0..num_rows {
            let mut row_cells: Vec<AnyElement> = Vec::with_capacity(num_cols);
            for col_idx in 0..num_cols {
                let is_editing = editing
                    .as_ref()
                    .is_some_and(|p| p.row == row_idx && p.col == col_idx);
                if is_editing {
                    row_cells.push(
                        div()
                            .flex()
                            .items_center()
                            .size_full()
                            .text_size(px(13.0))
                            .text_color(theme.tokens.foreground)
                            .child(format!("{}|", edit_val))
                            .into_any_element(),
                    );
                } else {
                    let content =
                        (state.columns[col_idx].cell_renderer)(&state.data[row_idx], row_idx);
                    row_cells.push(content);
                }
            }
            all_cells.push(row_cells);
        }

        let total_width: f32 = col_infos.iter().map(|c| -> f32 { c.width.into() }).sum();
        let total_width_px = px(total_width);

        let header_cells: Vec<AnyElement> = col_infos
            .iter()
            .enumerate()
            .map(|(col_idx, info)| {
                let is_sorted = sort_col.as_ref() == Some(&info.id);
                let sort_indicator = if is_sorted {
                    match sort_dir {
                        GridSortDirection::Ascending => "\u{25B2}",
                        GridSortDirection::Descending => "\u{25BC}",
                        GridSortDirection::None => "",
                    }
                } else {
                    ""
                };

                let header_content = div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(info.header.clone())
                    .when(!sort_indicator.is_empty(), |el| {
                        el.child(div().text_size(px(10.0)).child(sort_indicator))
                    });

                let base = div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .relative()
                    .w(info.width)
                    .min_w(info.width)
                    .flex_1()
                    .px(cell_px)
                    .py(cell_py)
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.tokens.muted_foreground)
                    .bg(theme.tokens.muted.opacity(0.5))
                    .when(bordered, |el| {
                        el.border_b_1()
                            .border_r_1()
                            .border_color(theme.tokens.border)
                    })
                    .child(header_content);

                let header_id = ElementId::NamedChild(
                    Box::new(grid_id.clone()),
                    format!("header-{col_idx}").into(),
                );
                let mut header_accessibility = AccessibilityAttributes::new(if info.sortable {
                    AccessibilityRole::Button
                } else {
                    AccessibilityRole::StaticText
                })
                .label(if info.sortable {
                    format!("Sort by {}", info.header)
                } else {
                    info.header.to_string()
                });
                if info.sortable {
                    header_accessibility = header_accessibility
                        .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
                }
                let base = base
                    .id(header_id.clone())
                    .accessibility(header_accessibility);

                let with_sort = if info.sortable {
                    let col_id = info.id.clone();
                    let st = state_entity.clone();
                    let focus_handle = window
                        .use_keyed_state(header_id, cx, |_, cx| cx.focus_handle())
                        .read(cx)
                        .clone();
                    let focus_on_mouse = focus_handle.clone();
                    let st_for_key = state_entity.clone();
                    let col_id_for_key = info.id.clone();
                    base.track_focus(&focus_handle.tab_index(0).tab_stop(true))
                        .cursor(CursorStyle::PointingHand)
                        .transition(theme.tokens.transition_fast)
                        .hover(|s| s.bg(theme.tokens.muted.opacity(0.7)))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            window.focus(&focus_on_mouse);
                            st.update(cx, |s, scx| {
                                s.sort_by_column(&col_id);
                                scx.notify();
                            });
                        })
                        .on_key_down(move |event: &KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                st_for_key.update(cx, |s, scx| {
                                    s.sort_by_column(&col_id_for_key);
                                    scx.notify();
                                });
                                cx.stop_propagation();
                            }
                        })
                } else {
                    base
                };

                let with_resize = if info.resizable {
                    let col_width = info.width;
                    let st = state_entity.clone();
                    with_sort.child(
                        div()
                            .id(ElementId::NamedChild(
                                Box::new(grid_id.clone()),
                                format!("resize-{col_idx}").into(),
                            ))
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::Separator)
                                    .label(format!("Resize {} column", info.header)),
                            )
                            .absolute()
                            .right(px(0.0))
                            .top(px(0.0))
                            .w(px(4.0))
                            .h_full()
                            .cursor(CursorStyle::ResizeLeftRight)
                            .transition(theme.tokens.transition_fast)
                            .hover(|s| s.bg(theme.tokens.primary.opacity(0.5)))
                            .on_mouse_down(
                                MouseButton::Left,
                                move |event: &MouseDownEvent, _, cx| {
                                    st.update(cx, |s, scx| {
                                        s.resizing_column = Some(col_idx);
                                        s.resize_start_x = event.position.x.into();
                                        s.resize_start_width = col_width;
                                        scx.notify();
                                    });
                                },
                            ),
                    )
                } else {
                    with_sort
                };

                with_resize.into_any_element()
            })
            .collect();

        let header_row = div()
            .flex()
            .w_full()
            .min_w(total_width_px)
            .children(header_cells);

        let body_rows: Vec<AnyElement> = all_cells
            .into_iter()
            .enumerate()
            .map(|(row_idx, cell_contents)| {
                let row_bg = if striped && row_idx % 2 == 1 {
                    theme.tokens.muted.opacity(0.3)
                } else {
                    theme.tokens.background
                };

                let cells: Vec<AnyElement> = cell_contents
                    .into_iter()
                    .enumerate()
                    .map(|(col_idx, content)| {
                        let width = col_infos[col_idx].width;
                        let is_editing = editing
                            .as_ref()
                            .is_some_and(|p| p.row == row_idx && p.col == col_idx);
                        let is_editable = col_infos[col_idx].editable;
                        let is_selected = selected_cells.contains(&CellPosition {
                            row: row_idx,
                            col: col_idx,
                        });
                        let mut cell_state = AccessibilityState::NONE;
                        if is_selected {
                            cell_state |= AccessibilityState::SELECTED;
                        }

                        let mut cell_accessibility = AccessibilityAttributes::new(if is_editing {
                            AccessibilityRole::TextInput
                        } else {
                            AccessibilityRole::Group
                        })
                        .label(if is_editing {
                            format!(
                                "Edit {} column, row {}",
                                col_infos[col_idx].header,
                                row_idx + 1
                            )
                        } else {
                            format!("{} column, row {}", col_infos[col_idx].header, row_idx + 1)
                        })
                        .states(cell_state);
                        if is_editing {
                            cell_accessibility = cell_accessibility
                                .value(AccessibilityValue::Text(edit_val.clone()));
                        } else {
                            cell_accessibility =
                                cell_accessibility.actions(vec![AccessibilityAction::Click]);
                        }

                        let mut cell = div()
                            .id(ElementId::NamedChild(
                                Box::new(grid_id.clone()),
                                format!("cell-{row_idx}-{col_idx}").into(),
                            ))
                            .accessibility(cell_accessibility)
                            .when(is_selected && !is_editing, |cell| {
                                cell.bg(theme.tokens.accent.opacity(0.18))
                                    .border_1()
                                    .border_color(theme.tokens.ring)
                            })
                            .on_mouse_down(MouseButton::Left, {
                                let st = state_entity.clone();
                                let focus_on_mouse = focus_handle.clone();
                                move |_, window, cx| {
                                    window.focus(&focus_on_mouse);
                                    st.update(cx, |s, scx| {
                                        s.select_cell(CellPosition {
                                            row: row_idx,
                                            col: col_idx,
                                        });
                                        scx.notify();
                                    });
                                }
                            })
                            .flex()
                            .items_center()
                            .w(width)
                            .min_w(width)
                            .flex_1()
                            .px(cell_px)
                            .py(cell_py)
                            .text_size(px(13.0))
                            .text_color(theme.tokens.foreground)
                            .overflow_hidden()
                            .text_ellipsis()
                            .when(bordered, |el| {
                                el.border_b_1()
                                    .border_r_1()
                                    .border_color(theme.tokens.border.opacity(0.5))
                            });

                        if is_editing {
                            cell = cell
                                .bg(theme.tokens.background)
                                .border_2()
                                .border_color(theme.tokens.ring);
                        }

                        if is_editable && !is_editing {
                            let st = state_entity.clone();
                            let editor = col_infos[col_idx].editor.clone();
                            cell = cell.cursor(CursorStyle::IBeam).on_mouse_down(
                                MouseButton::Left,
                                move |event: &MouseDownEvent, window, cx| {
                                    if event.click_count < 2 {
                                        return;
                                    }
                                    let fh = st.update(cx, |s, scx| {
                                        if s.editing_cell.is_some() {
                                            s.commit_edit();
                                        }
                                        let position = CellPosition {
                                            row: row_idx,
                                            col: col_idx,
                                        };
                                        if editor == CellEditor::Checkbox {
                                            s.toggle_checkbox(position);
                                        } else {
                                            s.start_editing(position);
                                        }
                                        scx.notify();
                                        s.focus_handle.clone()
                                    });
                                    if let Some(handle) = fh {
                                        window.focus(&handle);
                                    }
                                },
                            );
                        }

                        cell.child(content).into_any_element()
                    })
                    .collect();

                div()
                    .accessibility(AccessibilityAttributes::new(AccessibilityRole::ListItem))
                    .flex()
                    .w_full()
                    .min_w(total_width_px)
                    .bg(row_bg)
                    .hover(|s| s.bg(theme.tokens.accent.opacity(0.1)))
                    .children(cells)
                    .into_any_element()
            })
            .collect();

        let body = div()
            .id(ElementId::NamedChild(
                Box::new(grid_id.clone()),
                "body".into(),
            ))
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::List))
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .children(body_rows)
            .when(num_rows == 0, |body| {
                body.child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h(px(96.0))
                        .items_center()
                        .justify_center()
                        .px(px(16.0))
                        .py(px(24.0))
                        .text_size(px(13.0))
                        .text_color(theme.tokens.muted_foreground)
                        .child(empty_message),
                )
            });

        let state_for_keys = state_entity.clone();
        let state_for_move = state_entity.clone();
        let state_for_up = state_entity.clone();

        div()
            .id(grid_id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group).label("Data grid"),
            )
            .track_focus(&focus_handle.tab_index(0).tab_stop(true))
            .flex()
            .flex_col()
            .w_full()
            .border_1()
            .border_color(theme.tokens.border)
            .rounded(theme.tokens.radius_lg)
            .overflow_x_scroll()
            .overflow_y_hidden()
            .bg(theme.tokens.card)
            .shadow_sm()
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                let handled = state_for_keys.update(cx, |s, scx| {
                    let key = event.keystroke.key.as_str();
                    let handled = if let Some(editing_cell) = s.editing_cell.clone() {
                        if key == "enter" {
                            s.commit_edit();
                            true
                        } else if key == "escape" {
                            s.cancel_edit();
                            true
                        } else if key == "tab" {
                            s.move_edit_next();
                            true
                        } else if key == "backspace" {
                            s.edit_value.pop();
                            true
                        } else if let Some(ref ch) = event.keystroke.key_char {
                            let editor = &s.columns[editing_cell.col].editor;
                            let accepts_input = match editor {
                                CellEditor::Number => ch.chars().all(|character| {
                                    character.is_ascii_digit()
                                        || matches!(character, '.' | '-' | '+' | 'e' | 'E')
                                }),
                                CellEditor::Text | CellEditor::Custom => true,
                                CellEditor::Checkbox => false,
                            };
                            if accepts_input {
                                s.edit_value.push_str(ch);
                            }
                            accepts_input
                        } else {
                            false
                        }
                    } else {
                        match key {
                            "left" => {
                                s.move_selection(0, -1);
                                true
                            }
                            "right" => {
                                s.move_selection(0, 1);
                                true
                            }
                            "up" => {
                                s.move_selection(-1, 0);
                                true
                            }
                            "down" => {
                                s.move_selection(1, 0);
                                true
                            }
                            "home" => {
                                let row = s.selected_cells.first().map_or(0, |cell| cell.row);
                                s.select_cell(CellPosition { row, col: 0 });
                                true
                            }
                            "end" => {
                                let row = s.selected_cells.first().map_or(0, |cell| cell.row);
                                let col = s.columns.len().saturating_sub(1);
                                s.select_cell(CellPosition { row, col });
                                true
                            }
                            "enter" | "space" => {
                                if let Some(position) = s.selected_cells.first().cloned() {
                                    if s.columns[position.col].editor == CellEditor::Checkbox {
                                        s.toggle_checkbox(position);
                                    } else {
                                        s.start_editing(position);
                                    }
                                }
                                true
                            }
                            _ => false,
                        }
                    };
                    if handled {
                        scx.notify();
                    }
                    handled
                });
                if handled {
                    cx.stop_propagation();
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                state_for_move.update(cx, |s, scx| {
                    if let Some(col_idx) = s.resizing_column {
                        let current_x: f32 = event.position.x.into();
                        let delta = current_x - s.resize_start_x;
                        let start_w: f32 = s.resize_start_width.into();
                        let new_width = px((start_w + delta).max(50.0));
                        let min = s.columns[col_idx].min_width.unwrap_or(px(50.0));
                        let max = s.columns[col_idx].max_width;
                        let clamped = if new_width < min {
                            min
                        } else if let Some(max_w) = max {
                            if new_width > max_w {
                                max_w
                            } else {
                                new_width
                            }
                        } else {
                            new_width
                        };
                        let col_id = s.columns[col_idx].id.clone();
                        s.column_widths.insert(col_id, clamped);
                        scx.notify();
                    }
                });
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                state_for_up.update(cx, |s, scx| {
                    if s.resizing_column.is_some() {
                        s.resizing_column = None;
                        scx.notify();
                    }
                });
            })
            .child(header_row)
            .child(body)
            .map(|mut el| {
                el.style().refine(&user_style);
                el
            })
    }
}

//! DataTable - High-performance table component with virtual scrolling and sorting.

use crate::components::icon_source::IconSource;
use crate::components::input::{Input, InputSize, InputState};
use crate::components::select::{Select, SelectEvent, SelectOption};
use crate::theme::{Theme, use_theme};
use crate::virtual_list::{
    ItemExtentProvider, hlist_variable, hlist_variable_view, vlist_uniform_view,
};
use kael::{prelude::FluentBuilder as _, *};
use std::collections::{BTreeSet, HashMap, VecDeque, hash_map::Entry};
use std::ops::Range;
use std::{panic::Location, rc::Rc};

#[derive(Clone)]
pub struct RowAction {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: Option<IconSource>,
    pub destructive: bool,
    pub on_click: Rc<dyn Fn(usize, &mut Window, &mut App)>,
}

impl RowAction {
    pub fn new<S: Into<SharedString>, F: Fn(usize, &mut Window, &mut App) + 'static>(
        id: S,
        label: S,
        handler: F,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            destructive: false,
            on_click: Rc::new(handler),
        }
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// Returns the action id length without exposing the id.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Returns the action label length without exposing the label.
    pub fn label_len_bytes(&self) -> usize {
        self.label.len()
    }

    /// Returns true when this action has an icon.
    pub fn has_icon(&self) -> bool {
        self.icon.is_some()
    }

    /// Returns true when this action is destructive.
    pub fn is_destructive(&self) -> bool {
        self.destructive
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "row_action(id_len_bytes={}, label_len_bytes={}, has_icon={}, destructive={}, has_handler=true)",
            self.id_len_bytes(),
            self.label_len_bytes(),
            self.has_icon(),
            self.is_destructive(),
        )
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// A scalable, owned snapshot of a data-table row selection.
///
/// Large "select all" operations use [`Self::AllExcept`] and retain only the
/// rows that a user subsequently deselects. Consumers can therefore persist,
/// send to a worker, or apply a bulk operation without materializing every row
/// index in a million-row table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataTableSelectionSnapshot {
    /// Only the listed rows are selected.
    Explicit {
        /// Total rows in the table when the snapshot was captured.
        total_rows: usize,
        /// Sorted selected row indices.
        selected: Vec<usize>,
    },
    /// Every row is selected except the listed rows.
    AllExcept {
        /// Total rows in the table when the snapshot was captured.
        total_rows: usize,
        /// Sorted row indices excluded from the selection.
        deselected: Vec<usize>,
    },
}

impl DataTableSelectionSnapshot {
    /// Total rows in the table when this snapshot was captured.
    pub fn total_rows(&self) -> usize {
        match self {
            Self::Explicit { total_rows, .. } | Self::AllExcept { total_rows, .. } => *total_rows,
        }
    }

    /// Number of selected rows without expanding an all-except selection.
    pub fn selected_count(&self) -> usize {
        match self {
            Self::Explicit { selected, .. } => selected.len(),
            Self::AllExcept {
                total_rows,
                deselected,
            } => total_rows.saturating_sub(deselected.len()),
        }
    }

    /// Number of row indices retained by the snapshot representation.
    pub fn stored_index_count(&self) -> usize {
        match self {
            Self::Explicit { selected, .. } => selected.len(),
            Self::AllExcept { deselected, .. } => deselected.len(),
        }
    }

    /// Stable representation key for diagnostics and serialization adapters.
    pub fn representation_key(&self) -> &'static str {
        match self {
            Self::Explicit { .. } => "explicit",
            Self::AllExcept { .. } => "all_except",
        }
    }

    /// Returns true when `row_index` belongs to this selection.
    pub fn contains(&self, row_index: usize) -> bool {
        if row_index >= self.total_rows() {
            return false;
        }
        match self {
            Self::Explicit { selected, .. } => selected.binary_search(&row_index).is_ok(),
            Self::AllExcept { deselected, .. } => deselected.binary_search(&row_index).is_err(),
        }
    }

    /// Returns true when every current row is selected.
    pub fn is_all_selected(&self) -> bool {
        self.total_rows() > 0 && self.selected_count() == self.total_rows()
    }

    /// Returns explicit selected indices when this snapshot already stores them.
    ///
    /// `None` means the snapshot is an all-except selection, not an empty
    /// selection. Use [`Self::contains`] or [`Self::try_materialize_selected`]
    /// when callers truly require selected indices.
    pub fn explicit_selected(&self) -> Option<&[usize]> {
        match self {
            Self::Explicit { selected, .. } => Some(selected),
            Self::AllExcept { .. } => None,
        }
    }

    /// Returns excluded indices for an all-except snapshot.
    pub fn deselected(&self) -> Option<&[usize]> {
        match self {
            Self::AllExcept { deselected, .. } => Some(deselected),
            Self::Explicit { .. } => None,
        }
    }

    /// Materialize selected indices only when the caller-provided bound permits it.
    ///
    /// This is intentionally fallible so a bulk callback cannot accidentally
    /// allocate a million indices. Explicit snapshots are cloned only when their
    /// selected count also fits the bound.
    pub fn try_materialize_selected(&self, max_indices: usize) -> Option<Vec<usize>> {
        if self.selected_count() > max_indices {
            return None;
        }
        match self {
            Self::Explicit { selected, .. } => Some(selected.clone()),
            Self::AllExcept {
                total_rows,
                deselected,
            } => {
                let mut selected = Vec::with_capacity(self.selected_count());
                let mut excluded = deselected.iter().copied().peekable();
                for index in 0..*total_rows {
                    if excluded.peek() == Some(&index) {
                        excluded.next();
                    } else {
                        selected.push(index);
                    }
                }
                Some(selected)
            }
        }
    }
}

#[derive(Clone, Debug)]
enum RowSelection {
    Explicit(BTreeSet<usize>),
    AllExcept(BTreeSet<usize>),
}

impl Default for RowSelection {
    fn default() -> Self {
        Self::Explicit(BTreeSet::new())
    }
}

impl RowSelection {
    fn clear(&mut self) {
        *self = Self::default();
    }

    fn select_all(&mut self) {
        *self = Self::AllExcept(BTreeSet::new());
    }

    fn toggle(&mut self, row_index: usize) {
        let indices = match self {
            Self::Explicit(indices) | Self::AllExcept(indices) => indices,
        };
        if !indices.remove(&row_index) {
            indices.insert(row_index);
        }
    }

    fn contains(&self, row_index: usize, total_rows: usize) -> bool {
        if row_index >= total_rows {
            return false;
        }
        match self {
            Self::Explicit(selected) => selected.contains(&row_index),
            Self::AllExcept(deselected) => !deselected.contains(&row_index),
        }
    }

    fn selected_count(&self, total_rows: usize) -> usize {
        match self {
            Self::Explicit(selected) => selected.len().min(total_rows),
            Self::AllExcept(deselected) => total_rows.saturating_sub(deselected.len()),
        }
    }

    fn stored_index_count(&self) -> usize {
        match self {
            Self::Explicit(selected) | Self::AllExcept(selected) => selected.len(),
        }
    }

    fn representation_key(&self) -> &'static str {
        match self {
            Self::Explicit(_) => "explicit",
            Self::AllExcept(_) => "all_except",
        }
    }

    fn snapshot(&self, total_rows: usize) -> DataTableSelectionSnapshot {
        match self {
            Self::Explicit(selected) => DataTableSelectionSnapshot::Explicit {
                total_rows,
                selected: selected
                    .iter()
                    .copied()
                    .take_while(|index| *index < total_rows)
                    .collect(),
            },
            Self::AllExcept(deselected) => DataTableSelectionSnapshot::AllExcept {
                total_rows,
                deselected: deselected
                    .iter()
                    .copied()
                    .take_while(|index| *index < total_rows)
                    .collect(),
            },
        }
    }

    fn remap_after_sort(&mut self, old_to_new: &HashMap<usize, usize>) {
        let indices = match self {
            Self::Explicit(indices) | Self::AllExcept(indices) => indices,
        };
        *indices = indices
            .iter()
            .filter_map(|old_index| old_to_new.get(old_index).copied())
            .collect();
    }
}

/// Identifies one virtual-data request within a specific table query.
///
/// The generation changes whenever the virtual data set is reset or its
/// server-side search/sort inputs change. Pass this value back to
/// [`DataTable::set_page_data_for`] so a response from an older query cannot
/// overwrite the current cache.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DataTablePageRequest {
    page_start: usize,
    page_size: usize,
    generation: u64,
}

impl DataTablePageRequest {
    pub fn page_start(self) -> usize {
        self.page_start
    }

    pub fn page_size(self) -> usize {
        self.page_size
    }

    pub fn generation(self) -> u64 {
        self.generation
    }

    /// Content-safe summary for diagnostics.
    pub fn to_text(self) -> String {
        format!(
            "data_table_page_request(page_start={}, page_size={}, generation={})",
            self.page_start, self.page_size, self.generation
        )
    }
}

impl SortDirection {
    /// Stable sort direction key for content-safe diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Ascending => "ascending",
            Self::Descending => "descending",
        }
    }
}

#[derive(Debug, Clone)]
struct ViewportState {
    viewport_height: f32,
    row_height: f32,
}

impl ViewportState {
    fn new(row_height: f32, viewport_height: f32) -> Self {
        Self {
            viewport_height,
            row_height,
        }
    }
}

fn should_capture_vertical_scroll(
    scroll_y: f32,
    content_height: f32,
    viewport_height: f32,
    delta_y: f32,
) -> bool {
    if !scroll_y.is_finite()
        || !content_height.is_finite()
        || !viewport_height.is_finite()
        || !delta_y.is_finite()
    {
        return false;
    }

    let max_scroll = (content_height - viewport_height).max(0.0);
    let scroll_y = scroll_y.clamp(0.0, max_scroll);
    (delta_y < 0.0 && scroll_y < max_scroll) || (delta_y > 0.0 && scroll_y > 0.0)
}

struct VirtualScroller {
    viewport: ViewportState,
    total_items: usize,
}

#[derive(Clone)]
struct ColumnExtents(Rc<Vec<Pixels>>);

impl ItemExtentProvider for ColumnExtents {
    fn extent(&self, index: usize) -> Pixels {
        self.0.get(index).copied().unwrap_or(px(1.0))
    }
}

#[derive(Clone)]
struct TableColumnSnapshot {
    header: SharedString,
    width: Pixels,
    resizable: bool,
    sortable: bool,
    focus_handle: FocusHandle,
}

impl VirtualScroller {
    fn new(total_items: usize, viewport: ViewportState) -> Self {
        Self {
            viewport,
            total_items,
        }
    }

    fn set_total_items(&mut self, count: usize) {
        self.total_items = count;
    }
}

pub struct ColumnDef<T: 'static> {
    pub id: SharedString,
    pub header: SharedString,
    pub accessor: Rc<dyn Fn(&T) -> SharedString>,
    pub width: Pixels,
    pub min_width: Pixels,
    pub resizable: bool,
    pub sortable: bool,
    pub editable: bool,
}

impl<T: 'static> ColumnDef<T> {
    pub fn new<S: Into<SharedString>, F: Fn(&T) -> SharedString + 'static>(
        id: S,
        header: S,
        accessor: F,
    ) -> Self {
        let id_string: SharedString = id.into();
        let header_string: SharedString = header.into();

        Self {
            id: id_string,
            header: header_string,
            accessor: Rc::new(accessor),
            width: px(150.0),
            min_width: px(80.0),
            resizable: true,
            sortable: true,
            editable: false,
        }
    }

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = valid_column_width(width.into(), px(150.0));
        self
    }

    pub fn min_width(mut self, width: impl Into<Pixels>) -> Self {
        self.min_width = valid_column_width(width.into(), px(80.0));
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

    /// Coarse minimum width class for content-safe diagnostics.
    pub fn min_width_class(&self) -> &'static str {
        let width: f32 = self.min_width.into();
        if width <= 64.0 {
            "compact"
        } else if width <= 120.0 {
            "standard"
        } else if width <= 240.0 {
            "wide"
        } else {
            "extra_wide"
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "column_def(id_len_bytes={}, header_len_bytes={}, width_class={}, min_width_class={}, resizable={}, sortable={}, editable={})",
            self.id_len_bytes(),
            self.header_len_bytes(),
            self.width_class(),
            self.min_width_class(),
            self.resizable,
            self.sortable,
            self.editable
        )
    }
}

fn valid_column_width(width: Pixels, fallback: Pixels) -> Pixels {
    let value: f32 = width.into();
    if value.is_finite() && value > 0.0 {
        width
    } else {
        fallback
    }
}

enum DataBacking<T: Clone + 'static> {
    InMemory {
        data: Vec<T>,
    },
    Virtual {
        total_items: usize,
        cache: HashMap<usize, T>,
        cached_pages: HashMap<usize, usize>,
        page_lru: VecDeque<usize>,
        in_flight_pages: HashMap<usize, u64>,
        page_size: usize,
        max_cached_pages: usize,
        generation: u64,
    },
}

const DEFAULT_MAX_CACHED_PAGES: usize = 16;
/// Maximum exact selection size delivered to the legacy slice callback.
///
/// Larger all-except selections remain available through
/// [`DataTableSelectionSnapshot`] and `on_selection_change_snapshot` without
/// materializing every selected row index.
pub const DATA_TABLE_LEGACY_SELECTION_MATERIALIZATION_LIMIT: usize = 16_384;

pub struct DataTableState<T: Clone + 'static> {
    columns: Vec<ColumnDef<T>>,
    column_widths: Vec<Pixels>,
    sort_column: Option<usize>,
    sort_direction: SortDirection,
    scroller: VirtualScroller,
    selection: RowSelection,
    backing: DataBacking<T>,
}

impl<T: Clone + 'static> DataTableState<T> {
    pub fn new(data: Vec<T>, columns: Vec<ColumnDef<T>>) -> Self {
        let column_widths = columns
            .iter()
            .map(|column| {
                let minimum = valid_column_width(column.min_width, px(80.0));
                valid_column_width(column.width, px(150.0)).max(minimum)
            })
            .collect();
        let total_items = data.len();
        let viewport = ViewportState::new(48.0, 600.0);

        Self {
            column_widths,
            columns,
            sort_column: None,
            sort_direction: SortDirection::Ascending,
            scroller: VirtualScroller::new(total_items, viewport),
            selection: RowSelection::default(),
            backing: DataBacking::InMemory { data },
        }
    }

    fn row_height(&self) -> f32 {
        self.scroller.viewport.row_height
    }

    fn viewport_height(&self) -> f32 {
        self.scroller.viewport.viewport_height
    }

    fn effective_viewport_height(&self, visible_items: usize) -> f32 {
        if self.is_virtual() {
            self.viewport_height()
        } else {
            self.viewport_height()
                .min(self.row_height() * visible_items.max(1) as f32)
        }
    }

    fn total_items(&self) -> usize {
        match &self.backing {
            DataBacking::InMemory { data } => data.len(),
            DataBacking::Virtual { total_items, .. } => *total_items,
        }
    }

    fn get_row(&self, index: usize) -> Option<&T> {
        match &self.backing {
            DataBacking::InMemory { data } => data.get(index),
            DataBacking::Virtual { cache, .. } => cache.get(&index),
        }
    }

    fn replace_in_memory_data(&mut self, data: Vec<T>) {
        let count = data.len();
        self.backing = DataBacking::InMemory { data };
        self.scroller.set_total_items(count);
        self.selection.clear();

        if let Some(column_index) = self.sort_column {
            self.sort_by_column(column_index, self.sort_direction);
        }
    }

    fn virtual_reset(&mut self, total_items: usize, page_size: Option<usize>) {
        match &mut self.backing {
            DataBacking::Virtual {
                total_items: t,
                cache,
                cached_pages,
                page_lru,
                in_flight_pages,
                page_size: ps,
                generation,
                ..
            } => {
                *t = total_items;
                cache.clear();
                cached_pages.clear();
                page_lru.clear();
                in_flight_pages.clear();
                *generation = generation.wrapping_add(1).max(1);
                if let Some(s) = page_size {
                    *ps = s.max(1);
                }
            }
            DataBacking::InMemory { .. } => {
                self.backing = DataBacking::Virtual {
                    total_items,
                    cache: HashMap::new(),
                    cached_pages: HashMap::new(),
                    page_lru: VecDeque::new(),
                    in_flight_pages: HashMap::new(),
                    page_size: page_size.unwrap_or(200).max(1),
                    max_cached_pages: DEFAULT_MAX_CACHED_PAGES,
                    generation: 1,
                };
            }
        }
        self.scroller.set_total_items(total_items);
        self.selection.clear();
    }

    fn virtual_set_page(
        &mut self,
        page_start: usize,
        rows: Vec<T>,
        request_generation: Option<u64>,
    ) -> bool {
        if let DataBacking::Virtual {
            total_items,
            cache,
            cached_pages,
            page_lru,
            in_flight_pages,
            page_size,
            max_cached_pages,
            generation,
        } = &mut self.backing
        {
            let Some(in_flight_generation) = in_flight_pages.get(&page_start).copied() else {
                return false;
            };
            if in_flight_generation != *generation
                || request_generation.is_some_and(|value| value != *generation)
            {
                return false;
            }

            if let Some(previous_count) = cached_pages.remove(&page_start) {
                for index in page_start..page_start.saturating_add(previous_count) {
                    cache.remove(&index);
                }
            }

            let accepted_count = rows
                .len()
                .min(*page_size)
                .min(total_items.saturating_sub(page_start));
            for (i, row) in rows.into_iter().take(accepted_count).enumerate() {
                cache.insert(page_start + i, row);
            }
            in_flight_pages.remove(&page_start);
            cached_pages.insert(page_start, accepted_count);
            touch_lru_page(page_lru, page_start);
            evict_lru_pages(cache, cached_pages, page_lru, (*max_cached_pages).max(1));
            return true;
        }
        false
    }

    fn invalidate_virtual_query(&mut self) -> bool {
        let DataBacking::Virtual {
            cache,
            cached_pages,
            page_lru,
            in_flight_pages,
            generation,
            ..
        } = &mut self.backing
        else {
            return false;
        };
        cache.clear();
        cached_pages.clear();
        page_lru.clear();
        in_flight_pages.clear();
        *generation = generation.wrapping_add(1).max(1);
        true
    }

    fn set_max_cached_pages(&mut self, max_cached_pages: usize) {
        if let DataBacking::Virtual {
            cache,
            cached_pages,
            page_lru,
            max_cached_pages: configured,
            ..
        } = &mut self.backing
        {
            *configured = max_cached_pages.max(1);
            evict_lru_pages(cache, cached_pages, page_lru, *configured);
        }
    }

    fn virtual_requests_for_range(&mut self, range: Range<usize>) -> Vec<DataTablePageRequest> {
        let DataBacking::Virtual {
            total_items,
            cached_pages,
            page_lru,
            in_flight_pages,
            page_size,
            generation,
            ..
        } = &mut self.backing
        else {
            return Vec::new();
        };
        if range.is_empty() || *total_items == 0 {
            return Vec::new();
        }

        let start = range.start.min(*total_items);
        let end = range.end.min(*total_items);
        if start >= end {
            return Vec::new();
        }

        let first_page = (start / *page_size) * *page_size;
        let last_page = ((end - 1) / *page_size) * *page_size;
        let mut requests = Vec::new();
        let mut page_start = first_page;
        loop {
            if cached_pages.contains_key(&page_start) {
                touch_lru_page(page_lru, page_start);
            } else if let Entry::Vacant(entry) = in_flight_pages.entry(page_start) {
                entry.insert(*generation);
                requests.push(DataTablePageRequest {
                    page_start,
                    page_size: (*page_size).min(total_items.saturating_sub(page_start)),
                    generation: *generation,
                });
            }

            if page_start == last_page {
                break;
            }
            let Some(next) = page_start.checked_add(*page_size) else {
                break;
            };
            page_start = next;
        }
        requests
    }

    fn virtual_page_failed(&mut self, request: DataTablePageRequest) -> bool {
        let DataBacking::Virtual {
            in_flight_pages,
            generation,
            ..
        } = &mut self.backing
        else {
            return false;
        };
        if request.generation != *generation
            || in_flight_pages.get(&request.page_start) != Some(generation)
        {
            return false;
        }
        in_flight_pages.remove(&request.page_start);
        true
    }

    pub fn sort_by_column(&mut self, column_index: usize, direction: SortDirection) {
        let Some(column) = self.columns.get(column_index) else {
            return;
        };
        if !column.sortable {
            return;
        }
        let accessor = column.accessor.clone();

        self.sort_column = Some(column_index);
        self.sort_direction = direction;

        if self.invalidate_virtual_query() {
            return;
        }

        if let DataBacking::InMemory { data } = &mut self.backing {
            let mut indexed_values: Vec<(usize, String, T)> = std::mem::take(data)
                .into_iter()
                .enumerate()
                .map(|(idx, row)| {
                    let value = accessor(&row).to_string();
                    (idx, value, row)
                })
                .collect();

            indexed_values.sort_by(|(_, a, _), (_, b, _)| match direction {
                SortDirection::Ascending => a.cmp(b),
                SortDirection::Descending => b.cmp(a),
            });

            let old_to_new = indexed_values
                .iter()
                .enumerate()
                .map(|(new_index, (old_index, _, _))| (*old_index, new_index))
                .collect::<HashMap<_, _>>();
            self.selection.remap_after_sort(&old_to_new);

            let sorted_data = indexed_values.into_iter().map(|(_, _, row)| row).collect();
            *data = sorted_data;
        }
    }

    pub fn toggle_row(&mut self, row_index: usize) {
        if row_index >= self.total_items() {
            return;
        }
        self.selection.toggle(row_index);
    }

    pub fn is_row_selected(&self, row_index: usize) -> bool {
        self.selection.contains(row_index, self.total_items())
    }

    pub fn resize_column(&mut self, column_index: usize, new_width: Pixels) {
        if let (Some(column), Some(width)) = (
            self.columns.get(column_index),
            self.column_widths.get_mut(column_index),
        ) {
            let minimum = valid_column_width(column.min_width, px(80.0));
            *width = valid_column_width(new_width, *width).max(minimum);
        }
    }

    /// Returns the number of configured columns.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Returns the total row count advertised by the backing store.
    pub fn row_count(&self) -> usize {
        self.total_items()
    }

    /// Stable backing kind key.
    pub fn backing_kind(&self) -> &'static str {
        match &self.backing {
            DataBacking::InMemory { .. } => "in_memory",
            DataBacking::Virtual { .. } => "virtual",
        }
    }

    /// Returns true when the table uses virtual backing.
    pub fn is_virtual(&self) -> bool {
        matches!(&self.backing, DataBacking::Virtual { .. })
    }

    /// Number of cached virtual rows.
    pub fn cached_row_count(&self) -> usize {
        match &self.backing {
            DataBacking::Virtual { cache, .. } => cache.len(),
            DataBacking::InMemory { data } => data.len(),
        }
    }

    /// Number of complete virtual pages retained in the bounded cache.
    pub fn cached_page_count(&self) -> usize {
        match &self.backing {
            DataBacking::Virtual { cached_pages, .. } => cached_pages.len(),
            DataBacking::InMemory { .. } => 0,
        }
    }

    /// Number of in-flight virtual pages.
    pub fn in_flight_page_count(&self) -> usize {
        match &self.backing {
            DataBacking::Virtual {
                in_flight_pages, ..
            } => in_flight_pages.len(),
            DataBacking::InMemory { .. } => 0,
        }
    }

    /// Configured maximum number of retained virtual pages.
    pub fn max_cached_pages(&self) -> Option<usize> {
        match &self.backing {
            DataBacking::Virtual {
                max_cached_pages, ..
            } => Some(*max_cached_pages),
            DataBacking::InMemory { .. } => None,
        }
    }

    /// Current virtual query generation.
    pub fn virtual_generation(&self) -> Option<u64> {
        match &self.backing {
            DataBacking::Virtual { generation, .. } => Some(*generation),
            DataBacking::InMemory { .. } => None,
        }
    }

    /// Page size for virtual backing, when enabled.
    pub fn page_size(&self) -> Option<usize> {
        match &self.backing {
            DataBacking::Virtual { page_size, .. } => Some(*page_size),
            DataBacking::InMemory { .. } => None,
        }
    }

    /// Number of selected rows.
    pub fn selected_count(&self) -> usize {
        self.selection.selected_count(self.total_items())
    }

    /// Number of row indices retained by the compressed selection model.
    pub fn stored_selection_index_count(&self) -> usize {
        self.selection.stored_index_count()
    }

    /// Stable key describing how the current selection is represented.
    pub fn selection_representation_key(&self) -> &'static str {
        self.selection.representation_key()
    }

    /// Capture an owned selection snapshot without expanding select-all state.
    pub fn selection_snapshot(&self) -> DataTableSelectionSnapshot {
        self.selection.snapshot(self.total_items())
    }

    /// Returns true when a sort column is configured.
    pub fn has_sort(&self) -> bool {
        self.sort_column.is_some()
    }

    /// Current sort column index, without exposing column ids or headers.
    pub fn sort_column_index(&self) -> Option<usize> {
        self.sort_column
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

    /// Coarse row-height class for content-safe diagnostics.
    pub fn row_height_class(&self) -> &'static str {
        match self.row_height() {
            h if h <= 36.0 => "compact",
            h if h <= 56.0 => "standard",
            h if h <= 80.0 => "spacious",
            _ => "extra_spacious",
        }
    }

    /// Coarse viewport class for content-safe diagnostics.
    pub fn viewport_class(&self) -> &'static str {
        match self.viewport_height() {
            h if h <= 360.0 => "short",
            h if h <= 720.0 => "medium",
            h if h <= 1080.0 => "tall",
            _ => "extra_tall",
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "data_table_state(columns={}, rows={}, backing={}, virtual={}, cached_rows={}, cached_pages={}, max_cached_pages={}, in_flight_pages={}, page_size={}, generation={}, selected={}, selection_representation={}, stored_selection_indices={}, has_sort={}, sort_column={}, sort_direction={}, sortable_columns={}, editable_columns={}, resizable_columns={}, row_height_class={}, viewport_class={})",
            self.column_count(),
            self.row_count(),
            self.backing_kind(),
            self.is_virtual(),
            self.cached_row_count(),
            self.cached_page_count(),
            self.max_cached_pages().unwrap_or(0),
            self.in_flight_page_count(),
            self.page_size().map_or(0, |size| size),
            self.virtual_generation().unwrap_or(0),
            self.selected_count(),
            self.selection_representation_key(),
            self.stored_selection_index_count(),
            self.has_sort(),
            self.sort_column_index()
                .map_or_else(|| "none".to_string(), |index| index.to_string()),
            self.sort_direction_key(),
            self.sortable_column_count(),
            self.editable_column_count(),
            self.resizable_column_count(),
            self.row_height_class(),
            self.viewport_class()
        )
    }
}

fn touch_lru_page(page_lru: &mut VecDeque<usize>, page_start: usize) {
    if let Some(position) = page_lru.iter().position(|entry| *entry == page_start) {
        page_lru.remove(position);
    }
    page_lru.push_back(page_start);
}

fn evict_lru_pages<T>(
    cache: &mut HashMap<usize, T>,
    cached_pages: &mut HashMap<usize, usize>,
    page_lru: &mut VecDeque<usize>,
    max_cached_pages: usize,
) {
    while cached_pages.len() > max_cached_pages {
        let Some(page_start) = page_lru.pop_front() else {
            break;
        };
        let Some(row_count) = cached_pages.remove(&page_start) else {
            continue;
        };
        for index in page_start..page_start.saturating_add(row_count) {
            cache.remove(&index);
        }
    }
}

pub struct DataTable<T: Clone + 'static> {
    id: ElementId,
    state: DataTableState<T>,
    resizing_column: Option<usize>,
    resize_start_x: f32,
    resize_start_width: Pixels,
    sticky_header: bool,
    on_load_more: Option<Box<dyn Fn(&mut Window, &mut Context<Self>) + 'static>>,
    load_more_threshold: f32,
    load_more_triggered: bool,
    scroll_handle: ScrollHandle,
    horizontal_scroll_handle: ScrollHandle,
    editing_cell: Option<(usize, usize)>,
    edit_input: Option<Entity<InputState>>,
    edit_column_id: SharedString,
    edit_old_value: SharedString,
    use_edit_dialog: bool,
    on_cell_edit: Option<
        Box<dyn Fn(usize, SharedString, SharedString, SharedString, &mut Context<Self>) + 'static>,
    >,
    on_cell_double_click: Option<
        Box<dyn Fn(&T, SharedString, SharedString, &mut Window, &mut Context<Self>) + 'static>,
    >,
    on_fetch_page: Option<Box<dyn Fn(usize, usize, &mut Window, &mut Context<Self>) + 'static>>,
    on_fetch_page_request:
        Option<Box<dyn Fn(DataTablePageRequest, &mut Window, &mut Context<Self>) + 'static>>,
    on_row_click: Option<Box<dyn Fn(usize, &T, &mut Window, &mut Context<Self>) + 'static>>,
    search_query: String,
    search_column: Option<usize>,
    show_search: bool,
    search_column_select: Entity<Select<usize>>,
    search_input: Entity<InputState>,
    show_selection: bool,
    /// Compatibility callback for explicitly enumerable selections.
    ///
    /// All-except selections larger than
    /// [`DATA_TABLE_LEGACY_SELECTION_MATERIALIZATION_LIMIT`]
    /// are intentionally not sent through this slice API; use
    /// `on_selection_change_snapshot` for every selection transition.
    on_selection_change: Option<Box<dyn Fn(&[usize], &mut Window, &mut Context<Self>) + 'static>>,
    on_selection_change_snapshot:
        Option<Box<dyn Fn(&DataTableSelectionSnapshot, &mut Window, &mut Context<Self>) + 'static>>,
    row_actions: Vec<RowAction>,
    context_menu: Option<(usize, Point<Pixels>)>,
    empty_message: SharedString,
    no_results_message: SharedString,
    select_all_focus_handle: FocusHandle,
    header_focus_handles: Vec<FocusHandle>,
    style: StyleRefinement,
}

impl<T: Clone + 'static> DataTable<T> {
    #[track_caller]
    pub fn new(data: Vec<T>, columns: Vec<ColumnDef<T>>, cx: &mut Context<Self>) -> Self {
        let caller = Location::caller();
        let id = ElementId::Name(
            format!(
                "data-table:{}:{}:{}",
                caller.file(),
                caller.line(),
                caller.column()
            )
            .into(),
        );
        let header_focus_handles = columns.iter().map(|_| cx.focus_handle()).collect();
        let select_all_focus_handle = cx.focus_handle();
        let mut select_options = vec![SelectOption::new(usize::MAX, "All Columns")];
        for (idx, column) in columns.iter().enumerate() {
            select_options.push(SelectOption::new(idx, column.header.clone()));
        }

        let search_column_select = cx.new(|cx| {
            Select::new(cx)
                .options(select_options)
                .selected_index(Some(0))
                .placeholder("Select column...")
        });

        cx.subscribe(
            &search_column_select,
            |this, _select, event: &SelectEvent, cx| match event {
                SelectEvent::Change => {
                    let selected = this.search_column_select.read(cx).selected_value().copied();
                    let next_column = if selected == Some(usize::MAX) {
                        None
                    } else {
                        selected
                    };
                    if this.search_column != next_column {
                        this.search_column = next_column;
                        this.state.invalidate_virtual_query();
                        cx.notify();
                    }
                }
            },
        )
        .detach();

        let search_input = cx.new(InputState::new);

        Self {
            id,
            state: DataTableState::new(data, columns),
            resizing_column: None,
            resize_start_x: 0.0,
            resize_start_width: px(0.0),
            sticky_header: true,
            on_load_more: None,
            load_more_threshold: 0.7,
            load_more_triggered: false,
            scroll_handle: ScrollHandle::new(),
            horizontal_scroll_handle: ScrollHandle::new(),
            editing_cell: None,
            edit_input: None,
            edit_column_id: SharedString::from(""),
            edit_old_value: SharedString::from(""),
            use_edit_dialog: true,
            on_cell_edit: None,
            on_cell_double_click: None,
            on_fetch_page: None,
            on_fetch_page_request: None,
            on_row_click: None,
            search_query: String::new(),
            search_column: None,
            show_search: true,
            search_column_select,
            search_input,
            show_selection: false,
            on_selection_change: None,
            on_selection_change_snapshot: None,
            row_actions: Vec::new(),
            context_menu: None,
            empty_message: "No rows to display".into(),
            no_results_message: "No matching rows".into(),
            select_all_focus_handle,
            header_focus_handles,
            style: StyleRefinement::default(),
        }
    }

    /// Overrides the stable identity used to scope generated child element ids.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn sticky_header(mut self, sticky: bool) -> Self {
        self.sticky_header = sticky;
        self
    }

    pub fn show_selection(mut self, show: bool) -> Self {
        self.show_selection = show;
        self
    }

    pub fn on_selection_change<F>(mut self, callback: F) -> Self
    where
        F: Fn(&[usize], &mut Window, &mut Context<Self>) + 'static,
    {
        self.on_selection_change = Some(Box::new(callback));
        self
    }

    /// Set a selection callback that remains exact and bounded for virtual tables.
    ///
    /// Unlike [`Self::on_selection_change`], this callback is invoked for every
    /// transition, including million-row select-all operations represented as
    /// [`DataTableSelectionSnapshot::AllExcept`].
    pub fn on_selection_change_snapshot<F>(mut self, callback: F) -> Self
    where
        F: Fn(&DataTableSelectionSnapshot, &mut Window, &mut Context<Self>) + 'static,
    {
        self.on_selection_change_snapshot = Some(Box::new(callback));
        self
    }

    #[track_caller]
    pub fn new_virtual(
        total_items: usize,
        columns: Vec<ColumnDef<T>>,
        page_size: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let caller = Location::caller();
        let id = ElementId::Name(
            format!(
                "data-table:{}:{}:{}",
                caller.file(),
                caller.line(),
                caller.column()
            )
            .into(),
        );
        let header_focus_handles = columns.iter().map(|_| cx.focus_handle()).collect();
        let select_all_focus_handle = cx.focus_handle();
        let mut select_options = vec![SelectOption::new(usize::MAX, "All Columns")];
        for (idx, column) in columns.iter().enumerate() {
            select_options.push(SelectOption::new(idx, column.header.clone()));
        }

        let search_column_select = cx.new(|cx| {
            Select::new(cx)
                .options(select_options)
                .selected_index(Some(0))
                .placeholder("Select column...")
        });

        cx.subscribe(
            &search_column_select,
            |this, _select, event: &SelectEvent, cx| match event {
                SelectEvent::Change => {
                    let selected = this.search_column_select.read(cx).selected_value().copied();
                    let next_column = if selected == Some(usize::MAX) {
                        None
                    } else {
                        selected
                    };
                    if this.search_column != next_column {
                        this.search_column = next_column;
                        this.state.invalidate_virtual_query();
                        cx.notify();
                    }
                }
            },
        )
        .detach();

        let search_input = cx.new(InputState::new);

        Self {
            id,
            state: DataTableState::new(Vec::new(), columns),
            resizing_column: None,
            resize_start_x: 0.0,
            resize_start_width: px(0.0),
            sticky_header: true,
            on_load_more: None,
            load_more_threshold: 0.7,
            load_more_triggered: false,
            scroll_handle: ScrollHandle::new(),
            horizontal_scroll_handle: ScrollHandle::new(),
            editing_cell: None,
            edit_input: None,
            edit_column_id: SharedString::from(""),
            edit_old_value: SharedString::from(""),
            use_edit_dialog: true,
            on_cell_edit: None,
            on_cell_double_click: None,
            on_fetch_page: None,
            on_fetch_page_request: None,
            on_row_click: None,
            search_query: String::new(),
            search_column: None,
            show_search: true,
            search_column_select,
            search_input,
            show_selection: false,
            on_selection_change: None,
            on_selection_change_snapshot: None,
            row_actions: Vec::new(),
            context_menu: None,
            empty_message: "No rows to display".into(),
            no_results_message: "No matching rows".into(),
            select_all_focus_handle,
            header_focus_handles,
            style: StyleRefinement::default(),
        }
        .with_virtual_backing(total_items, page_size)
    }

    fn with_virtual_backing(mut self, total_items: usize, page_size: usize) -> Self {
        self.state.virtual_reset(total_items, Some(page_size));
        self
    }

    /// Set callback for when user scrolls near the end
    ///
    /// This is useful for infinite scrolling / pagination - load more data when needed
    pub fn on_load_more<F>(mut self, callback: F) -> Self
    where
        F: Fn(&mut Window, &mut Context<Self>) + 'static,
    {
        self.on_load_more = Some(Box::new(callback));
        self
    }

    /// Set the threshold (0.0-1.0) for when to trigger load_more
    ///
    /// Default is 0.7 (70%) - callback fires when user scrolls past 70% of loaded data
    pub fn load_more_threshold(mut self, threshold: f32) -> Self {
        self.load_more_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn on_fetch_page<F>(mut self, callback: F) -> Self
    where
        F: Fn(usize, usize, &mut Window, &mut Context<Self>) + 'static,
    {
        self.on_fetch_page = Some(Box::new(callback));
        self
    }

    /// Sets a generation-safe virtual page callback.
    ///
    /// Prefer this over [`Self::on_fetch_page`] for asynchronous data sources.
    /// Return results through [`Self::set_page_data_for`] and report failures
    /// through [`Self::page_load_failed`] so retries remain possible.
    pub fn on_fetch_page_request<F>(mut self, callback: F) -> Self
    where
        F: Fn(DataTablePageRequest, &mut Window, &mut Context<Self>) + 'static,
    {
        self.on_fetch_page_request = Some(Box::new(callback));
        self
    }

    /// Configures the maximum number of virtual pages retained in memory.
    ///
    /// The default is 16 pages and the minimum is one page.
    pub fn max_cached_pages(mut self, max_cached_pages: usize) -> Self {
        self.state.set_max_cached_pages(max_cached_pages);
        self
    }

    /// Set callback for when a cell is edited
    ///
    /// The callback receives: (row_index, column_id, old_value, new_value, context)
    pub fn on_cell_edit<F>(mut self, callback: F) -> Self
    where
        F: Fn(usize, SharedString, SharedString, SharedString, &mut Context<Self>) + 'static,
    {
        self.on_cell_edit = Some(Box::new(callback));
        self
    }

    /// Set callback for cell double-click: (row_data, column_id, cell_value)
    pub fn on_cell_double_click<F>(mut self, callback: F) -> Self
    where
        F: Fn(&T, SharedString, SharedString, &mut Window, &mut Context<Self>) + 'static,
    {
        self.on_cell_double_click = Some(Box::new(callback));
        self
    }

    /// Set whether to use a confirmation dialog when editing cells
    ///
    /// - `true` (default): Shows a dialog with Save/Cancel buttons before applying changes
    pub fn use_edit_dialog(mut self, use_dialog: bool) -> Self {
        self.use_edit_dialog = use_dialog;
        self
    }

    /// Set callback for when a row is clicked
    ///
    pub fn on_row_click<F>(mut self, callback: F) -> Self
    where
        F: Fn(usize, &T, &mut Window, &mut Context<Self>) + 'static,
    {
        self.on_row_click = Some(Box::new(callback));
        self
    }

    pub fn show_search(mut self, show: bool) -> Self {
        self.show_search = show;
        self
    }

    pub fn row_actions(mut self, actions: Vec<RowAction>) -> Self {
        self.row_actions = actions;
        self
    }

    /// Sets the message displayed when the table's backing data is empty.
    pub fn empty_message(mut self, message: impl Into<SharedString>) -> Self {
        self.empty_message = message.into();
        self
    }

    /// Sets the message displayed when filtering produces no matching rows.
    pub fn no_results_message(mut self, message: impl Into<SharedString>) -> Self {
        self.no_results_message = message.into();
        self
    }

    /// Returns true when sticky headers are enabled.
    pub fn has_sticky_header(&self) -> bool {
        self.sticky_header
    }

    /// Returns true when infinite-scroll load-more is configured.
    pub fn has_load_more_handler(&self) -> bool {
        self.on_load_more.is_some()
    }

    /// Coarse load-more threshold class for content-safe diagnostics.
    pub fn load_more_threshold_class(&self) -> &'static str {
        if self.load_more_threshold <= 0.33 {
            "early"
        } else if self.load_more_threshold <= 0.75 {
            "normal"
        } else {
            "late"
        }
    }

    /// Returns true when page fetching is configured for virtual data.
    pub fn has_fetch_page_handler(&self) -> bool {
        self.on_fetch_page.is_some() || self.on_fetch_page_request.is_some()
    }

    /// Returns true when generation-safe page fetching is configured.
    pub fn has_generation_safe_fetch_handler(&self) -> bool {
        self.on_fetch_page_request.is_some()
    }

    /// Returns true when cell edit callbacks are configured.
    pub fn has_cell_edit_handler(&self) -> bool {
        self.on_cell_edit.is_some()
    }

    /// Returns true when cell double-click callbacks are configured.
    pub fn has_cell_double_click_handler(&self) -> bool {
        self.on_cell_double_click.is_some()
    }

    /// Returns true when row click callbacks are configured.
    pub fn has_row_click_handler(&self) -> bool {
        self.on_row_click.is_some()
    }

    /// Returns true when row selection UI is enabled.
    pub fn has_selection_ui(&self) -> bool {
        self.show_selection
    }

    /// Returns true when selection-change callbacks are configured.
    pub fn has_selection_change_handler(&self) -> bool {
        self.on_selection_change.is_some() || self.on_selection_change_snapshot.is_some()
    }

    /// Returns true when the compressed selection snapshot callback is configured.
    pub fn has_scalable_selection_change_handler(&self) -> bool {
        self.on_selection_change_snapshot.is_some()
    }

    /// Returns true when search UI is enabled.
    pub fn has_search_ui(&self) -> bool {
        self.show_search
    }

    /// Returns true when the active search query is non-empty.
    pub fn has_search_query(&self) -> bool {
        !self.search_query.is_empty()
    }

    /// Returns the active search query length without exposing the query.
    pub fn search_query_len_bytes(&self) -> usize {
        self.search_query.len()
    }

    /// Returns true when search is scoped to one column.
    pub fn has_search_column(&self) -> bool {
        self.search_column.is_some()
    }

    /// Returns the number of row actions.
    pub fn row_action_count(&self) -> usize {
        self.row_actions.len()
    }

    /// Returns true when the context menu is open.
    pub fn has_context_menu(&self) -> bool {
        self.context_menu.is_some()
    }

    /// Returns true when a cell is currently being edited.
    pub fn has_editing_cell(&self) -> bool {
        self.editing_cell.is_some()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "data_table({}, sticky_header={}, load_more={}, load_more_threshold={}, load_more_triggered={}, fetch_page={}, edit_handler={}, double_click_handler={}, row_click_handler={}, selection_ui={}, selection_handler={}, search_ui={}, has_search_query={}, search_query_len_bytes={}, search_column={}, row_actions={}, context_menu={}, editing_cell={}, edit_dialog={})",
            self.state.to_text(),
            self.has_sticky_header(),
            self.has_load_more_handler(),
            self.load_more_threshold_class(),
            self.load_more_triggered,
            self.has_fetch_page_handler(),
            self.has_cell_edit_handler(),
            self.has_cell_double_click_handler(),
            self.has_row_click_handler(),
            self.has_selection_ui(),
            self.has_selection_change_handler(),
            self.has_search_ui(),
            self.has_search_query(),
            self.search_query_len_bytes(),
            self.has_search_column(),
            self.row_action_count(),
            self.has_context_menu(),
            self.has_editing_cell(),
            self.use_edit_dialog
        )
    }

    pub fn set_search(&mut self, query: String, cx: &mut Context<Self>) {
        if self.search_query == query {
            return;
        }
        self.search_query = query;
        self.state.invalidate_virtual_query();
        cx.notify();
    }

    pub fn set_search_column(&mut self, column_index: Option<usize>, cx: &mut Context<Self>) {
        if self.search_column == column_index {
            return;
        }
        self.search_column = column_index;
        self.state.invalidate_virtual_query();
        cx.notify();
    }

    fn row_matches_lowercase_query(&self, row: &T, query_lower: &str) -> bool {
        if let Some(col_idx) = self.search_column {
            if let Some(column) = self.state.columns.get(col_idx) {
                let cell_value = (column.accessor)(row);
                cell_value.to_string().to_lowercase().contains(query_lower)
            } else {
                false
            }
        } else {
            self.state.columns.iter().any(|column| {
                let cell_value = (column.accessor)(row);
                cell_value.to_string().to_lowercase().contains(query_lower)
            })
        }
    }

    fn get_filtered_indices(&self) -> Vec<usize> {
        if let DataBacking::InMemory { data } = &self.state.backing {
            if self.search_query.is_empty() {
                return Vec::new();
            }
            let query_lower = self.search_query.to_lowercase();
            data.iter()
                .enumerate()
                .filter(|(_, row)| self.row_matches_lowercase_query(row, &query_lower))
                .map(|(idx, _)| idx)
                .collect()
        } else {
            // Virtual queries are delegated to the backing data source through
            // `on_fetch_page_request`; materializing the logical row range here
            // would defeat virtualization for million-row tables.
            Vec::new()
        }
    }

    pub fn set_data(&mut self, data: Vec<T>, cx: &mut Context<Self>) {
        let _new_count = data.len();
        self.state.replace_in_memory_data(data);
        self.load_more_triggered = false;
        cx.notify();
    }

    pub fn append_data(&mut self, mut new_data: Vec<T>, cx: &mut Context<Self>) {
        if let DataBacking::InMemory { data } = &mut self.state.backing {
            data.append(&mut new_data);
            let new_count = data.len();
            self.state.scroller.set_total_items(new_count);
            self.load_more_triggered = false;
            cx.notify();
        }
    }

    pub fn virtual_reset(
        &mut self,
        total_items: usize,
        page_size: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        self.state.virtual_reset(total_items, page_size);
        self.load_more_triggered = false;
        cx.notify();
    }

    pub fn set_page_data(&mut self, page_start: usize, rows: Vec<T>, cx: &mut Context<Self>) {
        if self.state.virtual_set_page(page_start, rows, None) {
            self.load_more_triggered = false;
            cx.notify();
        }
    }

    /// Commits a virtual page only when it belongs to the current query.
    ///
    /// Returns `false` for cancelled, duplicate, or stale requests without
    /// mutating the current cache.
    pub fn set_page_data_for(
        &mut self,
        request: DataTablePageRequest,
        rows: Vec<T>,
        cx: &mut Context<Self>,
    ) -> bool {
        let accepted =
            self.state
                .virtual_set_page(request.page_start, rows, Some(request.generation));
        if accepted {
            self.load_more_triggered = false;
            cx.notify();
        }
        accepted
    }

    /// Clears a failed current request so the page can be retried.
    ///
    /// Stale failures are ignored and return `false`.
    pub fn page_load_failed(
        &mut self,
        request: DataTablePageRequest,
        cx: &mut Context<Self>,
    ) -> bool {
        let cleared = self.state.virtual_page_failed(request);
        if cleared {
            cx.notify();
        }
        cleared
    }

    pub fn data(&self) -> &[T] {
        match &self.state.backing {
            DataBacking::InMemory { data } => data,
            _ => &[],
        }
    }

    pub fn data_count(&self) -> usize {
        self.state.total_items()
    }

    /// Number of rows currently resident in the bounded virtual page cache.
    pub fn cached_row_count(&self) -> usize {
        self.state.cached_row_count()
    }

    /// Number of resident virtual pages.
    pub fn cached_page_count(&self) -> usize {
        self.state.cached_page_count()
    }

    /// Configured maximum resident virtual pages, or `None` for in-memory data.
    pub fn max_cached_page_count(&self) -> Option<usize> {
        self.state.max_cached_pages()
    }

    /// Number of row indices retained by the compressed selection model.
    pub fn stored_selection_index_count(&self) -> usize {
        self.state.stored_selection_index_count()
    }

    /// Capture the current row selection without expanding select-all state.
    pub fn selection_snapshot(&self) -> DataTableSelectionSnapshot {
        self.state.selection_snapshot()
    }

    /// Return selected row indices only when they are explicitly stored.
    ///
    /// `None` means the table currently uses an all-except representation. It
    /// never means "no selection", so callers cannot accidentally treat a
    /// million-row selection as empty. Prefer [`Self::selection_snapshot`].
    pub fn selected_rows(&self) -> Option<Vec<usize>> {
        self.selection_snapshot()
            .explicit_selected()
            .map(<[usize]>::to_vec)
    }

    fn emit_selection_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let snapshot = self.state.selection_snapshot();
        if let Some(ref callback) = self.on_selection_change_snapshot {
            callback(&snapshot, window, cx);
        }
        if let Some(ref callback) = self.on_selection_change
            && let Some(selected) =
                snapshot.try_materialize_selected(DATA_TABLE_LEGACY_SELECTION_MATERIALIZATION_LIMIT)
        {
            callback(&selected, window, cx);
        }
    }

    pub fn toggle_row_selection(
        &mut self,
        row_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.toggle_row(row_index);
        self.emit_selection_change(window, cx);
        cx.notify();
    }

    pub fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.selection.select_all();
        self.emit_selection_change(window, cx);
        cx.notify();
    }

    pub fn clear_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.selection.clear();
        self.emit_selection_change(window, cx);
        cx.notify();
    }

    fn is_all_selected(&self) -> bool {
        self.state.selection_snapshot().is_all_selected()
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        if let Some((row_idx, _col_idx)) = self.editing_cell {
            let new_value_string: String = if let Some(ref input) = self.edit_input {
                input.read(cx).content().to_string()
            } else {
                String::new()
            };

            let row_idx_copy = row_idx;
            let column_id = self.edit_column_id.clone();
            let old_value = self.edit_old_value.clone();

            self.editing_cell = None;
            self.edit_input = None;

            if let Some(ref callback) = self.on_cell_edit {
                callback(
                    row_idx_copy,
                    column_id,
                    old_value,
                    new_value_string.into(),
                    cx,
                );
            }

            cx.notify();
        }
    }

    fn render_search_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<T> {
        let theme = Theme::of(cx);

        div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(16.0))
            .py(px(12.0))
            .border_b_1()
            .border_color(theme.tokens.border)
            .bg(theme.tokens.muted.opacity(0.3))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(theme.tokens.muted_foreground)
                            .child("Search in:"),
                    )
                    .child(div().w(px(200.0)).child(self.search_column_select.clone())),
            )
            .child(
                div().w(px(300.0)).child(
                    Input::new(&self.search_input)
                        .size(InputSize::Sm)
                        .placeholder("Type to search...")
                        .on_change({
                            let entity = cx.entity();
                            move |value: SharedString, cx| {
                                entity.update(cx, |this, cx| {
                                    let next_query = value.to_string();
                                    if this.search_query != next_query {
                                        this.search_query = next_query;
                                        this.state.invalidate_virtual_query();
                                        cx.notify();
                                    }
                                });
                            }
                        }),
                ),
            )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<T> {
        let theme = Theme::of(cx);

        let mut header_row = div()
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::Row).row_index(1))
            .flex()
            .w_full();

        if self.show_selection {
            let all_selected = self.is_all_selected();
            let focus_handle = self.select_all_focus_handle.clone();
            let focus_on_mouse = focus_handle.clone();
            let checked_state = if all_selected {
                AccessibilityState::CHECKED
            } else {
                AccessibilityState::NONE
            };

            header_row = header_row.child(
                div()
                    .id(ElementId::NamedChild(
                        Box::new(self.id.clone()),
                        "select-all".into(),
                    ))
                    .accessibility(
                        AccessibilityAttributes::new(AccessibilityRole::CheckBox)
                            .label(if all_selected {
                                "Clear row selection"
                            } else {
                                "Select all rows"
                            })
                            .states(checked_state)
                            .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]),
                    )
                    .track_focus(&focus_handle.tab_index(0).tab_stop(true))
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(50.0))
                    .flex_shrink_0()
                    .px(px(16.0))
                    .py(px(12.0))
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.tokens.muted_foreground)
                    .border_b_1()
                    .border_r_1()
                    .border_color(theme.tokens.border)
                    .bg(theme.tokens.muted.opacity(0.5))
                    .cursor(CursorStyle::PointingHand)
                    .hover(|style| style.bg(theme.tokens.muted.opacity(0.7)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event, window, cx| {
                            window.focus(&focus_on_mouse);
                            if this.is_all_selected() {
                                this.clear_selection(window, cx);
                            } else {
                                this.select_all(window, cx);
                            }
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            if this.is_all_selected() {
                                this.clear_selection(window, cx);
                            } else {
                                this.select_all(window, cx);
                            }
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        div()
                            .w(px(16.0))
                            .h(px(16.0))
                            .rounded(theme.tokens.radius_sm)
                            .border_1()
                            .border_color(if all_selected {
                                theme.tokens.primary
                            } else {
                                theme.tokens.border
                            })
                            .bg(if all_selected {
                                theme.tokens.primary
                            } else {
                                theme.tokens.background
                            }),
                    ),
            );
        }

        let snapshots = Rc::new(
            self.state
                .columns
                .iter()
                .enumerate()
                .map(|(index, column)| TableColumnSnapshot {
                    header: column.header.clone(),
                    width: self.state.column_widths[index],
                    resizable: column.resizable,
                    sortable: column.sortable,
                    focus_handle: self.header_focus_handles[index].clone(),
                })
                .collect::<Vec<_>>(),
        );
        let extents = ColumnExtents(Rc::new(
            snapshots.iter().map(|column| column.width).collect(),
        ));
        let snapshot_count = snapshots.len();
        let table_id = self.id.clone();
        let table_entity = cx.entity().clone();
        let horizontal_scroll = self.horizontal_scroll_handle.clone();
        let header_list = hlist_variable(
            ElementId::NamedChild(Box::new(table_id.clone()), "header-columns".into()),
            snapshot_count,
            extents,
            move |range, _window, app| {
                let theme = use_theme();
                range
                    .map(|col_idx| {
                        let column = &snapshots[col_idx];
                        let is_sorted = table_entity.read(app).state.sort_column == Some(col_idx);
                        let sort_direction = table_entity.read(app).state.sort_direction;
                        let focus_handle = column.focus_handle.clone();
                        let focus_on_mouse = focus_handle.clone();
                        let entity_for_mouse = table_entity.clone();
                        let entity_for_key = table_entity.clone();
                        let entity_for_resize = table_entity.clone();
                        let header_label = if column.sortable {
                            format!("Sort by {}", column.header)
                        } else {
                            column.header.to_string()
                        };
                        let mut header_accessibility =
                            AccessibilityAttributes::new(AccessibilityRole::ColumnHeader)
                                .label(header_label)
                                .column_index(col_idx + 1);
                        if column.sortable {
                            header_accessibility = header_accessibility.actions(vec![
                                AccessibilityAction::Focus,
                                AccessibilityAction::Click,
                            ]);
                        }
                        if is_sorted {
                            header_accessibility =
                                header_accessibility.sort_direction(match sort_direction {
                                    SortDirection::Ascending => {
                                        AccessibilitySortDirection::Ascending
                                    }
                                    SortDirection::Descending => {
                                        AccessibilitySortDirection::Descending
                                    }
                                });
                        }
                        let mut cell = div()
                            .id(ElementId::NamedChild(
                                Box::new(table_id.clone()),
                                format!("header-{col_idx}").into(),
                            ))
                            .accessibility(header_accessibility)
                            .track_focus(&focus_handle.tab_index(0).tab_stop(true))
                            .relative()
                            .flex()
                            .items_center()
                            .justify_between()
                            .size_full()
                            .px(px(16.0))
                            .py(px(12.0))
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.tokens.muted_foreground)
                            .border_b_1()
                            .border_r_1()
                            .border_color(theme.tokens.border)
                            .bg(theme.tokens.muted.opacity(0.5))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(column.header.clone())
                                    .when(is_sorted, |element| {
                                        element.child(div().text_size(px(10.0)).child(
                                            match sort_direction {
                                                SortDirection::Ascending => "▲",
                                                SortDirection::Descending => "▼",
                                            },
                                        ))
                                    }),
                            );
                        if column.sortable {
                            cell = cell
                                .cursor(CursorStyle::PointingHand)
                                .hover(|style| style.bg(theme.tokens.muted.opacity(0.7)))
                                .on_mouse_down(MouseButton::Left, move |_, window, app| {
                                    window.focus(&focus_on_mouse);
                                    entity_for_mouse.update(app, |this, cx| {
                                        let direction = if this.state.sort_column == Some(col_idx) {
                                            match this.state.sort_direction {
                                                SortDirection::Ascending => {
                                                    SortDirection::Descending
                                                }
                                                SortDirection::Descending => {
                                                    SortDirection::Ascending
                                                }
                                            }
                                        } else {
                                            SortDirection::Ascending
                                        };
                                        this.state.sort_by_column(col_idx, direction);
                                        cx.notify();
                                    });
                                })
                                .on_key_down(move |event: &KeyDownEvent, _, app| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        entity_for_key.update(app, |this, cx| {
                                            let direction =
                                                if this.state.sort_column == Some(col_idx) {
                                                    match this.state.sort_direction {
                                                        SortDirection::Ascending => {
                                                            SortDirection::Descending
                                                        }
                                                        SortDirection::Descending => {
                                                            SortDirection::Ascending
                                                        }
                                                    }
                                                } else {
                                                    SortDirection::Ascending
                                                };
                                            this.state.sort_by_column(col_idx, direction);
                                            cx.notify();
                                        });
                                        app.stop_propagation();
                                    }
                                });
                        }
                        if column.resizable {
                            let header = column.header.clone();
                            cell = cell.child(
                                div()
                                    .id(ElementId::NamedChild(
                                        Box::new(table_id.clone()),
                                        format!("resize-{col_idx}").into(),
                                    ))
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Separator)
                                            .label(format!("Resize {header} column")),
                                    )
                                    .absolute()
                                    .right(px(0.0))
                                    .top(px(0.0))
                                    .w(px(4.0))
                                    .h_full()
                                    .cursor(CursorStyle::ResizeLeftRight)
                                    .hover(|style| style.bg(theme.tokens.primary.opacity(0.5)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        move |event: &MouseDownEvent, _, app| {
                                            entity_for_resize.update(app, |this, cx| {
                                                this.resizing_column = Some(col_idx);
                                                this.resize_start_x = event.position.x.into();
                                                this.resize_start_width =
                                                    this.state.column_widths[col_idx];
                                                cx.notify();
                                            });
                                        },
                                    ),
                            );
                        }
                        cell.into_any_element()
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&horizontal_scroll)
        .overscan(2)
        .h(px(self.state.row_height()))
        .flex_1();

        header_row
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::Group).label("Columns"))
            .child(header_list)
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use kael::{TestAppContext, px};

    #[derive(Clone)]
    struct PrivateRow {
        name: SharedString,
        revenue: SharedString,
    }

    fn private_columns() -> Vec<ColumnDef<PrivateRow>> {
        vec![
            ColumnDef::new("private-name", "Secret Customer", |row: &PrivateRow| {
                row.name.clone()
            })
            .width(px(220.0))
            .sortable(true)
            .editable(true),
            ColumnDef::new("private-revenue", "Private Revenue", |row: &PrivateRow| {
                row.revenue.clone()
            })
            .resizable(false)
            .sortable(false),
        ]
    }

    #[::core::prelude::v1::test]
    fn data_table_column_and_action_summary_is_content_safe() {
        let column = ColumnDef::new("private-customer", "Secret Customer", |row: &PrivateRow| {
            row.name.clone()
        })
        .width(px(240.0))
        .min_width(px(120.0))
        .sortable(true)
        .editable(true);

        assert_eq!(column.id_len_bytes(), "private-customer".len());
        assert_eq!(column.header_len_bytes(), "Secret Customer".len());
        assert_eq!(column.width_class(), "wide");
        assert_eq!(column.min_width_class(), "standard");

        let column_summary = column.to_text();
        assert!(column_summary.contains("sortable=true"));
        assert!(column_summary.contains("editable=true"));
        assert!(!column_summary.contains("private-customer"));
        assert!(!column_summary.contains("Secret Customer"));
        assert!(!column_summary.contains("240"));
        assert!(!column_summary.contains("120"));

        let action =
            RowAction::new("private-delete", "Delete Secret Customer", |_, _, _| {}).destructive();
        assert_eq!(action.id_len_bytes(), "private-delete".len());
        assert_eq!(action.label_len_bytes(), "Delete Secret Customer".len());
        assert!(action.is_destructive());

        let action_summary = action.to_text();
        assert!(action_summary.contains("destructive=true"));
        assert!(!action_summary.contains("private-delete"));
        assert!(!action_summary.contains("Delete Secret Customer"));
    }

    #[::core::prelude::v1::test]
    fn data_table_state_summary_is_content_safe() {
        let mut state = DataTableState::new(
            vec![
                PrivateRow {
                    name: "Acme Private".into(),
                    revenue: "$42,000".into(),
                },
                PrivateRow {
                    name: "Zenith Secret".into(),
                    revenue: "$12,000".into(),
                },
            ],
            private_columns(),
        );

        state.sort_by_column(0, SortDirection::Descending);
        state.toggle_row(1);

        assert_eq!(SortDirection::Descending.to_text(), "descending");
        assert_eq!(state.column_count(), 2);
        assert_eq!(state.row_count(), 2);
        assert_eq!(state.backing_kind(), "in_memory");
        assert_eq!(state.selected_count(), 1);
        assert!(state.has_sort());
        assert_eq!(state.sort_column_index(), Some(0));
        assert_eq!(state.sort_direction_key(), "descending");
        assert_eq!(state.sortable_column_count(), 1);
        assert_eq!(state.editable_column_count(), 1);
        assert_eq!(state.resizable_column_count(), 1);

        let summary = state.to_text();
        assert!(summary.contains("columns=2"));
        assert!(summary.contains("rows=2"));
        assert!(summary.contains("backing=in_memory"));
        assert!(summary.contains("selected=1"));
        assert!(!summary.contains("Acme Private"));
        assert!(!summary.contains("Zenith Secret"));
        assert!(!summary.contains("Secret Customer"));
        assert!(!summary.contains("private-name"));
        assert!(!summary.contains("$42,000"));
    }

    #[::core::prelude::v1::test]
    fn sorting_preserves_selected_records_and_rejects_inert_columns() {
        let mut state = DataTableState::new(
            vec![
                PrivateRow {
                    name: "Bravo".into(),
                    revenue: "$20".into(),
                },
                PrivateRow {
                    name: "Alpha".into(),
                    revenue: "$10".into(),
                },
            ],
            private_columns(),
        );
        state.toggle_row(0);

        state.sort_by_column(0, SortDirection::Ascending);

        assert_eq!(
            state.selection_snapshot(),
            DataTableSelectionSnapshot::Explicit {
                total_rows: 2,
                selected: vec![1],
            }
        );
        assert_eq!(state.get_row(1).map(|row| row.name.as_ref()), Some("Bravo"));

        state.sort_by_column(1, SortDirection::Descending);
        assert_eq!(state.sort_column_index(), Some(0));
        state.sort_by_column(99, SortDirection::Descending);
        assert_eq!(state.sort_column_index(), Some(0));
    }

    #[::core::prelude::v1::test]
    fn data_changes_and_column_resizing_keep_state_valid() {
        let mut state = DataTableState::new(
            vec![PrivateRow {
                name: "Selected".into(),
                revenue: "$1".into(),
            }],
            private_columns(),
        );
        state.toggle_row(0);
        state.toggle_row(9);
        assert_eq!(
            state.selection_snapshot(),
            DataTableSelectionSnapshot::Explicit {
                total_rows: 1,
                selected: vec![0],
            }
        );

        state.resize_column(0, px(f32::NAN));
        assert_eq!(state.column_widths[0], px(220.0));
        state.resize_column(0, px(1.0));
        assert_eq!(state.column_widths[0], px(80.0));

        state.replace_in_memory_data(vec![PrivateRow {
            name: "Replacement".into(),
            revenue: "$2".into(),
        }]);
        assert_eq!(state.selected_count(), 0);
    }

    #[::core::prelude::v1::test]
    fn short_in_memory_tables_fit_their_rows_but_virtual_tables_keep_a_viewport() {
        let short = DataTableState::new(
            vec![
                PrivateRow {
                    name: "One".into(),
                    revenue: "$1".into(),
                },
                PrivateRow {
                    name: "Two".into(),
                    revenue: "$2".into(),
                },
            ],
            private_columns(),
        );
        assert_eq!(short.effective_viewport_height(2), 96.0);
        assert_eq!(short.effective_viewport_height(0), 48.0);

        let mut virtual_table = DataTableState::new(Vec::<PrivateRow>::new(), private_columns());
        virtual_table.virtual_reset(10_000, Some(200));
        assert_eq!(virtual_table.effective_viewport_height(10_000), 600.0);
    }

    #[::core::prelude::v1::test]
    fn nested_table_scroll_only_captures_wheel_input_it_can_consume() {
        assert!(!should_capture_vertical_scroll(0.0, 192.0, 192.0, -40.0));
        assert!(!should_capture_vertical_scroll(0.0, 192.0, 192.0, 40.0));

        assert!(should_capture_vertical_scroll(0.0, 960.0, 240.0, -40.0));
        assert!(!should_capture_vertical_scroll(0.0, 960.0, 240.0, 40.0));
        assert!(should_capture_vertical_scroll(320.0, 960.0, 240.0, -40.0));
        assert!(should_capture_vertical_scroll(320.0, 960.0, 240.0, 40.0));
        assert!(!should_capture_vertical_scroll(720.0, 960.0, 240.0, -40.0));
        assert!(should_capture_vertical_scroll(720.0, 960.0, 240.0, 40.0));

        assert!(!should_capture_vertical_scroll(
            f32::NAN,
            960.0,
            240.0,
            -40.0
        ));
    }

    #[::core::prelude::v1::test]
    fn data_table_virtual_summary_is_content_safe() {
        let mut state = DataTableState::new(Vec::<PrivateRow>::new(), private_columns());
        state.virtual_reset(10_000, Some(250));
        let request = state.virtual_requests_for_range(0..1).remove(0);
        assert!(state.virtual_set_page(
            request.page_start(),
            vec![PrivateRow {
                name: "Cached Secret Row".into(),
                revenue: "$99,000".into(),
            }],
            Some(request.generation()),
        ));

        assert_eq!(state.backing_kind(), "virtual");
        assert!(state.is_virtual());
        assert_eq!(state.row_count(), 10_000);
        assert_eq!(state.cached_row_count(), 1);
        assert_eq!(state.page_size(), Some(250));

        let summary = state.to_text();
        assert!(summary.contains("backing=virtual"));
        assert!(summary.contains("rows=10000"));
        assert!(summary.contains("cached_rows=1"));
        assert!(!summary.contains("Cached Secret Row"));
        assert!(!summary.contains("$99,000"));
    }

    #[::core::prelude::v1::test]
    fn virtual_pages_use_a_bounded_lru_cache() {
        let mut state = DataTableState::new(Vec::<PrivateRow>::new(), private_columns());
        state.virtual_reset(1_000_000, Some(10));
        state.set_max_cached_pages(2);

        for page_start in [0, 10, 20] {
            let request = state
                .virtual_requests_for_range(page_start..page_start + 1)
                .remove(0);
            let rows = (0..10)
                .map(|offset| PrivateRow {
                    name: format!("row-{}", page_start + offset).into(),
                    revenue: "$1".into(),
                })
                .collect();
            assert!(
                state.virtual_set_page(request.page_start(), rows, Some(request.generation()),)
            );
        }

        assert_eq!(state.row_count(), 1_000_000);
        assert_eq!(state.cached_page_count(), 2);
        assert_eq!(state.cached_row_count(), 20);
        assert!(state.get_row(0).is_none());
        assert!(state.get_row(10).is_some());
        assert!(state.get_row(20).is_some());
    }

    #[::core::prelude::v1::test]
    fn million_row_select_all_is_constant_space_and_exact() {
        let mut state = DataTableState::new(Vec::<PrivateRow>::new(), private_columns());
        state.virtual_reset(1_000_000, Some(128));

        state.selection.select_all();
        assert_eq!(state.selected_count(), 1_000_000);
        assert_eq!(state.stored_selection_index_count(), 0);
        assert_eq!(state.selection_representation_key(), "all_except");
        assert!(state.is_row_selected(0));
        assert!(state.is_row_selected(999_999));

        state.toggle_row(123_456);
        let snapshot = state.selection_snapshot();
        assert_eq!(snapshot.selected_count(), 999_999);
        assert_eq!(snapshot.stored_index_count(), 1);
        assert!(!snapshot.contains(123_456));
        assert!(snapshot.contains(123_455));
        assert_eq!(snapshot.deselected(), Some([123_456].as_slice()));
        assert!(snapshot.try_materialize_selected(16_384).is_none());
    }

    #[::core::prelude::v1::test]
    fn all_except_selection_remaps_exclusions_when_in_memory_rows_sort() {
        let mut state = DataTableState::new(
            vec![
                PrivateRow {
                    name: "Bravo".into(),
                    revenue: "$20".into(),
                },
                PrivateRow {
                    name: "Alpha".into(),
                    revenue: "$10".into(),
                },
            ],
            private_columns(),
        );
        state.selection.select_all();
        state.toggle_row(0);

        state.sort_by_column(0, SortDirection::Ascending);

        let snapshot = state.selection_snapshot();
        assert_eq!(snapshot.deselected(), Some([1].as_slice()));
        assert!(snapshot.contains(0));
        assert!(!snapshot.contains(1));
    }

    #[::core::prelude::v1::test]
    fn bounded_snapshot_materialization_is_exact() {
        let all_except = DataTableSelectionSnapshot::AllExcept {
            total_rows: 5,
            deselected: vec![1, 3],
        };
        assert_eq!(all_except.try_materialize_selected(3), Some(vec![0, 2, 4]));
        assert!(all_except.try_materialize_selected(2).is_none());
        assert_eq!(all_except.representation_key(), "all_except");

        let explicit = DataTableSelectionSnapshot::Explicit {
            total_rows: 5,
            selected: vec![1, 4],
        };
        assert_eq!(explicit.explicit_selected(), Some([1, 4].as_slice()));
        assert!(explicit.deselected().is_none());
        assert_eq!(explicit.try_materialize_selected(2), Some(vec![1, 4]));
    }

    #[::core::prelude::v1::test]
    fn stale_virtual_page_responses_cannot_overwrite_a_new_query() {
        let mut state = DataTableState::new(Vec::<PrivateRow>::new(), private_columns());
        state.virtual_reset(1_000, Some(100));
        let stale = state.virtual_requests_for_range(0..1).remove(0);

        state.virtual_reset(1_000, None);
        let current = state.virtual_requests_for_range(0..1).remove(0);
        assert_ne!(stale.generation(), current.generation());
        assert!(!state.virtual_set_page(
            stale.page_start(),
            vec![PrivateRow {
                name: "stale".into(),
                revenue: "$0".into(),
            }],
            Some(stale.generation()),
        ));
        assert!(state.get_row(0).is_none());

        assert!(state.virtual_set_page(
            current.page_start(),
            vec![PrivateRow {
                name: "current".into(),
                revenue: "$1".into(),
            }],
            Some(current.generation()),
        ));
        assert_eq!(state.get_row(0).unwrap().name, "current");
    }

    #[::core::prelude::v1::test]
    fn virtual_requests_are_deduplicated_and_failures_can_retry() {
        let mut state = DataTableState::new(Vec::<PrivateRow>::new(), private_columns());
        state.virtual_reset(500, Some(50));
        let request = state.virtual_requests_for_range(10..80);
        assert_eq!(request.len(), 2);
        assert!(state.virtual_requests_for_range(10..80).is_empty());
        assert_eq!(state.in_flight_page_count(), 2);

        assert!(state.virtual_page_failed(request[0]));
        let retry = state.virtual_requests_for_range(0..1);
        assert_eq!(retry, vec![request[0]]);
    }

    #[::core::prelude::v1::test]
    fn data_table_summary_is_content_safe() {
        let cx = TestAppContext::single();
        let table = cx.update(|cx| {
            cx.new(|cx| {
                DataTable::new(
                    vec![PrivateRow {
                        name: "Delta Confidential".into(),
                        revenue: "$7,000".into(),
                    }],
                    private_columns(),
                    cx,
                )
                .show_selection(true)
                .on_selection_change(|_, _, _| {})
                .on_load_more(|_, _| {})
                .on_fetch_page(|_, _, _, _| {})
                .on_cell_edit(|_, _, _, _, _| {})
                .on_cell_double_click(|_, _, _, _, _| {})
                .on_row_click(|_, _, _, _| {})
                .row_actions(vec![RowAction::new(
                    "private-open",
                    "Open Secret Customer",
                    |_, _, _| {},
                )])
            })
        });

        cx.update(|cx| {
            let mut table = table.update(cx, |table, cx| {
                table.set_search("delta confidential".to_string(), cx);
                table.set_search_column(Some(0), cx);
                table.to_text()
            });

            assert!(table.contains("selection_ui=true"));
            assert!(table.contains("load_more=true"));
            assert!(table.contains("fetch_page=true"));
            assert!(table.contains("edit_handler=true"));
            assert!(table.contains("row_actions=1"));
            assert!(table.contains("has_search_query=true"));
            assert!(!table.contains("delta confidential"));
            assert!(!table.contains("Delta Confidential"));
            assert!(!table.contains("Open Secret Customer"));
            assert!(!table.contains("$7,000"));

            table.clear();
        });
    }
}

impl<T: Clone + 'static> Styled for DataTable<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + 'static> Render for DataTable<T> {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();

        let user_style = self.style.clone();

        let (total_items, filtered_indices): (usize, Option<Rc<Vec<usize>>>) = match &self
            .state
            .backing
        {
            DataBacking::InMemory { data } if self.search_query.is_empty() => (data.len(), None),
            DataBacking::InMemory { .. } => {
                let indices = Rc::new(self.get_filtered_indices());
                (indices.len(), Some(indices))
            }
            DataBacking::Virtual { .. } => (self.state.total_items(), None),
        };
        let viewport_height = self.state.effective_viewport_height(total_items);
        let row_extent = px(self.state.row_height());
        let column_extents = ColumnExtents(Rc::new(self.state.column_widths.clone()));
        let horizontal_scroll_handle = self.horizontal_scroll_handle.clone();

        let view_entity = cx.entity().clone();
        let filtered_indices_for_render = filtered_indices.clone();
        let table_id = self.id.clone();
        let table_id_for_rows = table_id.clone();
        let column_extents_for_rows = column_extents.clone();
        let horizontal_scroll_for_rows = horizontal_scroll_handle.clone();
        let renderer = move |this: &mut DataTable<T>,
                             range: Range<usize>,
                             window: &mut Window,
                             cx: &mut Context<DataTable<T>>| {
            let theme = Theme::of(cx).clone();
            range
                .map(|row_idx| {
                    let actual_idx = if let Some(ref map) = filtered_indices_for_render {
                        map.get(row_idx).copied().unwrap_or(row_idx)
                    } else {
                        row_idx
                    };

                    if let Some(row_data) = this.state.get_row(actual_idx) {
                        let is_selected = this.state.is_row_selected(actual_idx);
                        let row_clickable = this.on_row_click.is_some();
                        let row_id = ElementId::NamedChild(
                            Box::new(table_id_for_rows.clone()),
                            format!("row-{actual_idx}").into(),
                        );
                        let row_focus_handle = window
                            .use_keyed_state(row_id.clone(), cx, |_, cx| cx.focus_handle())
                            .read(cx)
                            .clone();
                        let mut row_state = AccessibilityState::NONE;
                        if is_selected {
                            row_state |= AccessibilityState::SELECTED;
                        }

                        let mut row_div = div()
                            .id(row_id)
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::Row)
                                    .label(format!("Row {}", actual_idx + 1))
                                    .row_index(row_idx + 2)
                                    .states(row_state),
                            )
                            .when(row_clickable, |row| {
                                row.track_focus(
                                    &row_focus_handle.clone().tab_index(0).tab_stop(true),
                                )
                            })
                            .flex()
                            .w_full()
                            .h(row_extent)
                            .bg(if is_selected {
                                theme.tokens.accent.opacity(0.2)
                            } else if row_idx % 2 == 0 {
                                theme.tokens.background
                            } else {
                                theme.tokens.muted.opacity(0.3)
                            })
                            .hover(|style| style.bg(theme.tokens.accent.opacity(0.1)));

                        if !this.row_actions.is_empty() {
                            row_div = row_div.on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                    this.context_menu = Some((actual_idx, event.position));
                                    cx.notify();
                                }),
                            );
                        }

                        if this.on_row_click.is_some() {
                            let focus_on_mouse = row_focus_handle.clone();
                            row_div = row_div
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                        if event.click_count > 1 {
                                            return;
                                        }
                                        window.focus(&focus_on_mouse);
                                        if let Some(row) = this.state.get_row(actual_idx)
                                            && let Some(ref cb) = this.on_row_click
                                        {
                                            (cb)(actual_idx, row, window, cx);
                                        }
                                    }),
                                )
                                .on_key_down(cx.listener(
                                    move |this, event: &KeyDownEvent, window, cx| {
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            if let Some(row) = this.state.get_row(actual_idx)
                                                && let Some(ref cb) = this.on_row_click
                                            {
                                                (cb)(actual_idx, row, window, cx);
                                            }
                                            cx.stop_propagation();
                                        }
                                    },
                                ));
                        }

                        if this.show_selection {
                            let selection_id = ElementId::NamedChild(
                                Box::new(table_id_for_rows.clone()),
                                format!("select-{actual_idx}").into(),
                            );
                            let selection_focus_handle = window
                                .use_keyed_state(selection_id.clone(), cx, |_, cx| {
                                    cx.focus_handle()
                                })
                                .read(cx)
                                .clone();
                            let focus_on_mouse = selection_focus_handle.clone();
                            row_div = row_div.child(
                                div()
                                    .id(selection_id)
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::CheckBox)
                                            .label(format!("Select row {}", actual_idx + 1))
                                            .states(if is_selected {
                                                AccessibilityState::CHECKED
                                            } else {
                                                AccessibilityState::NONE
                                            })
                                            .actions(vec![
                                                AccessibilityAction::Focus,
                                                AccessibilityAction::Click,
                                            ]),
                                    )
                                    .track_focus(
                                        &selection_focus_handle.tab_index(0).tab_stop(true),
                                    )
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(50.0))
                                    .flex_shrink_0()
                                    .px(px(16.0))
                                    .py(px(12.0))
                                    .border_b_1()
                                    .border_r_1()
                                    .border_color(theme.tokens.border.opacity(0.5))
                                    .cursor(CursorStyle::PointingHand)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _event, window, cx| {
                                            window.focus(&focus_on_mouse);
                                            this.toggle_row_selection(actual_idx, window, cx);
                                            cx.stop_propagation();
                                        }),
                                    )
                                    .on_key_down(cx.listener(
                                        move |this, event: &KeyDownEvent, window, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) {
                                                this.toggle_row_selection(actual_idx, window, cx);
                                                cx.stop_propagation();
                                            }
                                        },
                                    ))
                                    .child(
                                        div()
                                            .w(px(16.0))
                                            .h(px(16.0))
                                            .rounded(theme.tokens.radius_sm)
                                            .border_1()
                                            .border_color(if is_selected {
                                                theme.tokens.primary
                                            } else {
                                                theme.tokens.border
                                            })
                                            .bg(if is_selected {
                                                theme.tokens.primary
                                            } else {
                                                theme.tokens.background
                                            }),
                                    ),
                            );
                        }


                        let row_entity = cx.entity().clone();
                        let row_data = row_data.clone();
                        let horizontal_scroll = horizontal_scroll_for_rows.clone();
                        let row_columns = hlist_variable_view(
                            row_entity,
                            ElementId::NamedChild(
                                Box::new(table_id_for_rows.clone()),
                                format!("row-columns-{actual_idx}").into(),
                            ),
                            this.state.columns.len(),
                            column_extents_for_rows.clone(),
                            move |this, visible_columns, _window, cx| {
                                let theme = Theme::of(cx).clone();
                                visible_columns
                                    .map(|col_idx| {
                                        let column = &this.state.columns[col_idx];
                                        let cell_value = (column.accessor)(&row_data);
                                        let is_editing =
                                            this.editing_cell == Some((actual_idx, col_idx));
                                        let mut cell = div()
                                            .accessibility(
                                                AccessibilityAttributes::new(
                                                    AccessibilityRole::Cell,
                                                )
                                                .label(format!(
                                                    "{} column, row {}",
                                                    column.header,
                                                    actual_idx + 1
                                                ))
                                                .row_index(row_idx + 2)
                                                .column_index(col_idx + 1)
                                                .state(
                                                    AccessibilityState::READ_ONLY,
                                                    !column.editable,
                                                ),
                                            )
                                            .flex()
                                            .items_center()
                                            .size_full()
                                            .px(px(16.0))
                                            .py(px(12.0))
                                            .text_size(px(13.0))
                                            .text_color(theme.tokens.foreground)
                                            .border_b_1()
                                            .border_r_1()
                                            .border_color(theme.tokens.border.opacity(0.5))
                                            .overflow_hidden()
                                            .text_ellipsis();

                                        if column.editable && !is_editing {
                                            let value_for_edit = cell_value.clone();
                                            let column_id = column.id.clone();
                                            let row_for_edit = row_data.clone();
                                            cell = cell.cursor(CursorStyle::IBeam).on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                                    if event.click_count < 2 {
                                                        return;
                                                    }
                                                    if let Some(ref callback) = this.on_cell_double_click {
                                                        callback(
                                                            &row_for_edit,
                                                            column_id.clone(),
                                                            value_for_edit.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                        return;
                                                    }

                                                    let input_state = cx.new(|cx| {
                                                        let mut state = InputState::new(cx);
                                                        state.set_value(
                                                            value_for_edit.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                        state
                                                    });
                                                    use crate::components::input::InputEvent;
                                                    cx.subscribe(
                                                        &input_state,
                                                        |this, _, event: &InputEvent, cx| match event {
                                                            InputEvent::Enter => this.save_edit(cx),
                                                            InputEvent::Blur if !this.use_edit_dialog => {
                                                                this.save_edit(cx);
                                                            }
                                                            _ => {}
                                                        },
                                                    )
                                                    .detach();
                                                    this.editing_cell = Some((actual_idx, col_idx));
                                                    this.edit_input = Some(input_state);
                                                    this.edit_column_id = column_id.clone();
                                                    this.edit_old_value = value_for_edit.clone();
                                                    if let Some(ref input) = this.edit_input {
                                                        window.focus(&input.read(cx).focus_handle(cx));
                                                    }
                                                    cx.notify();
                                                }),
                                            );
                                        }

                                        if is_editing {
                                            if let Some(ref input_state) = this.edit_input {
                                                cell.child(Input::new(input_state).size(InputSize::Sm))
                                                    .into_any_element()
                                            } else {
                                                cell.child(cell_value).into_any_element()
                                            }
                                        } else {
                                            cell.child(cell_value).into_any_element()
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            },
                        )
                        .track_scroll(&horizontal_scroll)
                        .overscan(2)
                        .h(row_extent)
                        .flex_1();

                        row_div.child(row_columns).into_any_element()
                    } else {
                        let mut skeleton_row =
                            div().flex().w_full().h(row_extent).bg(
                                if row_idx % 2 == 0 {
                                    theme.tokens.background
                                } else {
                                    theme.tokens.muted.opacity(0.3)
                                },
                            );
                        if this.show_selection {
                            skeleton_row = skeleton_row.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(50.0))
                                    .flex_shrink_0()
                                    .px(px(16.0))
                                    .py(px(12.0))
                                    .border_b_1()
                                    .border_r_1()
                                    .border_color(theme.tokens.border.opacity(0.5)),
                            );
                        }

                        let horizontal_scroll = horizontal_scroll_for_rows.clone();
                        let skeleton_theme = theme.clone();
                        let skeleton_columns = hlist_variable(
                            ElementId::NamedChild(
                                Box::new(table_id_for_rows.clone()),
                                format!("skeleton-columns-{actual_idx}").into(),
                            ),
                            this.state.columns.len(),
                            column_extents_for_rows.clone(),
                            move |visible_columns, _window, _app| {
                                visible_columns
                                    .map(|_| {
                                        div()
                                            .flex()
                                            .items_center()
                                            .size_full()
                                            .px(px(16.0))
                                            .py(px(12.0))
                                            .border_b_1()
                                            .border_r_1()
                                            .border_color(skeleton_theme.tokens.border.opacity(0.5))
                                            .child(
                                                div()
                                                    .w(px(96.0))
                                                    .h(px(12.0))
                                                    .rounded(skeleton_theme.tokens.radius_sm)
                                                    .bg(skeleton_theme.tokens.muted.opacity(0.6)),
                                            )
                                            .into_any_element()
                                    })
                                    .collect::<Vec<_>>()
                            },
                        )
                        .track_scroll(&horizontal_scroll)
                        .overscan(2)
                        .h(row_extent)
                        .flex_1();
                        skeleton_row.child(skeleton_columns).into_any_element()
                    }
                })
                .collect::<Vec<_>>()
        };

        let view_for_visible = view_entity.clone();
        let view_for_near_end = view_entity.clone();
        let rendered_total_items = total_items;
        let body_scroll = vlist_uniform_view(
            view_entity,
            ElementId::NamedChild(Box::new(table_id.clone()), "body-list".into()),
            total_items,
            row_extent,
            renderer,
        )
        .track_scroll(&self.scroll_handle)
        .overscan(8)
        .h(px(viewport_height))
        .on_visible_range(move |range, window, app| {
            let start = range.start;
            let end = range.end;
            let _ = window;
            view_for_visible.update(app, |this: &mut DataTable<T>, cx| {
                let total_items = rendered_total_items;
                if total_items > 0 && !this.load_more_triggered {
                    let progress = end as f32 / total_items as f32;
                    if progress >= this.load_more_threshold
                        && let Some(ref callback) = this.on_load_more
                    {
                        this.load_more_triggered = true;
                        callback(window, cx);
                    }
                }

                if this.on_fetch_page_request.is_some() || this.on_fetch_page.is_some() {
                    let requests = this.state.virtual_requests_for_range(start..end);
                    for request in requests {
                        if let Some(ref fetch_cb) = this.on_fetch_page_request {
                            fetch_cb(request, window, cx);
                        } else if let Some(ref fetch_cb) = this.on_fetch_page {
                            fetch_cb(request.page_start, request.page_size, window, cx);
                        }
                    }
                }
            });
        })
        .on_near_end(self.load_more_threshold, move |window, app| {
            view_for_near_end.update(app, |this: &mut DataTable<T>, cx| {
                if let Some(ref callback) = this.on_load_more {
                    this.load_more_triggered = true;
                    callback(window, cx);
                }
            });
        });

        let body_content_height = total_items as f32 * self.state.row_height();
        let body_viewport_height = viewport_height;
        let empty_body_message = if self.search_query.is_empty() {
            self.empty_message.clone()
        } else {
            self.no_results_message.clone()
        };
        let body_content = if total_items == 0 {
            div()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .px(px(16.0))
                .text_size(px(13.0))
                .text_color(theme.tokens.muted_foreground)
                .child(empty_body_message)
                .into_any_element()
        } else {
            body_scroll.into_any_element()
        };
        let body_container = div()
            .id(ElementId::NamedChild(
                Box::new(table_id.clone()),
                "body".into(),
            ))
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group).label("Table rows"),
            )
            .h(px(viewport_height))
            .on_scroll_wheel(
                cx.listener(move |view, event: &ScrollWheelEvent, _window, cx| {
                    let delta_y: f32 = match &event.delta {
                        ScrollDelta::Lines(delta) => delta.y,
                        ScrollDelta::Pixels(delta) => delta.y.into(),
                    };

                    let scroll_offset = view.scroll_handle.offset();
                    let scroll_y: f32 = (-scroll_offset.y).into();
                    if should_capture_vertical_scroll(
                        scroll_y,
                        body_content_height,
                        body_viewport_height,
                        delta_y,
                    ) {
                        cx.stop_propagation();
                    }
                }),
            )
            .child(body_content);

        let scrollable_content = div()
            .id(ElementId::NamedChild(
                Box::new(table_id.clone()),
                "content".into(),
            ))
            .flex()
            .flex_col()
            .overflow_hidden()
            .w_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .child(self.render_header(cx))
                    .child(body_container),
            )
            .child(
                div().w_full().h(px(12.0)).child(
                    scroll_bar(
                        ElementId::NamedChild(
                            Box::new(table_id.clone()),
                            "horizontal-scrollbar".into(),
                        ),
                        horizontal_scroll_handle,
                    )
                    .horizontal(),
                ),
            );

        let table_div = if self.sticky_header {
            div()
                .flex()
                .flex_col()
                .w_full()
                .border_1()
                .border_color(theme.tokens.border)
                .rounded(theme.tokens.radius_lg)
                .overflow_hidden()
                .bg(theme.tokens.card)
                .shadow_sm()
                .when(self.show_search, |div| {
                    div.child(self.render_search_bar(cx))
                })
                .child(scrollable_content)
                .map(|mut this| {
                    this.style().refine(&user_style);
                    this
                })
        } else {
            div()
                .flex()
                .flex_col()
                .w_full()
                .border_1()
                .border_color(theme.tokens.border)
                .rounded(theme.tokens.radius_lg)
                .overflow_hidden()
                .bg(theme.tokens.card)
                .shadow_sm()
                .when(self.show_search, |div| {
                    div.child(self.render_search_bar(cx))
                })
                .child(scrollable_content)
                .map(|mut this| {
                    this.style().refine(&user_style);
                    this
                })
        };

        let context_menu_elem = self
            .context_menu
            .map(|(row_idx, position)| self.render_context_menu(row_idx, position, cx));

        div()
            .id(table_id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Table)
                    .label("Data table")
                    .row_count(total_items.saturating_add(1))
                    .column_count(self.state.columns.len()),
            )
            .relative()
            .w_full()
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if let Some(col_idx) = this.resizing_column {
                    let current_x: f32 = event.position.x.into();
                    let delta_x = current_x - this.resize_start_x;
                    let new_width_f32: f32 = this.resize_start_width.into();
                    let new_width = px(new_width_f32 + delta_x);

                    let min_width = this.state.columns[col_idx].min_width;
                    let final_width = if new_width > min_width {
                        new_width
                    } else {
                        min_width
                    };

                    this.state.resize_column(col_idx, final_width);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    if this.resizing_column.is_some() {
                        this.resizing_column = None;
                        cx.notify();
                    }
                }),
            )
            .child(table_div)
            .children(context_menu_elem)
    }
}

impl<T: Clone + 'static> DataTable<T> {
    fn render_context_menu(
        &self,
        row_idx: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<T> {
        let theme = Theme::of(cx);

        deferred(
            anchored()
                .position(position)
                .snap_to_window_with_margin(px(8.))
                .anchor(Corner::TopLeft)
                .child(
                    div()
                        .occlude()
                        .min_w(px(200.0))
                        .bg(theme.tokens.popover)
                        .border_1()
                        .border_color(theme.tokens.border)
                        .rounded(theme.tokens.radius_lg)
                        .shadow_xl()
                        .p(px(4.0))
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.context_menu = None;
                            cx.notify();
                        }))
                        .children(self.row_actions.iter().map(|action| {
                            let action = action.clone();
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .px(px(12.0))
                                .py(px(8.0))
                                .rounded(theme.tokens.radius_sm)
                                .cursor(CursorStyle::PointingHand)
                                .transition(theme.tokens.transition_fast)
                                .hover(|style| style.bg(theme.tokens.accent))
                                .text_size(px(14.0))
                                .text_color(if action.destructive {
                                    theme.tokens.destructive
                                } else {
                                    theme.tokens.popover_foreground
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _event, window, cx| {
                                        (action.on_click)(row_idx, window, cx);
                                        this.context_menu = None;
                                        cx.notify();
                                    }),
                                )
                                .when_some(action.icon, |div, icon| {
                                    div.child(
                                        crate::components::icon::Icon::new(icon)
                                            .size(px(16.0))
                                            .color(if action.destructive {
                                                theme.tokens.destructive
                                            } else {
                                                theme.tokens.popover_foreground
                                            }),
                                    )
                                })
                                .child(action.label)
                                .into_any_element()
                        })),
                ),
        )
    }
}

//! A bounded, two-axis virtual spreadsheet grid.
//!
//! [`VirtualSheetGrid`] keeps sheet dimensions logical. Only visible rows,
//! visible columns, bounded frozen panes, cached tiles, and sparse edits are
//! materialized. This makes the same component suitable for native and WebAssembly
//! applications without asking either platform to allocate a million rows.

use crate::components::input::{Input, InputSize};
use crate::components::input_state::InputState;
use crate::theme::Theme;
use crate::virtual_list::{hlist_uniform, vlist_uniform_view};
use kael::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::ops::Range;
use std::rc::Rc;

/// Maximum logical row count supported by the spreadsheet primitive.
pub const VIRTUAL_SHEET_MAX_ROWS: usize = 1_000_000;
/// Maximum logical column count supported by the spreadsheet primitive (XFD).
pub const VIRTUAL_SHEET_MAX_COLUMNS: usize = 16_384;
/// Maximum number of values accepted in one tile.
pub const VIRTUAL_SHEET_MAX_TILE_CELLS: usize = 4_096;
/// Maximum UTF-8 bytes accepted in one tile response.
pub const VIRTUAL_SHEET_MAX_TILE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum UTF-8 bytes accepted in one cell value.
pub const VIRTUAL_SHEET_MAX_CELL_BYTES: usize = 1024 * 1024;
/// Maximum number of tiles retained by the default cache.
pub const VIRTUAL_SHEET_DEFAULT_CACHE_TILES: usize = 128;
/// Maximum number of outstanding tile requests retained by default.
pub const VIRTUAL_SHEET_DEFAULT_PENDING_TILES: usize = 64;
/// Maximum number of cells copied or pasted in one operation.
pub const VIRTUAL_SHEET_CLIPBOARD_CELL_LIMIT: usize = 65_536;
/// Maximum UTF-8 bytes accepted from, or produced for, clipboard interchange.
pub const VIRTUAL_SHEET_CLIPBOARD_BYTE_LIMIT: usize = 8 * 1024 * 1024;
/// Maximum number of cells held in the sparse local edit overlay by default.
pub const VIRTUAL_SHEET_DEFAULT_EDIT_LIMIT: usize = 100_000;
/// Maximum UTF-8 bytes retained in the sparse edit overlay by default.
pub const VIRTUAL_SHEET_DEFAULT_EDIT_BYTE_LIMIT: usize = 64 * 1024 * 1024;
/// Maximum number of cell deltas retained across undo batches by default.
pub const VIRTUAL_SHEET_DEFAULT_UNDO_CELL_LIMIT: usize = 65_536;
/// Maximum UTF-8 bytes retained across undo and redo batches by default.
pub const VIRTUAL_SHEET_DEFAULT_UNDO_BYTE_LIMIT: usize = 64 * 1024 * 1024;
/// Frozen panes are deliberately bounded so a hostile count cannot materialize
/// an unbounded number of retained cells.
pub const VIRTUAL_SHEET_MAX_FROZEN_ROWS: usize = 32;
/// See [`VIRTUAL_SHEET_MAX_FROZEN_ROWS`].
pub const VIRTUAL_SHEET_MAX_FROZEN_COLUMNS: usize = 32;

const DEFAULT_TILE_ROWS: usize = 64;
const DEFAULT_TILE_COLUMNS: usize = 32;
const DEFAULT_ROW_HEIGHT: f32 = 28.0;
const DEFAULT_COLUMN_WIDTH: f32 = 112.0;
const DEFAULT_HEADER_HEIGHT: f32 = 32.0;

/// A zero-based logical cell coordinate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SheetCellPosition {
    pub row: usize,
    pub column: usize,
}

impl SheetCellPosition {
    pub const fn new(row: usize, column: usize) -> Self {
        Self { row, column }
    }
}

/// A rectangular selection represented by an anchor and a focus cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SheetCellRange {
    pub anchor: SheetCellPosition,
    pub focus: SheetCellPosition,
}

impl SheetCellRange {
    pub const fn new(anchor: SheetCellPosition, focus: SheetCellPosition) -> Self {
        Self { anchor, focus }
    }

    pub fn normalized(self) -> SheetNormalizedRange {
        SheetNormalizedRange {
            row_start: self.anchor.row.min(self.focus.row),
            row_end: self.anchor.row.max(self.focus.row).saturating_add(1),
            column_start: self.anchor.column.min(self.focus.column),
            column_end: self.anchor.column.max(self.focus.column).saturating_add(1),
        }
    }

    pub fn contains(self, position: SheetCellPosition) -> bool {
        let range = self.normalized();
        range.rows().contains(&position.row) && range.columns().contains(&position.column)
    }
}

/// A normalized half-open rectangular range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SheetNormalizedRange {
    pub row_start: usize,
    pub row_end: usize,
    pub column_start: usize,
    pub column_end: usize,
}

impl SheetNormalizedRange {
    pub fn rows(self) -> Range<usize> {
        self.row_start..self.row_end
    }

    pub fn columns(self) -> Range<usize> {
        self.column_start..self.column_end
    }

    pub fn cell_count(self) -> Option<usize> {
        self.row_end
            .checked_sub(self.row_start)?
            .checked_mul(self.column_end.checked_sub(self.column_start)?)
    }
}

/// Stable address of a row/column tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SheetTileKey {
    pub tile_row: usize,
    pub tile_column: usize,
}

/// A generation-scoped request for a rectangular, row-major tile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetTileRequest {
    pub generation: u64,
    pub key: SheetTileKey,
    pub rows: Range<usize>,
    pub columns: Range<usize>,
}

impl SheetTileRequest {
    pub fn cell_count(&self) -> Option<usize> {
        self.rows
            .end
            .checked_sub(self.rows.start)?
            .checked_mul(self.columns.end.checked_sub(self.columns.start)?)
    }

    /// Content-safe diagnostic summary. Cell values are never included.
    pub fn to_text(&self) -> String {
        format!(
            "SheetTileRequest(generation={}, tile_row={}, tile_column={}, cells={})",
            self.generation,
            self.key.tile_row,
            self.key.tile_column,
            self.cell_count().unwrap_or(0)
        )
    }
}

/// A committed cell value and why it changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetCellEdit {
    pub position: SheetCellPosition,
    pub value: SharedString,
    pub reason: SheetEditReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetEditReason {
    Edit,
    Paste,
    Undo,
    Redo,
}

/// Bounded clipboard representations for a selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetClipboardExport {
    pub tsv: String,
    pub html: String,
    pub cell_count: usize,
}

/// Last ranges mounted by the retained two-axis viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SheetViewportMetrics {
    pub mounted_rows: usize,
    pub mounted_columns: usize,
    pub mounted_cells: usize,
}

/// Failures are intentionally metadata-only and never echo cell or clipboard
/// contents into logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VirtualSheetGridError {
    InvalidDimensions,
    CellOutOfBounds,
    InvalidTileShape,
    InvalidFrozenPane,
    StaleTileGeneration { expected: u64, received: u64 },
    UnexpectedTile,
    TileValueCount { expected: usize, received: usize },
    TileByteLimit { limit: usize },
    CellByteLimit { limit: usize },
    ClipboardByteLimit { limit: usize },
    ClipboardCellLimit { limit: usize },
    ClipboardMalformed,
    ClipboardCellUnavailable,
    EditLimit { limit: usize },
    EditByteLimit { limit: usize },
    AllocationFailed,
}

impl fmt::Display for VirtualSheetGridError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions => {
                formatter.write_str("sheet dimensions are outside supported bounds")
            }
            Self::CellOutOfBounds => formatter.write_str("cell is outside sheet bounds"),
            Self::InvalidTileShape => formatter.write_str("tile shape is outside supported bounds"),
            Self::InvalidFrozenPane => {
                formatter.write_str("frozen pane count is outside supported bounds")
            }
            Self::StaleTileGeneration { expected, received } => write!(
                formatter,
                "tile generation is stale (expected {expected}, received {received})"
            ),
            Self::UnexpectedTile => {
                formatter.write_str("tile does not match an outstanding request")
            }
            Self::TileValueCount { expected, received } => write!(
                formatter,
                "tile value count mismatch (expected {expected}, received {received})"
            ),
            Self::TileByteLimit { limit } => {
                write!(formatter, "tile byte limit exceeded ({limit})")
            }
            Self::CellByteLimit { limit } => {
                write!(formatter, "cell byte limit exceeded ({limit})")
            }
            Self::ClipboardByteLimit { limit } => {
                write!(formatter, "clipboard byte limit exceeded ({limit})")
            }
            Self::ClipboardCellLimit { limit } => {
                write!(formatter, "clipboard cell limit exceeded ({limit})")
            }
            Self::ClipboardMalformed => formatter.write_str("clipboard table is malformed"),
            Self::ClipboardCellUnavailable => {
                formatter.write_str("selection contains an unloaded cell")
            }
            Self::EditLimit { limit } => write!(formatter, "sparse edit limit exceeded ({limit})"),
            Self::EditByteLimit { limit } => {
                write!(formatter, "sparse edit byte limit exceeded ({limit})")
            }
            Self::AllocationFailed => formatter.write_str("bounded allocation failed"),
        }
    }
}

impl std::error::Error for VirtualSheetGridError {}

type FetchTileCallback =
    Rc<dyn Fn(SheetTileRequest, Entity<VirtualSheetGrid>, &mut Window, &mut App)>;
type CommitEditCallback = Rc<dyn Fn(SheetCellEdit, &mut Window, &mut App)>;

#[derive(Clone)]
struct CachedTile {
    request: SheetTileRequest,
    values: Vec<SharedString>,
    last_used: u64,
}

#[derive(Clone)]
struct EditDelta {
    position: SheetCellPosition,
    before_overlay: Option<SharedString>,
    before_value: SharedString,
    after: SharedString,
}

#[derive(Clone, Default)]
struct EditBatch {
    deltas: Vec<EditDelta>,
}

impl EditBatch {
    fn byte_count(&self) -> usize {
        self.deltas.iter().fold(0usize, |total, delta| {
            total
                .saturating_add(delta.before_overlay.as_ref().map_or(0, |value| value.len()))
                .saturating_add(delta.before_value.len())
                .saturating_add(delta.after.len())
        })
    }
}

/// Public state and retained view for a large spreadsheet.
pub struct VirtualSheetGrid {
    row_count: usize,
    column_count: usize,
    tile_rows: usize,
    tile_columns: usize,
    generation: u64,
    cache: HashMap<SheetTileKey, CachedTile>,
    pending: HashSet<SheetTileKey>,
    lru_clock: u64,
    cache_tile_limit: usize,
    pending_tile_limit: usize,
    selection: SheetCellRange,
    frozen_rows: usize,
    frozen_columns: usize,
    edits: HashMap<SheetCellPosition, SharedString>,
    edit_limit: usize,
    edit_byte_count: usize,
    edit_byte_limit: usize,
    undo: VecDeque<EditBatch>,
    redo: VecDeque<EditBatch>,
    undo_cell_count: usize,
    undo_cell_limit: usize,
    undo_byte_count: usize,
    undo_byte_limit: usize,
    editing_cell: Option<SheetCellPosition>,
    edit_input: Option<Entity<InputState>>,
    focus_handle: FocusHandle,
    vertical_scroll: ScrollHandle,
    horizontal_scroll: ScrollHandle,
    row_height: Pixels,
    column_width: Pixels,
    header_height: Pixels,
    viewport_metrics: SheetViewportMetrics,
    on_fetch_tile: Option<FetchTileCallback>,
    on_commit_edit: Option<CommitEditCallback>,
}

impl VirtualSheetGrid {
    /// Create a bounded sheet. Zero-sized sheets and dimensions above the public
    /// limits are rejected rather than clamped or allocated.
    pub fn new(
        row_count: usize,
        column_count: usize,
        cx: &mut Context<Self>,
    ) -> Result<Self, VirtualSheetGridError> {
        if row_count == 0
            || column_count == 0
            || row_count > VIRTUAL_SHEET_MAX_ROWS
            || column_count > VIRTUAL_SHEET_MAX_COLUMNS
        {
            return Err(VirtualSheetGridError::InvalidDimensions);
        }
        Ok(Self {
            row_count,
            column_count,
            tile_rows: DEFAULT_TILE_ROWS,
            tile_columns: DEFAULT_TILE_COLUMNS,
            generation: 1,
            cache: HashMap::new(),
            pending: HashSet::new(),
            lru_clock: 0,
            cache_tile_limit: VIRTUAL_SHEET_DEFAULT_CACHE_TILES,
            pending_tile_limit: VIRTUAL_SHEET_DEFAULT_PENDING_TILES,
            selection: SheetCellRange::default(),
            frozen_rows: 0,
            frozen_columns: 0,
            edits: HashMap::new(),
            edit_limit: VIRTUAL_SHEET_DEFAULT_EDIT_LIMIT,
            edit_byte_count: 0,
            edit_byte_limit: VIRTUAL_SHEET_DEFAULT_EDIT_BYTE_LIMIT,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            undo_cell_count: 0,
            undo_cell_limit: VIRTUAL_SHEET_DEFAULT_UNDO_CELL_LIMIT,
            undo_byte_count: 0,
            undo_byte_limit: VIRTUAL_SHEET_DEFAULT_UNDO_BYTE_LIMIT,
            editing_cell: None,
            edit_input: None,
            focus_handle: cx.focus_handle(),
            vertical_scroll: ScrollHandle::new(),
            horizontal_scroll: ScrollHandle::new(),
            row_height: px(DEFAULT_ROW_HEIGHT),
            column_width: px(DEFAULT_COLUMN_WIDTH),
            header_height: px(DEFAULT_HEADER_HEIGHT),
            viewport_metrics: SheetViewportMetrics::default(),
            on_fetch_tile: None,
            on_commit_edit: None,
        })
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn column_count(&self) -> usize {
        self.column_count
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn cached_tile_count(&self) -> usize {
        self.cache.len()
    }

    pub fn pending_tile_count(&self) -> usize {
        self.pending.len()
    }

    pub fn sparse_edit_count(&self) -> usize {
        self.edits.len()
    }

    pub fn selection(&self) -> SheetCellRange {
        self.selection
    }

    pub fn editing_cell(&self) -> Option<SheetCellPosition> {
        self.editing_cell
    }

    pub fn viewport_metrics(&self) -> SheetViewportMetrics {
        self.viewport_metrics
    }

    pub fn with_tile_shape(
        mut self,
        rows: usize,
        columns: usize,
    ) -> Result<Self, VirtualSheetGridError> {
        self.set_tile_shape(rows, columns)?;
        Ok(self)
    }

    pub fn set_tile_shape(
        &mut self,
        rows: usize,
        columns: usize,
    ) -> Result<(), VirtualSheetGridError> {
        let Some(cells) = rows.checked_mul(columns) else {
            return Err(VirtualSheetGridError::InvalidTileShape);
        };
        if rows == 0 || columns == 0 || cells > VIRTUAL_SHEET_MAX_TILE_CELLS {
            return Err(VirtualSheetGridError::InvalidTileShape);
        }
        if self.tile_rows != rows || self.tile_columns != columns {
            self.tile_rows = rows;
            self.tile_columns = columns;
            self.reload();
        }
        Ok(())
    }

    pub fn with_cache_limits(mut self, cached_tiles: usize, pending_tiles: usize) -> Self {
        self.cache_tile_limit = cached_tiles.max(1);
        self.pending_tile_limit = pending_tiles.max(1);
        self.evict_cache();
        self
    }

    pub fn with_edit_limits(mut self, edits: usize, undo_cells: usize) -> Self {
        self.edit_limit = edits.max(1);
        self.undo_cell_limit = undo_cells.max(1);
        self.trim_undo();
        self
    }

    pub fn with_edit_byte_limits(mut self, edit_bytes: usize, undo_bytes: usize) -> Self {
        self.edit_byte_limit = edit_bytes.max(1);
        self.undo_byte_limit = undo_bytes.max(1);
        self.trim_undo();
        self
    }

    pub fn with_frozen_panes(
        mut self,
        rows: usize,
        columns: usize,
    ) -> Result<Self, VirtualSheetGridError> {
        self.set_frozen_panes(rows, columns)?;
        Ok(self)
    }

    pub fn set_frozen_panes(
        &mut self,
        rows: usize,
        columns: usize,
    ) -> Result<(), VirtualSheetGridError> {
        if rows > self.row_count
            || columns > self.column_count
            || rows > VIRTUAL_SHEET_MAX_FROZEN_ROWS
            || columns > VIRTUAL_SHEET_MAX_FROZEN_COLUMNS
        {
            return Err(VirtualSheetGridError::InvalidFrozenPane);
        }
        self.frozen_rows = rows;
        self.frozen_columns = columns;
        Ok(())
    }

    pub fn on_fetch_tile(
        mut self,
        callback: impl Fn(SheetTileRequest, Entity<VirtualSheetGrid>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_fetch_tile = Some(Rc::new(callback));
        self
    }

    pub fn on_commit_edit(
        mut self,
        callback: impl Fn(SheetCellEdit, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_commit_edit = Some(Rc::new(callback));
        self
    }

    /// Advance the request generation and discard cache/pending state. Sparse
    /// edits remain as the local overlay.
    pub fn reload(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.cache.clear();
        self.pending.clear();
        self.lru_clock = 0;
    }

    fn request_for_key(&self, key: SheetTileKey) -> Option<SheetTileRequest> {
        let row_start = key.tile_row.checked_mul(self.tile_rows)?;
        let column_start = key.tile_column.checked_mul(self.tile_columns)?;
        if row_start >= self.row_count || column_start >= self.column_count {
            return None;
        }
        Some(SheetTileRequest {
            generation: self.generation,
            key,
            rows: row_start..row_start.saturating_add(self.tile_rows).min(self.row_count),
            columns: column_start
                ..column_start
                    .saturating_add(self.tile_columns)
                    .min(self.column_count),
        })
    }

    /// Request every missing tile intersecting the supplied ranges. Requests
    /// are de-duplicated and the pending set is hard-bounded.
    pub fn request_viewport(
        &mut self,
        rows: Range<usize>,
        columns: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<usize, VirtualSheetGridError> {
        let Some(rows) = checked_range(rows, self.row_count) else {
            return Err(VirtualSheetGridError::CellOutOfBounds);
        };
        let Some(columns) = checked_range(columns, self.column_count) else {
            return Err(VirtualSheetGridError::CellOutOfBounds);
        };
        if rows.is_empty() || columns.is_empty() {
            return Ok(0);
        }
        let Some(callback) = self.on_fetch_tile.clone() else {
            return Ok(0);
        };
        let first_tile_row = rows.start / self.tile_rows;
        let last_tile_row = (rows.end - 1) / self.tile_rows;
        let first_tile_column = columns.start / self.tile_columns;
        let last_tile_column = (columns.end - 1) / self.tile_columns;
        let mut issued = 0usize;
        for tile_row in first_tile_row..=last_tile_row {
            for tile_column in first_tile_column..=last_tile_column {
                if self.pending.len() >= self.pending_tile_limit {
                    return Ok(issued);
                }
                let key = SheetTileKey {
                    tile_row,
                    tile_column,
                };
                if self.cache.contains_key(&key) || self.pending.contains(&key) {
                    continue;
                }
                let Some(request) = self.request_for_key(key) else {
                    continue;
                };
                self.pending.insert(key);
                issued += 1;
                callback(request, cx.entity().clone(), window, cx);
            }
        }
        Ok(issued)
    }

    /// Accept a row-major tile only when it exactly matches a live request in
    /// the current generation.
    pub fn provide_tile(
        &mut self,
        request: SheetTileRequest,
        values: Vec<SharedString>,
    ) -> Result<(), VirtualSheetGridError> {
        if request.generation != self.generation {
            return Err(VirtualSheetGridError::StaleTileGeneration {
                expected: self.generation,
                received: request.generation,
            });
        }
        let Some(expected_request) = self.request_for_key(request.key) else {
            return Err(VirtualSheetGridError::UnexpectedTile);
        };
        if request != expected_request || !self.pending.remove(&request.key) {
            return Err(VirtualSheetGridError::UnexpectedTile);
        }
        let expected = request
            .cell_count()
            .ok_or(VirtualSheetGridError::InvalidTileShape)?;
        if values.len() != expected {
            return Err(VirtualSheetGridError::TileValueCount {
                expected,
                received: values.len(),
            });
        }
        let tile_bytes = values.iter().try_fold(0usize, |total, value| {
            if value.len() > VIRTUAL_SHEET_MAX_CELL_BYTES {
                return Err(VirtualSheetGridError::CellByteLimit {
                    limit: VIRTUAL_SHEET_MAX_CELL_BYTES,
                });
            }
            total
                .checked_add(value.len())
                .ok_or(VirtualSheetGridError::TileByteLimit {
                    limit: VIRTUAL_SHEET_MAX_TILE_BYTES,
                })
        })?;
        if tile_bytes > VIRTUAL_SHEET_MAX_TILE_BYTES {
            return Err(VirtualSheetGridError::TileByteLimit {
                limit: VIRTUAL_SHEET_MAX_TILE_BYTES,
            });
        }
        self.lru_clock = self.lru_clock.wrapping_add(1);
        self.cache.insert(
            request.key,
            CachedTile {
                request,
                values,
                last_used: self.lru_clock,
            },
        );
        self.evict_cache();
        Ok(())
    }

    fn evict_cache(&mut self) {
        while self.cache.len() > self.cache_tile_limit {
            let Some(key) = self
                .cache
                .iter()
                .min_by_key(|(_, tile)| tile.last_used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.cache.remove(&key);
        }
    }

    fn loaded_value(&mut self, position: SheetCellPosition) -> Option<SharedString> {
        if let Some(value) = self.edits.get(&position) {
            return Some(value.clone());
        }
        if position.row >= self.row_count || position.column >= self.column_count {
            return None;
        }
        let key = SheetTileKey {
            tile_row: position.row / self.tile_rows,
            tile_column: position.column / self.tile_columns,
        };
        self.lru_clock = self.lru_clock.wrapping_add(1);
        let tile = self.cache.get_mut(&key)?;
        tile.last_used = self.lru_clock;
        let width = tile.request.columns.end - tile.request.columns.start;
        let local_row = position.row.checked_sub(tile.request.rows.start)?;
        let local_column = position.column.checked_sub(tile.request.columns.start)?;
        let index = local_row.checked_mul(width)?.checked_add(local_column)?;
        tile.values.get(index).cloned()
    }

    pub fn cell_value(&mut self, position: SheetCellPosition) -> Option<SharedString> {
        self.loaded_value(position)
    }

    pub fn select(
        &mut self,
        position: SheetCellPosition,
        extend: bool,
    ) -> Result<(), VirtualSheetGridError> {
        self.validate_position(position)?;
        if extend {
            self.selection.focus = position;
        } else {
            self.selection = SheetCellRange::new(position, position);
        }
        Ok(())
    }

    pub fn move_focus(&mut self, row_delta: isize, column_delta: isize, extend: bool) {
        let current = self.selection.focus;
        let next = SheetCellPosition {
            row: current
                .row
                .saturating_add_signed(row_delta)
                .min(self.row_count - 1),
            column: current
                .column
                .saturating_add_signed(column_delta)
                .min(self.column_count - 1),
        };
        let _ = self.select(next, extend);
    }

    pub fn select_row_edge(&mut self, end: bool, extend: bool) {
        let position = SheetCellPosition {
            row: self.selection.focus.row,
            column: if end { self.column_count - 1 } else { 0 },
        };
        let _ = self.select(position, extend);
    }

    pub fn select_sheet_edge(&mut self, end: bool, extend: bool) {
        let position = if end {
            SheetCellPosition::new(self.row_count - 1, self.column_count - 1)
        } else {
            SheetCellPosition::default()
        };
        let _ = self.select(position, extend);
    }

    /// Position the focused cell at the leading edge of the scrollable pane.
    /// Frozen coordinates do not move their axis.
    pub fn scroll_to_cell(&self, position: SheetCellPosition) -> Result<(), VirtualSheetGridError> {
        self.validate_position(position)?;
        let mut horizontal = self.horizontal_scroll.offset();
        let mut vertical = self.vertical_scroll.offset();
        if position.column >= self.frozen_columns {
            horizontal.x =
                -(self.column_width * position.column.saturating_sub(self.frozen_columns) as f32);
        }
        if position.row >= self.frozen_rows {
            vertical.y = -(self.row_height * position.row.saturating_sub(self.frozen_rows) as f32);
        }
        self.horizontal_scroll.set_offset(horizontal);
        self.vertical_scroll.set_offset(vertical);
        Ok(())
    }

    fn validate_position(&self, position: SheetCellPosition) -> Result<(), VirtualSheetGridError> {
        if position.row >= self.row_count || position.column >= self.column_count {
            Err(VirtualSheetGridError::CellOutOfBounds)
        } else {
            Ok(())
        }
    }

    /// Begin editing through Kael's IME-aware [`InputState`]. The grid never
    /// constructs text from key events.
    pub fn start_editing(
        &mut self,
        position: SheetCellPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), VirtualSheetGridError> {
        self.validate_position(position)?;
        let value = self.loaded_value(position).unwrap_or_default();
        let input = cx.new(|cx| {
            let mut state = InputState::new(cx);
            state.set_value(value, window, cx);
            state
        });
        window.focus(&input.read(cx).focus_handle(cx));
        self.editing_cell = Some(position);
        self.edit_input = Some(input);
        cx.notify();
        Ok(())
    }

    pub fn cancel_editing(&mut self, cx: &mut Context<Self>) {
        self.editing_cell = None;
        self.edit_input = None;
        cx.notify();
    }

    pub fn commit_editing(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), VirtualSheetGridError> {
        let Some(position) = self.editing_cell else {
            return Ok(());
        };
        let Some(input) = &self.edit_input else {
            return Ok(());
        };
        let value: SharedString = input.read(cx).content().to_owned().into();
        self.apply_values(vec![(position, value)], SheetEditReason::Edit, window, cx)?;
        self.editing_cell = None;
        self.edit_input = None;
        window.focus(&self.focus_handle);
        cx.notify();
        Ok(())
    }

    fn apply_values(
        &mut self,
        values: Vec<(SheetCellPosition, SharedString)>,
        reason: SheetEditReason,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<(), VirtualSheetGridError> {
        let mut prospective_bytes = self.edit_byte_count;
        let mut final_lengths = HashMap::<SheetCellPosition, usize>::new();
        final_lengths
            .try_reserve(values.len())
            .map_err(|_| VirtualSheetGridError::AllocationFailed)?;
        for (position, value) in &values {
            self.validate_position(*position)?;
            if value.len() > VIRTUAL_SHEET_MAX_CELL_BYTES {
                return Err(VirtualSheetGridError::CellByteLimit {
                    limit: VIRTUAL_SHEET_MAX_CELL_BYTES,
                });
            }
            let previous = final_lengths
                .get(position)
                .copied()
                .unwrap_or_else(|| self.edits.get(position).map_or(0, |value| value.len()));
            prospective_bytes = prospective_bytes
                .checked_sub(previous)
                .and_then(|total| total.checked_add(value.len()))
                .ok_or(VirtualSheetGridError::EditByteLimit {
                    limit: self.edit_byte_limit,
                })?;
            final_lengths.insert(*position, value.len());
        }
        if prospective_bytes > self.edit_byte_limit {
            return Err(VirtualSheetGridError::EditByteLimit {
                limit: self.edit_byte_limit,
            });
        }
        let new_keys = values
            .iter()
            .filter(|(position, _)| !self.edits.contains_key(position))
            .map(|(position, _)| *position)
            .collect::<HashSet<_>>();
        if self.edits.len().saturating_add(new_keys.len()) > self.edit_limit {
            return Err(VirtualSheetGridError::EditLimit {
                limit: self.edit_limit,
            });
        }
        let mut batch = EditBatch::default();
        batch
            .deltas
            .try_reserve(values.len())
            .map_err(|_| VirtualSheetGridError::AllocationFailed)?;
        for (position, after) in values {
            self.validate_position(position)?;
            let before_overlay = self.edits.get(&position).cloned();
            let before_value = self.loaded_value(position).unwrap_or_default();
            if before_value == after {
                continue;
            }
            if let Some(previous) = self.edits.insert(position, after.clone()) {
                self.edit_byte_count = self.edit_byte_count.saturating_sub(previous.len());
            }
            self.edit_byte_count = self.edit_byte_count.saturating_add(after.len());
            batch.deltas.push(EditDelta {
                position,
                before_overlay,
                before_value,
                after: after.clone(),
            });
            self.emit_edit(position, after, reason, window, cx);
        }
        if !batch.deltas.is_empty() {
            self.clear_redo_history();
            self.undo_cell_count = self.undo_cell_count.saturating_add(batch.deltas.len());
            self.undo_byte_count = self.undo_byte_count.saturating_add(batch.byte_count());
            self.undo.push_back(batch);
            self.trim_undo();
        }
        Ok(())
    }

    /// Commit one sparse value programmatically through the same bounded path
    /// used by the IME editor.
    pub fn set_cell_value(
        &mut self,
        position: SheetCellPosition,
        value: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), VirtualSheetGridError> {
        self.apply_values(
            vec![(position, value.into())],
            SheetEditReason::Edit,
            window,
            cx,
        )?;
        cx.notify();
        Ok(())
    }

    fn emit_edit(
        &self,
        position: SheetCellPosition,
        value: SharedString,
        reason: SheetEditReason,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(callback) = &self.on_commit_edit {
            callback(
                SheetCellEdit {
                    position,
                    value,
                    reason,
                },
                window,
                cx,
            );
        }
    }

    fn trim_undo(&mut self) {
        while self.undo_cell_count > self.undo_cell_limit
            || self.undo_byte_count > self.undo_byte_limit
        {
            let batch = self.undo.pop_front().or_else(|| self.redo.pop_front());
            let Some(batch) = batch else { break };
            self.undo_cell_count = self.undo_cell_count.saturating_sub(batch.deltas.len());
            self.undo_byte_count = self.undo_byte_count.saturating_sub(batch.byte_count());
        }
    }

    fn clear_redo_history(&mut self) {
        for batch in self.redo.drain(..) {
            self.undo_cell_count = self.undo_cell_count.saturating_sub(batch.deltas.len());
            self.undo_byte_count = self.undo_byte_count.saturating_sub(batch.byte_count());
        }
    }

    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(batch) = self.undo.pop_back() else {
            return false;
        };
        for delta in batch.deltas.iter().rev() {
            if let Some(previous) = self.edits.remove(&delta.position) {
                self.edit_byte_count = self.edit_byte_count.saturating_sub(previous.len());
            }
            if let Some(value) = &delta.before_overlay {
                self.edits.insert(delta.position, value.clone());
                self.edit_byte_count = self.edit_byte_count.saturating_add(value.len());
            }
            self.emit_edit(
                delta.position,
                delta.before_value.clone(),
                SheetEditReason::Undo,
                window,
                cx,
            );
        }
        self.redo.push_back(batch);
        cx.notify();
        true
    }

    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(batch) = self.redo.pop_back() else {
            return false;
        };
        for delta in &batch.deltas {
            if let Some(previous) = self.edits.insert(delta.position, delta.after.clone()) {
                self.edit_byte_count = self.edit_byte_count.saturating_sub(previous.len());
            }
            self.edit_byte_count = self.edit_byte_count.saturating_add(delta.after.len());
            self.emit_edit(
                delta.position,
                delta.after.clone(),
                SheetEditReason::Redo,
                window,
                cx,
            );
        }
        self.undo.push_back(batch);
        self.trim_undo();
        cx.notify();
        true
    }

    pub fn export_selection(&mut self) -> Result<SheetClipboardExport, VirtualSheetGridError> {
        let selected = self.selection.normalized();
        let cell_count =
            selected
                .cell_count()
                .ok_or(VirtualSheetGridError::ClipboardCellLimit {
                    limit: VIRTUAL_SHEET_CLIPBOARD_CELL_LIMIT,
                })?;
        if cell_count > VIRTUAL_SHEET_CLIPBOARD_CELL_LIMIT {
            return Err(VirtualSheetGridError::ClipboardCellLimit {
                limit: VIRTUAL_SHEET_CLIPBOARD_CELL_LIMIT,
            });
        }
        let mut tsv = String::new();
        let mut html = String::from("<table>");
        for row in selected.rows() {
            append_bounded(&mut html, "<tr>")?;
            for column in selected.columns() {
                let value = self
                    .loaded_value(SheetCellPosition::new(row, column))
                    .ok_or(VirtualSheetGridError::ClipboardCellUnavailable)?;
                if column != selected.column_start {
                    append_bounded(&mut tsv, "\t")?;
                }
                append_tsv_value(&mut tsv, &value)?;
                append_bounded(&mut html, "<td>")?;
                append_html_value(&mut html, &value)?;
                append_bounded(&mut html, "</td>")?;
            }
            if row + 1 != selected.row_end {
                append_bounded(&mut tsv, "\n")?;
            }
            append_bounded(&mut html, "</tr>")?;
        }
        append_bounded(&mut html, "</table>")?;
        Ok(SheetClipboardExport {
            tsv,
            html,
            cell_count,
        })
    }

    /// Copy TSV with an HTML-table companion payload. Both representations are
    /// generated under the same byte/cell limits before touching the clipboard.
    pub fn copy_selection_to_clipboard(
        &mut self,
        cx: &mut App,
    ) -> Result<usize, VirtualSheetGridError> {
        let export = self.export_selection()?;
        let metadata = ClipboardHtmlMetadata::new(export.html)
            .map_err(|_| VirtualSheetGridError::ClipboardMalformed)?;
        cx.write_to_clipboard(ClipboardItem::new_string_with_json_metadata(
            export.tsv, metadata,
        ));
        Ok(export.cell_count)
    }

    /// Read the platform's plain-text clipboard representation and apply it as
    /// bounded TSV at the focused cell.
    pub fn paste_from_clipboard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<SheetNormalizedRange, VirtualSheetGridError> {
        let text = cx
            .read_from_clipboard()
            .map_err(|_| VirtualSheetGridError::ClipboardMalformed)?
            .and_then(|item| item.text())
            .ok_or(VirtualSheetGridError::ClipboardMalformed)?;
        self.paste_tsv(&text, window, cx)
    }

    pub fn paste_tsv(
        &mut self,
        input: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<SheetNormalizedRange, VirtualSheetGridError> {
        let rows = parse_tsv(input)?;
        let start = self.selection.focus;
        let mut values = Vec::new();
        let count = rows
            .iter()
            .try_fold(0usize, |total, row| total.checked_add(row.len()))
            .ok_or(VirtualSheetGridError::ClipboardCellLimit {
                limit: VIRTUAL_SHEET_CLIPBOARD_CELL_LIMIT,
            })?;
        values
            .try_reserve(count)
            .map_err(|_| VirtualSheetGridError::AllocationFailed)?;
        let mut row_end = start.row;
        let mut column_end = start.column;
        for (row_offset, row) in rows.into_iter().enumerate() {
            let Some(target_row) = start.row.checked_add(row_offset) else {
                break;
            };
            if target_row >= self.row_count {
                break;
            }
            row_end = target_row.saturating_add(1);
            for (column_offset, value) in row.into_iter().enumerate() {
                let Some(target_column) = start.column.checked_add(column_offset) else {
                    break;
                };
                if target_column >= self.column_count {
                    break;
                }
                column_end = column_end.max(target_column.saturating_add(1));
                values.push((
                    SheetCellPosition::new(target_row, target_column),
                    value.into(),
                ));
            }
        }
        self.apply_values(values, SheetEditReason::Paste, window, cx)?;
        let pasted = SheetNormalizedRange {
            row_start: start.row,
            row_end,
            column_start: start.column,
            column_end,
        };
        if row_end > start.row && column_end > start.column {
            self.selection.focus = SheetCellPosition::new(row_end - 1, column_end - 1);
        }
        cx.notify();
        Ok(pasted)
    }

    /// Content-safe runtime diagnostics. Counts and limits are reported, never
    /// cell values, edit values, or clipboard contents.
    pub fn to_text(&self) -> String {
        format!(
            "VirtualSheetGrid(rows={}, columns={}, generation={}, cached_tiles={}, pending_tiles={}, sparse_edits={}, sparse_edit_bytes={}, undo_batches={}, redo_batches={})",
            self.row_count,
            self.column_count,
            self.generation,
            self.cache.len(),
            self.pending.len(),
            self.edits.len(),
            self.edit_byte_count,
            self.undo.len(),
            self.redo.len()
        )
    }

    fn render_cell(
        &mut self,
        position: SheetCellPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let selected = self.selection.contains(position);
        let value = self.loaded_value(position).unwrap_or_default();
        let editing = self.editing_cell == Some(position);
        let input = if editing {
            self.edit_input.clone()
        } else {
            None
        };
        let entity = cx.entity().clone();
        let mut states = AccessibilityState::NONE;
        if selected {
            states |= AccessibilityState::SELECTED;
        }
        let mut cell = div()
            .id((
                "virtual-sheet-cell",
                position.row * VIRTUAL_SHEET_MAX_COLUMNS + position.column,
            ))
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Cell)
                    .row_index(position.row + 2)
                    .column_index(position.column + 1)
                    .states(states),
            )
            .flex()
            .items_center()
            .w(self.column_width)
            .h(self.row_height)
            .flex_shrink_0()
            .px(px(8.0))
            .text_size(px(13.0))
            .text_color(theme.tokens.foreground)
            .border_r_1()
            .border_b_1()
            .border_color(theme.tokens.border.opacity(0.65))
            .overflow_hidden()
            .bg(if selected {
                theme.tokens.accent
            } else {
                theme.tokens.card
            })
            .on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    entity.update(cx, |grid, cx| {
                        let _ = grid.select(position, event.modifiers.shift);
                        window.focus(&grid.focus_handle);
                        if event.click_count >= 2 {
                            let _ = grid.start_editing(position, window, cx);
                        }
                        cx.notify();
                    });
                },
            );
        if let Some(input) = input {
            let entity = cx.entity().clone();
            cell = cell.child(Input::new(&input).size(InputSize::Sm).on_submit(
                move |_, window, cx| {
                    entity.update(cx, |grid, cx| {
                        let _ = grid.commit_editing(window, cx);
                    });
                },
            ));
        } else {
            cell = cell.child(value);
        }
        let _ = window;
        cell.into_any_element()
    }

    fn render_scrolling_columns(
        &mut self,
        row: usize,
        relative_columns: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let columns = relative_columns.start.saturating_add(self.frozen_columns)
            ..relative_columns
                .end
                .saturating_add(self.frozen_columns)
                .min(self.column_count);
        self.viewport_metrics.mounted_columns = columns
            .len()
            .saturating_add(self.frozen_columns)
            .min(self.column_count);
        self.viewport_metrics.mounted_cells = self
            .viewport_metrics
            .mounted_rows
            .saturating_mul(self.viewport_metrics.mounted_columns);
        let _ = self.request_viewport(row..row.saturating_add(1), columns.clone(), window, cx);
        columns
            .map(|column| self.render_cell(SheetCellPosition::new(row, column), window, cx))
            .collect()
    }

    fn render_data_row(
        &mut self,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        if self.frozen_columns > 0 {
            let _ = self.request_viewport(row..row + 1, 0..self.frozen_columns, window, cx);
        }
        let frozen = (0..self.frozen_columns)
            .map(|column| self.render_cell(SheetCellPosition::new(row, column), window, cx))
            .collect::<Vec<_>>();
        let entity = cx.entity().clone();
        let column_width = self.column_width;
        let horizontal_scroll = self.horizontal_scroll.clone();
        let scrollable_count = self.column_count.saturating_sub(self.frozen_columns);
        let scrolled = hlist_uniform(
            ("virtual-sheet-row-columns", row),
            scrollable_count,
            column_width,
            move |range, window, cx| {
                entity.update(cx, |grid, cx| {
                    grid.render_scrolling_columns(row, range, window, cx)
                })
            },
        )
        .track_scroll(&horizontal_scroll)
        .overscan(2)
        .h(self.row_height)
        .flex_1();
        div()
            .id(("virtual-sheet-row", row))
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Row)
                    .label(format!("Row {}", row + 1))
                    .row_index(row + 2),
            )
            .flex()
            .w_full()
            .h(self.row_height)
            .bg(theme.tokens.card)
            .children(frozen)
            .child(scrolled)
            .into_any_element()
    }

    fn render_column_header(column: usize, width: Pixels, height: Pixels, cx: &App) -> AnyElement {
        let theme = Theme::of(cx);
        let label = column_label(column);
        div()
            .id(("virtual-sheet-column-header", column))
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::ColumnHeader)
                    .label(label.clone())
                    .column_index(column + 1),
            )
            .flex()
            .items_center()
            .justify_center()
            .w(width)
            .h(height)
            .flex_shrink_0()
            .text_size(px(12.0))
            .text_color(theme.tokens.muted_foreground)
            .border_r_1()
            .border_b_1()
            .border_color(theme.tokens.border)
            .bg(theme.tokens.muted)
            .child(label)
            .into_any_element()
    }
}

impl Focusable for VirtualSheetGrid {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for VirtualSheetGrid {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let entity = cx.entity().clone();
        let horizontal_scroll = self.horizontal_scroll.clone();
        let header_height = self.header_height;
        let frozen_headers = (0..self.frozen_columns)
            .map(|column| Self::render_column_header(column, self.column_width, header_height, cx))
            .collect::<Vec<_>>();
        let frozen_width = self.column_width * self.frozen_columns as f32;
        let scrollable_column_count = self.column_count.saturating_sub(self.frozen_columns);
        let frozen_columns = self.frozen_columns;
        let column_width = self.column_width;
        let scrolled_headers = hlist_uniform(
            "virtual-sheet-column-headers",
            scrollable_column_count,
            column_width,
            move |range, _window, cx| {
                range
                    .map(|relative| {
                        Self::render_column_header(
                            relative + frozen_columns,
                            column_width,
                            header_height,
                            cx,
                        )
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&horizontal_scroll)
        .overscan(2)
        .h(self.header_height)
        .flex_1();
        let header = div()
            .flex()
            .w_full()
            .h(self.header_height)
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .w(frozen_width)
                    .h_full()
                    .children(frozen_headers),
            )
            .child(scrolled_headers);

        let mut frozen_rows = Vec::with_capacity(self.frozen_rows);
        for row in 0..self.frozen_rows {
            frozen_rows.push(self.render_data_row(row, _window, cx));
        }

        let entity_for_rows = entity.clone();
        let frozen_row_count = self.frozen_rows;
        let vertical_scroll = self.vertical_scroll.clone();
        let scrollable_row_count = self.row_count.saturating_sub(self.frozen_rows);
        let body = vlist_uniform_view(
            entity_for_rows,
            "virtual-sheet-rows",
            scrollable_row_count,
            self.row_height,
            move |grid, range, window, cx| {
                grid.viewport_metrics.mounted_rows = range
                    .len()
                    .saturating_add(frozen_row_count)
                    .min(grid.row_count);
                grid.viewport_metrics.mounted_cells = grid
                    .viewport_metrics
                    .mounted_rows
                    .saturating_mul(grid.viewport_metrics.mounted_columns);
                range
                    .map(|relative| grid.render_data_row(relative + frozen_row_count, window, cx))
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&vertical_scroll)
        .overscan(3)
        .flex_1();

        let entity_for_keys = entity.clone();
        div()
            .id("virtual-sheet-grid")
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Grid)
                    .label("Spreadsheet grid")
                    .row_count(self.row_count + 1)
                    .column_count(self.column_count),
            )
            .track_focus(&self.focus_handle)
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                entity_for_keys.update(cx, |grid, cx| {
                    if grid.editing_cell.is_some() {
                        return;
                    }
                    let extend = event.keystroke.modifiers.shift;
                    let handled = match key {
                        "left" => {
                            grid.move_focus(0, -1, extend);
                            true
                        }
                        "right" => {
                            grid.move_focus(0, 1, extend);
                            true
                        }
                        "up" => {
                            grid.move_focus(-1, 0, extend);
                            true
                        }
                        "down" => {
                            grid.move_focus(1, 0, extend);
                            true
                        }
                        "home" if event.keystroke.modifiers.secondary() => {
                            grid.select_sheet_edge(false, extend);
                            true
                        }
                        "end" if event.keystroke.modifiers.secondary() => {
                            grid.select_sheet_edge(true, extend);
                            true
                        }
                        "home" => {
                            grid.select_row_edge(false, extend);
                            true
                        }
                        "end" => {
                            grid.select_row_edge(true, extend);
                            true
                        }
                        "enter" => {
                            let position = grid.selection.focus;
                            let _ = grid.start_editing(position, window, cx);
                            true
                        }
                        "c" if event.keystroke.modifiers.secondary() => {
                            let _ = grid.copy_selection_to_clipboard(cx);
                            true
                        }
                        "v" if event.keystroke.modifiers.secondary() => {
                            let _ = grid.paste_from_clipboard(window, cx);
                            true
                        }
                        "z" if event.keystroke.modifiers.secondary()
                            && event.keystroke.modifiers.shift =>
                        {
                            grid.redo(window, cx)
                        }
                        "z" if event.keystroke.modifiers.secondary() => grid.undo(window, cx),
                        "y" if event.keystroke.modifiers.secondary() => grid.redo(window, cx),
                        _ => false,
                    };
                    if handled {
                        let _ = grid.scroll_to_cell(grid.selection.focus);
                        cx.notify();
                    }
                });
            })
            .flex()
            .flex_col()
            .size_full()
            .min_h(px(120.0))
            .border_1()
            .border_color(theme.tokens.border)
            .rounded(theme.tokens.radius_md)
            .overflow_hidden()
            .bg(theme.tokens.card)
            .child(header)
            .children(frozen_rows)
            .child(body)
    }
}

fn checked_range(range: Range<usize>, upper: usize) -> Option<Range<usize>> {
    (range.start <= range.end && range.end <= upper).then_some(range)
}

fn append_bounded(output: &mut String, value: &str) -> Result<(), VirtualSheetGridError> {
    let next =
        output
            .len()
            .checked_add(value.len())
            .ok_or(VirtualSheetGridError::ClipboardByteLimit {
                limit: VIRTUAL_SHEET_CLIPBOARD_BYTE_LIMIT,
            })?;
    if next > VIRTUAL_SHEET_CLIPBOARD_BYTE_LIMIT {
        return Err(VirtualSheetGridError::ClipboardByteLimit {
            limit: VIRTUAL_SHEET_CLIPBOARD_BYTE_LIMIT,
        });
    }
    output
        .try_reserve(value.len())
        .map_err(|_| VirtualSheetGridError::AllocationFailed)?;
    output.push_str(value);
    Ok(())
}

fn append_tsv_value(output: &mut String, value: &str) -> Result<(), VirtualSheetGridError> {
    let quoted = value.contains(['\t', '\n', '\r', '"']);
    if quoted {
        append_bounded(output, "\"")?;
    }
    let mut start = 0usize;
    for (index, character) in value.char_indices() {
        if character == '"' {
            append_bounded(output, &value[start..index])?;
            append_bounded(output, "\"\"")?;
            start = index + character.len_utf8();
        }
    }
    append_bounded(output, &value[start..])?;
    if quoted {
        append_bounded(output, "\"")?;
    }
    Ok(())
}

fn append_html_value(output: &mut String, value: &str) -> Result<(), VirtualSheetGridError> {
    let mut start = 0usize;
    for (index, character) in value.char_indices() {
        let replacement = match character {
            '&' => Some("&amp;"),
            '<' => Some("&lt;"),
            '>' => Some("&gt;"),
            '"' => Some("&quot;"),
            '\'' => Some("&#39;"),
            _ => None,
        };
        if let Some(replacement) = replacement {
            append_bounded(output, &value[start..index])?;
            append_bounded(output, replacement)?;
            start = index + character.len_utf8();
        }
    }
    append_bounded(output, &value[start..])
}

#[cfg(test)]
fn escape_tsv(value: &str) -> String {
    let mut output = String::new();
    append_tsv_value(&mut output, value).unwrap();
    output
}

#[cfg(test)]
fn escape_html(value: &str) -> String {
    let mut output = String::new();
    append_html_value(&mut output, value).unwrap();
    output
}

fn parse_tsv(input: &str) -> Result<Vec<Vec<String>>, VirtualSheetGridError> {
    if input.len() > VIRTUAL_SHEET_CLIPBOARD_BYTE_LIMIT {
        return Err(VirtualSheetGridError::ClipboardByteLimit {
            limit: VIRTUAL_SHEET_CLIPBOARD_BYTE_LIMIT,
        });
    }
    let mut rows = Vec::<Vec<String>>::new();
    let mut row = Vec::<String>::new();
    let mut cell = String::new();
    let mut characters = input.chars().peekable();
    let mut quoted = false;
    let mut quote_closed = false;
    let mut cell_started = false;
    let mut cell_count = 0usize;
    while let Some(character) = characters.next() {
        if quoted {
            if character == '"' && characters.peek() == Some(&'"') {
                characters.next();
                cell.push('"');
            } else if character == '"' {
                quoted = false;
                quote_closed = true;
            } else {
                cell.push(character);
            }
            continue;
        }
        if quote_closed {
            match character {
                '\t' => {
                    push_parsed_cell(&mut row, &mut cell, &mut cell_count)?;
                    quote_closed = false;
                    cell_started = false;
                }
                '\n' => {
                    push_parsed_cell(&mut row, &mut cell, &mut cell_count)?;
                    rows.push(std::mem::take(&mut row));
                    quote_closed = false;
                    cell_started = false;
                }
                '\r' => {
                    if characters.peek() == Some(&'\n') {
                        characters.next();
                    }
                    push_parsed_cell(&mut row, &mut cell, &mut cell_count)?;
                    rows.push(std::mem::take(&mut row));
                    quote_closed = false;
                    cell_started = false;
                }
                _ => return Err(VirtualSheetGridError::ClipboardMalformed),
            }
            continue;
        }
        match character {
            '"' if !cell_started => {
                quoted = true;
                cell_started = true;
            }
            '"' => return Err(VirtualSheetGridError::ClipboardMalformed),
            '\t' => {
                push_parsed_cell(&mut row, &mut cell, &mut cell_count)?;
                cell_started = false;
            }
            '\n' => {
                push_parsed_cell(&mut row, &mut cell, &mut cell_count)?;
                rows.push(std::mem::take(&mut row));
                cell_started = false;
            }
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                push_parsed_cell(&mut row, &mut cell, &mut cell_count)?;
                rows.push(std::mem::take(&mut row));
                cell_started = false;
            }
            _ => {
                cell_started = true;
                cell.push(character);
            }
        }
    }
    if quoted {
        return Err(VirtualSheetGridError::ClipboardMalformed);
    }
    if quote_closed || !row.is_empty() || !cell.is_empty() || rows.is_empty() {
        push_parsed_cell(&mut row, &mut cell, &mut cell_count)?;
        rows.push(row);
    }
    Ok(rows)
}

fn push_parsed_cell(
    row: &mut Vec<String>,
    cell: &mut String,
    count: &mut usize,
) -> Result<(), VirtualSheetGridError> {
    *count = count
        .checked_add(1)
        .ok_or(VirtualSheetGridError::ClipboardCellLimit {
            limit: VIRTUAL_SHEET_CLIPBOARD_CELL_LIMIT,
        })?;
    if *count > VIRTUAL_SHEET_CLIPBOARD_CELL_LIMIT {
        return Err(VirtualSheetGridError::ClipboardCellLimit {
            limit: VIRTUAL_SHEET_CLIPBOARD_CELL_LIMIT,
        });
    }
    row.try_reserve(1)
        .map_err(|_| VirtualSheetGridError::AllocationFailed)?;
    row.push(std::mem::take(cell));
    Ok(())
}

fn column_label(mut index: usize) -> String {
    let mut label = [0u8; 4];
    let mut cursor = label.len();
    index += 1;
    while index > 0 {
        index -= 1;
        cursor -= 1;
        label[cursor] = b'A' + (index % 26) as u8;
        index /= 26;
    }
    String::from_utf8_lossy(&label[cursor..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kael::TestAppContext;
    use std::cell::RefCell;

    #[::core::prelude::v1::test]
    fn million_by_xfd_is_logical_and_bounded() {
        assert_eq!(VIRTUAL_SHEET_MAX_ROWS, 1_000_000);
        assert_eq!(VIRTUAL_SHEET_MAX_COLUMNS, 16_384);
        const { assert!(VIRTUAL_SHEET_MAX_TILE_CELLS < VIRTUAL_SHEET_MAX_ROWS) };
        assert_eq!(column_label(0), "A");
        assert_eq!(column_label(25), "Z");
        assert_eq!(column_label(26), "AA");
        assert_eq!(column_label(16_383), "XFD");
    }

    #[::core::prelude::v1::test]
    fn ranges_use_checked_cell_counts() {
        let range = SheetCellRange::new(SheetCellPosition::new(9, 7), SheetCellPosition::new(2, 3))
            .normalized();
        assert_eq!(range.rows(), 2..10);
        assert_eq!(range.columns(), 3..8);
        assert_eq!(range.cell_count(), Some(40));
        let hostile = SheetNormalizedRange {
            row_start: 0,
            row_end: usize::MAX,
            column_start: 0,
            column_end: usize::MAX,
        };
        assert_eq!(hostile.cell_count(), None);
    }

    #[::core::prelude::v1::test]
    fn tsv_parser_is_quoted_and_bounded() {
        let rows = parse_tsv("a\t\"b\tc\"\r\n\"d\nq\"\t\"e\"\"f\"").unwrap();
        assert_eq!(rows[0], ["a", "b\tc"]);
        assert_eq!(rows[1], ["d\nq", "e\"f"]);
        assert_eq!(
            parse_tsv("\"unterminated"),
            Err(VirtualSheetGridError::ClipboardMalformed)
        );
        assert_eq!(
            parse_tsv("plain\"quote"),
            Err(VirtualSheetGridError::ClipboardMalformed)
        );
        assert_eq!(
            parse_tsv("\"closed\"suffix"),
            Err(VirtualSheetGridError::ClipboardMalformed)
        );
        let oversized = "x".repeat(VIRTUAL_SHEET_CLIPBOARD_BYTE_LIMIT + 1);
        assert!(matches!(
            parse_tsv(&oversized),
            Err(VirtualSheetGridError::ClipboardByteLimit { .. })
        ));
        let too_many_cells = "\t".repeat(VIRTUAL_SHEET_CLIPBOARD_CELL_LIMIT);
        assert!(matches!(
            parse_tsv(&too_many_cells),
            Err(VirtualSheetGridError::ClipboardCellLimit { .. })
        ));
    }

    #[::core::prelude::v1::test]
    fn clipboard_escaping_does_not_emit_active_html() {
        assert_eq!(escape_html("<&\"'>"), "&lt;&amp;&quot;&#39;&gt;");
        assert_eq!(escape_tsv("a\tb\""), "\"a\tb\"\"\"");
    }

    #[::core::prelude::v1::test]
    fn tile_request_diagnostics_do_not_contain_values() {
        let request = SheetTileRequest {
            generation: 7,
            key: SheetTileKey {
                tile_row: 2,
                tile_column: 3,
            },
            rows: 128..192,
            columns: 96..128,
        };
        let text = request.to_text();
        assert!(text.contains("generation=7"));
        assert!(text.contains("cells=2048"));
        assert!(!text.contains("secret"));
    }

    #[::core::prelude::v1::test]
    fn stale_malformed_and_oversized_tiles_are_rejected() {
        let cx = TestAppContext::single();
        let grid = cx.update(|cx| {
            cx.new(|cx| {
                VirtualSheetGrid::new(VIRTUAL_SHEET_MAX_ROWS, VIRTUAL_SHEET_MAX_COLUMNS, cx)
                    .unwrap()
            })
        });
        cx.update(|cx| {
            grid.update(cx, |grid, _| {
                grid.set_tile_shape(1, 1).unwrap();
                let request = grid
                    .request_for_key(SheetTileKey {
                        tile_row: 0,
                        tile_column: 0,
                    })
                    .unwrap();
                grid.pending.insert(request.key);
                assert!(matches!(
                    grid.provide_tile(request.clone(), Vec::new()),
                    Err(VirtualSheetGridError::TileValueCount {
                        expected: 1,
                        received: 0
                    })
                ));

                grid.pending.insert(request.key);
                assert!(matches!(
                    grid.provide_tile(
                        request.clone(),
                        vec!["x".repeat(VIRTUAL_SHEET_MAX_CELL_BYTES + 1).into()]
                    ),
                    Err(VirtualSheetGridError::CellByteLimit { .. })
                ));

                grid.pending.insert(request.key);
                grid.reload();
                assert!(matches!(
                    grid.provide_tile(request, vec!["secret".into()]),
                    Err(VirtualSheetGridError::StaleTileGeneration { .. })
                ));
                assert!(grid.cache.is_empty());
                assert!(grid.pending.is_empty());
            });
        });
    }

    #[::core::prelude::v1::test]
    fn lru_cache_evicts_by_recency_without_materializing_sheet() {
        let cx = TestAppContext::single();
        let grid = cx.update(|cx| {
            cx.new(|cx| {
                VirtualSheetGrid::new(VIRTUAL_SHEET_MAX_ROWS, VIRTUAL_SHEET_MAX_COLUMNS, cx)
                    .unwrap()
                    .with_cache_limits(2, 2)
                    .with_tile_shape(1, 1)
                    .unwrap()
            })
        });
        cx.update(|cx| {
            grid.update(cx, |grid, _| {
                for column in 0..2 {
                    let request = grid
                        .request_for_key(SheetTileKey {
                            tile_row: 0,
                            tile_column: column,
                        })
                        .unwrap();
                    grid.pending.insert(request.key);
                    grid.provide_tile(request, vec![format!("v{column}").into()])
                        .unwrap();
                }
                assert_eq!(grid.cache.len(), 2);
                assert_eq!(
                    grid.loaded_value(SheetCellPosition::new(0, 0))
                        .map(|value| value.to_string()),
                    Some("v0".to_string())
                );
                let request = grid
                    .request_for_key(SheetTileKey {
                        tile_row: 0,
                        tile_column: 2,
                    })
                    .unwrap();
                grid.pending.insert(request.key);
                grid.provide_tile(request, vec!["v2".into()]).unwrap();
                assert!(grid.cache.contains_key(&SheetTileKey {
                    tile_row: 0,
                    tile_column: 0
                }));
                assert!(!grid.cache.contains_key(&SheetTileKey {
                    tile_row: 0,
                    tile_column: 1
                }));
                assert!(grid.cache.contains_key(&SheetTileKey {
                    tile_row: 0,
                    tile_column: 2
                }));
            });
        });
    }

    #[::core::prelude::v1::test]
    fn pending_requests_and_sparse_edits_stay_bounded() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::astryx_neutral()));
        let requested = Rc::new(RefCell::new(Vec::new()));
        let requested_for_grid = requested.clone();
        let (grid, window) = cx.add_window_view(move |_, cx| {
            VirtualSheetGrid::new(VIRTUAL_SHEET_MAX_ROWS, VIRTUAL_SHEET_MAX_COLUMNS, cx)
                .unwrap()
                .with_cache_limits(2, 3)
                .with_edit_limits(2, 8)
                .on_fetch_tile(move |request, _, _, _| {
                    requested_for_grid.borrow_mut().push(request);
                })
        });
        window.update(|window, cx| {
            grid.update(cx, |grid, cx| {
                let issued = grid
                    .request_viewport(0..10_000, 0..1_000, window, cx)
                    .unwrap();
                assert!(issued > 0 && issued <= 3);
                assert_eq!(grid.pending_tile_count(), 3);
                assert_eq!(requested.borrow().len(), 3);

                grid.set_cell_value(SheetCellPosition::new(0, 0), "a", window, cx)
                    .unwrap();
                grid.set_cell_value(SheetCellPosition::new(0, 1), "b", window, cx)
                    .unwrap();
                assert!(matches!(
                    grid.set_cell_value(SheetCellPosition::new(0, 2), "c", window, cx),
                    Err(VirtualSheetGridError::EditLimit { limit: 2 })
                ));
                assert_eq!(grid.sparse_edit_count(), 2);
                assert!(grid.undo(window, cx));
                assert_eq!(grid.sparse_edit_count(), 1);
                assert!(grid.redo(window, cx));
                assert_eq!(grid.sparse_edit_count(), 2);
            });
        });
    }

    #[::core::prelude::v1::test]
    fn paste_is_clipped_atomic_and_undoable() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::astryx_neutral()));
        let (grid, window) = cx.add_window_view(|_, cx| {
            VirtualSheetGrid::new(2, 2, cx)
                .unwrap()
                .with_edit_limits(4, 8)
        });
        window.update(|window, cx| {
            grid.update(cx, |grid, cx| {
                let pasted = grid.paste_tsv("a\tb\nc\td\ne\tf", window, cx).unwrap();
                assert_eq!(
                    pasted,
                    SheetNormalizedRange {
                        row_start: 0,
                        row_end: 2,
                        column_start: 0,
                        column_end: 2
                    }
                );
                assert_eq!(grid.sparse_edit_count(), 4);
                assert!(grid.undo(window, cx));
                assert_eq!(grid.sparse_edit_count(), 0);

                grid.edit_limit = 1;
                grid.select(SheetCellPosition::new(0, 0), false).unwrap();
                assert!(matches!(
                    grid.paste_tsv("x\ty", window, cx),
                    Err(VirtualSheetGridError::EditLimit { limit: 1 })
                ));
                assert_eq!(grid.sparse_edit_count(), 0);
            });
        });
    }

    #[::core::prelude::v1::test]
    fn rendered_accessibility_is_logical_and_mount_bounded() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::astryx_neutral()));
        let (grid, window) = cx.add_window_view(|_, cx| {
            VirtualSheetGrid::new(VIRTUAL_SHEET_MAX_ROWS, VIRTUAL_SHEET_MAX_COLUMNS, cx).unwrap()
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            let nodes = window
                .accessibility_tree()
                .nodes
                .values()
                .collect::<Vec<_>>();
            let root = nodes
                .iter()
                .find(|node| node.role == AccessibilityRole::Grid)
                .expect("grid node");
            assert_eq!(root.row_count, Some(VIRTUAL_SHEET_MAX_ROWS + 1));
            assert_eq!(root.column_count, Some(VIRTUAL_SHEET_MAX_COLUMNS));
            assert!(nodes.iter().any(|node| {
                node.role == AccessibilityRole::ColumnHeader && node.column_index == Some(1)
            }));
            assert!(
                nodes.iter().any(|node| {
                    node.role == AccessibilityRole::Row && node.row_index == Some(2)
                })
            );
            assert!(nodes.iter().any(|node| {
                node.role == AccessibilityRole::Cell
                    && node.row_index == Some(2)
                    && node.column_index == Some(1)
            }));
            let metrics = grid.read(cx).viewport_metrics();
            assert!(
                nodes.len()
                    <= metrics
                        .mounted_cells
                        .saturating_mul(3)
                        .saturating_add(metrics.mounted_rows.saturating_mul(2))
                        .saturating_add(metrics.mounted_columns.saturating_mul(2))
                        .saturating_add(16),
                "mounted accessibility tree was unbounded: nodes={}, rows={}, columns={}, cells={}",
                nodes.len(),
                metrics.mounted_rows,
                metrics.mounted_columns,
                metrics.mounted_cells
            );

            grid.update(cx, |grid, _| {
                let metrics = grid.viewport_metrics();
                assert!(metrics.mounted_rows < 128);
                assert!(metrics.mounted_columns < 128);
                assert!(metrics.mounted_cells < 4_096);
                grid.move_focus(-1, -1, false);
                assert_eq!(grid.selection.focus, SheetCellPosition::new(0, 0));
                grid.select_sheet_edge(true, false);
                grid.move_focus(1, 1, false);
                assert_eq!(
                    grid.selection.focus,
                    SheetCellPosition::new(
                        VIRTUAL_SHEET_MAX_ROWS - 1,
                        VIRTUAL_SHEET_MAX_COLUMNS - 1
                    )
                );
            });
        });
    }
}

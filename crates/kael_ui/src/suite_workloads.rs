//! Portable reference workloads for document, spreadsheet, presentation, and
//! whiteboard applications.
//!
//! These models deliberately keep logical data separate from mounted UI. They
//! are used by Kael's native and browser release probes and are also small
//! examples of the architecture expected from large one-codebase applications.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::mem::size_of;
use std::ops::Range;
use std::time::Duration;

use kael::{
    PointerId, PointerInputEvent, PointerPhase, PointerSample, SceneRect, SpatialIndex, TileDamage,
    TileDamageTracker,
};
use kael_engines::canvas::{CanvasRect, TileCache, TileCoord};
use kael_engines::game_loop::{FixedFrameClock, FrameAdvance};
use kael_engines::undo::UndoHistory;
use web_time::Instant;

/// Logical rows in the maintained spreadsheet workload.
pub const REFERENCE_SHEET_ROWS: usize = 1_000_000;
/// Logical columns in the maintained spreadsheet workload (Excel-scale).
pub const REFERENCE_SHEET_COLUMNS: usize = 16_384;
/// Logical blocks in the maintained large-document workload.
pub const REFERENCE_DOCUMENT_BLOCKS: usize = 250_000;
/// Logical slides in the maintained presentation workload.
pub const REFERENCE_SLIDES: usize = 10_000;
/// Shapes in the maintained whiteboard workload.
pub const REFERENCE_WHITEBOARD_SHAPES: usize = 100_000;

const MAX_SHEET_PAGE_ROWS: usize = 4_096;
const MAX_DOCUMENT_EDITS: usize = 4_096;
const MAX_DOCUMENT_EDIT_BYTES: usize = 1024 * 1024;
const MAX_SEARCH_BLOCKS_PER_BATCH: usize = 16_384;
const MAX_SEARCH_RESULTS_PER_BATCH: usize = 4_096;
const MAX_ACTIVE_POINTERS: usize = 32;
const MAX_POINTER_SAMPLES_PER_STROKE: usize = 8_192;
const WHITEBOARD_TILE_CACHE_BYTES: usize = 512 * 1024;
const WHITEBOARD_TILE_CACHE_ENTRIES: usize = 64;

/// A mounted interval over one logical virtual axis.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualAxisWindow {
    logical_items: usize,
    mounted: Range<usize>,
}

impl VirtualAxisWindow {
    /// Calculate a viewport-bounded interval for uniformly sized items.
    pub fn uniform(
        logical_items: usize,
        scroll_offset: f64,
        viewport_extent: f64,
        item_extent: f64,
        overscan: usize,
    ) -> Self {
        if logical_items == 0
            || !scroll_offset.is_finite()
            || !viewport_extent.is_finite()
            || !item_extent.is_finite()
            || viewport_extent <= 0.0
            || item_extent <= 0.0
        {
            return Self {
                logical_items,
                mounted: 0..0,
            };
        }

        let max_offset = logical_items as f64 * item_extent;
        let offset = scroll_offset.max(0.0).min(max_offset);
        let first = (offset / item_extent).floor() as usize;
        let last = ((offset + viewport_extent) / item_extent).ceil() as usize;
        let start = first.min(logical_items).saturating_sub(overscan);
        let end = last
            .min(logical_items)
            .saturating_add(overscan)
            .min(logical_items);
        Self {
            logical_items,
            mounted: start..end.max(start),
        }
    }

    /// Total logical item count.
    pub fn logical_items(&self) -> usize {
        self.logical_items
    }

    /// Mounted interval, including overscan.
    pub fn mounted_range(&self) -> Range<usize> {
        self.mounted.clone()
    }

    /// Number of mounted items, independent of logical collection size.
    pub fn mounted_count(&self) -> usize {
        self.mounted.len()
    }
}

/// A viewport-bounded row and column interval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TwoAxisVirtualWindow {
    /// Mounted rows.
    pub rows: VirtualAxisWindow,
    /// Mounted columns.
    pub columns: VirtualAxisWindow,
}

impl TwoAxisVirtualWindow {
    /// Number of mounted cells implied by the two intervals.
    pub fn mounted_cell_count(&self) -> usize {
        self.rows
            .mounted_count()
            .saturating_mul(self.columns.mounted_count())
    }
}

/// A row handle generated on demand by [`SheetWorkload`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SheetRow {
    /// Zero-based logical row index.
    pub index: usize,
}

/// Million-row, many-column spreadsheet workload without a resident cell matrix.
#[derive(Clone, Debug)]
pub struct SheetWorkload {
    rows: usize,
    columns: usize,
    row_extent: f64,
    column_extent: f64,
}

impl SheetWorkload {
    /// Construct the maintained million-by-16,384 reference sheet.
    pub fn reference() -> Self {
        Self {
            rows: REFERENCE_SHEET_ROWS,
            columns: REFERENCE_SHEET_COLUMNS,
            row_extent: 30.0,
            column_extent: 112.0,
        }
    }

    /// Logical row count.
    pub fn row_count(&self) -> usize {
        self.rows
    }

    /// Logical column count.
    pub fn column_count(&self) -> usize {
        self.columns
    }

    /// Compute the row/column intervals to mount for one viewport.
    pub fn visible_window(
        &self,
        scroll_x: f64,
        scroll_y: f64,
        viewport_width: f64,
        viewport_height: f64,
    ) -> TwoAxisVirtualWindow {
        TwoAxisVirtualWindow {
            rows: VirtualAxisWindow::uniform(
                self.rows,
                scroll_y,
                viewport_height,
                self.row_extent,
                8,
            ),
            columns: VirtualAxisWindow::uniform(
                self.columns,
                scroll_x,
                viewport_width,
                self.column_extent,
                2,
            ),
        }
    }

    /// Generate one bounded page for `DataTable::set_page_data_for`.
    pub fn page(&self, page_start: usize, requested_rows: usize) -> Vec<SheetRow> {
        let end = page_start
            .saturating_add(requested_rows.min(MAX_SHEET_PAGE_ROWS))
            .min(self.rows);
        (page_start.min(self.rows)..end)
            .map(|index| SheetRow { index })
            .collect()
    }

    /// Compute a deterministic cell value without storing a cell matrix.
    pub fn cell_value(&self, row: usize, column: usize) -> Option<u64> {
        if row >= self.rows || column >= self.columns {
            return None;
        }
        Some(
            (row as u64 + 1)
                .wrapping_mul(1_000_003)
                .wrapping_add((column as u64 + 1).wrapping_mul(97)),
        )
    }

    /// Bytes retained by a generated row page, excluding allocator overhead.
    pub fn row_page_payload_bytes(rows: &[SheetRow]) -> usize {
        rows.len().saturating_mul(size_of::<SheetRow>())
    }
}

/// Result of one bounded document search step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentSearchBatch {
    /// Matching logical block indices.
    pub matches: Vec<usize>,
    /// Cursor to pass to the next search step.
    pub next_block: usize,
    /// Number of blocks inspected in this step.
    pub scanned_blocks: usize,
    /// Whether the logical document was fully searched.
    pub complete: bool,
}

/// Large block/page document with sparse edits and bounded transactional undo.
#[derive(Clone, Debug)]
pub struct DocumentWorkload {
    block_count: usize,
    blocks_per_page: usize,
    page_extent: f64,
    edits: UndoHistory<BTreeMap<usize, String>>,
}

impl DocumentWorkload {
    /// Construct the maintained 250,000-block document.
    pub fn reference() -> Self {
        let mut edits = UndoHistory::new(BTreeMap::new());
        edits.set_limit(64);
        Self {
            block_count: REFERENCE_DOCUMENT_BLOCKS,
            blocks_per_page: 48,
            page_extent: 1_056.0,
            edits,
        }
    }

    /// Logical block count.
    pub fn block_count(&self) -> usize {
        self.block_count
    }

    /// Logical page count.
    pub fn page_count(&self) -> usize {
        self.block_count.div_ceil(self.blocks_per_page)
    }

    /// Page interval mounted for the current vertical scroll position.
    pub fn visible_pages(&self, scroll_y: f64, viewport_height: f64) -> VirtualAxisWindow {
        VirtualAxisWindow::uniform(
            self.page_count(),
            scroll_y,
            viewport_height,
            self.page_extent,
            2,
        )
    }

    /// Logical block interval covered by a mounted page interval.
    pub fn blocks_for_pages(&self, pages: &VirtualAxisWindow) -> Range<usize> {
        let page_range = pages.mounted_range();
        let start = page_range.start.saturating_mul(self.blocks_per_page);
        let end = page_range
            .end
            .saturating_mul(self.blocks_per_page)
            .min(self.block_count);
        start..end
    }

    /// Read an edited block or lazily generate its immutable base text.
    pub fn block_text(&self, block: usize) -> Option<String> {
        if block >= self.block_count {
            return None;
        }
        self.edits
            .current()
            .get(&block)
            .cloned()
            .or_else(|| Some(Self::generated_block_text(block)))
    }

    /// Commit a sparse block edit as one undo transaction.
    pub fn edit_block(&mut self, block: usize, text: impl Into<String>) -> bool {
        if block >= self.block_count
            || (self.edits.current().len() >= MAX_DOCUMENT_EDITS
                && !self.edits.current().contains_key(&block))
        {
            return false;
        }
        let text = text.into();
        if text.len() > MAX_DOCUMENT_EDIT_BYTES {
            return false;
        }
        self.edits.edit(|edits| {
            edits.insert(block, text);
        });
        true
    }

    /// Undo one document edit.
    pub fn undo(&mut self) -> bool {
        self.edits.undo()
    }

    /// Redo one document edit.
    pub fn redo(&mut self) -> bool {
        self.edits.redo()
    }

    /// Number of sparse edited blocks resident in the current document state.
    pub fn resident_edit_count(&self) -> usize {
        self.edits.current().len()
    }

    /// Run one bounded search step suitable for a UI task or existing Kael worker.
    pub fn search_chunk(
        &self,
        query: &str,
        start_block: usize,
        max_blocks: usize,
        max_results: usize,
    ) -> DocumentSearchBatch {
        let start = start_block.min(self.block_count);
        let scan_limit = max_blocks.min(MAX_SEARCH_BLOCKS_PER_BATCH);
        let result_limit = max_results.min(MAX_SEARCH_RESULTS_PER_BATCH);
        let end = start.saturating_add(scan_limit).min(self.block_count);
        let query = query.to_lowercase();
        let mut matches = Vec::new();
        if !query.is_empty() && result_limit > 0 {
            for block in start..end {
                let text = self
                    .block_text(block)
                    .expect("bounded block index must resolve");
                if text.to_lowercase().contains(&query) {
                    matches.push(block);
                    if matches.len() == result_limit {
                        return DocumentSearchBatch {
                            matches,
                            next_block: block.saturating_add(1),
                            scanned_blocks: block.saturating_add(1).saturating_sub(start),
                            complete: block.saturating_add(1) >= self.block_count,
                        };
                    }
                }
            }
        }
        DocumentSearchBatch {
            matches,
            next_block: end,
            scanned_blocks: end.saturating_sub(start),
            complete: end >= self.block_count,
        }
    }

    fn generated_block_text(block: usize) -> String {
        format!(
            "Document block {block}: portable retained text, section {}, paragraph {}.",
            block / 48,
            block % 48
        )
    }
}

/// The single retained slide surface reused while navigating a large deck.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetainedSlideSurface {
    slide_index: usize,
    revision: u64,
    retained_node_count: usize,
}

impl RetainedSlideSurface {
    /// Selected slide index.
    pub fn slide_index(&self) -> usize {
        self.slide_index
    }

    /// Monotonic retained-surface revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Nodes retained for only the active slide.
    pub fn retained_node_count(&self) -> usize {
        self.retained_node_count
    }
}

/// Large slide deck with virtualized thumbnails and one retained slide surface.
#[derive(Clone, Debug)]
pub struct SlideDeckWorkload {
    slide_count: usize,
    thumbnail_extent: f64,
    surface: RetainedSlideSurface,
}

impl SlideDeckWorkload {
    /// Construct the maintained 10,000-slide deck.
    pub fn reference() -> Self {
        Self {
            slide_count: REFERENCE_SLIDES,
            thumbnail_extent: 118.0,
            surface: RetainedSlideSurface {
                slide_index: 0,
                revision: 1,
                retained_node_count: 12,
            },
        }
    }

    /// Logical slide count.
    pub fn slide_count(&self) -> usize {
        self.slide_count
    }

    /// Mounted thumbnail interval.
    pub fn visible_thumbnails(&self, scroll_y: f64, viewport_height: f64) -> VirtualAxisWindow {
        VirtualAxisWindow::uniform(
            self.slide_count,
            scroll_y,
            viewport_height,
            self.thumbnail_extent,
            3,
        )
    }

    /// Reuse the retained surface for a different slide.
    pub fn select_slide(&mut self, slide_index: usize) -> bool {
        if slide_index >= self.slide_count || slide_index == self.surface.slide_index {
            return false;
        }
        self.surface.slide_index = slide_index;
        self.surface.revision = self.surface.revision.wrapping_add(1).max(1);
        self.surface.retained_node_count = 12 + (slide_index % 5);
        true
    }

    /// Current retained slide surface.
    pub fn surface(&self) -> &RetainedSlideSurface {
        &self.surface
    }
}

/// One retained shape in the whiteboard workload.
#[derive(Clone, Debug, PartialEq)]
pub struct WhiteboardShape {
    /// Stable shape identifier.
    pub id: usize,
    /// World-space shape bounds.
    pub bounds: SceneRect<f64>,
}

/// A 100,000-shape retained whiteboard using Kael's spatial, damage, tile,
/// rich-pointer, and fixed-frame primitives.
pub struct WhiteboardWorkload {
    shapes: Vec<WhiteboardShape>,
    spatial: SpatialIndex<usize>,
    damage: TileDamageTracker,
    tile_cache: TileCache,
    frame_clock: FixedFrameClock,
    active_strokes: HashMap<PointerId, VecDeque<PointerSample>>,
    completed_strokes: u64,
}

impl WhiteboardWorkload {
    /// Construct and index the maintained 100,000-shape scene.
    pub fn reference() -> Self {
        Self::with_shape_count(REFERENCE_WHITEBOARD_SHAPES)
    }

    /// Construct a deterministic scene with up to one million shapes.
    pub fn with_shape_count(shape_count: usize) -> Self {
        let shape_count = shape_count.min(1_000_000);
        let mut shapes = Vec::with_capacity(shape_count);
        let mut spatial = SpatialIndex::new();
        const COLUMNS: usize = 400;
        for id in 0..shape_count {
            let column = id % COLUMNS;
            let row = id / COLUMNS;
            let bounds = SceneRect::new(column as f64 * 48.0, row as f64 * 36.0, 32.0, 22.0);
            spatial
                .insert_checked(bounds, id)
                .expect("generated whiteboard bounds and capacity are valid");
            shapes.push(WhiteboardShape { id, bounds });
        }
        Self {
            shapes,
            spatial,
            damage: TileDamageTracker::new_checked(256.0, 4_096)
                .expect("reference damage limits are valid"),
            tile_cache: TileCache::with_limits(
                WHITEBOARD_TILE_CACHE_BYTES,
                WHITEBOARD_TILE_CACHE_ENTRIES,
            ),
            frame_clock: FixedFrameClock::default(),
            active_strokes: HashMap::new(),
            completed_strokes: 0,
        }
    }

    /// Logical retained shape count.
    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    /// Read one retained shape by stable index.
    pub fn shape(&self, shape_index: usize) -> Option<&WhiteboardShape> {
        self.shapes.get(shape_index)
    }

    /// Query only shapes intersecting a world-space viewport.
    pub fn visible_shape_indices(&self, viewport: SceneRect<f64>) -> Vec<usize> {
        self.spatial
            .query_rect(&viewport)
            .into_iter()
            .copied()
            .collect()
    }

    /// Candidate count examined by the most recent spatial query.
    pub fn last_spatial_candidate_count(&self) -> usize {
        self.spatial.last_query_candidate_count()
    }

    /// Occupied cells in the spatial hash.
    pub fn spatial_cell_count(&self) -> usize {
        self.spatial.cell_count()
    }

    /// Mark one shape's tiles as damaged.
    pub fn invalidate_shape(&mut self, shape_index: usize) -> bool {
        let Some(shape) = self.shapes.get(shape_index) else {
            return false;
        };
        self.damage.invalidate(shape.bounds);
        true
    }

    /// Consume accumulated tile damage.
    pub fn take_damage(&mut self) -> TileDamage {
        self.damage.take()
    }

    /// Populate bounded retained tile payloads for a viewport.
    pub fn cache_visible_tiles(
        &mut self,
        viewport: SceneRect<f64>,
        zoom: u8,
        bytes_per_tile: usize,
    ) -> usize {
        let canvas_viewport = CanvasRect {
            x: viewport.x,
            y: viewport.y,
            width: viewport.width,
            height: viewport.height,
        };
        let payload_bytes = bytes_per_tile.min(64 * 1024);
        let visible = self.tile_cache.visible_tiles(&canvas_viewport, zoom);
        for coord in visible {
            let _ = self.tile_cache.insert(coord, vec![0_u8; payload_bytes]);
        }
        self.tile_cache.len()
    }

    /// Resident tile payload bytes.
    pub fn tile_cache_bytes(&self) -> usize {
        self.tile_cache.byte_len()
    }

    /// Resident tile entries.
    pub fn tile_cache_entries(&self) -> usize {
        self.tile_cache.len()
    }

    /// Verify whether one tile currently resides in the cache.
    pub fn has_cached_tile(&self, coordinate: TileCoord) -> bool {
        self.tile_cache.get(&coordinate).is_some()
    }

    /// Feed a native or browser rich pointer event into the bounded stroke state.
    pub fn handle_pointer(&mut self, event: &PointerInputEvent) {
        if event.phase == PointerPhase::Down
            && !self.active_strokes.contains_key(&event.pointer_id)
            && self.active_strokes.len() >= MAX_ACTIVE_POINTERS
        {
            return;
        }
        if event.phase == PointerPhase::Down {
            self.active_strokes.entry(event.pointer_id).or_default();
        }
        let Some(stroke) = self.active_strokes.get_mut(&event.pointer_id) else {
            return;
        };
        for sample in event.stroke_samples() {
            if stroke.len() == MAX_POINTER_SAMPLES_PER_STROKE {
                stroke.pop_front();
            }
            stroke.push_back(sample);
        }
        if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
            self.active_strokes.remove(&event.pointer_id);
            self.completed_strokes = self.completed_strokes.saturating_add(1);
        }
    }

    /// Number of simultaneous active pointers retained.
    pub fn active_pointer_count(&self) -> usize {
        self.active_strokes.len()
    }

    /// Total retained rich-pointer samples across active strokes.
    pub fn retained_pointer_sample_count(&self) -> usize {
        self.active_strokes.values().map(VecDeque::len).sum()
    }

    /// Completed stroke sequences.
    pub fn completed_stroke_count(&self) -> u64 {
        self.completed_strokes
    }

    /// Advance the deterministic fixed-update clock for one display frame.
    pub fn advance_frame(&mut self, frame_delta: Duration) -> FrameAdvance {
        self.frame_clock.advance_by(frame_delta)
    }
}

/// Deterministic release evidence for all four maintained suite workloads.
#[derive(Clone, Debug)]
pub struct SuiteWorkloadProbeReport {
    /// Spreadsheet logical rows.
    pub sheet_rows: usize,
    /// Spreadsheet logical columns.
    pub sheet_columns: usize,
    /// Spreadsheet cells mounted for a representative viewport.
    pub sheet_mounted_cells: usize,
    /// Document logical blocks.
    pub document_blocks: usize,
    /// Document blocks mounted for a representative viewport.
    pub document_mounted_blocks: usize,
    /// Blocks inspected by one bounded search batch.
    pub document_search_scanned: usize,
    /// Presentation logical slides.
    pub slides: usize,
    /// Presentation thumbnails mounted for a representative viewport.
    pub mounted_thumbnails: usize,
    /// Whiteboard logical shapes.
    pub whiteboard_shapes: usize,
    /// Whiteboard shapes returned for a representative viewport.
    pub whiteboard_visible_shapes: usize,
    /// Spatial candidates examined for that viewport.
    pub whiteboard_spatial_candidates: usize,
    /// Whiteboard retained tile bytes.
    pub whiteboard_tile_bytes: usize,
    /// Fixed updates emitted for a deliberately long display frame.
    pub bounded_frame_updates: u32,
    /// Time to construct and index the 100,000-shape workload.
    pub whiteboard_build_millis: u128,
    /// Time for its representative viewport query.
    pub whiteboard_query_micros: u128,
}

impl SuiteWorkloadProbeReport {
    /// Return true when logical scale, mount/cache bounds, and generous CI
    /// latency budgets all hold.
    pub fn passed(&self) -> bool {
        self.sheet_rows == REFERENCE_SHEET_ROWS
            && self.sheet_columns == REFERENCE_SHEET_COLUMNS
            && self.sheet_mounted_cells <= 2_048
            && self.document_blocks == REFERENCE_DOCUMENT_BLOCKS
            && self.document_mounted_blocks <= 288
            && self.document_search_scanned <= 8_192
            && self.slides == REFERENCE_SLIDES
            && self.mounted_thumbnails <= 24
            && self.whiteboard_shapes == REFERENCE_WHITEBOARD_SHAPES
            && self.whiteboard_visible_shapes <= 2_048
            && self.whiteboard_spatial_candidates <= 4_096
            && self.whiteboard_tile_bytes <= WHITEBOARD_TILE_CACHE_BYTES
            && self.bounded_frame_updates <= 8
            && self.whiteboard_build_millis <= 30_000
            && self.whiteboard_query_micros <= 2_000_000
    }
}

/// Run the maintained deterministic suite-scale release probe.
pub fn run_suite_workload_probe() -> SuiteWorkloadProbeReport {
    let sheet = SheetWorkload::reference();
    let sheet_window = sheet.visible_window(10_000.0, 25_000_000.0, 1_280.0, 720.0);

    let mut document = DocumentWorkload::reference();
    let pages = document.visible_pages(2_000_000.0, 900.0);
    let document_mounted_blocks = document.blocks_for_pages(&pages).len();
    let _ = document.edit_block(42, "Kael suite probe edited paragraph");
    let _ = document.undo();
    let _ = document.redo();
    let search = document.search_chunk("paragraph 7", 0, 8_192, 64);

    let mut deck = SlideDeckWorkload::reference();
    let thumbnails = deck.visible_thumbnails(500_000.0, 720.0);
    let _ = deck.select_slide(9_999);

    let whiteboard_started = Instant::now();
    let mut whiteboard = WhiteboardWorkload::reference();
    let whiteboard_build_millis = whiteboard_started.elapsed().as_millis();
    let viewport = SceneRect::new(3_840.0, 2_400.0, 1_280.0, 720.0);
    let query_started = Instant::now();
    let visible = whiteboard.visible_shape_indices(viewport);
    let whiteboard_query_micros = query_started.elapsed().as_micros();
    let whiteboard_spatial_candidates = whiteboard.last_spatial_candidate_count();
    let _ = whiteboard.invalidate_shape(50_000);
    let _ = whiteboard.take_damage();
    let _ = whiteboard.cache_visible_tiles(viewport, 1, 16 * 1024);
    let frame = whiteboard.advance_frame(Duration::from_millis(500));

    SuiteWorkloadProbeReport {
        sheet_rows: sheet.row_count(),
        sheet_columns: sheet.column_count(),
        sheet_mounted_cells: sheet_window.mounted_cell_count(),
        document_blocks: document.block_count(),
        document_mounted_blocks,
        document_search_scanned: search.scanned_blocks,
        slides: deck.slide_count(),
        mounted_thumbnails: thumbnails.mounted_count(),
        whiteboard_shapes: whiteboard.shape_count(),
        whiteboard_visible_shapes: visible.len(),
        whiteboard_spatial_candidates,
        whiteboard_tile_bytes: whiteboard.tile_cache_bytes(),
        bounded_frame_updates: frame.update_steps(),
        whiteboard_build_millis,
        whiteboard_query_micros,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kael::{Modifiers, PointerButtons, PointerType, point, px};

    #[test]
    fn spreadsheet_viewport_and_pages_stay_bounded_at_full_scale() {
        let sheet = SheetWorkload::reference();
        let window = sheet.visible_window(1_700_000.0, 29_000_000.0, 1_280.0, 720.0);
        assert_eq!(sheet.row_count(), 1_000_000);
        assert_eq!(sheet.column_count(), 16_384);
        assert!(window.rows.mounted_count() <= 42);
        assert!(window.columns.mounted_count() <= 18);
        assert!(window.mounted_cell_count() <= 756);
        let page = sheet.page(999_900, 10_000);
        assert_eq!(page.len(), 100);
        assert!(SheetWorkload::row_page_payload_bytes(&page) <= 800);
        assert!(sheet.cell_value(999_999, 16_383).is_some());
        assert!(sheet.cell_value(1_000_000, 0).is_none());
    }

    #[test]
    fn document_edit_search_undo_and_mounting_are_bounded() {
        let mut document = DocumentWorkload::reference();
        let pages = document.visible_pages(3_000_000.0, 900.0);
        assert!(pages.mounted_count() <= 6);
        assert!(document.blocks_for_pages(&pages).len() <= 288);
        let original = document.block_text(42).unwrap();
        assert!(document.edit_block(42, "Unique Kael whiteboard paragraph"));
        assert_eq!(document.resident_edit_count(), 1);
        assert_eq!(
            document.block_text(42).unwrap(),
            "Unique Kael whiteboard paragraph"
        );
        assert!(document.undo());
        assert_eq!(document.block_text(42).unwrap(), original);
        assert!(document.redo());
        let search = document.search_chunk("unique kael", 0, 1_024, 16);
        assert_eq!(search.matches, vec![42]);
        assert!(search.scanned_blocks <= 1_024);
    }

    #[test]
    fn slide_thumbnails_are_virtual_and_surface_is_reused() {
        let mut deck = SlideDeckWorkload::reference();
        let thumbnails = deck.visible_thumbnails(800_000.0, 720.0);
        assert!(thumbnails.mounted_count() <= 14);
        let first_revision = deck.surface().revision();
        assert!(deck.select_slide(9_999));
        assert_eq!(deck.surface().slide_index(), 9_999);
        assert!(deck.surface().revision() > first_revision);
        assert!(deck.surface().retained_node_count() <= 16);
    }

    #[test]
    fn whiteboard_culls_one_hundred_thousand_shapes_and_bounds_realtime_state() {
        let mut whiteboard = WhiteboardWorkload::reference();
        let viewport = SceneRect::new(4_000.0, 2_000.0, 1_280.0, 720.0);
        let visible = whiteboard.visible_shape_indices(viewport);
        assert_eq!(whiteboard.shape_count(), 100_000);
        assert!(!visible.is_empty());
        assert!(visible.len() <= 2_048);
        assert!(whiteboard.last_spatial_candidate_count() <= 4_096);
        assert!(whiteboard.spatial_cell_count() < whiteboard.shape_count());

        assert!(whiteboard.invalidate_shape(50_000));
        let damage = whiteboard.take_damage();
        assert!(!damage.is_empty());
        assert!(!damage.is_full());

        whiteboard.cache_visible_tiles(viewport, 1, 32 * 1024);
        assert!(whiteboard.tile_cache_entries() <= WHITEBOARD_TILE_CACHE_ENTRIES);
        assert!(whiteboard.tile_cache_bytes() <= WHITEBOARD_TILE_CACHE_BYTES);

        let event = PointerInputEvent {
            phase: PointerPhase::Down,
            pointer_id: PointerId::new(7),
            pointer_type: PointerType::Pen,
            position: point(px(10.0), px(20.0)),
            buttons: PointerButtons::PRIMARY,
            modifiers: Modifiers::default(),
            pressure: 0.7,
            is_primary: true,
            ..Default::default()
        };
        whiteboard.handle_pointer(&event);
        assert_eq!(whiteboard.active_pointer_count(), 1);
        assert_eq!(whiteboard.retained_pointer_sample_count(), 1);
        let mut ended = event;
        ended.phase = PointerPhase::Up;
        ended.buttons = PointerButtons::empty();
        whiteboard.handle_pointer(&ended);
        assert_eq!(whiteboard.active_pointer_count(), 0);
        assert_eq!(whiteboard.completed_stroke_count(), 1);

        let frame = whiteboard.advance_frame(Duration::from_millis(500));
        assert!(frame.update_steps() <= 8);
        assert!(frame.dropped_time() > Duration::ZERO);
    }

    #[test]
    fn combined_reference_probe_meets_release_budgets() {
        let report = run_suite_workload_probe();
        assert!(report.passed(), "suite workload report: {report:?}");
    }
}

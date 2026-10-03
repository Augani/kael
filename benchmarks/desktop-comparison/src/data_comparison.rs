//! Native table comparison using shipped controls, not a benchmark renderer.
//! Kael uses VirtualSheetGrid with bounded local tiles; GPUI Kit uses DataTable
//! with its documented delegate. The same two immutable datasets are retained
//! in both processes. Replacement swaps datasets; it is not an allocation test.
//! Native editing is excluded because GPUI Kit delegates that workflow to apps.
#![recursion_limit = "256"]
#[cfg(all(feature = "kael-engine", feature = "gpui-kit-engine"))]
compile_error!("select exactly one framework engine");
#[cfg(feature = "gpui-kit-engine")]
use gpui_kit as ui;
#[cfg(feature = "gpui-kit-engine")]
use gpui_kit::component::{
    Sizable,
    table::{Column, DataTable, TableDelegate, TableState},
};
#[cfg(feature = "kael-engine")]
use kael as ui;
#[cfg(feature = "kael-engine")]
use kael_ui::display::virtual_sheet_grid::{SheetCellPosition, VirtualSheetGrid};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use ui::{prelude::*, *};

#[path = "native_geometry.rs"]
mod native_geometry;
use native_geometry::NativeGeometry;

const CONTRACT: &str = "native-data-table-v1";
const FONT_FAMILY: &str = "Menlo";
const FONT_SIZE: f32 = 13.0;
const LINE_HEIGHT: f32 = 20.0;
const HEADER_FONT_SIZE: f32 = 12.0;
const ROW_HEIGHT: f32 = 28.0;
const COLUMN_WIDTH: f32 = 112.0;
const TABLE_WIDTH: f32 = 780.0;
const COLUMNS: [&str; 8] = [
    "Title", "Owner", "Status", "Score", "Sprint", "Updated", "Tags", "Notes",
];

#[derive(Clone, Copy)]
struct Options {
    rows: usize,
    quick: bool,
}
impl Options {
    fn parse() -> Self {
        let mut options = Self {
            rows: 100_000,
            quick: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--quick" => options.quick = true,
                "--rows" => {
                    options.rows = args
                        .next()
                        .expect("--rows needs a count")
                        .parse()
                        .expect("positive row count")
                }
                _ => panic!("unknown flag {arg}; use --quick or --rows N"),
            }
        }
        assert!(
            (1..=1_000_000).contains(&options.rows),
            "rows must be 1..=1000000"
        );
        options
    }
    fn duration(self, phase: &str) -> Duration {
        Duration::from_secs(if self.quick {
            1
        } else if phase.starts_with("idle") {
            5
        } else {
            12
        })
    }
}

#[derive(Clone)]
struct Record {
    values: [SharedString; 8],
}
fn expected_cell(row: usize, column: usize, generation: usize) -> String {
    match column {
        0 => format!("Record {row:06} 日本語"),
        1 => format!("Team {:02} café", row % 97),
        2 => format!("{} · ✓", ["Ready", "Review", "Active"][row % 3]),
        3 => format!("{:03} 点", row.wrapping_mul(37) % 1000),
        4 => format!("Sprint {:02} Δ", row / 1000 % 52),
        5 => format!("2026-10-{:02} 火", row % 28 + 1),
        6 => format!("tag{:02} 👩‍💻", row % 31),
        7 => format!("rev {generation:02} · naïve {row:06}"),
        _ => unreachable!(),
    }
}
struct Dataset {
    records: Arc<[Record]>,
    generation: usize,
    hashes: [String; 2],
    bytes: usize,
}
impl Dataset {
    fn new(rows: usize, generation: usize) -> Arc<Self> {
        let records: Arc<[Record]> = (0..rows)
            .map(|row| Record {
                values: std::array::from_fn(|column| expected_cell(row, column, generation).into()),
            })
            .collect::<Vec<_>>()
            .into();
        let bytes = records
            .iter()
            .flat_map(|record| &record.values)
            .map(|value| value.len())
            .sum();
        let hashes = [ordered_hash(&records, false), ordered_hash(&records, true)];
        Arc::new(Self {
            records,
            generation,
            hashes,
            bytes,
        })
    }
}
fn ordered_hash(records: &[Record], reversed: bool) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for index in 0..records.len() {
        let record = &records[if reversed {
            records.len() - 1 - index
        } else {
            index
        }];
        for value in &record.values {
            // Length prefixes distinguish cell and row boundaries without
            // relying on a character never appearing in the actual fixture.
            for byte in (value.len() as u64)
                .to_le_bytes()
                .into_iter()
                .chain(value.bytes())
            {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
    }
    format!("fnv1a64:{hash:016x}")
}
#[derive(Clone)]
struct Snapshot {
    dataset: Arc<Dataset>,
    reversed: bool,
}
impl Snapshot {
    fn record_id(&self, row: usize) -> usize {
        if self.reversed {
            self.dataset.records.len() - 1 - row
        } else {
            row
        }
    }
    fn cell(&self, row: usize, column: usize) -> SharedString {
        self.dataset.records[self.record_id(row)].values[column].clone()
    }
    fn hash(&self) -> String {
        ordered_hash(&self.dataset.records, self.reversed)
    }
}
type SharedSource = Rc<RefCell<Snapshot>>;

#[cfg(feature = "gpui-kit-engine")]
struct Delegate {
    source: SharedSource,
}
#[cfg(feature = "gpui-kit-engine")]
impl TableDelegate for Delegate {
    fn columns_count(&self, _: &App) -> usize {
        COLUMNS.len()
    }
    fn rows_count(&self, _: &App) -> usize {
        self.source.borrow().dataset.records.len()
    }
    fn column(&self, column: usize, _: &App) -> Column {
        Column::new(COLUMNS[column], COLUMNS[column])
            .width(px(COLUMN_WIDTH))
            .resizable(false)
            .movable(false)
            .paddings(Edges {
                top: px(0.0),
                bottom: px(0.0),
                left: px(8.0),
                right: px(8.0),
            })
            .when(column == 0, |column| column.fixed_left())
    }
    fn render_header(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        // The public header delegate can set the outer extent independently
        // of DataTable's native 28px leaf cells. Match Kael's 32px header so
        // both body viewports have the same number of 28px rows.
        div().id("header").h(px(32.0))
    }
    fn render_th(
        &mut self,
        column: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .size_full()
            .text_size(px(HEADER_FONT_SIZE))
            .child(COLUMNS[column])
    }
    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // DataTable owns the virtual rows/columns, native scrolling, selection,
        // fixed pane, and header. Only application cell content is delegated.
        div()
            .size_full()
            .text_size(px(FONT_SIZE))
            .overflow_hidden()
            .child(self.source.borrow().cell(row, column))
    }
    fn cell_text(&self, row: usize, column: usize, _: &App) -> String {
        self.source.borrow().cell(row, column).to_string()
    }
}
#[cfg(feature = "gpui-kit-engine")]
type Control = TableState<Delegate>;
#[cfg(feature = "kael-engine")]
type Control = VirtualSheetGrid;

fn make_control(source: SharedSource, window: &mut Window, cx: &mut App) -> Entity<Control> {
    #[cfg(feature = "gpui-kit-engine")]
    {
        cx.new(|cx| {
            TableState::new(Delegate { source }, window, cx)
                .cell_selectable(true)
                .row_selectable(false)
                .col_selectable(false)
                .row_header(false)
                .col_resizable(false)
                .col_movable(false)
                .sortable(false)
                .loop_selection(false)
        })
    }
    #[cfg(feature = "kael-engine")]
    {
        let _ = window;
        let rows = source.borrow().dataset.records.len();
        cx.new(|cx| {
            let mut grid = VirtualSheetGrid::new(rows, COLUMNS.len(), cx)
                .unwrap()
                .with_tile_shape(64, 8)
                .unwrap()
                .with_cache_limits(16, 4)
                .with_frozen_panes(0, 1)
                .unwrap()
                .on_fetch_tile(move |request, entity, _, cx| {
                    let snapshot = source.borrow().clone();
                    let mut values = Vec::with_capacity(request.cell_count().unwrap());
                    for row in request.rows.clone() {
                        for column in request.columns.clone() {
                            values.push(snapshot.cell(row, column));
                        }
                    }
                    cx.defer(move |cx| {
                        entity.update(cx, |grid, cx| {
                            // A query can replace the dataset between request and
                            // this deferred foreground completion. Stale tiles are
                            // deliberately rejected by the shipped grid API.
                            if grid.provide_tile(request, values).is_ok() {
                                cx.notify();
                            }
                        })
                    });
                });
            for (column, title) in COLUMNS.iter().enumerate() {
                grid.set_column_header(column, *title).unwrap();
            }
            grid
        })
    }
}
fn control_element(control: &Entity<Control>) -> AnyElement {
    #[cfg(feature = "kael-engine")]
    {
        control.clone().into_any_element()
    }
    #[cfg(feature = "gpui-kit-engine")]
    {
        DataTable::new(control)
            .bordered(false)
            .stripe(false)
            .with_size(px(ROW_HEIGHT))
            .into_any_element()
    }
}
fn scroll(control: &Entity<Control>, row: usize, column: usize, cx: &mut App) {
    control.update(cx, |control, cx| {
        #[cfg(feature = "kael-engine")]
        control
            .scroll_to_cell(SheetCellPosition::new(row, column))
            .unwrap();
        #[cfg(feature = "gpui-kit-engine")]
        {
            control.scroll_to_row(row, cx);
            control.scroll_to_col(column, cx);
        }
        cx.notify();
    });
}
fn select(control: &Entity<Control>, row: usize, column: usize, cx: &mut App) {
    control.update(cx, |control, cx| {
        #[cfg(feature = "kael-engine")]
        control
            .select(SheetCellPosition::new(row, column), false)
            .unwrap();
        #[cfg(feature = "gpui-kit-engine")]
        control.set_selected_cell(row, column, cx);
        cx.notify();
    });
}
fn reset_control(control: &Entity<Control>, cx: &mut App) {
    control.update(cx, |control, cx| {
        #[cfg(feature = "kael-engine")]
        control.reset_data(cx);
        #[cfg(feature = "gpui-kit-engine")]
        {
            control.refresh(cx);
            control.clear_selection(cx);
            control.set_selected_cell(0, 0, cx);
        }
        cx.notify();
    });
}

struct Workload {
    control: Entity<Control>,
    _observer: Subscription,
    source: SharedSource,
    datasets: [Arc<Dataset>; 2],
    options: Options,
    selected: (usize, usize),
    scroll_target: (usize, usize),
    expected_generation: usize,
    expected_reversed: bool,
    fixture_hash: String,
    sequence: usize,
    operations: [u64; 7],
    operation_us: Vec<u64>,
    phase_checks: Vec<(String, bool)>,
    phase_oracles: Vec<serde_json::Value>,
    measuring: bool,
    phase: &'static str,
    started: Instant,
    first_render_us: Option<u128>,
    active_us: Vec<u128>,
    last_render: Option<Instant>,
    renders: u64,
    draw_us: Vec<u64>,
    submission_us: Vec<u64>,
    first_submission_us: Option<u128>,
    gpu_samples: Vec<(String, Option<u64>)>,
    geometry: NativeGeometry,
    #[cfg(all(feature = "kael-engine", feature = "frame-timing"))]
    last_draw: Option<u64>,
    #[cfg(all(feature = "kael-engine", feature = "frame-timing"))]
    last_submission: Option<u64>,
    #[cfg(all(feature = "gpui-kit-engine", feature = "frame-timing"))]
    collector: gpui_kit::profiler::FrameTimingCollector,
}
impl Workload {
    fn new(
        started: Instant,
        options: Options,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let datasets = [Dataset::new(options.rows, 0), Dataset::new(options.rows, 1)];
        let fixture_hash = datasets[0].hashes[0].clone();
        let source = Rc::new(RefCell::new(Snapshot {
            dataset: datasets[0].clone(),
            reversed: false,
        }));
        let control = make_control(source.clone(), window, cx);
        select(&control, 0, 0, cx);
        let observer = cx.observe(&control, |_, _, cx| cx.notify());
        Self {
            control,
            _observer: observer,
            source,
            datasets,
            options,
            selected: (0, 0),
            scroll_target: (0, 1),
            expected_generation: 0,
            expected_reversed: false,
            fixture_hash,
            sequence: 0,
            operations: [0; 7],
            operation_us: Vec::with_capacity(4096),
            phase_checks: Vec::with_capacity(4),
            phase_oracles: Vec::with_capacity(4),
            measuring: true,
            phase: "startup",
            started,
            first_render_us: None,
            active_us: Vec::with_capacity(4096),
            last_render: None,
            renders: 0,
            draw_us: Vec::with_capacity(if cfg!(feature = "frame-timing") {
                4096
            } else {
                0
            }),
            submission_us: Vec::with_capacity(if cfg!(feature = "frame-timing") {
                4096
            } else {
                0
            }),
            first_submission_us: None,
            gpu_samples: Vec::with_capacity(5),
            geometry: NativeGeometry::new(),
            #[cfg(all(feature = "kael-engine", feature = "frame-timing"))]
            last_draw: None,
            #[cfg(all(feature = "kael-engine", feature = "frame-timing"))]
            last_submission: None,
            #[cfg(all(feature = "gpui-kit-engine", feature = "frame-timing"))]
            collector: gpui_kit::profiler::FrameTimingCollector::new(),
        }
    }
    fn set_phase(&mut self, phase: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.collect(window);
        self.snapshot_gpu();
        self.measuring = true;
        self.phase = phase;
        self.geometry.record(phase, "begin", window);
        self.sequence = 0;
        self.last_render = None;
        #[cfg(feature = "kael-engine")]
        if phase.starts_with("idle") {
            window.blur();
        } else {
            window.focus(&self.control.read(cx).focus_handle(cx));
        }
        #[cfg(feature = "gpui-kit-engine")]
        if phase.starts_with("idle") {
            window.blur(cx);
        } else {
            window.focus(&self.control.read(cx).focus_handle(cx), cx);
        }
        cx.notify();
        window.refresh();
    }
    fn prepare_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.geometry.record(self.phase, "end", window);
        self.collect(window);
        self.measuring = false;
        // No forced draw or cache fill: wait for the widget's existing native
        // update/paint turn, then validate its actual visible-row cache/export.
        let _ = (window, cx);
    }
    fn check_phase(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.source.borrow().clone();
        let expected = &self.datasets[self.expected_generation];
        assert_eq!(
            snapshot.dataset.generation, self.expected_generation,
            "dataset identity"
        );
        assert_eq!(snapshot.reversed, self.expected_reversed, "query direction");
        assert_eq!(
            snapshot.dataset.records.len(),
            self.options.rows,
            "exact row count"
        );
        assert!(
            Arc::ptr_eq(&snapshot.dataset.records, &expected.records),
            "all immutable model cell identities match the current expected dataset"
        );
        let actual_hash = snapshot.hash();
        assert_eq!(
            actual_hash,
            expected.hashes[usize::from(self.expected_reversed)],
            "exact ordered all-cell fingerprint"
        );
        let (selected, cells, viewport_rows, viewport_columns) =
            self.control.update(cx, |control, cx| {
                #[cfg(feature = "kael-engine")]
                {
                    let _ = cx;
                    assert_eq!(control.row_count(), self.options.rows);
                    assert_eq!(control.column_count(), COLUMNS.len());
                    assert_eq!(
                        control.pending_tile_count(),
                        0,
                        "local synchronous source settled"
                    );
                    let position = control.selection().focus;
                    let values = (0..COLUMNS.len())
                        .map(|column| {
                            control
                                .cell_value(SheetCellPosition::new(self.scroll_target.0, column))
                                .expect("native viewport row loaded for oracle")
                                .to_string()
                        })
                        .collect::<Vec<_>>();
                    (
                        (position.row, position.column),
                        values,
                        control.viewport_metrics().mounted_rows,
                        control.viewport_metrics().mounted_columns,
                    )
                }
                #[cfg(feature = "gpui-kit-engine")]
                {
                    assert_eq!(control.delegate().rows_count(cx), self.options.rows);
                    assert_eq!(control.delegate().columns_count(cx), COLUMNS.len());
                    let (_, rows) =
                        control.dump_range(self.scroll_target.0..self.scroll_target.0 + 1, cx);
                    (
                        control.selected_cell().expect("single selected cell"),
                        rows.into_iter().next().expect("selected row exported"),
                        control.visible_range().rows().len(),
                        control.visible_range().cols().len(),
                    )
                }
            });
        assert_eq!(selected, self.selected, "native control selection");
        assert_eq!(
            cells.len(),
            COLUMNS.len(),
            "actual control exports every column"
        );
        assert!(
            viewport_rows > 0 && viewport_columns > 0,
            "native virtual control completed layout"
        );
        assert!(
            viewport_rows <= 64 && viewport_columns <= COLUMNS.len(),
            "viewport stays bounded independently of logical rows"
        );
        let record_id = if self.expected_reversed {
            self.options.rows - 1 - self.scroll_target.0
        } else {
            self.scroll_target.0
        };
        for (column, actual) in cells.iter().enumerate() {
            assert_eq!(
                *actual,
                expected_cell(record_id, column, self.expected_generation),
                "actual control cell export/cache read"
            );
        }
        self.phase_checks.push((self.phase.to_owned(), true));
        self.phase_oracles.push(serde_json::json!({"phase": self.phase, "correct":true, "query_reversed":self.expected_reversed,
            "dataset_generation":self.expected_generation, "selected_cell":[self.selected.0,self.selected.1], "verified_control_row":self.scroll_target.0, "ordered_hash":actual_hash,
            "verified_model_cells": self.options.rows*COLUMNS.len(), "verified_control_cells":cells.len(),
            "native_viewport_rows":viewport_rows, "native_viewport_scrollable_columns":viewport_columns}));
    }
    fn replace_query(&mut self, generation: usize, reversed: bool, cx: &mut Context<Self>) {
        *self.source.borrow_mut() = Snapshot {
            dataset: self.datasets[generation].clone(),
            reversed,
        };
        self.expected_generation = generation;
        self.expected_reversed = reversed;
        reset_control(&self.control, cx);
        self.selected = (0, 0);
        scroll(
            &self.control,
            self.scroll_target.0,
            self.scroll_target.1,
            cx,
        );
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.collect(window);
        let started = Instant::now();
        if self.phase == "churn" && self.sequence.is_multiple_of(30) {
            self.replace_query(1 - self.expected_generation, self.expected_reversed, cx);
            self.operations[6] += 1;
        }
        let operation = self.sequence % 6;
        match operation {
            0 => {
                self.selected = (
                    (self.sequence / 6 * 7919) % self.options.rows,
                    (self.sequence / 6) % COLUMNS.len(),
                );
                select(&self.control, self.selected.0, self.selected.1, cx);
                self.scroll_target.0 = self.selected.0;
                scroll(
                    &self.control,
                    self.scroll_target.0,
                    self.scroll_target.1,
                    cx,
                );
            }
            1 => {
                self.scroll_target.0 =
                    (self.sequence / 6 * 3571 + self.options.rows / 2) % self.options.rows;
                scroll(
                    &self.control,
                    self.scroll_target.0,
                    self.scroll_target.1,
                    cx,
                );
            }
            2 => {
                self.scroll_target.1 = if self.scroll_target.1 == 1 {
                    COLUMNS.len() - 1
                } else {
                    1
                };
                scroll(
                    &self.control,
                    self.scroll_target.0,
                    self.scroll_target.1,
                    cx,
                );
            }
            3 => self.replace_query(self.expected_generation, !self.expected_reversed, cx),
            4 => {
                self.scroll_target = (0, 1);
                scroll(&self.control, 0, 1, cx);
            }
            5 => {
                self.selected = (self.options.rows - 1, COLUMNS.len() - 1);
                self.scroll_target = self.selected;
                select(&self.control, self.selected.0, self.selected.1, cx);
                scroll(&self.control, self.selected.0, self.selected.1, cx);
            }
            _ => unreachable!(),
        }
        self.operations[operation] += 1;
        self.sequence += 1;
        if self.operation_us.len() < 4096 {
            self.operation_us.push(started.elapsed().as_micros() as u64);
        }
        cx.notify();
        window.refresh();
    }
    fn collect(&mut self, window: &mut Window) {
        #[cfg(not(feature = "frame-timing"))]
        let _ = window;
        #[cfg(all(feature = "kael-engine", feature = "frame-timing"))]
        {
            for frame in window.frame_timeline().iter() {
                if self.last_draw.is_none_or(|last| frame.frame_number > last) {
                    if self.measuring && self.draw_us.len() < 4096 {
                        self.draw_us.push(frame.duration_us);
                    }
                    self.last_draw = Some(frame.frame_number);
                }
            }
            for frame in window.frame_submissions() {
                if self
                    .last_submission
                    .is_none_or(|last| frame.frame_number > last)
                {
                    if self.measuring && self.submission_us.len() < 4096 {
                        self.submission_us.push(frame.duration_us);
                    }
                    if self.measuring {
                        self.first_submission_us.get_or_insert_with(|| {
                            self.started
                                .elapsed()
                                .saturating_sub(frame.submitted_at.elapsed())
                                .as_micros()
                        });
                    }
                    self.last_submission = Some(frame.frame_number);
                }
            }
        }
        #[cfg(all(feature = "gpui-kit-engine", feature = "frame-timing"))]
        {
            let _ = window;
            for event in self.collector.collect_unseen() {
                match event {
                    gpui_kit::profiler::FrameEvent::Draw(frame) => {
                        if self.measuring && self.draw_us.len() < 4096 {
                            self.draw_us.push(frame.draw_duration().as_micros() as u64);
                        }
                    }
                    gpui_kit::profiler::FrameEvent::Present(frame) => {
                        if self.measuring && self.submission_us.len() < 4096 {
                            self.submission_us
                                .push(frame.present_duration().as_micros() as u64);
                        }
                        if self.measuring {
                            self.first_submission_us.get_or_insert_with(|| {
                                self.started
                                    .elapsed()
                                    .saturating_sub(frame.present_end.elapsed())
                                    .as_micros()
                            });
                        }
                    }
                }
            }
        }
    }

    fn snapshot_gpu(&mut self) {
        #[cfg(target_os = "macos")]
        let bytes = metal::Device::system_default().map(|device| device.current_allocated_size());
        #[cfg(not(target_os = "macos"))]
        let bytes: Option<u64> = None;
        self.gpu_samples.push((self.phase.to_string(), bytes));
    }

    fn report(&self) {
        println!(
            "KAEL_COMPARISON {}",
            serde_json::json!({
                "contract": CONTRACT, "engine":engine(), "quick":self.options.quick, "frame_timing_enabled":cfg!(feature="frame-timing"), "validation_phase_markers":true,
                "rows":self.options.rows, "columns":COLUMNS, "column_count":COLUMNS.len(), "fixture_hash":self.fixture_hash,
                "fixture_bytes":self.datasets[0].bytes, "fixture_retained_datasets":2, "final_hash":self.source.borrow().hash(),
                "phase_correctness":self.phase_checks, "phase_oracles":self.phase_oracles, "font_family":FONT_FAMILY, "font_size_px":FONT_SIZE,
                "header_font_size_px":HEADER_FONT_SIZE, "line_height_px":LINE_HEIGHT, "row_height_px":ROW_HEIGHT, "column_width_px":COLUMN_WIDTH,
                "window_width_px":1100, "window_height_px":760, "table_width_px":TABLE_WIDTH, "table_height_px":704,
                "fixed_columns":1, "header_height_px":32,
            "leaf_header_height_px":if cfg!(feature="kael-engine") {32} else {28},
                "component":if cfg!(feature="kael-engine") {"VirtualSheetGrid"} else {"DataTable"},
                "theme_mode":"dark", "native_cell_editing":false, "cell_wrap":"nowrap",
                "operations":{"selection_reveal":self.operations[0], "vertical_scroll":self.operations[1],
                    "horizontal_scroll":self.operations[2], "query_reverse":self.operations[3], "home":self.operations[4],
                    "end_selection":self.operations[5], "replace_model":self.operations[6]},
                "operation_cpu_us":self.operation_us, "first_render_us":self.first_render_us,
                "render_callback_intervals_us":self.active_us, "renders":self.renders, "elapsed_us":self.started.elapsed().as_micros(),
                "first_submission_us":self.first_submission_us, "draw_cpu_us":self.draw_us, "submission_cpu_us":self.submission_us,
                "native_window_geometry": self.geometry.samples,
                "gpu_allocated_bytes":self.gpu_samples,
                "submission_scope":"CPU platform submission, excluding GPU completion/compositor display",
                "timing_scope":"shipped native virtual data controls; public selection/navigation/model invalidation APIs; paced separate frames",
            "query_scope":"application-owned ascending/descending record-ID mapping with native control refresh/reset",
                "oracle_scope":"every ordered fixture cell plus current query, selection, and eight actual control values; validation frames excluded",
                "model_replacement_scope":"alternate two prebuilt immutable datasets; application snapshot replacement, not allocation stress",
                "differences":["Kael bounded local tile cache16x512cells; GPUI Kit delegate reads immutable dataset directly",
                    "Both header wrappers32px and body rows28px; GPUI Kit native leaf header cells28px",
                    "Kael grid provides native editing; GPUI Kit table delegates editing to apps, so editing excluded",
                    "Navigation targets both horizontal extremes; controls retain their native reveal/clamp semantics"]
            })
        );
    }
}
fn engine() -> &'static str {
    #[cfg(feature = "kael-engine")]
    {
        "kael"
    }
    #[cfg(feature = "gpui-kit-engine")]
    {
        "gpui-kit"
    }
}
impl Render for Workload {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if self.measuring {
            self.renders += 1;
            self.first_render_us
                .get_or_insert_with(|| self.started.elapsed().as_micros());
            if self.phase == "active" || self.phase == "churn" {
                let now = Instant::now();
                if let Some(previous) = self.last_render.replace(now)
                    && self.active_us.len() < 4096
                {
                    self.active_us
                        .push(now.duration_since(previous).as_micros());
                }
            } else {
                self.last_render = None;
            }
        }
        div().size_full().flex().flex_col().bg(rgb(0x11151b)).text_color(rgb(0xe2e6ed)).font_family(FONT_FAMILY).text_size(px(FONT_SIZE)).line_height(px(LINE_HEIGHT)).whitespace_nowrap()
            .child(div().h(px(56.0)).flex_shrink_0().px_4().flex().items_center().justify_between()
                .child("Unicode records · fixed Title column").child(format!("{} · {} rows · {}",engine(),self.options.rows,self.phase)))
            .child(div().flex_1().min_h(px(0.0)).w_full().flex()
                .child(div().w(px(TABLE_WIDTH)).h_full().flex_shrink_0().overflow_hidden().child(control_element(&self.control)))
                .child(div().flex_1().p_4().child(format!("{} logical records\n8 Unicode columns\nSelection · two-axis navigation\nQuery reversal · snapshot replacement",self.options.rows))))
    }
}
fn launch(started: Instant, options: Options, cx: &mut App) {
    #[cfg(feature = "kael-engine")]
    {
        kael_ui::init(cx);
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::astryx_neutral_dark());
    }
    #[cfg(feature = "gpui-kit-engine")]
    {
        gpui_kit::init(cx);
        gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
    }
    let window=cx.open_window(WindowOptions {
        window_bounds:Some(WindowBounds::Windowed(Bounds::centered(None,size(px(1100.0),px(760.0)),cx))),..Default::default()
    },|window,cx| {
        let view=cx.new(|cx|Workload::new(started,options,window,cx));let weak=view.downgrade();
        window.spawn(cx,async move |cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            for phase in ["idle-before","active","idle-after","churn"] {
                weak.update_in(cx,|view,window,cx|view.set_phase(phase,window,cx)).unwrap();
                println!("KAEL_PHASE {}",serde_json::json!({"phase":phase,"elapsed_us":started.elapsed().as_micros()}));
                if phase.starts_with("idle") {cx.background_executor().timer(options.duration(phase)).await;}
                else {
                    let until=Instant::now()+options.duration(phase);
                    while Instant::now()<until {
                        cx.background_executor().timer(Duration::from_micros(16_667)).await;
                        weak.update_in(cx,|view,window,cx|view.tick(window,cx)).unwrap();
                    }
                }
                println!("KAEL_PHASE {}",serde_json::json!({"phase":"validation","elapsed_us":started.elapsed().as_micros()}));
                weak.update_in(cx,|view,window,cx|view.prepare_check(window,cx)).unwrap();
                cx.background_executor().timer(Duration::from_millis(40)).await;
                weak.update_in(cx,|view,_,cx|view.check_phase(cx)).unwrap();
            }
            println!("KAEL_PHASE {}",serde_json::json!({"phase":"finished","elapsed_us":started.elapsed().as_micros()}));
            weak.update_in(cx,|view,window,_| {view.collect(window);view.snapshot_gpu();view.report();}).unwrap();
            cx.update(|_,cx|cx.quit()).unwrap();
        }).detach();view
    });
    if let Err(error) = window {
        eprintln!("data comparison window failed: {error}");
        cx.quit();
    }
    cx.activate(true);
}
fn main() {
    let options = Options::parse();
    let started = Instant::now();
    #[cfg(feature = "kael-engine")]
    Application::try_new()
        .expect("native Kael platform")
        .run(move |cx| launch(started, options, cx));
    #[cfg(feature = "gpui-kit-engine")]
    {
        #[cfg(feature = "frame-timing")]
        gpui_kit::profiler::set_trace_enabled(true);
        gpui_kit::application().run(move |cx| launch(started, options, cx));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn snapshots_preserve_exact_unicode_cells_and_reverse_identity() {
        let first = Dataset::new(100, 0);
        let second = Dataset::new(100, 1);
        assert_ne!(first.hashes[0], first.hashes[1]);
        assert_ne!(first.hashes[0], second.hashes[0]);
        for reversed in [false, true] {
            let snapshot = Snapshot {
                dataset: first.clone(),
                reversed,
            };
            for row in 0..100 {
                for column in 0..8 {
                    assert_eq!(
                        snapshot.cell(row, column).as_ref(),
                        expected_cell(if reversed { 99 - row } else { row }, column, 0)
                    );
                }
            }
            assert_eq!(snapshot.hash(), first.hashes[usize::from(reversed)]);
        }
    }
    #[::core::prelude::v1::test]
    fn production_fixture_has_stable_full_cell_fingerprint() {
        let data = Dataset::new(100_000, 0);
        assert_eq!(data.bytes, 12_166_666);
        assert_eq!(data.hashes[0], "fnv1a64:4ea6da623acce6b5");
        assert_eq!(data.records.len(), 100_000);
    }
    #[::core::prelude::v1::test]
    fn two_axis_contract_has_real_overflow_and_phase_durations() {
        assert!(COLUMN_WIDTH * COLUMNS.len() as f32 > TABLE_WIDTH);
        assert_eq!(
            Options {
                rows: 100_000,
                quick: false
            }
            .duration("idle-before"),
            Duration::from_secs(5)
        );
        assert_eq!(
            Options {
                rows: 100_000,
                quick: false
            }
            .duration("active"),
            Duration::from_secs(12)
        );
        assert_eq!(
            Options {
                rows: 100_000,
                quick: true
            }
            .duration("churn"),
            Duration::from_secs(1)
        );
    }
}

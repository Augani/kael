//! One-source native/WebAssembly office-suite scale release workload.

use std::rc::Rc;

use kael::{AccessibilityAttributes, AccessibilityRole, ImageFormat, PointerInputEvent, SceneRect};
use kael_ui::prelude::*;
use kael_ui::suite_workloads::{
    DocumentWorkload, REFERENCE_DOCUMENT_BLOCKS, REFERENCE_SHEET_COLUMNS, REFERENCE_SHEET_ROWS,
    REFERENCE_SLIDES, REFERENCE_WHITEBOARD_SHAPES, SheetWorkload, SlideDeckWorkload,
    SuiteWorkloadProbeReport, WhiteboardWorkload, run_suite_workload_probe,
};
use kael_ui::virtual_list::vlist_uniform;

const SHEET_CACHE_TILES: usize = 8;

#[cfg(target_arch = "wasm32")]
fn publish_attributes(attributes: &[(&str, String)]) {
    let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    else {
        return;
    };
    for (name, value) in attributes {
        let _ = root.set_attribute(name, value);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish_attributes(_attributes: &[(&str, String)]) {}

fn publish_probe(report: &SuiteWorkloadProbeReport) {
    publish_attributes(&[
        (
            "data-kael-suite-workloads",
            if report.passed() { "passed" } else { "failed" }.to_string(),
        ),
        ("data-kael-suite-sheet-rows", report.sheet_rows.to_string()),
        (
            "data-kael-suite-sheet-columns",
            report.sheet_columns.to_string(),
        ),
        (
            "data-kael-suite-sheet-mounted-cells",
            report.sheet_mounted_cells.to_string(),
        ),
        (
            "data-kael-suite-document-blocks",
            report.document_blocks.to_string(),
        ),
        (
            "data-kael-suite-document-mounted-blocks",
            report.document_mounted_blocks.to_string(),
        ),
        ("data-kael-suite-slides", report.slides.to_string()),
        (
            "data-kael-suite-whiteboard-shapes",
            report.whiteboard_shapes.to_string(),
        ),
        (
            "data-kael-suite-whiteboard-visible-shapes",
            report.whiteboard_visible_shapes.to_string(),
        ),
        (
            "data-kael-suite-whiteboard-candidates",
            report.whiteboard_spatial_candidates.to_string(),
        ),
        (
            "data-kael-suite-whiteboard-tile-bytes",
            report.whiteboard_tile_bytes.to_string(),
        ),
        (
            "data-kael-suite-frame-updates",
            report.bounded_frame_updates.to_string(),
        ),
        (
            "data-kael-suite-whiteboard-build-ms",
            report.whiteboard_build_millis.to_string(),
        ),
        (
            "data-kael-suite-whiteboard-query-us",
            report.whiteboard_query_micros.to_string(),
        ),
    ]);
}

fn publish_sheet_cache(sheet: &VirtualSheetGrid) {
    let cached_tiles = sheet.cached_tile_count();
    publish_attributes(&[
        (
            "data-kael-suite-sheet-cache",
            if cached_tiles > 0 && cached_tiles <= SHEET_CACHE_TILES {
                "passed"
            } else {
                "pending"
            }
            .to_string(),
        ),
        (
            "data-kael-suite-sheet-cached-pages",
            cached_tiles.to_string(),
        ),
        (
            "data-kael-suite-sheet-max-pages",
            SHEET_CACHE_TILES.to_string(),
        ),
        (
            "data-kael-suite-sheet-cached-rows",
            cached_tiles
                .saturating_mul(VIRTUAL_SHEET_MAX_TILE_CELLS)
                .to_string(),
        ),
        (
            "data-kael-suite-sheet-render-columns",
            REFERENCE_SHEET_COLUMNS.to_string(),
        ),
    ]);
}

fn publish_sheet_selection(sheet: &VirtualSheetGrid) {
    let selection = sheet.selection();
    let all_selected = selection.anchor == SheetCellPosition::new(0, 0)
        && selection.focus
            == SheetCellPosition::new(REFERENCE_SHEET_ROWS - 1, REFERENCE_SHEET_COLUMNS - 1);
    publish_attributes(&[
        (
            "data-kael-suite-sheet-selection",
            if all_selected { "passed" } else { "failed" }.to_string(),
        ),
        (
            "data-kael-suite-sheet-selection-representation",
            "anchor_focus".to_string(),
        ),
        ("data-kael-suite-sheet-selection-stored", "2".to_string()),
        (
            "data-kael-suite-sheet-selection-count",
            (REFERENCE_SHEET_ROWS as u64 * REFERENCE_SHEET_COLUMNS as u64).to_string(),
        ),
    ]);
}

fn publish_sheet_viewport(sheet: &VirtualSheetGrid) {
    let metrics = sheet.viewport_metrics();
    publish_attributes(&[
        (
            "data-kael-suite-sheet-grid",
            if sheet.row_count() == REFERENCE_SHEET_ROWS
                && sheet.column_count() == REFERENCE_SHEET_COLUMNS
            {
                "passed"
            } else {
                "failed"
            }
            .to_string(),
        ),
        (
            "data-kael-suite-sheet-virtual",
            if metrics.mounted_cells > 0 && metrics.mounted_cells <= 2_048 {
                "passed"
            } else {
                "pending"
            }
            .to_string(),
        ),
        (
            "data-kael-suite-sheet-mounted-rows",
            metrics.mounted_rows.to_string(),
        ),
        (
            "data-kael-suite-sheet-mounted-columns",
            metrics.mounted_columns.to_string(),
        ),
        (
            "data-kael-suite-sheet-mounted-cells",
            metrics.mounted_cells.to_string(),
        ),
        (
            "data-kael-suite-sheet-accessibility",
            if sheet.row_count() == 1_000_000 && sheet.column_count() == 16_384 {
                "passed"
            } else {
                "failed"
            }
            .to_string(),
        ),
    ]);
}

struct SuiteScaleShowcase {
    sheet: Entity<VirtualSheetGrid>,
    document: DocumentWorkload,
    deck: SlideDeckWorkload,
    whiteboard: WhiteboardWorkload,
    sheet_select_all_requested: bool,
    frame_export_requested: bool,
    last_document_mount: usize,
    last_thumbnail_mount: usize,
    pointer_events: u64,
}

struct SuiteSecondaryWindow;

impl Render for SuiteSecondaryWindow {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        publish_attributes(&[
            ("data-kael-suite-secondary-rendered", "true".to_string()),
            (
                "data-kael-suite-secondary-width",
                f32::from(window.viewport_size().width).round().to_string(),
            ),
            (
                "data-kael-suite-secondary-height",
                f32::from(window.viewport_size().height).round().to_string(),
            ),
        ]);
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.0))
            .bg(rgb(0x1f2940))
            .text_color(rgb(0xf4f7ff))
            .child(
                div()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Independent retained window"),
            )
            .child(
                Button::new("close-suite-parity-window", "Close parity window")
                    .on_click(|_, window, _| window.close_window()),
            )
    }
}

impl SuiteScaleShowcase {
    fn new(cx: &mut Context<Self>) -> Self {
        let report = run_suite_workload_probe();
        publish_probe(&report);

        let sheet_model = Rc::new(SheetWorkload::reference());
        let fetch_model = sheet_model.clone();
        let sheet = cx.new(|cx| {
            VirtualSheetGrid::new(REFERENCE_SHEET_ROWS, REFERENCE_SHEET_COLUMNS, cx)
                .expect("reference sheet dimensions are supported")
                .with_cache_limits(SHEET_CACHE_TILES, 16)
                .with_frozen_panes(1, 1)
                .expect("reference frozen panes are supported")
                .on_fetch_tile(move |request, entity, _, cx| {
                    let mut values = Vec::with_capacity(request.cell_count().unwrap_or_default());
                    for row in request.rows.clone() {
                        for column in request.columns.clone() {
                            values.push(
                                fetch_model
                                    .cell_value(row, column)
                                    .unwrap_or_default()
                                    .to_string()
                                    .into(),
                            );
                        }
                    }
                    cx.defer(move |app| {
                        entity.update(app, |sheet, cx| {
                            let _ = sheet.provide_tile(request, values);
                            publish_sheet_cache(sheet);
                            publish_sheet_viewport(sheet);
                            cx.notify();
                        });
                    });
                })
                .on_commit_edit(|_, _, _| {
                    publish_attributes(&[(
                        "data-kael-suite-sheet-edit-callback",
                        "passed".to_string(),
                    )]);
                })
        });

        let mut document = DocumentWorkload::reference();
        let _ = document.edit_block(42, "Kael portable document edit with bounded undo");
        let _ = document.undo();
        let _ = document.redo();
        let mut deck = SlideDeckWorkload::reference();
        let _ = deck.select_slide(9_999);

        Self {
            sheet,
            document,
            deck,
            whiteboard: WhiteboardWorkload::reference(),
            sheet_select_all_requested: false,
            frame_export_requested: false,
            last_document_mount: 0,
            last_thumbnail_mount: 0,
            pointer_events: 0,
        }
    }

    fn metric(metric_label: &'static str, value: String, theme: &Theme) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(3.0))
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(theme.tokens.muted_foreground)
                    .child(metric_label),
            )
            .child(
                div()
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(value),
            )
    }

    fn handle_pointer_input(&mut self, event: &PointerInputEvent) {
        self.whiteboard.handle_pointer(event);
        self.pointer_events = self.pointer_events.saturating_add(1);
        let passed = self.whiteboard.completed_stroke_count() > 0;
        publish_attributes(&[
            (
                "data-kael-suite-pointer",
                if passed { "passed" } else { "active" }.to_string(),
            ),
            (
                "data-kael-suite-pointer-events",
                self.pointer_events.to_string(),
            ),
            (
                "data-kael-suite-active-pointers",
                self.whiteboard.active_pointer_count().to_string(),
            ),
        ]);
    }

    fn card(
        title: &'static str,
        subtitle: String,
        content: impl IntoElement,
        theme: &Theme,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .overflow_hidden()
            .rounded(px(14.0))
            .border_1()
            .border_color(theme.tokens.border)
            .bg(theme.tokens.card)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .py(px(10.0))
                    .border_b_1()
                    .border_color(theme.tokens.border)
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.tokens.muted_foreground)
                            .child(subtitle),
                    ),
            )
            .child(content)
    }
}

impl Render for SuiteScaleShowcase {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let compact = window.viewport_size().width < px(900.0);

        if !self.sheet_select_all_requested {
            let sheet = self.sheet.clone();
            window.defer(cx, move |window, app| {
                sheet.update(app, |sheet, cx| {
                    let edit_ok = sheet
                        .set_cell_value(SheetCellPosition::new(0, 0), "Kael suite edit", window, cx)
                        .is_ok();
                    let _ = sheet.select(SheetCellPosition::new(0, 0), false);
                    let copy_ok = sheet
                        .copy_selection_to_clipboard(cx)
                        .is_ok_and(|cells| cells == 1);
                    let paste_ok = sheet
                        .paste_tsv("Kael suite paste\t42", window, cx)
                        .is_ok_and(|range| range.cell_count() == Some(2));
                    let undo_ok = sheet.undo(window, cx);
                    let redo_ok = sheet.redo(window, cx);
                    let _ = sheet.select(SheetCellPosition::new(0, 0), false);
                    let _ = sheet.select(
                        SheetCellPosition::new(
                            REFERENCE_SHEET_ROWS - 1,
                            REFERENCE_SHEET_COLUMNS - 1,
                        ),
                        true,
                    );
                    publish_attributes(&[
                        (
                            "data-kael-suite-sheet-edit",
                            if edit_ok && undo_ok && redo_ok {
                                "passed"
                            } else {
                                "failed"
                            }
                            .to_string(),
                        ),
                        (
                            "data-kael-suite-sheet-copy",
                            if copy_ok { "passed" } else { "failed" }.to_string(),
                        ),
                        (
                            "data-kael-suite-sheet-paste",
                            if paste_ok { "passed" } else { "failed" }.to_string(),
                        ),
                    ]);
                    publish_sheet_selection(sheet);
                    cx.notify();
                });
            });
            self.sheet_select_all_requested = true;
        }
        publish_sheet_viewport(self.sheet.read(cx));
        if !self.frame_export_requested {
            window.on_next_frame(|window, _| {
                // Frame callbacks run before that frame's platform draw. The
                // second callback captures the first fully presented scene.
                window.on_next_frame(|window, _| {
                    let export = window.export_frame_png();
                    let (passed, byte_len, error) = match export {
                        Ok(image) => {
                            let bytes = image.bytes();
                            let passed = image.format() == ImageFormat::Png
                                && bytes.len() > 1_024
                                && bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]);
                            (passed, bytes.len(), String::new())
                        }
                        Err(error) => (false, 0, error.to_string()),
                    };
                    publish_attributes(&[
                        (
                            "data-kael-suite-frame-export",
                            if passed { "passed" } else { "failed" }.to_string(),
                        ),
                        ("data-kael-suite-frame-export-bytes", byte_len.to_string()),
                        ("data-kael-suite-frame-export-error", error),
                    ]);
                });
            });
            self.frame_export_requested = true;
        }

        let document_entity = cx.weak_entity();
        let document = self.document.clone();
        let document_pages = document.page_count();
        let document_list = vlist_uniform(
            "suite-document-pages",
            document_pages,
            px(214.0),
            move |range, _, _| {
                range
                    .map(|page| {
                        let first_block = page * 48;
                        div()
                            .h(px(206.0))
                            .mx(px(12.0))
                            .my(px(4.0))
                            .p(px(14.0))
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(rgb(0xd9dce5))
                            .bg(rgb(0xffffff))
                            .text_color(rgb(0x20242e))
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(rgb(0x777b87))
                                    .child(format!("Page {}", page + 1)),
                            )
                            .children((0..6).map(|offset| {
                                let block = first_block + offset;
                                div()
                                    .mt(px(7.0))
                                    .h(px(13.0))
                                    .overflow_hidden()
                                    .text_size(px(10.0))
                                    .child(
                                        document
                                            .block_text(block)
                                            .unwrap_or_else(|| "End of document".to_string()),
                                    )
                            }))
                    })
                    .collect::<Vec<_>>()
            },
        )
        .overscan(2)
        .on_visible_range(move |range, _, app| {
            let mounted = range.len();
            publish_attributes(&[
                (
                    "data-kael-suite-document-virtual",
                    if mounted > 0 && mounted <= 8 {
                        "passed"
                    } else {
                        "failed"
                    }
                    .to_string(),
                ),
                (
                    "data-kael-suite-document-mounted-pages",
                    mounted.to_string(),
                ),
            ]);
            let _ = document_entity.update(app, |this, cx| {
                if this.last_document_mount != mounted {
                    this.last_document_mount = mounted;
                    cx.notify();
                }
            });
        })
        .h(px(320.0));

        let thumbnail_entity = cx.weak_entity();
        let thumbnail_list = vlist_uniform(
            "suite-slide-thumbnails",
            self.deck.slide_count(),
            px(82.0),
            move |range, _, _| {
                range
                    .map(|slide| {
                        div()
                            .h(px(74.0))
                            .mx(px(8.0))
                            .my(px(4.0))
                            .p(px(7.0))
                            .rounded(px(7.0))
                            .border_1()
                            .border_color(rgb(0x383d4d))
                            .bg(if slide % 2 == 0 {
                                rgb(0x202638)
                            } else {
                                rgb(0x252c41)
                            })
                            .text_size(px(10.0))
                            .child(format!("Slide {}", slide + 1))
                    })
                    .collect::<Vec<_>>()
            },
        )
        .overscan(3)
        .on_visible_range(move |range, _, app| {
            let mounted = range.len();
            publish_attributes(&[
                (
                    "data-kael-suite-slides-virtual",
                    if mounted > 0 && mounted <= 16 {
                        "passed"
                    } else {
                        "failed"
                    }
                    .to_string(),
                ),
                ("data-kael-suite-slides-mounted", mounted.to_string()),
            ]);
            let _ = thumbnail_entity.update(app, |this, cx| {
                if this.last_thumbnail_mount != mounted {
                    this.last_thumbnail_mount = mounted;
                    cx.notify();
                }
            });
        })
        .h(px(280.0));

        let surface = self.deck.surface().clone();
        let slides_content = div()
            .flex()
            .h(px(320.0))
            .child(
                div()
                    .w(px(150.0))
                    .border_r_1()
                    .border_color(theme.tokens.border)
                    .child(thumbnail_list),
            )
            .child(
                div()
                    .flex_1()
                    .m(px(18.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(theme.tokens.border)
                    .bg(rgb(0xf7f7fb))
                    .text_color(rgb(0x20242e))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(20.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("Slide {}", surface.slide_index() + 1)),
                    )
                    .child(div().text_size(px(10.0)).child(format!(
                        "retained surface rev {} · {} nodes",
                        surface.revision(),
                        surface.retained_node_count()
                    ))),
            );

        let viewport = SceneRect::new(0.0, 0.0, 640.0, 300.0);
        let visible_indices = self.whiteboard.visible_shape_indices(viewport);
        let candidate_count = self.whiteboard.last_spatial_candidate_count();
        let visible_shapes = visible_indices
            .iter()
            .filter_map(|index| self.whiteboard.shape(*index))
            .map(|shape| (shape.id, shape.bounds))
            .collect::<Vec<_>>();
        publish_attributes(&[
            (
                "data-kael-suite-whiteboard-render",
                if !visible_shapes.is_empty()
                    && visible_shapes.len() <= 512
                    && candidate_count <= 1_024
                {
                    "passed"
                } else {
                    "failed"
                }
                .to_string(),
            ),
            (
                "data-kael-suite-whiteboard-rendered",
                visible_shapes.len().to_string(),
            ),
            (
                "data-kael-suite-whiteboard-render-candidates",
                candidate_count.to_string(),
            ),
        ]);
        let primary = theme.tokens.primary;
        let accent = theme.tokens.accent;
        let canvas_view = canvas(size(px(640.0), px(300.0)), move |draw, _, _| {
            draw.reserve_commands(visible_shapes.len());
            draw.fill_rects(visible_shapes.into_iter().map(|(id, shape)| {
                let bounds = Bounds::new(
                    point(px(shape.x as f32), px(shape.y as f32)),
                    size(px(shape.width as f32), px(shape.height as f32)),
                );
                let fill = if id % 3 == 0 { primary } else { accent };
                (bounds, fill.into())
            }));
        });
        let pointer_surface = div()
            .id("suite-whiteboard-pointer-surface")
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label("Suite whiteboard rich pointer surface"),
            )
            .relative()
            .h(px(300.0))
            .overflow_hidden()
            .on_pointer_event(cx.listener(|this, event: &PointerInputEvent, _, cx| {
                this.handle_pointer_input(event);
                cx.notify();
            }))
            .child(canvas_view);

        let sheet_card = Self::card(
            "Sheets",
            format!(
                "{} rows × {} columns · 2-axis virtual",
                REFERENCE_SHEET_ROWS, REFERENCE_SHEET_COLUMNS
            ),
            div()
                .h(px(410.0))
                .overflow_hidden()
                .child(self.sheet.clone()),
            &theme,
        );
        let document_card = Self::card(
            "Docs",
            format!(
                "{} blocks · {} pages · sparse edit/search/undo",
                REFERENCE_DOCUMENT_BLOCKS,
                self.document.page_count()
            ),
            document_list,
            &theme,
        );
        let slides_card = Self::card(
            "Slides",
            format!("{} slides · virtual thumbnails", REFERENCE_SLIDES),
            slides_content,
            &theme,
        );
        let whiteboard_card = Self::card(
            "Whiteboard",
            format!(
                "{} shapes · {} visible · {} candidates",
                REFERENCE_WHITEBOARD_SHAPES,
                visible_indices.len(),
                candidate_count
            ),
            pointer_surface,
            &theme,
        );
        let suite_cards = if compact {
            div()
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(sheet_card)
                .child(document_card)
                .child(slides_card)
                .child(whiteboard_card)
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(
                    div()
                        .flex()
                        .items_stretch()
                        .gap(px(14.0))
                        .child(div().flex_1().min_w_0().child(sheet_card))
                        .child(div().flex_1().min_w_0().child(document_card)),
                )
                .child(
                    div()
                        .flex()
                        .items_stretch()
                        .gap(px(14.0))
                        .child(div().flex_1().min_w_0().child(slides_card))
                        .child(div().flex_1().min_w_0().child(whiteboard_card)),
                )
        };

        let suite_header_title = div()
            .w_full()
            .child(
                div()
                    .text_size(px(23.0))
                    .font_weight(FontWeight::BOLD)
                    .child("Kael suite-scale parity lab"),
            )
            .child(
                div()
                    .mt(px(4.0))
                    .text_size(px(11.0))
                    .text_color(theme.tokens.muted_foreground)
                    .child(
                        "The same Rust source and retained primitives run natively and in WebAssembly.",
                    ),
            );
        let suite_header_metrics = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(if compact { px(12.0) } else { px(18.0) })
            .child(
                div()
                    .id("suite-pointer-ci-target")
                    .accessibility(
                        AccessibilityAttributes::new(AccessibilityRole::Group)
                            .label("Suite rich pointer CI target"),
                    )
                    .w(px(42.0))
                    .h(px(32.0))
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(theme.tokens.border)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(9.0))
                    .child("PEN"),
            )
            .child(Self::metric(
                "DOCUMENT MOUNT",
                self.last_document_mount.to_string(),
                &theme,
            ))
            .child(Self::metric(
                "THUMBNAIL MOUNT",
                self.last_thumbnail_mount.to_string(),
                &theme,
            ));
        let suite_header = div()
            .flex()
            .gap(px(16.0))
            .mb(px(14.0))
            .child(suite_header_title)
            .child(suite_header_metrics);
        let suite_header = if compact {
            suite_header.flex_col().items_start()
        } else {
            suite_header.items_end().justify_between()
        };

        div()
            .id("suite-scale-root")
            .size_full()
            .overflow_y_scroll()
            .on_pointer_event(|event: &PointerInputEvent, _, _| {
                publish_attributes(&[(
                    "data-kael-suite-pointer-dispatch",
                    format!("{:?}", event.phase).to_ascii_lowercase(),
                )]);
            })
            .on_mouse_down(MouseButton::Left, |_, _, _| {
                publish_attributes(&[(
                    "data-kael-suite-legacy-mouse-dispatch",
                    "passed".to_string(),
                )]);
            })
            .on_pointer_event(cx.listener(|this, event: &PointerInputEvent, _, cx| {
                this.handle_pointer_input(event);
                cx.notify();
            }))
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .p(px(16.0))
            .child(suite_header)
            .child(suite_cards)
    }
}

fn main() {
    let application = Application::try_new().expect("failed to initialize Kael");
    let mut route_event_count = 0_u64;
    application.on_open_urls(move |urls| {
        route_event_count = route_event_count.saturating_add(1);
        let last_url = urls.last().cloned().unwrap_or_default();
        publish_attributes(&[
            (
                "data-kael-suite-route-event-count",
                route_event_count.to_string(),
            ),
            ("data-kael-suite-route-batch-size", urls.len().to_string()),
            ("data-kael-suite-route-url", last_url),
        ]);
    });
    let mut reopen_count = 0_u64;
    application.on_reopen(move |_| {
        reopen_count = reopen_count.saturating_add(1);
        publish_attributes(&[("data-kael-suite-reopen-count", reopen_count.to_string())]);
    });
    publish_attributes(&[
        ("data-kael-suite-route-hook", "registered".to_string()),
        ("data-kael-suite-reopen-hook", "registered".to_string()),
    ]);

    application.run(|cx| {
        kael_ui::init(cx);
        install_theme(cx, Theme::tokyo_night());
        let primary = cx
            .open_window(
                WindowOptions {
                    titlebar: None,
                    ..Default::default()
                },
                |_, cx| cx.new(SuiteScaleShowcase::new),
            )
            .expect("failed to open suite-scale window");
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(24.0), px(24.0)),
                    size(px(320.0), px(190.0)),
                ))),
                titlebar: None,
                focus: true,
                parent: Some(primary.into()),
                ..Default::default()
            },
            |_, cx| cx.new(|_| SuiteSecondaryWindow),
        )
        .expect("failed to open suite-scale secondary window");
    });
}

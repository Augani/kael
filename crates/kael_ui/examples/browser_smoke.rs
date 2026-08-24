//! End-to-end Kael browser application and million-row virtualization proof.

use kael::{
    AccessibilityAttributes, AccessibilityRole, ExternalDropData, ExternalFile, Image, ImageFormat,
    PathPromptOptions, PrintJob, PrintTextStyle, SaveDialogBuilder, ScrollStrategy,
    UniformListScrollHandle, uniform_list, webview_html,
};
use kael_ui::components::{input::Input, input_state::InputState};
use kael_ui::prelude::*;
use std::sync::Arc;
use web_time::Instant;

const LOGICAL_ROW_COUNT: usize = 1_000_000;
const MAX_VIEWPORT_MOUNTED_ROWS: usize = 64;
const ROW_HEIGHT: f32 = 34.0;
#[cfg(target_arch = "wasm32")]
const IME_PROBE_TEXT: &str = "Kael 日本語 🧠!";
#[cfg(target_arch = "wasm32")]
const CLIPBOARD_PROBE_TEXT: &str = "Kael 日本語 🧠! clipboard";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VirtualizationEvidence {
    range_start: usize,
    range_end: usize,
    materialize_micros: u128,
}

impl VirtualizationEvidence {
    fn mounted_rows(self) -> usize {
        self.range_end.saturating_sub(self.range_start)
    }

    fn is_viewport_bounded(self) -> bool {
        self.mounted_rows() > 1
            && self.mounted_rows() <= MAX_VIEWPORT_MOUNTED_ROWS
            && self.range_end <= LOGICAL_ROW_COUNT
    }
}

#[cfg(target_arch = "wasm32")]
fn publish_virtualization_evidence(evidence: VirtualizationEvidence, jump_verified: bool) {
    let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    else {
        return;
    };

    let status = if evidence.is_viewport_bounded() {
        "verified"
    } else {
        "failed"
    };
    let attributes = [
        ("data-kael-virtual-table", status.to_string()),
        (
            "data-kael-virtual-logical-rows",
            LOGICAL_ROW_COUNT.to_string(),
        ),
        (
            "data-kael-virtual-mounted-rows",
            evidence.mounted_rows().to_string(),
        ),
        (
            "data-kael-virtual-range-start",
            evidence.range_start.to_string(),
        ),
        (
            "data-kael-virtual-range-end-exclusive",
            evidence.range_end.to_string(),
        ),
        (
            "data-kael-virtual-mount-bound",
            MAX_VIEWPORT_MOUNTED_ROWS.to_string(),
        ),
        (
            "data-kael-virtual-materialize-us",
            evidence.materialize_micros.to_string(),
        ),
        (
            "data-kael-virtual-jump",
            if jump_verified {
                "last-row-visible"
            } else {
                "pending"
            }
            .to_string(),
        ),
        ("data-kael-virtual-scrollbar", "always-visible".to_string()),
    ];
    for (name, value) in attributes {
        let _ = root.set_attribute(name, &value);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish_virtualization_evidence(_evidence: VirtualizationEvidence, _jump_verified: bool) {}

#[cfg(target_arch = "wasm32")]
fn publish_gpu_evidence(window: &Window) {
    let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    else {
        return;
    };
    let Some(gpu) = window.gpu_specs() else {
        let _ = root.set_attribute("data-kael-gpu-probe", "unavailable");
        return;
    };
    let attributes = [
        ("data-kael-gpu-probe", "reported"),
        (
            "data-kael-gpu-software-emulated",
            if gpu.is_software_emulated {
                "true"
            } else {
                "false"
            },
        ),
        ("data-kael-gpu-device", gpu.device_name.as_str()),
        ("data-kael-gpu-vendor", gpu.driver_name.as_str()),
        ("data-kael-gpu-driver-info", gpu.driver_info.as_str()),
    ];
    for (name, value) in attributes {
        let _ = root.set_attribute(name, value);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish_gpu_evidence(_window: &Window) {}

#[cfg(target_arch = "wasm32")]
fn publish_text_input_probes(value: &str) {
    let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    else {
        return;
    };
    if value == IME_PROBE_TEXT {
        let _ = root.set_attribute("data-kael-ime-probe", "passed");
    }
    if value == CLIPBOARD_PROBE_TEXT {
        let _ = root.set_attribute("data-kael-clipboard-input", "passed");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish_text_input_probes(_value: &str) {}

#[cfg(target_arch = "wasm32")]
fn publish_file_drop_probe(file_count: usize, byte_count: usize, all_available: bool) {
    let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    else {
        return;
    };
    let passed = file_count > 0 && all_available;
    let _ = root.set_attribute(
        "data-kael-file-drop",
        if passed { "passed" } else { "failed" },
    );
    let _ = root.set_attribute("data-kael-file-drop-count", &file_count.to_string());
    let _ = root.set_attribute("data-kael-file-drop-bytes", &byte_count.to_string());
}

#[cfg(not(target_arch = "wasm32"))]
fn publish_file_drop_probe(_file_count: usize, _byte_count: usize, _all_available: bool) {}

#[cfg(target_arch = "wasm32")]
fn publish_file_picker_probe(file_count: usize, byte_count: usize, all_available: bool) {
    let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    else {
        return;
    };
    let _ = root.set_attribute(
        "data-kael-file-picker",
        if file_count > 0 && all_available {
            "passed"
        } else {
            "failed"
        },
    );
    let _ = root.set_attribute("data-kael-file-picker-count", &file_count.to_string());
    let _ = root.set_attribute("data-kael-file-picker-bytes", &byte_count.to_string());
}

#[cfg(not(target_arch = "wasm32"))]
fn publish_file_picker_probe(_file_count: usize, _byte_count: usize, _all_available: bool) {}

#[cfg(target_arch = "wasm32")]
fn publish_file_export_probe(passed: bool) {
    let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    else {
        return;
    };
    let _ = root.set_attribute(
        "data-kael-file-export",
        if passed { "passed" } else { "failed" },
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn publish_file_export_probe(_passed: bool) {}

#[cfg(target_arch = "wasm32")]
fn publish_accessibility_action_probe(clicks: usize) {
    let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    else {
        return;
    };
    let _ = root.set_attribute("data-kael-accessibility-action-count", &clicks.to_string());
    if clicks > 0 {
        let _ = root.set_attribute("data-kael-accessibility-action", "passed");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish_accessibility_action_probe(_clicks: usize) {}

struct BrowserSmoke {
    clicks: usize,
    webview_messages: usize,
    virtualization: Option<VirtualizationEvidence>,
    table_scroll: UniformListScrollHandle,
    table_scrollbar: ScrollbarState,
    jump_requested: bool,
    jump_verified: bool,
    return_requested: bool,
    ime_probe: Entity<InputState>,
    ime_focus_requested: bool,
    dropped_files: usize,
    dropped_files_available: bool,
    picked_files: usize,
    picked_files_available: bool,
    export_triggered: bool,
    image: Arc<Image>,
}

impl Render for BrowserSmoke {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        publish_gpu_evidence(window);
        let theme = Theme::of(cx);
        let viewport_width = window.viewport_size().width;
        let viewport_height = window.viewport_size().height;
        let compact = viewport_width < px(520.0);
        let short = viewport_height < px(520.0);
        let dense = compact || short;
        let narrow = viewport_width < px(800.0);
        let compact_section_height =
            px(((f32::from(window.viewport_size().height) - 100.0) * 0.47).max(220.0));
        let webview_entity = cx.weak_entity();
        let virtualization_entity = cx.weak_entity();
        let drop_entity = cx.weak_entity();
        let picker_entity = cx.weak_entity();
        let export_entity = cx.weak_entity();
        let row_border = theme.tokens.border;
        let row_muted = theme.tokens.muted;
        let row_muted_foreground = theme.tokens.muted_foreground;
        let row_foreground = theme.tokens.foreground;
        let row_accent = theme.tokens.accent;

        if !self.ime_focus_requested {
            window.focus(&self.ime_probe.read(cx).focus_handle(cx));
            self.ime_focus_requested = true;
        }

        // Prove O(1) large-index seeking after the first viewport has mounted.
        // Once the last row is observed, the callback returns the showcase to
        // row one so a human opens the page at a natural starting position.
        if self.virtualization.is_some() && !self.jump_requested {
            self.table_scroll
                .scroll_to_item_strict(LOGICAL_ROW_COUNT - 1, ScrollStrategy::Bottom);
            self.jump_requested = true;
        }

        let table_scroll = self.table_scroll.clone();
        let table_base_scroll = self.table_scroll.0.borrow().base_handle.clone();
        let table_scrollbar = self.table_scrollbar.clone();
        let mounted_rows = self
            .virtualization
            .map_or(0, |evidence| evidence.mounted_rows());
        let materialize_micros = self
            .virtualization
            .map_or(0, |evidence| evidence.materialize_micros);
        let visible_range = self.virtualization.map_or_else(
            || "Measuring viewport…".to_string(),
            |evidence| format!("Rows {}–{}", evidence.range_start + 1, evidence.range_end),
        );
        let virtualization_summary = self.virtualization.map_or_else(
            || "Mounting the first visible range".to_string(),
            |evidence| {
                format!(
                    "{} mounted · {} µs · {}",
                    evidence.mounted_rows(),
                    evidence.materialize_micros,
                    if evidence.is_viewport_bounded() {
                        "viewport bounded"
                    } else {
                        "bound exceeded"
                    }
                )
            },
        );

        let row_id_width = if narrow { px(68.0) } else { px(82.0) };
        let row_cost_width = if narrow { px(64.0) } else { px(76.0) };
        let row_state_width = if narrow { px(56.0) } else { px(70.0) };
        let table_list = uniform_list(
            "browser-million-row-table",
            LOGICAL_ROW_COUNT,
            move |range, _, cx| {
                let started = Instant::now();
                let rows = range
                    .clone()
                    .map(|index| {
                        let workload = match index % 4 {
                            0 => "Scene batch",
                            1 => "Physics step",
                            2 => "Asset stream",
                            _ => "UI update",
                        };
                        let cost_micros = 12 + (index.wrapping_mul(37) % 83);
                        let warm = index % 11 != 0;

                        div()
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::Row)
                                    .row_index(index + 2),
                            )
                            .h(px(ROW_HEIGHT))
                            .w_full()
                            .flex()
                            .items_center()
                            .px(px(12.0))
                            .border_b_1()
                            .border_color(row_border)
                            .when(index % 2 == 1, |row| row.bg(row_muted.opacity(0.12)))
                            .text_size(px(11.0))
                            .text_color(row_foreground)
                            .child(
                                div()
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Cell)
                                            .label(format!("Row {} identifier", index + 1))
                                            .row_index(index + 2)
                                            .column_index(1),
                                    )
                                    .w(row_id_width)
                                    .text_color(row_muted_foreground)
                                    .child(format!("#{:07}", index + 1)),
                            )
                            .child(
                                div()
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Cell)
                                            .label(format!("{workload}, row {}", index + 1))
                                            .row_index(index + 2)
                                            .column_index(2),
                                    )
                                    .flex_1()
                                    .min_w_0()
                                    .child(workload),
                            )
                            .child(
                                div()
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Cell)
                                            .label(format!(
                                                "{cost_micros} microseconds, row {}",
                                                index + 1
                                            ))
                                            .row_index(index + 2)
                                            .column_index(3),
                                    )
                                    .w(row_cost_width)
                                    .text_color(row_muted_foreground)
                                    .child(format!("{cost_micros} µs")),
                            )
                            .child(
                                div()
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Cell)
                                            .label(format!(
                                                "{} state, row {}",
                                                if warm { "ready" } else { "warming" },
                                                index + 1
                                            ))
                                            .row_index(index + 2)
                                            .column_index(4),
                                    )
                                    .w(row_state_width)
                                    .text_color(if warm {
                                        row_accent
                                    } else {
                                        row_muted_foreground
                                    })
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(if warm { "READY" } else { "WARM" }),
                            )
                    })
                    .collect::<Vec<_>>();

                // `uniform_list` first asks for one row to establish uniform
                // height. Only its multi-row callback represents a mounted
                // viewport and is therefore published as release evidence.
                if range.len() > 1 {
                    let evidence = VirtualizationEvidence {
                        range_start: range.start,
                        range_end: range.end,
                        materialize_micros: started.elapsed().as_micros(),
                    };
                    let _ = virtualization_entity.update(cx, move |this, cx| {
                        let first_measurement = this.virtualization.is_none();
                        let range_changed = this.virtualization.is_none_or(|previous| {
                            previous.range_start != evidence.range_start
                                || previous.range_end != evidence.range_end
                        });
                        let reached_last_row = evidence.range_end == LOGICAL_ROW_COUNT;
                        if reached_last_row {
                            this.jump_verified = true;
                        }
                        if reached_last_row && !this.return_requested {
                            this.table_scroll
                                .scroll_to_item_strict(0, ScrollStrategy::Top);
                            this.return_requested = true;
                        }
                        if range_changed || reached_last_row {
                            this.virtualization = Some(evidence);
                            publish_virtualization_evidence(evidence, this.jump_verified);
                            // The uniform list already invalidates the window when its
                            // viewport changes. Re-notifying the entire application for
                            // every scroll range made the proof panel trigger a redundant
                            // full view pass. The panel only needs an explicit refresh for
                            // its first measurement and the one-time direct-jump result;
                            // browser release evidence is published directly above on
                            // every range change.
                            if first_measurement || reached_last_row {
                                cx.notify();
                            }
                        }
                    });
                }
                rows
            },
        )
        .track_scroll(table_scroll)
        .flex_1()
        .w_full();

        let table = div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Grid)
                    .label("Million-row execution table")
                    .row_count(LOGICAL_ROW_COUNT + 1)
                    .column_count(4),
            )
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_w_0()
            .overflow_hidden()
            .rounded(px(14.0))
            .border_1()
            .border_color(theme.tokens.border)
            .bg(theme.tokens.card)
            .when(compact, |table| {
                table
                    .flex_initial()
                    .flex_shrink_0()
                    .h(compact_section_height)
            })
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .px(px(14.0))
                    .py(px(11.0))
                    .border_b_1()
                    .border_color(theme.tokens.border)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .text_size(px(16.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Million-row execution table"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.tokens.muted_foreground)
                                    .child(virtualization_summary),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .px(px(10.0))
                            .py(px(5.0))
                            .rounded_full()
                            .bg(theme.tokens.accent.opacity(0.18))
                            .text_color(theme.tokens.accent_foreground)
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("1,000,000 rows"),
                    ),
            )
            .child(
                div()
                    .accessibility(
                        AccessibilityAttributes::new(AccessibilityRole::Row).row_index(1),
                    )
                    .h(px(31.0))
                    .w_full()
                    .flex()
                    .items_center()
                    .px(px(12.0))
                    .bg(theme.tokens.muted.opacity(0.30))
                    .border_b_1()
                    .border_color(theme.tokens.border)
                    .text_size(px(10.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.tokens.muted_foreground)
                    .child(
                        div()
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::ColumnHeader)
                                    .label("Row")
                                    .column_index(1),
                            )
                            .w(row_id_width)
                            .child("ROW"),
                    )
                    .child(
                        div()
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::ColumnHeader)
                                    .label("Workload")
                                    .column_index(2),
                            )
                            .flex_1()
                            .child("WORKLOAD"),
                    )
                    .child(
                        div()
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::ColumnHeader)
                                    .label("Cost")
                                    .column_index(3),
                            )
                            .w(row_cost_width)
                            .child("COST"),
                    )
                    .child(
                        div()
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::ColumnHeader)
                                    .label("State")
                                    .column_index(4),
                            )
                            .w(row_state_width)
                            .child("STATE"),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(table_list)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .child(
                                Scrollbar::vertical(&table_scrollbar, &table_base_scroll)
                                    .scroll_size(size(
                                        px(1.0),
                                        px(LOGICAL_ROW_COUNT as f32 * ROW_HEIGHT),
                                    ))
                                    .always_visible(),
                            ),
                    ),
            );

        let metric_card = |label: &'static str, value: String| {
            div()
                .flex_1()
                .min_w_0()
                .p(px(10.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(theme.tokens.border)
                .bg(theme.tokens.muted.opacity(0.12))
                .child(
                    div()
                        .text_size(px(9.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.tokens.muted_foreground)
                        .child(label),
                )
                .child(
                    div()
                        .mt(px(4.0))
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(value),
                )
        };
        let proof_row = |label: &'static str, verified: bool| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .py(px(4.0))
                .text_size(px(10.0))
                .child(label)
                .child(
                    div()
                        .text_color(if verified {
                            theme.tokens.accent
                        } else {
                            theme.tokens.muted_foreground
                        })
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(if verified { "VERIFIED" } else { "RUNNING" }),
                )
        };

        let runtime_checks = div()
            .relative()
            .w(if compact {
                relative(1.0)
            } else if narrow {
                px(250.0).into()
            } else {
                px(306.0).into()
            })
            .h_full()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(if dense { px(7.0) } else { px(11.0) })
            .p(px(12.0))
            .rounded(px(14.0))
            .border_1()
            .border_color(theme.tokens.border)
            .bg(theme.tokens.card)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size(px(1.0))
                    .overflow_hidden()
                    .opacity(0.0)
                    .child(
                        Input::new(&self.ime_probe)
                            .aria_label("Browser IME release probe")
                            .on_change(|value, _cx| publish_text_input_probes(value.as_ref())),
                    ),
            )
            .when(compact, |panel| {
                panel
                    .flex_initial()
                    .flex_shrink_0()
                    .h(compact_section_height)
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .size(if dense { px(42.0) } else { px(48.0) })
                            .flex_shrink_0()
                            .overflow_hidden()
                            .rounded(px(10.0))
                            .child(
                                img(self.image.clone())
                                    .size_full()
                                    .object_fit(ObjectFit::Cover),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(FontWeight::BOLD)
                                    .child("Retained Scene + WebGL2"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(theme.tokens.muted_foreground)
                                    .child("One Rust view · native or browser"),
                            ),
                    ),
            )
            .when(!dense, |panel| {
                panel
                    .child(
                        div()
                            .flex()
                            .gap(px(8.0))
                            .child(metric_card("LOGICAL", "1,000,000".to_string()))
                            .child(metric_card("MOUNTED", mounted_rows.to_string())),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.0))
                            .child(metric_card("VISIBLE", visible_range))
                            .child(metric_card("BUILD", format!("{materialize_micros} µs"))),
                    )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(9.0))
                    .bg(theme.tokens.accent.opacity(0.10))
                    .text_size(px(10.0))
                    .child("Direct jump to row 1,000,000")
                    .child(
                        div()
                            .text_color(theme.tokens.accent)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if self.jump_verified { "VERIFIED" } else { "RUNNING" }),
                    ),
            )
            .child(
                Button::new("browser-smoke-button", "Test pointer, text, and animation")
                    .ripple(true)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        publish_accessibility_action_probe(this.clicks);
                        cx.notify();
                    })),
            )
            .when(!dense, |panel| panel.child(
                div()
                    .text_size(px(10.0))
                    .text_color(theme.tokens.muted_foreground)
                    .child(format!("button activations: {}", self.clicks)),
            ))
            .when(!dense, |panel| {
                panel.child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new("browser-file-picker", "Import bytes")
                                .on_click(move |_, window, cx| {
                                    let receiver = cx.prompt_for_files(PathPromptOptions {
                                        files: true,
                                        directories: false,
                                        multiple: true,
                                        prompt: Some("Import portable files".into()),
                                        filters: Vec::new(),
                                    });
                                    let picker_entity = picker_entity.clone();
                                    window
                                        .spawn(cx, async move |cx| {
                                            let Ok(Ok(Some(files))) = receiver.await else {
                                                return;
                                            };
                                            let file_count = files.len();
                                            let byte_count =
                                                files.iter().map(ExternalFile::byte_len).sum();
                                            let all_available =
                                                files.iter().all(ExternalFile::is_available);
                                            let _ = cx.update(|_, cx| {
                                                let _ = picker_entity.update(cx, |this, cx| {
                                                    this.picked_files = file_count;
                                                    this.picked_files_available = all_available;
                                                    publish_file_picker_probe(
                                                        file_count,
                                                        byte_count,
                                                        all_available,
                                                    );
                                                    cx.notify();
                                                });
                                            });
                                        })
                                        .detach();
                                }),
                        )
                        .child(
                            Button::new("browser-file-export", "Export Blob")
                                .on_click(move |_, window, cx| {
                                    let receiver = cx.save_file_bytes(
                                        SaveDialogBuilder::new(".")
                                            .suggested_name("kael-browser-proof.txt")
                                            .text(),
                                        b"Kael portable browser export".to_vec(),
                                        "text/plain",
                                    );
                                    let export_entity = export_entity.clone();
                                    window
                                        .spawn(cx, async move |cx| {
                                            let passed = matches!(receiver.await, Ok(Ok(true)));
                                            let _ = cx.update(|_, cx| {
                                                let _ = export_entity.update(cx, |this, cx| {
                                                    this.export_triggered = passed;
                                                    publish_file_export_probe(passed);
                                                    cx.notify();
                                                });
                                            });
                                        })
                                        .detach();
                                }),
                        )
                        .child(
                            Button::new("browser-print-proof", "Print").on_click(
                                |_, window, cx| {
                                    let job = PrintJob::letter(
                                        "Kael browser print proof",
                                        |page, _cx| {
                                            page.draw_text(
                                                "Kael retained PrintJob",
                                                point(px(36.0), px(48.0)),
                                                PrintTextStyle::new(px(22.0)),
                                            );
                                            page.draw_text_block(
                                                "The same Rust print-page commands are rendered by desktop and browser targets.",
                                                Bounds::new(
                                                    point(px(36.0), px(92.0)),
                                                    size(px(540.0), px(120.0)),
                                                ),
                                                PrintTextStyle::new(px(13.0)),
                                            );
                                        },
                                    );
                                    if let Err(error) = window.show_print_dialog(job, cx) {
                                        eprintln!("browser print proof failed: {error:#}");
                                    }
                                },
                            ),
                        ),
                )
            })
            .child(
                div()
                    .w_full()
                    .h(if dense { px(68.0) } else { px(96.0) })
                    .child(
                    webview_html(
                        "browser-smoke-webview",
                        r#"<meta charset="utf-8"><title>Kael browser WebView</title><style>html,body{height:100%;margin:0}body{box-sizing:border-box;display:grid;place-items:center;padding:12px;background:linear-gradient(145deg,#172033,#111827);color:#dbeafe;border:1px solid #3b82f6;border-radius:10px;font:600 13px system-ui;text-align:center}</style><body>Iframe WebView bridge<br>ready and authenticated</body><script>window.gpui.postMessage({ kind: "browser-webview-ready" });</script>"#,
                    )
                    .on_message(move |message, _, cx| {
                        webview_entity
                            .update(cx, |this, cx| {
                                if message.get("kind").and_then(|value| value.as_str())
                                    == Some("browser-webview-ready")
                                {
                                    this.webview_messages += 1;
                                    cx.notify();
                                }
                            })
                            .ok();
                    })
                        .size_full(),
                    ),
            )
            .when(!dense, |panel| panel.child(
                div()
                    .text_size(px(10.0))
                    .text_color(theme.tokens.muted_foreground)
                    .child(format!(
                        "authenticated iframe messages: {}",
                        self.webview_messages
                    )),
            ))
            .when(!dense, |panel| {
                panel.child(
                    div()
                        .flex_1()
                        .min_h(px(88.0))
                        .p(px(10.0))
                        .rounded(px(10.0))
                        .border_1()
                        .border_color(theme.tokens.border)
                        .bg(theme.tokens.muted.opacity(0.10))
                        .child(
                            div()
                                .mb(px(5.0))
                                .text_size(px(10.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.tokens.muted_foreground)
                                .child("LIVE RELEASE EVIDENCE"),
                        )
                        .child(proof_row(
                            "Viewport mount bound",
                            self.virtualization
                                .is_some_and(VirtualizationEvidence::is_viewport_bounded),
                        ))
                        .child(proof_row("Millionth-row jump", self.jump_verified))
                        .child(proof_row(
                            "Authenticated WebView IPC",
                            self.webview_messages == 1,
                        ))
                        .child(proof_row(
                            "Byte-backed browser file drop",
                            self.dropped_files > 0 && self.dropped_files_available,
                        ))
                        .child(proof_row(
                            "Byte-backed browser file picker",
                            self.picked_files > 0 && self.picked_files_available,
                        ))
                        .child(proof_row("Blob file export", self.export_triggered))
                        .child(proof_row("Native + WASM source", true)),
                )
            });

        div()
            .id("browser-smoke")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(if compact { px(10.0) } else { px(16.0) })
            .overflow_hidden()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .can_drop(|value, _, _| {
                ExternalDropData::from_drag_value(value).is_some_and(|data| data.has_files())
            })
            .on_external_drop(move |data, _, cx| {
                let file_count = data.file_count();
                let byte_count = data.file_bytes();
                let all_available = data.files().iter().all(ExternalFile::is_available);
                let _ = drop_entity.update(cx, move |this, cx| {
                    this.dropped_files = file_count;
                    this.dropped_files_available = all_available;
                    publish_file_drop_probe(file_count, byte_count, all_available);
                    cx.notify();
                });
            })
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .text_size(if compact { px(19.0) } else { px(23.0) })
                                    .font_weight(FontWeight::BOLD)
                                    .child("Kael browser release proof"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.tokens.muted_foreground)
                                    .child("A desktop-grade Rust UI, packaged for the web without a DOM rewrite"),
                            ),
                    )
                    .when(!compact, |header| header.child(
                        div()
                            .flex_shrink_0()
                            .px(px(10.0))
                            .py(px(6.0))
                            .rounded_full()
                            .border_1()
                            .border_color(theme.tokens.accent.opacity(0.45))
                            .bg(theme.tokens.accent.opacity(0.10))
                            .text_color(theme.tokens.accent)
                            .text_size(px(10.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("VIEWPORT ≤ 64 ROWS"),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .w_full()
                    .gap(px(12.0))
                    .overflow_hidden()
                    .when(compact, |content| content.flex_col())
                    .child(table)
                    .child(runtime_checks),
            )
    }
}

fn main() {
    Application::try_new()
        .expect("failed to initialize Kael's browser platform")
        .run(|cx| {
            kael_ui::init(cx);
            install_theme(cx, Theme::tokyo_night());
            cx.open_window(
                WindowOptions {
                    titlebar: None,
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| BrowserSmoke {
                        clicks: 0,
                        webview_messages: 0,
                        virtualization: None,
                        table_scroll: UniformListScrollHandle::new(),
                        table_scrollbar: ScrollbarState::default(),
                        jump_requested: false,
                        jump_verified: false,
                        return_requested: false,
                        ime_probe: cx.new(InputState::new),
                        ime_focus_requested: false,
                        dropped_files: 0,
                        dropped_files_available: false,
                        picked_files: 0,
                        picked_files_available: false,
                        export_triggered: false,
                        image: Arc::new(Image::from_bytes(
                            ImageFormat::Jpeg,
                            include_bytes!("../assets/images/carousel_2.jpg").to_vec(),
                        )),
                    })
                },
            )
            .expect("failed to open #blade browser canvas");
        });
}

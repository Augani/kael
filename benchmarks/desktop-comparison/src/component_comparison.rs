//! A matched rendered Unicode document workload using each framework's shipped Editor.
//! Selection/edit/undo/redo/scroll each get a separate paced frame. Syntax is plain
//! on both sides; the shared fixture contains real Rust and Markdown source.
#[cfg(all(feature = "kael-engine", feature = "gpui-kit-engine"))]
compile_error!("select exactly one framework engine");
#[cfg(feature = "gpui-kit-engine")]
use gpui_kit as ui;
#[cfg(feature = "gpui-kit-engine")]
use gpui_kit::component::input::{Editor, EditorState, Redo, Undo};
#[cfg(feature = "kael-engine")]
use kael as ui;
#[cfg(feature = "kael-engine")]
use kael_ui::components::editor::{Editor, EditorState, Redo, Undo};
use std::{
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};
use ui::{prelude::*, *};

const CONTRACT: &str = "native-editor-document-v1";
const REPLACEMENT: &str = "新文書";
const FONT_FAMILY: &str = "Menlo";
const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 21.0;

#[derive(Clone, Copy)]
struct Options {
    sections: usize,
    quick: bool,
}
impl Options {
    fn parse() -> Self {
        let mut options = Self {
            sections: 2_000,
            quick: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--quick" => options.quick = true,
                "--sections" => {
                    options.sections = args
                        .next()
                        .expect("--sections needs a count")
                        .parse()
                        .expect("positive section count")
                }
                _ => panic!("unknown flag {arg}; use --quick or --sections N"),
            }
        }
        assert!(
            (1..=10_000).contains(&options.sections),
            "sections must be 1..=10000"
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

struct Fixture {
    text: String,
    edits: Arc<[Range<usize>]>,
    lines: usize,
}
fn fixture(sections: usize, generation: usize) -> Fixture {
    let mut text = String::with_capacity(sections * 240);
    let mut edits = Vec::with_capacity(sections);
    for section in 0..sections {
        text.push_str(&format!(
            "# Section {section:05} · revision {generation:06} · café 日本語 👩‍💻\n\n"
        ));
        text.push_str("- [ ] Preserve Unicode selections and undo history.\n```rust\n");
        text.push_str(&format!("let label_{section:05} = \""));
        let start = text.len();
        text.push_str("日本語");
        edits.push(start..text.len());
        text.push_str(" café 👩‍💻\";\n");
        text.push_str(&format!("println!(\"{{label_{section:05}}}\");\n```\n\n"));
    }
    let lines = text.bytes().filter(|byte| *byte == b'\n').count() + 1;
    Fixture {
        text,
        edits: edits.into(),
        lines,
    }
}
// FNV-1a is an explicit stable exact-byte fingerprint, independent of Rust's
// randomized HashMap hasher and identical for the two framework processes.
fn fingerprint(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("fnv1a64:{hash:016x}")
}

fn make_editor(window: &mut Window, cx: &mut App, text: &str) -> Entity<EditorState> {
    #[cfg(feature = "kael-engine")]
    {
        let _ = window;
        cx.new(|cx| {
            let mut state = EditorState::new(cx);
            state.set_font_size(FONT_SIZE, cx);
            state.set_font_family(FONT_FAMILY, cx);
            state.set_content(text, cx);
            state
        })
    }
    #[cfg(feature = "gpui-kit-engine")]
    cx.new(|cx| {
        EditorState::new(window, cx)
            .default_value(text.to_owned())
            .folding(false)
    })
}
fn select(editor: &Entity<EditorState>, range: Range<usize>, cx: &mut App) {
    editor.update(cx, |state, cx| {
        #[cfg(feature = "kael-engine")]
        state
            .set_selection_bytes(range.start, range.end, cx)
            .expect("fixture UTF-8 boundary");
        #[cfg(feature = "gpui-kit-engine")]
        state.set_selected_range(range, cx);
    });
}
fn replace(editor: &Entity<EditorState>, window: &mut Window, cx: &mut App) {
    editor.update(cx, |state, cx| {
        #[cfg(feature = "kael-engine")]
        {
            let _ = window;
            state.replace_selection(REPLACEMENT, cx);
        }
        #[cfg(feature = "gpui-kit-engine")]
        state.replace(REPLACEMENT, window, cx);
    });
}
fn replace_document(editor: &Entity<EditorState>, text: &str, window: &mut Window, cx: &mut App) {
    editor.update(cx, |state, cx| {
        #[cfg(feature = "kael-engine")]
        {
            let _ = window;
            state.set_content(text, cx);
        }
        #[cfg(feature = "gpui-kit-engine")]
        state.set_value(text.to_owned(), window, cx);
    });
}
fn editor_text(editor: &Entity<EditorState>, cx: &App) -> String {
    #[cfg(feature = "kael-engine")]
    {
        editor.read(cx).content()
    }
    #[cfg(feature = "gpui-kit-engine")]
    {
        editor.read(cx).value().to_string()
    }
}
fn editor_element(editor: &Entity<EditorState>) -> AnyElement {
    #[cfg(feature = "kael-engine")]
    {
        Editor::new(editor)
            .show_border(false)
            .size_full()
            .into_any_element()
    }
    #[cfg(feature = "gpui-kit-engine")]
    {
        Editor::new(editor)
            .appearance(false)
            .bordered(false)
            .font_family(FONT_FAMILY)
            .text_size(px(FONT_SIZE))
            .line_height(relative(1.5))
            .h(relative(1.0))
            .w_full()
            .into_any_element()
    }
}

struct Workload {
    editor: Entity<EditorState>,
    _observer: Subscription,
    options: Options,
    oracle: String,
    edit_ranges: Arc<[Range<usize>]>,
    rows: usize,
    fixture_hash: String,
    fixture_bytes: usize,
    active_edit: Range<usize>,
    previous_text: String,
    sequence: usize,
    operations: [u64; 7],
    operation_us: Vec<u64>,
    phase_checks: Vec<(String, bool)>,
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
        let fixture = fixture(options.sections, 0);
        let editor = make_editor(window, cx, &fixture.text);
        let observer = cx.observe(&editor, |_, _, cx| cx.notify());
        Self {
            editor,
            _observer: observer,
            options,
            rows: fixture.lines,
            fixture_hash: fingerprint(&fixture.text),
            fixture_bytes: fixture.text.len(),
            oracle: fixture.text,
            edit_ranges: fixture.edits,
            active_edit: 0..0,
            previous_text: String::new(),
            sequence: 0,
            operations: [0; 7],
            operation_us: Vec::with_capacity(4096),
            phase_checks: Vec::with_capacity(4),
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
        self.sequence = 0;
        self.last_render = None;
        #[cfg(feature = "kael-engine")]
        if phase.starts_with("idle") {
            window.blur();
        } else {
            window.focus(&self.editor.read(cx).focus_handle(cx));
        }
        #[cfg(feature = "gpui-kit-engine")]
        if phase.starts_with("idle") {
            window.blur(cx);
        } else {
            window.focus(&self.editor.read(cx).focus_handle(cx), cx);
        }
        cx.notify();
        window.refresh();
    }
    fn begin_validation(&mut self, window: &mut Window) {
        self.collect(window);
        self.measuring = false;
    }
    fn check_phase(&mut self, cx: &App) {
        // Validate outside paced interaction/draw samples, once per phase.
        let actual = editor_text(&self.editor, cx);
        let correct = actual == self.oracle;
        self.phase_checks.push((self.phase.to_owned(), correct));
        assert!(
            correct,
            "{} {} document diverged from identical edit/undo/redo oracle",
            engine(),
            self.phase
        );
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.collect(window);
        let started = Instant::now();
        if self.phase == "churn" && self.sequence.is_multiple_of(30) {
            let fixture = fixture(self.options.sections, self.sequence / 30 + 1);
            replace_document(&self.editor, &fixture.text, window, cx);
            self.oracle = fixture.text;
            self.edit_ranges = fixture.edits;
            self.operations[6] += 1;
        }
        let operation = self.sequence % 6;
        match operation {
            0 => {
                let index = (self.sequence / 6 * 7919) % self.edit_ranges.len();
                self.active_edit = self.edit_ranges[index].clone();
                self.previous_text = self.oracle[self.active_edit.clone()].to_owned();
                select(&self.editor, self.active_edit.clone(), cx);
            }
            1 => {
                replace(&self.editor, window, cx);
                self.oracle
                    .replace_range(self.active_edit.clone(), REPLACEMENT);
            }
            2 => {
                window.dispatch_action(Box::new(Undo), cx);
                self.oracle
                    .replace_range(self.active_edit.clone(), &self.previous_text);
            }
            3 => {
                window.dispatch_action(Box::new(Redo), cx);
                self.oracle
                    .replace_range(self.active_edit.clone(), REPLACEMENT);
            }
            4 => {
                let index = (self.sequence / 6 * 3571 + self.edit_ranges.len() / 2)
                    % self.edit_ranges.len();
                let byte = self.edit_ranges[index].start;
                select(&self.editor, byte..byte, cx);
            }
            5 => select(&self.editor, 0..0, cx),
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
                "contract": CONTRACT, "engine": engine(), "quick": self.options.quick,
                "frame_timing_enabled": cfg!(feature = "frame-timing"), "validation_phase_markers": true,
                "rows": self.rows, "sections": self.options.sections,
                "fixture_hash": self.fixture_hash, "fixture_bytes": self.fixture_bytes,
                "final_hash": fingerprint(&self.oracle), "phase_correctness": self.phase_checks,
                "font_family": FONT_FAMILY, "font_size_px": FONT_SIZE, "line_height_px": LINE_HEIGHT,
                "window_width_px": 1100, "window_height_px": 760, "syntax": "plain", "theme_mode": "dark",
                "operations": {"selection":self.operations[0], "replace":self.operations[1],
                    "undo":self.operations[2], "redo":self.operations[3], "scroll_to_caret":self.operations[4],
                    "home":self.operations[5], "replace_document":self.operations[6]},
                "operation_cpu_us": self.operation_us,
                "first_render_us": self.first_render_us, "render_callback_intervals_us": self.active_us,
                "renders": self.renders, "elapsed_us": self.started.elapsed().as_micros(),
                "first_submission_us": self.first_submission_us,
                "draw_cpu_us": self.draw_us, "submission_cpu_us": self.submission_us,
                "gpu_allocated_bytes": self.gpu_samples,
                "submission_scope": "CPU platform submission, excluding GPU completion/compositor display",
                "timing_scope": "native rendered Editor; public interaction APIs and routed undo/redo; paced separate frames",
                "oracle_scope": "same exact-byte document oracle on both engines; phase validation excluded from paced samples"
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
            let now = Instant::now();
            if self.phase == "active" || self.phase == "churn" {
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
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x11151b))
            .text_color(rgb(0xe2e6ed))
            .font_family(FONT_FAMILY)
            .text_size(px(FONT_SIZE))
            .child(
                div()
                    .h(px(56.0))
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child("Unicode document · Rust / Markdown")
                    .child(format!(
                        "{} · {} lines · {}",
                        engine(),
                        self.rows,
                        self.phase
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .overflow_hidden()
                    .child(editor_element(&self.editor)),
            )
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
    let window = cx.open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1100.0), px(760.0)), cx))),
        ..Default::default()
    }, |window, cx| {
        let view = cx.new(|cx| Workload::new(started, options, window, cx));
        let weak = view.downgrade();
        window.spawn(cx, async move |cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            for phase in ["idle-before", "active", "idle-after", "churn"] {
                weak.update_in(cx, |view, window, cx| view.set_phase(phase, window, cx)).unwrap();
                println!("KAEL_PHASE {}", serde_json::json!({"phase":phase,"elapsed_us":started.elapsed().as_micros()}));
                if phase.starts_with("idle") { cx.background_executor().timer(options.duration(phase)).await; }
                else {
                    let until = Instant::now() + options.duration(phase);
                    while Instant::now() < until {
                        cx.background_executor().timer(Duration::from_micros(16_667)).await;
                        weak.update_in(cx, |view, window, cx| view.tick(window, cx)).unwrap();
                    }
                    // Routed actions settle in the foreground after dispatch;
                    // validate after a separate event turn rather than inside it.
                    cx.background_executor().timer(Duration::from_millis(20)).await;
                }
                println!("KAEL_PHASE {}", serde_json::json!({"phase":"validation","elapsed_us":started.elapsed().as_micros()}));
                weak.update_in(cx, |view, window, _| view.begin_validation(window)).unwrap();
                weak.update_in(cx, |view, _, cx| view.check_phase(cx)).unwrap();
            }
            println!("KAEL_PHASE {}", serde_json::json!({"phase":"finished","elapsed_us":started.elapsed().as_micros()}));
            weak.update_in(cx, |view, window, _| { view.collect(window); view.snapshot_gpu(); view.report(); }).unwrap();
            cx.update(|_, cx| cx.quit()).unwrap();
        }).detach();
        view
    });
    if let Err(error) = window {
        eprintln!("component comparison window failed: {error}");
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
    fn fixture_is_exact_byte_stable_and_replacement_preserves_offsets() {
        let fixture = fixture(2_000, 0);
        assert_eq!(fixture.lines, 16_001);
        assert_eq!(fixture.edits.len(), 2_000);
        assert_eq!(fingerprint(&fixture.text), "fnv1a64:694401c508afd37d");
        for range in fixture.edits.iter() {
            assert_eq!(&fixture.text[range.clone()], "日本語");
            assert_eq!(range.len(), REPLACEMENT.len());
        }
    }
}

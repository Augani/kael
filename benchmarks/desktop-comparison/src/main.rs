//! An identical native navigation/detail workload against two pinned engines.
//! See README.md for the timing scopes and external process instrumentation.

#[cfg(all(feature = "kael-engine", feature = "gpui-kit-engine"))]
compile_error!("select exactly one framework engine");
#[cfg(feature = "gpui-kit-engine")]
use gpui_kit as ui;
#[cfg(feature = "kael-engine")]
use kael as ui;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use ui::{prelude::*, *};

#[path = "native_geometry.rs"]
mod native_geometry;
use native_geometry::NativeGeometry;

const ROWS: usize = 100_000;
const ACTIVE_SECONDS: u64 = 12;
const IDLE_SECONDS: u64 = 5;
const CHURN_SECONDS: u64 = 12;

struct Workload {
    labels: Arc<[SharedString]>,
    selected: usize,
    sequence: usize,
    scroll: UniformListScrollHandle,
    started: Instant,
    first_render_us: Option<u128>,
    active_us: Vec<u128>,
    last_render: Option<Instant>,
    renders: u64,
    rendered_rows: u64,
    phase: &'static str,
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
    fn new(started: Instant) -> Self {
        Self {
            labels: (0..ROWS)
                .map(|index| format!("document_{index:06}.rs").into())
                .collect::<Vec<_>>()
                .into(),
            selected: 0,
            sequence: 0,
            scroll: UniformListScrollHandle::default(),
            started,
            first_render_us: None,
            active_us: Vec::with_capacity(4096),
            last_render: None,
            renders: 0,
            rendered_rows: 0,
            phase: "startup",
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

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.collect(window);
        self.sequence += 1;
        self.selected = (self.sequence * 7919) % ROWS;
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Center);
        if self.phase == "churn" {
            // Same shared-model replacement and label ownership on both engines.
            // Force sustained replacement rather than a one-time static list.
            let generation = self.sequence / 30;
            if self.sequence.is_multiple_of(30) {
                self.labels = (0..ROWS)
                    .map(|index| format!("revision_{generation:04}_{index:06}.rs").into())
                    .collect::<Vec<_>>()
                    .into();
            }
        }
        cx.notify();
        window.refresh();
    }

    fn rows(&mut self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.rendered_rows += range.len() as u64;
        let view = cx.entity();
        range
            .map(|index| {
                row(
                    self.labels[index].clone(),
                    index,
                    self.selected == index,
                    view.clone(),
                )
            })
            .collect()
    }

    fn collect(&mut self, window: &mut Window) {
        #[cfg(not(feature = "frame-timing"))]
        let _ = window;
        #[cfg(all(feature = "kael-engine", feature = "frame-timing"))]
        {
            for frame in window.frame_timeline().iter() {
                if self.last_draw.is_none_or(|last| frame.frame_number > last) {
                    if self.draw_us.len() < 4096 {
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
                    if self.submission_us.len() < 4096 {
                        self.submission_us.push(frame.duration_us);
                    }
                    self.first_submission_us.get_or_insert_with(|| {
                        self.started
                            .elapsed()
                            .saturating_sub(frame.submitted_at.elapsed())
                            .as_micros()
                    });
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
                        if self.draw_us.len() < 4096 {
                            self.draw_us.push(frame.draw_duration().as_micros() as u64);
                        }
                    }
                    gpui_kit::profiler::FrameEvent::Present(frame) => {
                        if self.submission_us.len() < 4096 {
                            self.submission_us
                                .push(frame.present_duration().as_micros() as u64);
                        }
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
                "contract": "native-navigation-detail-v1", "engine": engine(),
                "frame_timing_enabled": cfg!(feature = "frame-timing"),
                "rows": ROWS, "first_render_us": self.first_render_us,
                "render_callback_intervals_us": self.active_us,
                "renders": self.renders, "rendered_rows": self.rendered_rows,
                "elapsed_us": self.started.elapsed().as_micros(),
                "first_submission_us": self.first_submission_us,
                "draw_cpu_us": self.draw_us, "submission_cpu_us": self.submission_us,
                "native_window_geometry": self.geometry.samples,
                "gpu_allocated_bytes": self.gpu_samples,
                "submission_scope": "CPU platform submission, excluding GPU completion/compositor display",
                "timing_scope": "root render callback; intervals include pacing, not GPU duration"
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

fn row(label: SharedString, index: usize, selected: bool, view: Entity<Workload>) -> AnyElement {
    div()
        .id(("file", index))
        .w_full()
        .h(px(28.0))
        .px_3()
        .flex()
        .items_center()
        .bg(if selected {
            rgb(0x354b6b)
        } else {
            rgb(0x171b22)
        })
        .text_color(rgb(0xe2e6ed))
        .text_size(px(13.0))
        .child(label)
        .on_click(move |_, _, cx| {
            view.update(cx, |view, cx| {
                view.selected = index;
                cx.notify();
            })
        })
        .into_any_element()
}

impl Render for Workload {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        self.renders += 1;
        self.first_render_us
            .get_or_insert_with(|| self.started.elapsed().as_micros());
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
        let list = {
            let view = cx.entity();
            let list = uniform_list("files", ROWS, move |range, _, cx| {
                view.update(cx, |view, cx| view.rows(range, cx))
            });
            #[cfg(feature = "kael-engine")]
            {
                list.track_scroll(self.scroll.clone())
            }
            #[cfg(feature = "gpui-kit-engine")]
            {
                list.track_scroll(&self.scroll)
            }
        };
        div().size_full().flex().flex_col().bg(rgb(0x11151b)).text_color(rgb(0xe2e6ed))
            .text_size(px(14.0)).font_family(".AppleSystemUIFont")
            .child(div().h(px(56.0)).px_4().flex().items_center().justify_between()
                .child("Desktop performance workspace")
                .child(format!("{} · {} rows · {}", engine(), ROWS, self.phase)))
            .child(div().flex_1().min_h(px(0.0)).flex()
                .child(div().w(px(320.0)).h_full().overflow_hidden().child(list.size_full()))
                .child(div().flex_1().p_6().flex().flex_col().gap_4()
                    .child(format!("Selected: {}", self.labels[self.selected]))
                    .child("The navigation list mounts the visible rows. Scroll jumps and model replacements use the same sequence in both frameworks.")
                    .children((0..12).map(|line| div().h(px(28.0)).child(format!("{line:02}  fn document_{}() {{ return {}; }}", self.selected, self.sequence))))))
    }
}

fn launch(started: Instant, cx: &mut App) {
    #[cfg(feature = "kael-engine")]
    {
        kael_ui::init(cx);
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::astryx_neutral());
    }
    #[cfg(feature = "gpui-kit-engine")]
    gpui_kit::init(cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1100.0), px(760.0)),
            cx,
        ))),
        ..Default::default()
    };
    let window = cx.open_window(options, |window, cx| {
        let view = cx.new(|_| Workload::new(started));
        let weak = view.downgrade();
        window.spawn(cx, async move |cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            for phase in ["idle-before", "active", "idle-after", "churn"] {
                let duration = if phase.starts_with("idle") { IDLE_SECONDS } else if phase == "active" { ACTIVE_SECONDS } else { CHURN_SECONDS };
                weak.update_in(cx, |view, window, cx| { view.collect(window); view.snapshot_gpu(); view.phase = phase; view.geometry.record(phase, "begin", window); view.last_render = None; cx.notify(); window.refresh(); }).unwrap();
                println!("KAEL_PHASE {}", serde_json::json!({"phase":phase,"elapsed_us":started.elapsed().as_micros()}));
                if phase.starts_with("idle") { cx.background_executor().timer(Duration::from_secs(duration)).await; }
                else {
                    let until = Instant::now() + Duration::from_secs(duration);
                    while Instant::now() < until {
                        cx.background_executor().timer(Duration::from_micros(16_667)).await;
                        weak.update_in(cx, |view, window, cx| view.tick(window, cx)).unwrap();
                    }
                }
                weak.update_in(cx, |view, window, _| view.geometry.record(phase, "end", window)).unwrap();
            }
            weak.update_in(cx, |view, window, _| { view.collect(window); view.snapshot_gpu(); view.report(); }).unwrap();
            cx.update(|_, cx| cx.quit()).unwrap();
        }).detach();
        view
    });
    if let Err(error) = window {
        eprintln!("comparison window failed: {error}");
        cx.quit();
    }
    cx.activate(true);
}

fn main() {
    let started = Instant::now();
    #[cfg(feature = "kael-engine")]
    Application::try_new()
        .expect("native Kael platform")
        .run(move |cx| launch(started, cx));
    #[cfg(feature = "gpui-kit-engine")]
    {
        #[cfg(feature = "frame-timing")]
        gpui_kit::profiler::set_trace_enabled(true);
        gpui_kit::application().run(move |cx| launch(started, cx));
    }
}

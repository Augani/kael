//! Shared bounded measurement bookkeeping for the tree/workspace contracts.
//! Validation consumes and discards framework records, including async redraws.

#[cfg(feature = "gpui-kit-engine")]
use gpui_kit as ui;
#[cfg(feature = "kael-engine")]
use kael as ui;
use std::time::{Duration, Instant};
use ui::Window;

#[path = "native_geometry.rs"]
mod native_geometry;
use native_geometry::NativeGeometry;

pub const WINDOW_WIDTH: f32 = 1100.0;
pub const WINDOW_HEIGHT: f32 = 760.0;
pub const CONTENT_HEIGHT: f32 = 704.0;
pub const FONT_FAMILY: &str = "Menlo";
pub const FONT_SIZE: f32 = 14.0;
pub const LINE_HEIGHT: f32 = 20.0;
pub const PHASES: [&str; 4] = ["idle-before", "active", "idle-after", "churn"];

pub fn phase_duration(quick: bool, phase: &str) -> Duration {
    Duration::from_secs(if quick {
        1
    } else if phase.starts_with("idle") {
        5
    } else {
        12
    })
}
pub fn marker(phase: &str, started: Instant) {
    println!(
        "KAEL_PHASE {}",
        serde_json::json!({"phase":phase,"elapsed_us":started.elapsed().as_micros()})
    );
}
pub fn engine() -> &'static str {
    #[cfg(feature = "kael-engine")]
    {
        "kael"
    }
    #[cfg(feature = "gpui-kit-engine")]
    {
        "gpui-kit"
    }
}
pub fn hash_fields<'a>(fields: impl IntoIterator<Item = &'a str>) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for field in fields {
        for byte in (field.len() as u64)
            .to_le_bytes()
            .into_iter()
            .chain(field.bytes())
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    format!("fnv1a64:{hash:016x}")
}

pub struct Metrics {
    pub started: Instant,
    pub phase: &'static str,
    pub measuring: bool,
    pub operations_us: Vec<u64>,
    pub checks: Vec<(String, bool)>,
    pub oracles: Vec<serde_json::Value>,
    first_render_us: Option<u128>,
    first_submission_us: Option<u128>,
    last_render: Option<Instant>,
    renders: u64,
    callback_intervals_us: Vec<u128>,
    draw_us: Vec<u64>,
    submission_us: Vec<u64>,
    gpu_samples: Vec<(String, Option<u64>)>,
    geometry: NativeGeometry,
    #[cfg(all(feature = "kael-engine", feature = "frame-timing"))]
    last_draw: Option<u64>,
    #[cfg(all(feature = "kael-engine", feature = "frame-timing"))]
    last_submission: Option<u64>,
    #[cfg(all(feature = "gpui-kit-engine", feature = "frame-timing"))]
    collector: gpui_kit::profiler::FrameTimingCollector,
}
impl Metrics {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            phase: "startup",
            measuring: true,
            operations_us: Vec::with_capacity(4096),
            checks: Vec::with_capacity(4),
            oracles: Vec::with_capacity(4),
            first_render_us: None,
            first_submission_us: None,
            last_render: None,
            renders: 0,
            callback_intervals_us: Vec::with_capacity(4096),
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
    pub fn rendered(&mut self) {
        if !self.measuring {
            return;
        }
        self.renders += 1;
        self.first_render_us
            .get_or_insert_with(|| self.started.elapsed().as_micros());
        if self.phase == "active" || self.phase == "churn" {
            let now = Instant::now();
            if let Some(previous) = self.last_render.replace(now)
                && self.callback_intervals_us.len() < 4096
            {
                self.callback_intervals_us
                    .push(now.duration_since(previous).as_micros());
            }
        } else {
            self.last_render = None;
        }
    }
    pub fn set_phase(&mut self, phase: &'static str, window: &mut Window) {
        // Drain first while the previous phase's measuring=false is retained.
        self.collect(window);
        self.snapshot_gpu();
        self.phase = phase;
        self.geometry.record(phase, "begin", window);
        self.measuring = true;
        self.last_render = None;
    }
    pub fn validation(&mut self, window: &mut Window) {
        self.geometry.record(self.phase, "end", window);
        self.collect(window);
        self.measuring = false;
        self.last_render = None;
    }
    pub fn operation(&mut self, started: Instant) {
        if self.operations_us.len() < 4096 {
            self.operations_us
                .push(started.elapsed().as_micros() as u64);
        }
    }
    pub fn collect(&mut self, window: &mut Window) {
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
    pub fn snapshot_gpu(&mut self) {
        #[cfg(target_os = "macos")]
        let bytes = metal::Device::system_default().map(|device| device.current_allocated_size());
        #[cfg(not(target_os = "macos"))]
        let bytes: Option<u64> = None;
        self.gpu_samples.push((self.phase.to_string(), bytes));
    }
    pub fn report(&self, mut details: serde_json::Value) {
        let common = serde_json::json!({
            "engine":engine(),"frame_timing_enabled":cfg!(feature="frame-timing"),"validation_phase_markers":true,
            "native_window_geometry":self.geometry.samples,
            "phase_correctness":self.checks,"phase_oracles":self.oracles,"font_family":FONT_FAMILY,
            "font_size_px":FONT_SIZE,"line_height_px":LINE_HEIGHT,"window_width_px":WINDOW_WIDTH,
            "window_height_px":WINDOW_HEIGHT,"theme_mode":"dark","operation_cpu_us":self.operations_us,
            "first_render_us":self.first_render_us,"first_submission_us":self.first_submission_us,
            "render_callback_intervals_us":self.callback_intervals_us,"renders":self.renders,
            "elapsed_us":self.started.elapsed().as_micros(),"draw_cpu_us":self.draw_us,
            "submission_cpu_us":self.submission_us,"gpu_allocated_bytes":self.gpu_samples,
            "submission_scope":"CPU native platform submission, excluding GPU completion/compositor display",
            "input_scope":"paced public component programmatic APIs; no physical pointer, keyboard, AT or IME claim"
        });
        details
            .as_object_mut()
            .unwrap()
            .extend(common.as_object().unwrap().clone());
        println!("KAEL_COMPARISON {details}");
    }
}

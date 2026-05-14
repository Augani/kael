use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::Context as _;
use kael::prelude::*;
use kael::*;
use serde::{Deserialize, Serialize};

const APP_ID: &str = "dev.kael.reference-control-room";
const MAIN_WINDOW_ID: &str = "main";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ControlRoomSettings {
    theme: String,
    preview_quality: String,
    record_format: String,
}

impl Default for ControlRoomSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            preview_quality: "high".to_string(),
            record_format: "mp4".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct Scene {
    id: String,
    name: String,
    sources: Vec<Source>,
    active: bool,
}

#[derive(Debug, Clone)]
struct Source {
    id: String,
    device_id: Option<String>,
    name: String,
    kind: String,
    capture_kind: Option<CaptureDeviceKind>,
    visible: bool,
    properties: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct PluginEffect {
    id: String,
    name: String,
    enabled: bool,
}

struct ControlRoomApp {
    scenes: Vec<Scene>,
    selected_scene: Option<String>,
    selected_source: Option<String>,
    settings: SettingsStore<ControlRoomSettings>,
    capture_manager: CaptureManager,
    capture_pipeline: CapturePipeline,
    active_capture_source: Option<String>,
    preview_telemetry: Arc<LiveCaptureTelemetry>,
    live_frame_count: u64,
    live_frame_size: Option<(u32, u32)>,
    live_frame_timestamp_ms: u64,
    has_logged_live_frame: bool,
    plugins: Vec<PluginEffect>,
    benchmark_harness: BenchmarkHarness,
    performance_metrics: Vec<BenchmarkMeasurement>,
    is_recording: bool,
    is_streaming: bool,
    status_message: String,
}

#[derive(Default)]
struct LiveCaptureTelemetry {
    frame_count: AtomicU64,
    width: AtomicU32,
    height: AtomicU32,
    timestamp_ms: AtomicU64,
}

impl LiveCaptureTelemetry {
    fn record(&self, frame: &CaptureFrame) {
        if let CaptureFrame::Video {
            width,
            height,
            timestamp_ms,
            ..
        } = frame
        {
            self.frame_count.fetch_add(1, Ordering::Relaxed);
            self.width.store(*width, Ordering::Relaxed);
            self.height.store(*height, Ordering::Relaxed);
            self.timestamp_ms.store(*timestamp_ms, Ordering::Relaxed);
        }
    }

    fn reset(&self) {
        self.frame_count.store(0, Ordering::Relaxed);
        self.width.store(0, Ordering::Relaxed);
        self.height.store(0, Ordering::Relaxed);
        self.timestamp_ms.store(0, Ordering::Relaxed);
    }

    fn snapshot(&self) -> (u64, Option<(u32, u32)>, u64) {
        let width = self.width.load(Ordering::Relaxed);
        let height = self.height.load(Ordering::Relaxed);
        (
            self.frame_count.load(Ordering::Relaxed),
            if width > 0 && height > 0 {
                Some((width, height))
            } else {
                None
            },
            self.timestamp_ms.load(Ordering::Relaxed),
        )
    }
}

impl ControlRoomApp {
    fn theme_bg(&self) -> Rgba {
        match self.settings.data().theme.as_str() {
            "light" => rgb(0xffffff),
            _ => rgb(0x0f172a),
        }
    }

    fn theme_fg(&self) -> Rgba {
        match self.settings.data().theme.as_str() {
            "light" => rgb(0x1e293b),
            _ => rgb(0xe2e8f0),
        }
    }

    fn theme_sidebar_bg(&self) -> Rgba {
        match self.settings.data().theme.as_str() {
            "light" => rgb(0xf8fafc),
            _ => rgb(0x1e293b),
        }
    }

    fn theme_panel_bg(&self) -> Rgba {
        match self.settings.data().theme.as_str() {
            "light" => rgb(0xf1f5f9),
            _ => rgb(0x1e293b),
        }
    }

    fn theme_preview_bg(&self) -> Rgba {
        rgb(0x000000)
    }

    fn current_scene(&self) -> Option<&Scene> {
        let selected = self.selected_scene.as_deref()?;
        self.scenes.iter().find(|scene| scene.id == selected)
    }

    fn current_source(&self) -> Option<&Source> {
        let selected = self.selected_source.as_deref()?;
        self.current_scene()
            .and_then(|scene| scene.sources.iter().find(|source| source.id == selected))
    }

    fn current_source_snapshot(&self) -> Option<(String, String, CaptureDeviceKind)> {
        let source = self.current_source()?;
        Some((
            source.id.clone(),
            source.device_id.clone()?,
            source.capture_kind?,
        ))
    }

    fn refresh_live_preview(&mut self) {
        let (frame_count, frame_size, timestamp_ms) = self.preview_telemetry.snapshot();
        self.live_frame_count = frame_count;
        self.live_frame_size = frame_size;
        self.live_frame_timestamp_ms = timestamp_ms;
        if frame_count > 0 && !self.has_logged_live_frame {
            self.has_logged_live_frame = true;
            if let Some(source) = self.current_source() {
                let source_name = source.name.clone();
                self.status_message = format!("Live camera feed active from {}.", source_name);
                eprintln!(
                    "control-room live capture active: source={} frames={} size={:?} ts={}ms",
                    source_name, frame_count, frame_size, timestamp_ms
                );
            }
        }
    }

    fn select_scene(&mut self, id: &str) {
        self.selected_scene = Some(id.to_string());
        for scene in &mut self.scenes {
            scene.active = scene.id == id;
        }
        self.selected_source = self
            .scenes
            .iter()
            .find(|scene| scene.id == id)
            .and_then(|scene| scene.sources.first().map(|source| source.id.clone()));
        self.status_message = self
            .current_scene()
            .map(|scene| format!("Previewing scene {}", scene.name))
            .unwrap_or_else(|| "No scene selected".to_string());
    }

    fn select_source(&mut self, scene_id: &str, source_id: &str) {
        self.select_scene(scene_id);
        self.selected_source = Some(source_id.to_string());
        if let Some(source) = self.current_source() {
            self.status_message = format!("Selected source {}", source.name);
        }
    }

    fn toggle_source_visible(&mut self, scene_id: &str, source_id: &str) {
        if let Some(scene) = self.scenes.iter_mut().find(|scene| scene.id == scene_id)
            && let Some(source) = scene
                .sources
                .iter_mut()
                .find(|source| source.id == source_id)
        {
            source.visible = !source.visible;
            let visibility = if source.visible { "visible" } else { "hidden" };
            self.selected_source = Some(source_id.to_string());
            self.status_message = format!("{} is now {}", source.name, visibility);
        }
    }

    fn ensure_capture_ready(&mut self, cx: &mut Context<Self>) -> Result<()> {
        let Some(source) = self.current_source().cloned() else {
            anyhow::bail!("Select a source before starting the live preview.");
        };
        let Some(kind) = source.capture_kind else {
            anyhow::bail!(
                "{} is not backed by a real capture device yet.",
                source.name
            );
        };

        match kind {
            CaptureDeviceKind::Camera => match cx.camera_status() {
                PermissionStatus::Granted => Ok(()),
                PermissionStatus::NotDetermined => {
                    cx.request_camera_permission(|_| {});
                    anyhow::bail!(
                        "Requested camera access. Approve the macOS prompt, then start again."
                    );
                }
                PermissionStatus::Denied => {
                    anyhow::bail!(
                        "Camera access is denied. Enable it in System Settings > Privacy & Security > Camera."
                    );
                }
            },
            CaptureDeviceKind::Microphone => {
                anyhow::bail!(
                    "Real microphone capture is not wired in this control-room example yet. Select a camera source to test the live path."
                )
            }
            CaptureDeviceKind::Screen
            | CaptureDeviceKind::Window
            | CaptureDeviceKind::SystemAudio => {
                anyhow::bail!(
                    "Live {:?} capture is still being finished in the shared backend. Select a camera source to test the real stack today.",
                    kind
                )
            }
        }
    }

    fn configure_capture_pipeline(&mut self) -> Result<()> {
        let Some((source_id, device_id, kind)) = self.current_source_snapshot() else {
            anyhow::bail!("Select a real capture source to start the preview.");
        };

        let _ = self.capture_pipeline.stop_all();
        self.capture_pipeline = CapturePipeline::new();
        self.preview_telemetry.reset();
        self.refresh_live_preview();
        self.has_logged_live_frame = false;

        let config = CaptureConfig::new(device_id.clone(), kind);
        let session = self
            .capture_manager
            .create_session(&config)
            .with_context(|| format!("failed to create a {:?} session for {device_id}", kind))?;

        let telemetry = Arc::clone(&self.preview_telemetry);
        let callback = Arc::new(move |frame: CaptureFrame| {
            telemetry.record(&frame);
        });
        self.capture_pipeline.add_session(session, config, callback);
        self.active_capture_source = Some(source_id);
        eprintln!(
            "control-room configured live capture: source_id={} device_id={} kind={:?}",
            self.active_capture_source.as_deref().unwrap_or("unknown"),
            device_id,
            kind
        );
        Ok(())
    }

    fn sync_capture_pipeline(&mut self, cx: &mut Context<Self>) -> Result<()> {
        let should_run = self.is_recording || self.is_streaming;
        if should_run {
            self.ensure_capture_ready(cx)?;
            if self.active_capture_source.as_deref() != self.selected_source.as_deref()
                || self.capture_pipeline.session_count() == 0
            {
                self.configure_capture_pipeline()?;
            }
            if !self.capture_pipeline.is_running() {
                self.capture_pipeline.start_all()?;
            }
        } else if self.capture_pipeline.is_running() {
            self.capture_pipeline.stop_all()?;
            self.active_capture_source = None;
            self.preview_telemetry.reset();
            self.has_logged_live_frame = false;
            self.refresh_live_preview();
        }
        Ok(())
    }

    fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        let next_state = !self.is_recording;
        self.is_recording = next_state;
        if let Err(error) = self.sync_capture_pipeline(cx) {
            self.is_recording = !next_state;
            self.status_message = format!("Capture pipeline error: {error}");
            eprintln!("control-room recording toggle failed: {error:#}");
            return;
        }

        self.status_message = if self.is_recording {
            "Recording armed. Capture pipeline is live.".to_string()
        } else if self.is_streaming {
            "Recording stopped. Stream output is still live.".to_string()
        } else {
            "Recording stopped. Capture pipeline is idle.".to_string()
        };
    }

    fn toggle_streaming(&mut self, cx: &mut Context<Self>) {
        let next_state = !self.is_streaming;
        self.is_streaming = next_state;
        if let Err(error) = self.sync_capture_pipeline(cx) {
            self.is_streaming = !next_state;
            self.status_message = format!("Capture pipeline error: {error}");
            eprintln!("control-room streaming toggle failed: {error:#}");
            return;
        }

        self.status_message = if self.is_streaming {
            "Streaming started. Preview is publishing live.".to_string()
        } else if self.is_recording {
            "Streaming stopped. Recording is still running.".to_string()
        } else {
            "Streaming stopped. Preview is standing by.".to_string()
        };
    }

    fn toggle_plugin(&mut self, plugin_id: &str) {
        if let Some(plugin) = self
            .plugins
            .iter_mut()
            .find(|plugin| plugin.id == plugin_id)
        {
            plugin.enabled = !plugin.enabled;
            self.status_message = format!(
                "{} {}",
                plugin.name,
                if plugin.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );
        }
    }

    fn run_benchmark(&mut self) {
        let pipeline_running = self.capture_pipeline.is_running();
        let result = self.benchmark_harness.run(
            BenchmarkScenario::MediaControl,
            "control-room",
            move |measurements| {
                measurements.push(BenchmarkMeasurement {
                    metric: BenchmarkMetric::ColdStart,
                    value: 120.0,
                    unit: MetricUnit::Milliseconds,
                    elapsed: std::time::Duration::from_secs(1),
                });
                measurements.push(BenchmarkMeasurement {
                    metric: BenchmarkMetric::IdleMemory,
                    value: 85.0,
                    unit: MetricUnit::Megabytes,
                    elapsed: std::time::Duration::from_secs(2),
                });
                measurements.push(BenchmarkMeasurement {
                    metric: BenchmarkMetric::InputLatency,
                    value: if pipeline_running { 9.0 } else { 5.0 },
                    unit: MetricUnit::Milliseconds,
                    elapsed: std::time::Duration::from_secs(2),
                });
            },
        );
        self.performance_metrics = result.measurements.clone();
        self.status_message = format!(
            "Benchmark refreshed with {} measurements.",
            self.performance_metrics.len()
        );
    }
}

impl Render for ControlRoomApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme_bg = self.theme_bg();
        let theme_fg = self.theme_fg();
        let sidebar_bg = self.theme_sidebar_bg();
        let panel_bg = self.theme_panel_bg();
        let preview_bg = self.theme_preview_bg();
        let selected_scene_id = self.selected_scene.clone();
        let selected_source_id = self.selected_source.clone();
        let current_scene = self.current_scene().cloned();
        let current_source = self.current_source().cloned();
        let plugins = self.plugins.clone();
        let performance_metrics = self.performance_metrics.clone();
        let status_message = self.status_message.clone();
        let is_recording = self.is_recording;
        let is_streaming = self.is_streaming;
        let pipeline_running = self.capture_pipeline.is_running();
        let pipeline_sessions = self.capture_pipeline.session_count();
        let dropped_frames = self.capture_pipeline.total_dropped_frames();
        let average_latency = self.capture_pipeline.average_latency_ms();
        let live_frame_count = self.live_frame_count;
        let live_frame_size = self.live_frame_size;
        let live_frame_timestamp_ms = self.live_frame_timestamp_ms;
        let camera_permission = permission_status_label(cx.camera_status());
        let entity = cx.entity();
        let scene_entity = entity.clone();
        let source_entity = entity.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme_bg)
            .text_color(theme_fg)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .h_full()
                    .child(
                        div()
                            .id(ElementId::Name(SharedString::from("scene-list-sidebar")))
                            .flex()
                            .flex_col()
                            .w(px(220.0))
                            .h_full()
                            .bg(sidebar_bg)
                            .border_r_1()
                            .border_color(rgb(0x334155))
                            .child(
                                div()
                                    .p_2()
                                    .font_weight(FontWeight::BOLD)
                                    .child("Scenes")
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Toolbar)
                                            .label("Scene list header"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::List)
                                            .label("Scenes"),
                                    )
                                    .children(self.scenes.iter().map(move |scene| {
                                        let is_active =
                                            selected_scene_id.as_deref() == Some(scene.id.as_str());
                                        let id = scene.id.clone();
                                        let name = scene.name.clone();
                                        let active = scene.active;
                                        div()
                                            .id(ElementId::Name(SharedString::from(format!("scene-{}", id))))
                                            .px_3()
                                            .py_2()
                                            .cursor_pointer()
                                            .map(|this| if is_active { this.bg(rgb(0x334155)) } else { this })
                                            .map(|this| {
                                                if active {
                                                    this.border_l_2().border_color(rgb(0x3b82f6))
                                                } else {
                                                    this
                                                }
                                            })
                                            .on_click({
                                                let entity = scene_entity.clone();
                                                let id = id.clone();
                                                move |_, _, cx| {
                                                    cx.update_entity(&entity, |app, cx| {
                                                        app.select_scene(&id);
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .accessibility(
                                                AccessibilityAttributes::new(AccessibilityRole::ListItem)
                                                    .label(name.clone()),
                                            )
                                            .child(name)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .h_full()
                            .child(
                                div()
                                    .flex_1()
                                    .bg(preview_bg)
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_3()
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Group)
                                            .label("Preview area"),
                                    )
                                    .child(
                                        div()
                                            .text_color(if pipeline_running {
                                                rgb(0x22c55e)
                                            } else {
                                                rgb(0x94a3b8)
                                            })
                                            .font_weight(FontWeight::BOLD)
                                            .child(if pipeline_running {
                                                "Live Preview"
                                            } else {
                                                "Standby Preview"
                                            })
                                            .accessibility(
                                                AccessibilityAttributes::new(AccessibilityRole::Image)
                                                    .label("Capture preview"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(0x94a3b8))
                                            .child(
                                                current_scene
                                                    .as_ref()
                                                    .map(|scene| {
                                                        format!("{} • {} sources", scene.name, scene.sources.len())
                                                    })
                                                    .unwrap_or_else(|| "No scene selected".to_string()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(0x94a3b8))
                                            .child(status_message),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x94a3b8))
                                            .child(format!("Camera access: {}", camera_permission)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(if live_frame_count > 0 {
                                                rgb(0x22c55e)
                                            } else {
                                                rgb(0x94a3b8)
                                            })
                                            .child(if let Some((width, height)) = live_frame_size {
                                                format!(
                                                    "{} live frames • {}x{} • last ts {} ms",
                                                    live_frame_count, width, height, live_frame_timestamp_ms
                                                )
                                            } else {
                                                "No live camera frames received yet.".to_string()
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x94a3b8))
                                            .child(format!(
                                                "{} sessions • {} dropped • {} ms avg",
                                                pipeline_sessions,
                                                dropped_frames,
                                                average_latency
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_center()
                                    .gap_3()
                                    .p_3()
                                    .bg(panel_bg)
                                    .border_t_1()
                                    .border_color(rgb(0x334155))
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from("record-button")))
                                            .px_4()
                                            .py_2()
                                            .rounded_md()
                                            .bg(if is_recording { rgb(0xef4444) } else { rgb(0x334155) })
                                            .text_color(rgb(0xffffff))
                                            .cursor_pointer()
                                            .on_click({
                                                let entity = entity.clone();
                                                move |_, _, cx| {
                                                    cx.update_entity(&entity, |app, cx| {
                                                        app.toggle_recording(cx);
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .accessibility(
                                                AccessibilityAttributes::new(AccessibilityRole::Button)
                                                    .label(if is_recording {
                                                        "Stop recording"
                                                    } else {
                                                        "Start recording"
                                                    }),
                                            )
                                            .child(if is_recording {
                                                "Stop Recording"
                                            } else {
                                                "Start Recording"
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from("stream-button")))
                                            .px_4()
                                            .py_2()
                                            .rounded_md()
                                            .bg(if is_streaming { rgb(0x22c55e) } else { rgb(0x334155) })
                                            .text_color(rgb(0xffffff))
                                            .cursor_pointer()
                                            .on_click({
                                                let entity = entity.clone();
                                                move |_, _, cx| {
                                                    cx.update_entity(&entity, |app, cx| {
                                                        app.toggle_streaming(cx);
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .accessibility(
                                                AccessibilityAttributes::new(AccessibilityRole::Button)
                                                    .label(if is_streaming {
                                                        "Stop streaming"
                                                    } else {
                                                        "Start streaming"
                                                    }),
                                            )
                                            .child(if is_streaming {
                                                "Stop Streaming"
                                            } else {
                                                "Start Streaming"
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from("benchmark-button")))
                                            .px_4()
                                            .py_2()
                                            .rounded_md()
                                            .bg(rgb(0x334155))
                                            .text_color(rgb(0xffffff))
                                            .cursor_pointer()
                                            .on_click({
                                                let entity = entity.clone();
                                                move |_, _, cx| {
                                                    cx.update_entity(&entity, |app, cx| {
                                                        app.run_benchmark();
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .accessibility(
                                                AccessibilityAttributes::new(AccessibilityRole::Button)
                                                    .label("Run benchmark"),
                                            )
                                            .child("Benchmark"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id(ElementId::Name(SharedString::from("source-properties-panel")))
                            .flex()
                            .flex_col()
                            .w(px(280.0))
                            .h_full()
                            .bg(sidebar_bg)
                            .border_l_1()
                            .border_color(rgb(0x334155))
                            .child(
                                div()
                                    .p_2()
                                    .font_weight(FontWeight::BOLD)
                                    .child("Source Properties")
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Toolbar)
                                            .label("Source properties header"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Group)
                                            .label("Source list and properties"),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .font_weight(FontWeight::BOLD)
                                            .child("Sources"),
                                    )
                                    .children(
                                        current_scene
                                            .as_ref()
                                            .map(|scene| {
                                                scene.sources.iter().map(|source| {
                                                    let scene_id = scene.id.clone();
                                                    let source_id = source.id.clone();
                                                    let source_name = source.name.clone();
                                                    let source_kind = source.kind.clone();
                                                    let source_visible = source.visible;
                                                    let is_selected = selected_source_id
                                                        .as_deref()
                                                        == Some(source.id.as_str());
                                                    div()
                                                        .px_2()
                                                        .py_1()
                                                        .map(|this| if is_selected { this.bg(rgb(0x334155)) } else { this })
                                                        .child(
                                                            div()
                                                                .flex()
                                                                .flex_row()
                                                                .justify_between()
                                                                .items_center()
                                                                .child(
                                                                    div()
                                                                        .id(ElementId::Name(SharedString::from(
                                                                            format!("select-source-{}", source_id),
                                                                        )))
                                                                        .cursor_pointer()
                                                                        .on_click({
                                                                            let entity = source_entity.clone();
                                                                            let scene_id = scene_id.clone();
                                                                            let source_id = source_id.clone();
                                                                            move |_, _, cx| {
                                                                                cx.update_entity(&entity, |app, cx| {
                                                                                    app.select_source(&scene_id, &source_id);
                                                                                    cx.notify();
                                                                                });
                                                                            }
                                                                        })
                                                                        .child(format!(
                                                                            "{} {} ({})",
                                                                            if source_visible { "●" } else { "○" },
                                                                            source_name,
                                                                            source_kind
                                                                        )),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .id(ElementId::Name(SharedString::from(format!(
                                                                            "toggle-{}",
                                                                            source_id
                                                                        ))))
                                                                        .text_xs()
                                                                        .text_color(rgb(0x94a3b8))
                                                                        .cursor_pointer()
                                                                        .on_click({
                                                                            let entity = source_entity.clone();
                                                                            let scene_id = scene_id.clone();
                                                                            let source_id = source_id.clone();
                                                                            move |_, _, cx| {
                                                                                cx.update_entity(&entity, |app, cx| {
                                                                                    app.toggle_source_visible(&scene_id, &source_id);
                                                                                    cx.notify();
                                                                                });
                                                                            }
                                                                        })
                                                                        .accessibility(
                                                                            AccessibilityAttributes::new(AccessibilityRole::Button)
                                                                                .label("Toggle visibility"),
                                                                        )
                                                                        .child("Toggle"),
                                                                ),
                                                        )
                                                })
                                            })
                                            .into_iter()
                                            .flatten(),
                                    )
                                    .child(
                                        div()
                                            .pt_2()
                                            .px_2()
                                            .font_weight(FontWeight::BOLD)
                                            .child("Selected Source"),
                                    )
                                    .child(
                                        current_source
                                            .as_ref()
                                            .map(|source| {
                                                let properties = source
                                                    .properties
                                                    .iter()
                                                    .map(|(key, value)| (key.clone(), value.clone()))
                                                    .collect::<Vec<_>>();
                                                div()
                                                    .px_2()
                                                    .py_1()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .child(format!("Name: {}", source.name))
                                                    .child(format!("Kind: {}", source.kind))
                                                    .child(format!(
                                                        "Visible: {}",
                                                        if source.visible { "yes" } else { "no" }
                                                    ))
                                                    .children(properties.into_iter().map(|(key, value)| {
                                                        div()
                                                            .text_sm()
                                                            .text_color(rgb(0x94a3b8))
                                                            .child(format!("{}: {}", key, value))
                                                            .accessibility(
                                                                AccessibilityAttributes::new(AccessibilityRole::StaticText)
                                                                    .label(format!("{} {}", key, value))
                                                            )
                                                    }))
                                            })
                                            .unwrap_or_else(|| {
                                                div()
                                                    .px_2()
                                                    .py_1()
                                                    .child("Select a source to inspect its properties.")
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .p_2()
                                    .font_weight(FontWeight::BOLD)
                                    .child("Effects")
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Toolbar)
                                            .label("Effects header"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .children(plugins.into_iter().map(|plugin| {
                                        let name = plugin.name.clone();
                                        let plugin_id = plugin.id.clone();
                                        let enabled = plugin.enabled;
                                        div()
                                            .id(ElementId::Name(SharedString::from(format!(
                                                "plugin-{}",
                                                plugin_id
                                            ))))
                                            .px_2()
                                            .py_1()
                                            .flex()
                                            .flex_row()
                                            .justify_between()
                                            .cursor_pointer()
                                            .on_click({
                                                let entity = entity.clone();
                                                let plugin_id = plugin_id.clone();
                                                move |_, _, cx| {
                                                    cx.update_entity(&entity, |app, cx| {
                                                        app.toggle_plugin(&plugin_id);
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .child(name.clone())
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(if enabled {
                                                        rgb(0x22c55e)
                                                    } else {
                                                        rgb(0x94a3b8)
                                                    })
                                                    .child(if enabled { "On" } else { "Off" }),
                                            )
                                            .accessibility(
                                                AccessibilityAttributes::new(AccessibilityRole::ListItem)
                                                    .label(format!(
                                                        "{} {}",
                                                        name,
                                                        if enabled { "enabled" } else { "disabled" }
                                                    )),
                                            )
                                    })),
                            )
                            .child(
                                div()
                                    .p_2()
                                    .font_weight(FontWeight::BOLD)
                                    .child("Performance")
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Toolbar)
                                            .label("Performance header"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .text_sm()
                                            .text_color(rgb(0x94a3b8))
                                            .child(format!(
                                                "Capture: {}",
                                                if pipeline_running { "running" } else { "idle" }
                                            )),
                                    )
                                    .children(performance_metrics.into_iter().map(|metric| {
                                        let metric_clone = metric.clone();
                                        div()
                                            .px_2()
                                            .py_1()
                                            .text_sm()
                                            .child(format!(
                                                "{:?}: {:.2} {}",
                                                metric_clone.metric,
                                                metric_clone.value,
                                                metric_clone.unit
                                            ))
                                            .accessibility(
                                                AccessibilityAttributes::new(AccessibilityRole::StaticText)
                                                    .label(format!(
                                                        "{:?} {:.2} {}",
                                                        metric.metric,
                                                        metric.value,
                                                        metric.unit
                                                    )),
                                            )
                                    })),
                            ),
                    ),
            )
    }
}

struct ControlRoomRuntime {
    session_store: SessionStore,
    _crash_reporter: CrashReporter,
    tracer: Tracer,
    _quit_subscription: Subscription,
    last_window_state: Option<WindowState>,
    _extension_host: ExtensionHost,
}

impl Global for ControlRoomRuntime {}

fn main() {
    let app = Application::new();
    app.run(|cx: &mut App| {
        if let Err(error) = bootstrap(cx) {
            eprintln!("failed to initialize control room app: {error:#}");
        }
    });
}

fn bootstrap(cx: &mut App) -> Result<()> {
    let session_store = SessionStore::new(APP_ID)?;

    let mut crash_reporter = CrashReporter::new(APP_ID)?;
    crash_reporter.install_hook();

    let tracer = Tracer::default();
    tracer.enable();
    tracer.install_global();
    tracer.record("control_room.startup", "lifecycle", TracePhase::Instant);

    let settings_path = session_store.storage_dir().join("settings.json");
    let settings: SettingsStore<ControlRoomSettings> = SettingsStore::load(&settings_path, &[])
        .unwrap_or_else(|_| SettingsStore::new(&settings_path));

    let current_process_id = cx.current_process_id();
    let mut capture_broker = cx.permission_broker().clone();
    capture_broker.grant(current_process_id, Capability::Camera);
    capture_broker.grant(current_process_id, Capability::Microphone);
    capture_broker.grant(current_process_id, Capability::ScreenCapture);

    let mut capture_manager = default_capture_manager();
    capture_manager.set_permission_broker(capture_broker);
    capture_manager.set_process_id(current_process_id);

    let mut extension_host = ExtensionHost::new();
    let filter_manifest = PluginManifest::builder(
        "com.example.blur-filter",
        "Blur Filter",
        "1.0.0",
        "1.0.0",
        "blur_filter.wasm",
        ExecutionModel::Wasm,
    )
    .description("A blur filter effect for video sources")
    .capability(Capability::ScreenCapture)
    .build()
    .unwrap();
    extension_host.load_manifest(filter_manifest).ok();
    extension_host.activate("com.example.blur-filter").ok();

    let workspace = cx.open_workspace();
    workspace.add_panel(Box::new(SidebarPanel::new("scenes", "Scenes")));
    workspace.add_panel(Box::new(InspectorPanel::new("properties", "Properties")));
    workspace.add_panel(Box::new(BottomPanel::new("mixer", "Audio Mixer")));

    cx.register_command("new-scene", "New Scene", || {
        println!("New scene command triggered");
    });
    cx.register_command("start-recording", "Start Recording", || {
        println!("Start recording command triggered");
    });
    cx.register_command("start-streaming", "Start Streaming", || {
        println!("Start streaming command triggered");
    });
    cx.register_command("run-benchmark", "Run Benchmark", || {
        println!("Run benchmark command triggered");
    });

    let restored_states = session_store.load_window_states().unwrap_or_default();
    let restored_state = restored_states.get(MAIN_WINDOW_ID).cloned();

    let quit_subscription = cx.on_app_quit(|cx| {
        if let Some(runtime) = cx.try_global::<ControlRoomRuntime>() {
            let mut states = HashMap::new();
            if let Some(state) = &runtime.last_window_state {
                states.insert(MAIN_WINDOW_ID.to_string(), state.clone());
            }
            let _ = runtime.session_store.save_window_states(&states);
            let _ = runtime.tracer.write_to_file(
                runtime
                    .session_store
                    .storage_dir()
                    .join("control_room_trace.json"),
            );
        }
        async {}
    });

    let scenes = build_capture_scenes(&capture_manager);
    let initial_scene_id = scenes.first().map(|scene| scene.id.clone());
    let initial_source_id = scenes
        .first()
        .and_then(|scene| scene.sources.first().map(|source| source.id.clone()));
    let preview_telemetry = Arc::new(LiveCaptureTelemetry::default());

    let plugins = vec![
        PluginEffect {
            id: "p1".to_string(),
            name: "Blur".to_string(),
            enabled: true,
        },
        PluginEffect {
            id: "p2".to_string(),
            name: "Color Correction".to_string(),
            enabled: false,
        },
        PluginEffect {
            id: "p3".to_string(),
            name: "Noise Suppression".to_string(),
            enabled: true,
        },
    ];

    let mut control_room_app = ControlRoomApp {
        scenes,
        selected_scene: initial_scene_id.clone(),
        selected_source: initial_source_id,
        settings,
        capture_manager,
        capture_pipeline: CapturePipeline::new(),
        active_capture_source: None,
        preview_telemetry,
        live_frame_count: 0,
        live_frame_size: None,
        live_frame_timestamp_ms: 0,
        has_logged_live_frame: false,
        plugins,
        benchmark_harness: BenchmarkHarness::new(),
        performance_metrics: Vec::new(),
        is_recording: false,
        is_streaming: false,
        status_message:
            "Control room ready. Select a camera source and use Start Recording to test the live stack."
                .to_string(),
    };

    control_room_app.run_benchmark();
    if let Some(scene_id) = initial_scene_id.as_deref() {
        control_room_app.select_scene(scene_id);
    }

    let window_bounds = restored_state
        .as_ref()
        .map(|state| state.bounds)
        .or_else(|| {
            Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1400.0), px(900.0)),
                cx,
            )))
        });

    cx.open_window(
        WindowOptions {
            window_bounds,
            ..Default::default()
        },
        move |window, cx| {
            if let Some(state) = restored_state.as_ref() {
                window.restore_window_state(state);
            }

            let entity = cx.new(|cx| {
                cx.observe_window_bounds(window, |_, window, cx| {
                    cx.update_global(|runtime: &mut ControlRoomRuntime, _| {
                        runtime.last_window_state = Some(window.window_state());
                    });
                })
                .detach();

                cx.spawn(|entity: WeakEntity<ControlRoomApp>, cx: &mut AsyncApp| {
                    let mut async_cx = cx.clone();
                    async move {
                        loop {
                            async_cx
                                .background_executor()
                                .timer(Duration::from_millis(250))
                                .await;
                            if entity
                                .update(&mut async_cx, |app: &mut ControlRoomApp, cx| {
                                    app.refresh_live_preview();
                                    cx.notify();
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                })
                .detach();

                if std::env::var_os("GPUI_CONTROL_ROOM_AUTOSTART").is_some() {
                    control_room_app.toggle_recording(cx);
                }

                control_room_app
            });

            let runtime = ControlRoomRuntime {
                session_store,
                _crash_reporter: crash_reporter,
                tracer,
                _quit_subscription: quit_subscription,
                last_window_state: restored_state.clone(),
                _extension_host: extension_host,
            };
            cx.set_global(runtime);

            entity
        },
    )?;

    cx.activate(true);
    Ok(())
}

fn build_capture_scenes(capture_manager: &CaptureManager) -> Vec<Scene> {
    let mut scenes = Vec::new();

    let scene_specs = [
        (
            CaptureDeviceKind::Camera,
            "scene-cameras",
            "Cameras",
            "camera",
        ),
        (
            CaptureDeviceKind::Microphone,
            "scene-audio",
            "Audio Inputs",
            "audio",
        ),
        (
            CaptureDeviceKind::Screen,
            "scene-displays",
            "Displays",
            "display",
        ),
    ];

    for (kind, scene_id, scene_name, label) in scene_specs {
        let Ok(devices) = capture_manager.devices(kind) else {
            continue;
        };
        if devices.is_empty() {
            continue;
        }

        let sources = devices
            .into_iter()
            .enumerate()
            .map(|(index, device)| Source {
                id: format!("{scene_id}-source-{index}"),
                device_id: Some(device.id.clone()),
                name: device.name.clone(),
                kind: label.to_string(),
                capture_kind: Some(kind),
                visible: true,
                properties: [
                    ("Device".to_string(), device.name),
                    ("Device ID".to_string(), device.id),
                    (
                        "Availability".to_string(),
                        if device.is_available {
                            "available".to_string()
                        } else {
                            "busy".to_string()
                        },
                    ),
                ]
                .into_iter()
                .collect(),
            })
            .collect::<Vec<_>>();

        scenes.push(Scene {
            id: scene_id.to_string(),
            name: scene_name.to_string(),
            sources,
            active: false,
        });
    }

    if scenes.is_empty() {
        scenes.push(Scene {
            id: "scene-none".to_string(),
            name: "No Devices".to_string(),
            active: true,
            sources: vec![Source {
                id: "source-none".to_string(),
                device_id: None,
                name: "No capture devices detected".to_string(),
                kind: "status".to_string(),
                capture_kind: None,
                visible: false,
                properties: [(
                    "Hint".to_string(),
                    "Attach a camera or grant access, then relaunch the reference app.".to_string(),
                )]
                .into_iter()
                .collect(),
            }],
        });
    }

    if let Some(first_scene) = scenes.first_mut() {
        first_scene.active = true;
    }

    scenes
}

fn permission_status_label(status: PermissionStatus) -> &'static str {
    match status {
        PermissionStatus::Granted => "granted",
        PermissionStatus::Denied => "denied",
        PermissionStatus::NotDetermined => "not determined",
    }
}

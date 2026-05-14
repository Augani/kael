use kael::{
    App, Application, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};
use kael::{
    CaptureConfig, CaptureDeviceKind, CaptureFrame, CapturePipeline, TracePhase, Tracer,
    default_capture_manager,
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

struct CaptureDemo {
    status: SharedString,
    frame_count: Arc<AtomicU64>,
    pipeline: CapturePipeline,
}

impl CaptureDemo {
    fn new() -> Self {
        Self {
            status: "Idle".into(),
            frame_count: Arc::new(AtomicU64::new(0)),
            pipeline: CapturePipeline::new(),
        }
    }

    fn refresh_devices(&mut self, kind: CaptureDeviceKind) -> Vec<kael::CaptureDeviceInfo> {
        let manager = default_capture_manager();
        match manager.devices(kind) {
            Ok(devices) => devices,
            Err(err) => {
                self.status = format!("Error listing devices: {}", err).into();
                Vec::new()
            }
        }
    }

    fn start_capture(&mut self, kind: CaptureDeviceKind) {
        let devices = self.refresh_devices(kind);
        if devices.is_empty() {
            self.status = format!("No {:?} devices found", kind).into();
            return;
        }

        let manager = default_capture_manager();
        let device = &devices[0];
        let config = CaptureConfig::new(device.id.clone(), kind);
        let session = match manager.create_session(&config) {
            Ok(session) => session,
            Err(err) => {
                self.status = format!("Failed to create session: {}", err).into();
                return;
            }
        };

        let frame_count = Arc::clone(&self.frame_count);
        let callback = Arc::new(move |_frame: CaptureFrame| {
            frame_count.fetch_add(1, Ordering::Relaxed);
        });

        self.pipeline = CapturePipeline::new();
        self.pipeline.add_session(session, config, callback);

        match self.pipeline.start_all() {
            Ok(()) => {
                self.status = format!("Capturing {:?}", kind).into();
            }
            Err(err) => {
                self.status = format!("Failed to start capture: {}", err).into();
            }
        }
    }

    fn stop_capture(&mut self) {
        let _ = self.pipeline.stop_all();
        self.status = "Stopped".into();
    }

    fn start_soak_test(&mut self, kind: CaptureDeviceKind, duration: Duration) {
        if let Some(tracer) = Tracer::global() {
            tracer.record("capture_soak_test", "capture", TracePhase::Begin);
        }
        self.start_capture(kind);
        self.status = format!("Soak test {:?} for {:?}; use Stop to end", kind, duration).into();
    }
}

impl Render for CaptureDemo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame_count = self.frame_count.load(Ordering::Relaxed);
        let session_count = self.pipeline.session_count();
        let is_running = self.pipeline.is_running();
        let dropped = self.pipeline.total_dropped_frames();
        let latency = self.pipeline.average_latency_ms();

        div()
            .flex()
            .flex_col()
            .gap_3()
            .bg(rgb(0x202020))
            .size(px(600.0))
            .p_4()
            .text_color(rgb(0xffffff))
            .child(div().text_xl().child("Capture Demo"))
            .child(div().child(format!("Status: {}", self.status)))
            .child(div().child(format!(
                "Sessions: {} | Running: {}",
                session_count, is_running
            )))
            .child(div().child(format!(
                "Frames: {} | Dropped: {} | Latency: {}ms",
                frame_count, dropped, latency
            )))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .id("start-screen")
                            .px_3()
                            .py_1()
                            .bg(rgb(0x404040))
                            .rounded_md()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_capture(CaptureDeviceKind::Screen);
                                cx.notify();
                            }))
                            .child("Start Screen"),
                    )
                    .child(
                        div()
                            .id("start-mic")
                            .px_3()
                            .py_1()
                            .bg(rgb(0x404040))
                            .rounded_md()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_capture(CaptureDeviceKind::Microphone);
                                cx.notify();
                            }))
                            .child("Start Mic"),
                    )
                    .child(
                        div()
                            .id("soak-test")
                            .px_3()
                            .py_1()
                            .bg(rgb(0x404040))
                            .rounded_md()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_soak_test(
                                    CaptureDeviceKind::Microphone,
                                    Duration::from_secs(10),
                                );
                                cx.notify();
                            }))
                            .child("Soak 10s"),
                    )
                    .child(
                        div()
                            .id("stop-capture")
                            .px_3()
                            .py_1()
                            .bg(rgb(0x404040))
                            .rounded_md()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.stop_capture();
                                cx.notify();
                            }))
                            .child("Stop"),
                    ),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(600.), px(400.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| CaptureDemo::new()),
        )
        .unwrap();
        cx.activate(true);
    });
}

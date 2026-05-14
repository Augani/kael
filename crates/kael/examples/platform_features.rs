use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context as _, Result};
use kael::*;

const APP_ID: &str = "dev.kael.gpui.platform-features";
const MAIN_WINDOW_ID: &str = "main";
const TRACE_FILE_NAME: &str = "platform_features_trace.json";

struct PlatformDemo {
    lines: Vec<SharedString>,
}

impl Render for PlatformDemo {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_4()
            .p_6()
            .bg(rgb(0x0f172a))
            .text_color(rgb(0xe2e8f0))
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::BOLD)
                    .child("Platform Features Demo"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x94a3b8))
                    .child(
                        "Session restore, crash reporting, tracing, file watching, notifications, workspace, commands, background jobs, and deep links.",
                    ),
            )
            .children(
                self.lines
                    .iter()
                    .cloned()
                    .map(|line| div().text_sm().child(line)),
            )
    }
}

struct DemoRuntime {
    session_store: SessionStore,
    tracer: Tracer,
    trace_path: PathBuf,
    last_window_state: Option<WindowState>,
    _crash_reporter: CrashReporter,
    _quit_subscription: Subscription,
    _restart_subscription: Subscription,
    _watcher: FileWatcher,
}

impl Global for DemoRuntime {}

fn main() {
    let _router = match SingleInstanceRouter::try_acquire(APP_ID) {
        Ok(router) => router,
        Err(_) => {
            let _ = SingleInstanceRouter::send_to_existing(APP_ID);
            std::process::exit(0);
        }
    };

    let app = Application::new();
    app.on_open_urls(|urls| {
        for url in urls {
            println!("Deep link received: {}", url);
        }
    });
    app.run(|cx: &mut App| {
        if let Err(error) = bootstrap(cx) {
            panic!("failed to initialize platform features demo: {error:#}");
        }
    });
}

fn bootstrap(cx: &mut App) -> Result<()> {
    let session_store = SessionStore::new(APP_ID)?;
    let trace_path = session_store.storage_dir().join(TRACE_FILE_NAME);

    let mut crash_reporter = CrashReporter::new(APP_ID)?;
    let crash_reports_dir = crash_reporter.reports_dir().to_path_buf();
    crash_reporter.install_hook();

    let tracer = Tracer::default();
    tracer.enable();
    tracer.install_global();
    tracer.record("platform_features.startup", "demo", TracePhase::Instant);

    let mut watcher = FileWatcher::new(cx, |event| {
        println!("File watch event: {event:?}");
    })?;
    watcher
        .watch(session_store.storage_dir(), true)
        .context("failed to watch session storage directory")?;

    let workspace = cx.open_workspace();
    workspace.add_panel(Box::new(SidebarPanel::new("sidebar", "Sidebar")));
    workspace.add_panel(Box::new(InspectorPanel::new("inspector", "Inspector")));
    workspace.add_panel(Box::new(BottomPanel::new("bottom", "Bottom Panel")));

    cx.register_command("save", "Save", || println!("Save command triggered"));
    cx.register_command("refresh", "Refresh", || {
        println!("Refresh command triggered")
    });

    let _ = cx.schedule_job(LogJob {
        id: "startup-log".to_string(),
        message: "Application started".to_string(),
    });

    let os_info = cx.os_info();
    let network_status = cx.network_status();
    let biometric_status = cx.biometric_status();
    let screen_capture_supported = cx.is_screen_capture_supported();
    let idle_time = cx.system_idle_time();
    let displays = cx.displays();
    let available_display_ids = displays
        .iter()
        .map(|display| display.id())
        .collect::<Vec<_>>();
    let primary_display_id = cx.primary_display().map(|display| display.id());
    let restored_states =
        session_store.restore_window_states(&available_display_ids, primary_display_id)?;
    let restored_state = restored_states.get(MAIN_WINDOW_ID).cloned();

    let restart_subscription = cx.on_app_restart(|cx| persist_runtime_state(cx));
    let quit_subscription = cx.on_app_quit(|cx| {
        persist_runtime_state(cx);
        async {}
    });

    cx.set_global(DemoRuntime {
        session_store,
        tracer,
        trace_path: trace_path.clone(),
        last_window_state: restored_state.clone(),
        _crash_reporter: crash_reporter,
        _quit_subscription: quit_subscription,
        _restart_subscription: restart_subscription,
        _watcher: watcher,
    });

    cx.on_network_status_change(|status, _cx| {
        println!("Network changed: {status:?}");
    });
    cx.on_system_power_event(|event, _cx| {
        println!("Power event: {event:?}");
    });
    cx.on_media_key_event(|event, _cx| {
        println!("Media key event: {event:?}");
    });
    let _ = cx.show_notification_with_actions(
        "GPUI demo ready",
        "Production modules are active for this session.",
        &[NotificationAction {
            id: "open-trace".to_string(),
            label: "Open trace path".to_string(),
        }],
        |action_id| {
            println!("Notification action: {action_id}");
        },
    );

    cx.set_dock_badge(Some("1"));
    cx.request_user_attention(AttentionType::Informational);

    let window_bounds = restored_state
        .as_ref()
        .map(|state| state.bounds)
        .or_else(|| {
            Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(820.0), px(520.0)),
                cx,
            )))
        });

    let lines = build_demo_lines(
        &os_info,
        network_status,
        biometric_status,
        idle_time,
        screen_capture_supported,
        &trace_path,
        &crash_reports_dir,
        cx.global::<DemoRuntime>().session_store.storage_dir(),
        available_display_ids.len(),
        restored_state.is_some(),
    );

    cx.open_window(
        WindowOptions {
            window_bounds,
            ..Default::default()
        },
        move |window, cx| {
            if let Some(state) = restored_state.as_ref() {
                window.restore_window_state(state);
            }

            window.set_progress_bar(ProgressBarState::Normal(0.42));

            cx.new(move |cx| {
                cx.update_global(|runtime: &mut DemoRuntime, _cx| {
                    runtime.last_window_state = Some(window.window_state());
                });
                cx.observe_window_bounds(window, |_, window, cx| {
                    cx.update_global(|runtime: &mut DemoRuntime, _cx| {
                        runtime.last_window_state = Some(window.window_state());
                    });
                })
                .detach();

                PlatformDemo { lines }
            })
        },
    )?;

    cx.activate(true);
    Ok(())
}

fn build_demo_lines(
    os_info: &OsInfo,
    network_status: NetworkStatus,
    biometric_status: BiometricStatus,
    idle_time: Option<std::time::Duration>,
    screen_capture_supported: bool,
    trace_path: &std::path::Path,
    crash_reports_dir: &std::path::Path,
    session_dir: &std::path::Path,
    display_count: usize,
    restored_previous_session: bool,
) -> Vec<SharedString> {
    vec![
        format!(
            "OS: {} {} ({})",
            os_info.name, os_info.version, os_info.arch
        )
        .into(),
        format!(
            "Locale: {} | Hostname: {}",
            os_info.locale, os_info.hostname
        )
        .into(),
        format!("Network: {network_status:?}").into(),
        format!("Biometrics: {biometric_status:?}").into(),
        format!("Idle time: {:?}", idle_time).into(),
        format!("Displays discovered: {display_count}").into(),
        format!("Screen capture supported: {screen_capture_supported}").into(),
        format!("Restored previous session: {restored_previous_session}").into(),
        format!("Session store: {}", session_dir.display()).into(),
        format!("Crash reports: {}", crash_reports_dir.display()).into(),
        format!("Trace export on quit: {}", trace_path.display()).into(),
        "Workspace with panels, command registry, and background jobs active.".into(),
    ]
}

fn persist_runtime_state(cx: &mut App) {
    let maybe_state = cx.global::<DemoRuntime>().last_window_state.clone();

    if let Some(state) = maybe_state {
        let mut states = HashMap::new();
        states.insert(MAIN_WINDOW_ID.to_string(), state);

        if let Err(error) = cx
            .global::<DemoRuntime>()
            .session_store
            .save_window_states(&states)
        {
            eprintln!("failed to save session state: {error:#}");
        }
    }

    let trace_result = {
        let runtime = cx.global::<DemoRuntime>();
        runtime.tracer.write_to_file(&runtime.trace_path)
    };
    if let Err(error) = trace_result {
        eprintln!("failed to write trace file: {error:#}");
    }

    let _ = Tracer::clear_global();
}

#[derive(serde::Serialize)]
struct LogJob {
    id: String,
    message: String,
}

impl BackgroundJob for LogJob {
    type Output = String;
    fn id(&self) -> &str {
        &self.id
    }
}

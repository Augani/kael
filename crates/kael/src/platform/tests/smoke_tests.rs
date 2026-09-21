// Feature: platform-parity-browser-runtime-features, Cross-platform smoke tests

use std::sync::Mutex;

use crate as kael;
use crate::{BiometricStatus, NetworkStatus, PermissionStatus, TraceEvent, TracePhase};

crate::actions!(
    menu_builder_test,
    [
        MenuBuilderOpen,
        MenuBuilderSave,
        MenuBuilderUndo,
        MenuBuilderRedo,
        MenuBuilderCut,
        MenuBuilderCopy,
        MenuBuilderPaste,
        MenuBuilderSelectAll
    ]
);

static PLATFORM_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(target_os = "macos")]
fn lock_platform_test_surface() -> (
    std::sync::MutexGuard<'static, ()>,
    std::sync::MutexGuard<'static, ()>,
) {
    (
        PLATFORM_TEST_LOCK.lock().unwrap(),
        crate::platform::mac::mac_appkit_test_lock().lock().unwrap(),
    )
}

#[cfg(not(target_os = "macos"))]
fn lock_platform_test_surface() -> std::sync::MutexGuard<'static, ()> {
    PLATFORM_TEST_LOCK.lock().unwrap()
}

// WindowKind::{Overlay,Bottom,Wallpaper} and their WindowIntentKind counterparts carry
// LayerShellOptions only under wayland (unit variants otherwise, since no other backend
// reads their payload). Top always carries LayerShellOptions on every platform - X11's
// struts and Windows' AppBar read its exclusive-zone fields too - so it needs no split.
#[cfg(feature = "wayland")]
fn overlay_window_kind() -> crate::WindowKind {
    crate::WindowKind::Overlay(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn overlay_window_kind() -> crate::WindowKind {
    crate::WindowKind::Overlay
}
#[cfg(feature = "wayland")]
fn bottom_window_kind() -> crate::WindowKind {
    crate::WindowKind::Bottom(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn bottom_window_kind() -> crate::WindowKind {
    crate::WindowKind::Bottom
}
#[cfg(feature = "wayland")]
fn wallpaper_window_kind() -> crate::WindowKind {
    crate::WindowKind::Wallpaper(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn wallpaper_window_kind() -> crate::WindowKind {
    crate::WindowKind::Wallpaper
}
fn top_window_kind() -> crate::WindowKind {
    crate::WindowKind::Top(crate::LayerShellOptions::default())
}

#[cfg(feature = "wayland")]
fn overlay_intent_kind() -> crate::WindowIntentKind {
    crate::WindowIntentKind::Overlay(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn overlay_intent_kind() -> crate::WindowIntentKind {
    crate::WindowIntentKind::Overlay
}
#[cfg(feature = "wayland")]
fn bottom_intent_kind() -> crate::WindowIntentKind {
    crate::WindowIntentKind::Bottom(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn bottom_intent_kind() -> crate::WindowIntentKind {
    crate::WindowIntentKind::Bottom
}
#[cfg(feature = "wayland")]
fn wallpaper_intent_kind() -> crate::WindowIntentKind {
    crate::WindowIntentKind::Wallpaper(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn wallpaper_intent_kind() -> crate::WindowIntentKind {
    crate::WindowIntentKind::Wallpaper
}
fn top_intent_kind() -> crate::WindowIntentKind {
    crate::WindowIntentKind::Top(crate::LayerShellOptions::default())
}

// The *(options)/no-arg split on WindowOptionsBuilder/WindowIntentBuilder mirrors the
// kind constructors above: genuinely differs by feature for overlay/bottom/wallpaper,
// but top() always takes LayerShellOptions on every platform.
#[cfg(feature = "wayland")]
fn with_overlay(builder: crate::WindowOptionsBuilder) -> crate::WindowOptionsBuilder {
    builder.overlay(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn with_overlay(builder: crate::WindowOptionsBuilder) -> crate::WindowOptionsBuilder {
    builder.overlay()
}

#[cfg(feature = "wayland")]
fn with_bottom(builder: crate::WindowOptionsBuilder) -> crate::WindowOptionsBuilder {
    builder.bottom(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn with_bottom(builder: crate::WindowOptionsBuilder) -> crate::WindowOptionsBuilder {
    builder.bottom()
}

#[cfg(feature = "wayland")]
fn with_wallpaper(builder: crate::WindowOptionsBuilder) -> crate::WindowOptionsBuilder {
    builder.wallpaper(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn with_wallpaper(builder: crate::WindowOptionsBuilder) -> crate::WindowOptionsBuilder {
    builder.wallpaper()
}

fn with_top(builder: crate::WindowOptionsBuilder) -> crate::WindowOptionsBuilder {
    builder.top(crate::LayerShellOptions::default())
}

#[cfg(feature = "wayland")]
fn overlay_intent_builder() -> crate::WindowIntentBuilder {
    crate::WindowIntentBuilder::overlay(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn overlay_intent_builder() -> crate::WindowIntentBuilder {
    crate::WindowIntentBuilder::overlay()
}

#[cfg(feature = "wayland")]
fn bottom_intent_builder() -> crate::WindowIntentBuilder {
    crate::WindowIntentBuilder::bottom(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn bottom_intent_builder() -> crate::WindowIntentBuilder {
    crate::WindowIntentBuilder::bottom()
}

#[cfg(feature = "wayland")]
fn wallpaper_intent_builder() -> crate::WindowIntentBuilder {
    crate::WindowIntentBuilder::wallpaper(crate::LayerShellOptions::default())
}
#[cfg(not(feature = "wayland"))]
fn wallpaper_intent_builder() -> crate::WindowIntentBuilder {
    crate::WindowIntentBuilder::wallpaper()
}

fn top_intent_builder() -> crate::WindowIntentBuilder {
    crate::WindowIntentBuilder::top(crate::LayerShellOptions::default())
}

#[test]
fn fallible_headless_startup_reports_initialization_result() {
    let _guard = lock_platform_test_surface();
    match crate::Application::try_headless() {
        Ok(application) => drop(application),
        Err(error) => assert!(
            error.to_string().contains("main thread"),
            "startup should only fail here because the test harness is off the main thread: {error:#}"
        ),
    }

    let executor = crate::try_background_executor()
        .expect("background executor initialization should succeed on the test host");
    drop(executor);
}

/// **Validates: Requirements 3.1**
///
/// Verify that `biometric_status` returns a valid enum variant on all
/// platforms without panicking.
#[test]
fn biometric_status_returns_valid_variant() {
    let _guard = lock_platform_test_surface();
    let platform = crate::platform::current_platform(true);
    let status = platform.biometric_status();
    assert!(
        matches!(
            status,
            BiometricStatus::Available(_) | BiometricStatus::Unavailable
        ),
        "biometric_status should return a valid BiometricStatus variant"
    );
}

/// **Validates: Requirements 8.1**
///
/// Verify that `microphone_status` returns a valid enum variant on all
/// platforms without panicking.
#[test]
fn microphone_status_returns_valid_variant() {
    let _guard = lock_platform_test_surface();
    let platform = crate::platform::current_platform(true);
    let status = platform.microphone_status();
    assert!(
        matches!(
            status,
            PermissionStatus::Granted | PermissionStatus::Denied | PermissionStatus::NotDetermined
        ),
        "microphone_status should return a valid PermissionStatus variant"
    );
}

/// **Validates: Requirements 25.3, 25.4**
///
/// Verify that `HasWindowHandle` / `HasDisplayHandle` don't panic on any
/// platform. We test this indirectly by creating a headless platform and
/// ensuring its window-related operations are safe.
#[test]
fn window_handles_do_not_panic() {
    let _guard = lock_platform_test_surface();
    let platform = crate::platform::current_platform(true);
    // The headless platform does not support windows, but querying it should
    // not panic.
    let _ = platform.active_window();
    let _ = platform.window_stack();
    let _ = platform.cursor_position();
    let _ = platform.displays();
    let _ = platform.primary_display();
}

/// Verify the cross-platform `PlatformDisplay::refresh_rate()` contract: the
/// test backend reports a concrete rate, and any reported rate is positive and
/// finite. Platform backends that cannot determine a rate return `None` rather
/// than a bogus value.
#[test]
fn display_refresh_rate_is_plausible_or_none() {
    use crate::PlatformDisplay;

    let test_display = crate::platform::test::TestDisplay::new();
    assert_eq!(test_display.refresh_rate(), Some(60.0));

    let _guard = lock_platform_test_surface();
    let platform = crate::platform::current_platform(true);
    for display in platform.displays() {
        if let Some(rate) = display.refresh_rate() {
            assert!(
                rate > 0.0 && rate.is_finite(),
                "refresh rate must be positive and finite, got {rate}"
            );
        }
    }
}

/// The default `PlatformDisplay::refresh_rate()` implementation returns `None`
/// so backends that do not override it never report a fabricated rate.
#[test]
fn display_refresh_rate_defaults_to_none() {
    use crate::{Bounds, DisplayId, Pixels, PlatformDisplay, Point, px};

    #[derive(Debug)]
    struct BareDisplay;
    impl PlatformDisplay for BareDisplay {
        fn id(&self) -> DisplayId {
            DisplayId(0)
        }
        fn uuid(&self) -> anyhow::Result<uuid::Uuid> {
            Ok(uuid::Uuid::nil())
        }
        fn bounds(&self) -> Bounds<Pixels> {
            Bounds::from_corners(Point::default(), Point::new(px(1.0), px(1.0)))
        }
    }

    assert_eq!(BareDisplay.refresh_rate(), None);
}

/// **Validates: Requirements 12.3**
///
/// Verify that `hide_other_apps` doesn't panic on Windows (or any platform).
#[test]
fn hide_other_apps_does_not_panic() {
    let _guard = lock_platform_test_surface();
    let platform = crate::platform::current_platform(true);
    platform.hide_other_apps();
}

/// **Validates: Requirements 8.1**
///
/// Verify that network status returns a valid variant.
#[test]
fn network_status_returns_valid_variant() {
    let _guard = lock_platform_test_surface();
    let platform = crate::platform::current_platform(true);
    let status = platform.network_status();
    assert!(
        matches!(status, NetworkStatus::Online | NetworkStatus::Offline),
        "network_status should return a valid NetworkStatus variant"
    );
}

/// **Validates: Requirements 15.1**
///
/// Verify that the auto-updater module types can be constructed and
/// serialized without panic.
#[cfg(feature = "auto-update")]
#[test]
fn auto_updater_types_are_safe() {
    let config = crate::AutoUpdaterConfig {
        feed_url: "https://example.com/feed.json".to_string(),
        check_interval: std::time::Duration::from_secs(3600),
        allow_prerelease: false,
    };
    let json = serde_json::to_string(&config);
    assert!(json.is_ok());
}

/// Verify checked media-source construction rejects common generated-player
/// mistakes before playback starts.
#[cfg(feature = "media")]
#[test]
fn checked_media_source_builder_validates_inputs() {
    use crate::media_playback::{MediaSource, MediaSourceBuilder};
    use std::{io::Cursor, sync::Arc};

    let source = MediaSourceBuilder::url("https://cdn.example.com/movie.mp4")
        .build_checked()
        .unwrap();
    assert!(matches!(source, MediaSource::Url(_)));

    assert!(
        MediaSourceBuilder::url(" https://cdn.example.com/movie.mp4")
            .validate()
            .is_err()
    );
    assert!(
        MediaSourceBuilder::url("ftp://cdn.example.com/movie.mp4")
            .validate()
            .is_err()
    );
    assert!(
        MediaSourceBuilder::file("")
            .require_existing_file()
            .validate()
            .is_err()
    );
    assert!(
        MediaSourceBuilder::file("/definitely/not/a/movie.mp4")
            .require_existing_file()
            .validate()
            .is_err()
    );
    assert!(
        MediaSourceBuilder::bytes(Arc::<[u8]>::from([]))
            .validate()
            .is_err()
    );
    assert!(
        MediaSourceBuilder::reader("", || Ok(Cursor::new(Vec::<u8>::new())))
            .validate()
            .is_err()
    );

    let controller = MediaSourceBuilder::bytes(Arc::<[u8]>::from([1, 2, 3]))
        .controller_checked()
        .unwrap();
    assert!(matches!(controller.source(), MediaSource::Bytes(_)));
}

/// Verify WebView-hosted browser-video fallback options can be checked before
/// embedding generated HTML.
#[cfg(feature = "media")]
#[test]
fn webview_video_options_validate_before_embedding() {
    use crate::media_playback::{WebViewVideoOptions, WebViewVideoTextTrack};

    let options = WebViewVideoOptions::default()
        .poster("https://cdn.example.com/poster.jpg")
        .controls_list(["nodownload", "nofullscreen"])
        .object_fit("cover")
        .text_track(
            WebViewVideoTextTrack::webvtt(
                "English",
                Some("en"),
                "https://cdn.example.com/captions.vtt",
            )
            .default(true),
        )
        .webvtt_text_track(
            "Inline",
            Some("en-inline"),
            "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nHello",
        );

    assert!(options.validate().is_ok());
    assert!(options.checked().is_ok());
    assert!(
        WebViewVideoOptions::default()
            .poster("javascript:alert(1)")
            .validate()
            .is_err()
    );
    assert!(
        WebViewVideoOptions::default()
            .controls_list(["no download"])
            .validate()
            .is_err()
    );
    assert!(
        WebViewVideoOptions::default()
            .object_fit("cover;position:absolute")
            .validate()
            .is_err()
    );
    assert!(
        WebViewVideoOptions::default()
            .text_track(WebViewVideoTextTrack::webvtt(
                "",
                Some("en"),
                "https://cdn.example.com/captions.vtt",
            ))
            .validate()
            .is_err()
    );
    assert!(
        WebViewVideoOptions::default()
            .text_track(WebViewVideoTextTrack::webvtt(
                "English",
                Some("en"),
                "javascript:alert(1)",
            ))
            .validate()
            .is_err()
    );
}

/// **Validates: Requirements 16.1**
///
/// Verify that the file watcher module types are safe to construct.
#[test]
fn file_watcher_types_are_safe() {
    let options = crate::FileWatchOptions::recursive();
    assert!(options.recursive);
    assert_eq!(options.max_depth, None);
}

/// **Validates: Requirements 18.1**
///
/// Verify that the session store can compute a storage directory without
/// panic.
#[test]
fn session_store_storage_dir_computes() {
    // SessionStore::new may fail if the platform data dir cannot be resolved,
    // but it should not panic.
    let _ = crate::SessionStore::new("test-smoke-app");
}

/// **Validates: Requirements 26.1**
///
/// Verify that the crash reporter can be constructed without panic.
#[test]
fn crash_reporter_constructs() {
    let _ = crate::CrashReporter::new("test-smoke-app");
    let temp_dir = std::env::temp_dir().join(format!(
        "kael_crash_smoke_{}_{}",
        std::process::id(),
        "builder"
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let reporter = crate::CrashReporterBuilder::new("test-smoke-app")
        .reports_dir(&temp_dir)
        .endpoint("https://crashes.example.test/reports")
        .build_checked()
        .unwrap();

    assert_eq!(reporter.reports_dir(), temp_dir.as_path());
    assert!(reporter.reports_dir().exists());
    let _ = std::fs::remove_dir_all(&temp_dir);
}

/// **Validates: Requirements 27.1**
///
/// Verify that the tracer can be enabled and disabled without panic.
#[test]
fn tracer_enable_disable() {
    let tracer = crate::Tracer::new(100);
    tracer.enable();
    assert!(tracer.is_enabled());
    tracer.record("test_event", "smoke", TracePhase::Instant);
    tracer.disable();
    assert!(!tracer.is_enabled());
}

/// **Validates: Requirements 27.2**
///
/// Verify that a trace event exports to valid Chrome Trace Event JSON.
#[test]
fn trace_event_exports_valid_json() {
    let event = TraceEvent {
        name: "smoke_test".to_string(),
        category: "test".to_string(),
        phase: TracePhase::Begin,
        timestamp_us: 0,
        process_id: 1,
        thread_id: 1,
        duration_us: None,
        args: None,
    };

    let json = event.to_chrome_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value.is_object());
    assert!(value.get("name").is_some());
    assert!(value.get("cat").is_some());
    assert!(value.get("ph").is_some());
    assert!(value.get("ts").is_some());
    assert!(value.get("pid").is_some());
    assert!(value.get("tid").is_some());
}

/// **Validates: Requirements 14.1**
///
/// Verify that notification actions can be constructed and serialized.
#[test]
fn notification_action_constructs() {
    let action = crate::NotificationAction::new("open", "Open");
    let json = serde_json::to_string(&action);
    assert!(json.is_ok());
}

/// **Validates: Requirements 14.1**
///
/// Verify that builder-friendly notifications validate their required fields and
/// preserve action metadata.
#[test]
fn notification_builder_validates_and_preserves_actions() {
    let notification = crate::NotificationBuilder::new("Build Complete", "All tests passed")
        .open_action("Open")
        .dismiss_action("Dismiss");

    assert!(notification.validate().is_ok());
    assert_eq!(notification.title(), "Build Complete");
    assert_eq!(notification.body(), "All tests passed");
    assert_eq!(notification.action_buttons().len(), 2);
    assert_eq!(notification.action_count(), 2);
    assert_eq!(
        notification.action_ids().collect::<Vec<_>>(),
        vec![
            crate::NotificationAction::OPEN_ID,
            crate::NotificationAction::DISMISS_ID
        ]
    );
    assert!(notification.has_actions());
    assert!(notification.has_open_action());
    assert!(notification.has_dismiss_action());
    assert!(!notification.has_retry_action());
    assert!(!notification.has_settings_action());
    assert_eq!(notification.custom_action_count(), 0);
    assert_eq!(
        notification.to_text(),
        "notification: urgency normal, silent false, tag false, group false, timeout false, actions 2, has actions true, open true, dismiss true, retry false, settings false, custom 0"
    );
    assert!(!notification.to_text().contains("Build Complete"));
    assert!(!notification.to_text().contains("All tests passed"));
    assert!(!notification.to_text().contains("Dismiss"));

    let (_, _, actions) = notification.into_parts();
    assert_eq!(actions[0], crate::NotificationAction::new("open", "Open"));
    assert_eq!(actions[1], crate::NotificationAction::dismiss("Dismiss"));

    assert!(
        crate::NotificationBuilder::new("", "Body")
            .validate()
            .is_err()
    );
    assert!(
        crate::NotificationBuilder::new("Title", "")
            .validate()
            .is_err()
    );
    assert!(
        crate::NotificationBuilder::new("Title", "Body")
            .action("", "Open")
            .validate()
            .is_err()
    );
    assert!(
        crate::NotificationBuilder::new("Title", "Body")
            .action("open", "Open")
            .action("open", "Open Again")
            .validate()
            .is_err()
    );
}

#[test]
fn notification_builder_tracks_delivery_metadata_safely() {
    let notification = crate::NotificationBuilder::new("Upload complete", "report.pdf is ready")
        .critical()
        .deliver_silently()
        .tag("workspace:private-project")
        .group("sync:documents")
        .timeout_secs(30)
        .retry_action("Retry");

    assert!(notification.validate().is_ok());
    assert_eq!(
        notification.urgency_level(),
        crate::NotificationUrgency::Critical
    );
    assert!(notification.is_silent());
    assert!(notification.has_tag());
    assert!(notification.has_group());
    assert!(notification.has_timeout());
    assert_eq!(notification.tag_value(), Some("workspace:private-project"));
    assert_eq!(notification.group_value(), Some("sync:documents"));
    assert_eq!(notification.timeout_millis(), Some(30_000));

    let summary = notification.to_text();
    assert_eq!(
        summary,
        "notification: urgency critical, silent true, tag true, group true, timeout true, actions 1, has actions true, open false, dismiss false, retry true, settings false, custom 0"
    );
    assert!(!summary.contains("Upload complete"));
    assert!(!summary.contains("report.pdf"));
    assert!(!summary.contains("workspace:private-project"));
    assert!(!summary.contains("sync:documents"));
    assert!(!summary.contains("30000"));

    assert!(
        crate::NotificationBuilder::new("Build", "Done")
            .tag(" private")
            .validate()
            .is_err()
    );
    assert!(
        crate::NotificationBuilder::new("Build", "Done")
            .group("sync\0")
            .validate()
            .is_err()
    );
    assert!(
        crate::NotificationBuilder::new("Build", "Done")
            .timeout_ms(0)
            .validate()
            .is_err()
    );
    assert_eq!(crate::NotificationUrgency::Low.key(), "low");
}

#[test]
fn notification_action_event_classifies_common_actions() {
    let retry =
        crate::NotificationActionEvent::Known(crate::NotificationAction::RETRY_ID.to_string());
    assert_eq!(retry.id(), "retry");
    assert!(retry.is_known());
    assert!(retry.is_retry());
    assert!(!retry.is_open());
    assert_eq!(
        retry.to_text(),
        "notification action: known true, open false, dismiss false, retry true, settings false"
    );

    let unknown = crate::NotificationActionEvent::Unknown("plugin:action".to_string());
    assert_eq!(unknown.id(), "plugin:action");
    assert!(!unknown.is_known());
    assert!(!unknown.is_settings());
    assert_eq!(
        unknown.to_text(),
        "notification action: known false, open false, dismiss false, retry false, settings false"
    );
    assert!(!unknown.to_text().contains("plugin:action"));
}

#[cfg(any(test, feature = "test-support"))]
#[test]
fn show_notification_checked_validates_plain_notifications() {
    let cx = crate::TestAppContext::single();

    let result = cx.read(|app| app.show_notification_checked("Build Complete", "All tests passed"));
    assert!(
        result.is_ok()
            || result
                .unwrap_err()
                .to_string()
                .contains("Notifications not supported")
    );

    assert!(
        cx.read(|app| app.show_notification_checked("", "All tests passed"))
            .is_err()
    );
    assert!(
        cx.read(|app| app.show_notification_checked(" Build Complete", "All tests passed"))
            .is_err()
    );
    assert!(
        cx.read(|app| app.show_notification_checked("Build Complete", "Done\0"))
            .is_err()
    );
}

#[cfg(any(test, feature = "test-support"))]
#[test]
fn show_desktop_notification_with_action_router_validates_before_dispatch() {
    let cx = crate::TestAppContext::single();

    assert!(
        cx.read(|app| app.show_desktop_notification_with_action_router(
            crate::NotificationBuilder::new("Update Available", "Version 2.0")
                .open_action("Install")
                .open_action("Install Again"),
            |_| {}
        ))
        .is_err()
    );
}

/// **Validates: Requirements 20.1**
///
/// Verify that builder-friendly message dialogs preserve native dialog options
/// and reject incomplete configuration before hitting platform APIs.
#[test]
fn message_dialog_builder_validates_and_preserves_options() {
    let dialog = crate::MessageDialogBuilder::confirm("Delete file?", "This cannot be undone")
        .detail("report.pdf will be removed permanently");

    assert!(dialog.validate().is_ok());
    assert_eq!(dialog.kind(), crate::DialogKind::Warning);
    assert_eq!(dialog.title().as_ref(), "Delete file?");
    assert_eq!(dialog.message().as_ref(), "This cannot be undone");
    assert_eq!(
        dialog.detail_text().map(|detail| detail.as_ref()),
        Some("report.pdf will be removed permanently")
    );
    assert_eq!(
        dialog
            .buttons_list()
            .iter()
            .map(|button| button.as_ref())
            .collect::<Vec<_>>(),
        vec!["Cancel", "OK"]
    );
    assert_eq!(dialog.cancel_button_index(), Some(0));
    assert_eq!(dialog.default_button_index(), Some(1));

    let plan = dialog.clone().build_checked().unwrap();
    assert_eq!(plan.kind(), crate::DialogKind::Warning);
    assert_eq!(plan.button_count(), 2);
    assert!(plan.has_cancel_button());
    assert!(plan.has_detail());
    assert_eq!(
        plan.to_text(),
        "message dialog warning: 2 buttons, detail true, default Some(1), cancel Some(0)"
    );
    assert!(!plan.to_text().contains("Delete file"));
    assert!(!plan.to_text().contains("report.pdf"));

    let options = dialog.into_options();
    assert_eq!(options.buttons.len(), 2);
    assert_eq!(options.cancel_button, Some(0));
    assert_eq!(options.default_button, Some(1));
    assert_eq!(options.button_count(), 2);
    assert!(options.has_detail());
    assert!(options.has_default_button());
    assert!(options.has_cancel_button());
    assert_eq!(
        options.to_text(),
        "dialog options warning: 2 buttons, detail true, default true, cancel true"
    );
    assert!(!options.to_text().contains("Delete file"));
    assert!(!options.to_text().contains("report.pdf"));

    let destructive = crate::MessageDialogBuilder::destructive_confirm(
        "Delete file?",
        "This cannot be undone",
        "Delete",
    );
    assert_eq!(
        destructive
            .buttons_list()
            .iter()
            .map(|button| button.as_ref())
            .collect::<Vec<_>>(),
        vec!["Cancel", "Delete"]
    );
    assert_eq!(
        destructive.to_text(),
        "dialog options warning: 2 buttons, detail false, default true, cancel true"
    );
    assert_eq!(destructive.cancel_button_index(), Some(0));
    assert_eq!(destructive.default_button_index(), Some(0));

    assert!(
        crate::MessageDialogBuilder::info("", "Message")
            .validate()
            .is_err()
    );
    assert!(
        crate::MessageDialogBuilder::info("Title", "")
            .validate()
            .is_err()
    );
    assert!(
        crate::MessageDialogBuilder::info("Title", "Message")
            .buttons(Vec::<&str>::new())
            .validate()
            .is_err()
    );
    assert!(
        crate::MessageDialogBuilder::info("Title", "Message")
            .buttons(["OK", ""])
            .validate()
            .is_err()
    );
    assert!(
        crate::MessageDialogBuilder::info("Title", "Message")
            .default_button(1)
            .validate()
            .is_err()
    );
    assert!(
        crate::MessageDialogBuilder::info("Title", "Message")
            .cancel_button(1)
            .validate()
            .is_err()
    );
}

/// **Validates: Requirements 14.1**
///
/// Verify that builder-friendly tray menus produce the same menu item tree as
/// the lower-level platform representation.
#[test]
fn tray_menu_builder_preserves_items() {
    let menu = crate::TrayMenuBuilder::new()
        .action("Show Window", "show")
        .separator()
        .submenu(
            "Status",
            crate::TrayMenuBuilder::new()
                .toggle("Available", true, "available")
                .action("Pause Sync", "pause"),
        )
        .action("Quit", "quit");

    assert_eq!(menu.items().len(), 4);
    assert!(menu.validate().is_ok());

    let items: Vec<crate::TrayMenuItem> = menu.into();
    assert_eq!(items[0], crate::TrayMenuItem::action("Show Window", "show"));
    assert_eq!(items[1], crate::TrayMenuItem::separator());
    assert_eq!(items[3], crate::TrayMenuItem::action("Quit", "quit"));

    match &items[2] {
        crate::TrayMenuItem::Submenu { label, items } => {
            assert_eq!(label.as_ref(), "Status");
            assert_eq!(items.len(), 2);
            assert_eq!(
                items[0],
                crate::TrayMenuItem::toggle("Available", true, "available")
            );
        }
        other => panic!("expected submenu, got {other:?}"),
    }
}

#[test]
fn tray_menu_builder_rejects_ambiguous_action_ids() {
    assert!(
        crate::TrayMenuBuilder::new()
            .action("Show", "show")
            .submenu(
                "Window",
                crate::TrayMenuBuilder::new().action("Show Again", "show"),
            )
            .validate()
            .is_err()
    );

    assert!(
        crate::TrayMenuBuilder::new()
            .toggle("Enabled", true, "")
            .validate()
            .is_err()
    );

    assert!(
        crate::TrayMenuBuilder::new()
            .submenu("Empty", Vec::<crate::TrayMenuItem>::new())
            .validate()
            .is_err()
    );
}

/// **Validates: Requirements 14.1**
///
/// Verify that builder-friendly context menus preserve the same native item
/// tree as the lower-level platform representation.
#[test]
fn context_menu_builder_preserves_items() {
    let menu = crate::NativeContextMenuBuilder::new()
        .action("Open", "open")
        .separator()
        .submenu(
            "Sort",
            crate::NativeContextMenuBuilder::new()
                .action("By Name", "sort-name")
                .toggle("Descending", false, "sort-desc"),
        )
        .action("Reveal in Folder", "reveal");

    assert_eq!(menu.items().len(), 4);
    assert!(menu.validate().is_ok());

    let items: Vec<crate::TrayMenuItem> = menu.into();
    assert_eq!(items[0], crate::TrayMenuItem::action("Open", "open"));
    assert_eq!(items[1], crate::TrayMenuItem::separator());
    assert_eq!(
        items[3],
        crate::TrayMenuItem::action("Reveal in Folder", "reveal")
    );

    match &items[2] {
        crate::TrayMenuItem::Submenu { label, items } => {
            assert_eq!(label.as_ref(), "Sort");
            assert_eq!(items.len(), 2);
            assert_eq!(
                items[0],
                crate::TrayMenuItem::action("By Name", "sort-name")
            );
            assert_eq!(
                items[1],
                crate::TrayMenuItem::toggle("Descending", false, "sort-desc")
            );
        }
        other => panic!("expected submenu, got {other:?}"),
    }
}

#[test]
fn context_menu_builder_rejects_invalid_labels_and_duplicate_ids() {
    assert!(
        crate::NativeContextMenuBuilder::new()
            .action("", "open")
            .validate()
            .is_err()
    );

    assert!(
        crate::NativeContextMenuBuilder::new()
            .action("Open", "open")
            .toggle("Open Toggle", false, "open")
            .validate()
            .is_err()
    );
}

/// Verify that taskbar/dock progress states reject invalid fractions before
/// generated apps hand them to platform APIs.
#[test]
fn progress_bar_state_validates_fraction() {
    assert_eq!(
        crate::ProgressBarState::normal(0.25).unwrap(),
        crate::ProgressBarState::Normal(0.25)
    );
    assert_eq!(
        crate::ProgressBarState::error(1.0).unwrap(),
        crate::ProgressBarState::Error(1.0)
    );
    assert_eq!(
        crate::ProgressBarState::paused(0.0).unwrap(),
        crate::ProgressBarState::Paused(0.0)
    );
    assert!(crate::ProgressBarState::Indeterminate.validate().is_ok());
    assert!(crate::ProgressBarState::None.validate().is_ok());

    for value in [-0.1, 1.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(crate::ProgressBarState::normal(value).is_err());
        assert!(crate::ProgressBarState::Error(value).validate().is_err());
        assert!(crate::ProgressBarState::Paused(value).validate().is_err());
    }
}

#[test]
fn focused_window_query_filters_valid_window_info() {
    let info = crate::FocusedWindowInfo {
        app_name: "Visual Studio Code".to_string(),
        window_title: "project - main.rs".to_string(),
        bundle_id: Some("com.microsoft.VSCode".to_string()),
        pid: Some(std::process::id().saturating_add(1)),
    };

    assert!(info.validate().is_ok());
    assert!(info.is_external_process());

    let query = crate::FocusedWindowQuery::builder()
        .require_title()
        .require_pid()
        .external_only()
        .app_name_contains("code")
        .bundle_id("com.microsoft.VSCode")
        .build_checked()
        .unwrap();

    assert!(info.matches_query(&query));

    let wrong_app = crate::FocusedWindowQuery::builder()
        .app_name("Terminal")
        .build_checked()
        .unwrap();
    assert!(!info.matches_query(&wrong_app));
}

#[test]
fn focused_window_query_rejects_generated_footguns() {
    assert!(
        crate::FocusedWindowQuery::builder()
            .external_only()
            .current_process_only()
            .build_checked()
            .is_err()
    );
    assert!(
        crate::FocusedWindowQuery::builder()
            .app_name("Code")
            .app_name_contains("Code")
            .build_checked()
            .is_err()
    );
    assert!(
        crate::FocusedWindowQuery::builder()
            .app_name(" Code")
            .build_checked()
            .is_err()
    );
    assert!(
        crate::FocusedWindowQuery::builder()
            .bundle_id("com.example.\nApp")
            .build_checked()
            .is_err()
    );
    assert!(
        crate::FocusedWindowQuery::builder()
            .pid(0)
            .build_checked()
            .is_err()
    );

    let missing_title = crate::FocusedWindowInfo {
        app_name: "Preview".to_string(),
        window_title: String::new(),
        bundle_id: None,
        pid: Some(42),
    };
    let query = crate::FocusedWindowQuery::builder()
        .require_title()
        .build_checked()
        .unwrap();
    assert!(!missing_title.matches_query(&query));
}

/// **Validates: Requirements 6.1**
///
/// Verify that clipboard text convenience helpers round-trip through the test
/// platform without requiring callers to construct `ClipboardItem` directly.
#[cfg(any(test, feature = "test-support"))]
#[test]
fn clipboard_text_helpers_round_trip() {
    let cx = crate::TestAppContext::single();

    cx.write_clipboard_text("copied text");

    assert_eq!(cx.read_clipboard_text().as_deref(), Some("copied text"));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("copied text")
    );
}

#[cfg(any(test, feature = "test-support"))]
#[test]
fn checked_clipboard_text_helper_validates_generated_text() {
    let cx = crate::TestAppContext::single();

    cx.read(|app| app.write_clipboard_text_checked("checked text"))
        .unwrap();
    assert_eq!(cx.read_clipboard_text().as_deref(), Some("checked text"));

    assert!(cx.read(|app| app.write_clipboard_text_checked("")).is_err());
    assert!(
        cx.read(|app| app.write_clipboard_text_checked("bad\0text"))
            .is_err()
    );
}

#[cfg(any(test, feature = "test-support"))]
#[test]
fn checked_clipboard_write_requires_clipboard_write_capability() {
    let cx = crate::TestAppContext::single();

    cx.read(|app| app.write_clipboard_text_checked("allowed text"))
        .unwrap();
    assert_eq!(cx.read_clipboard_text().as_deref(), Some("allowed text"));

    cx.update(|app| {
        app.configure_permission_broker_checked(
            crate::PermissionBrokerInstallBuilder::new()
                .process_class(crate::ProcessClass::Extension)
                .deny_ungranted(),
        )
        .unwrap();
    });

    assert!(
        cx.read(|app| app.write_clipboard_text_checked("denied text"))
            .is_err()
    );
    assert_eq!(cx.read_clipboard_text().as_deref(), Some("allowed text"));
}

#[cfg(any(test, feature = "test-support"))]
#[test]
fn checked_clipboard_clear_validates_reason_and_clears() {
    let cx = crate::TestAppContext::single();

    cx.read(|app| app.write_clipboard_text_checked("temporary token"))
        .unwrap();
    assert_eq!(cx.read_clipboard_text().as_deref(), Some("temporary token"));

    let clear = cx
        .read(|app| {
            app.clear_clipboard_checked(crate::ClipboardClearBuilder::new(
                "Temporary token expired",
            ))
        })
        .unwrap();

    assert_eq!(clear.reason(), "Temporary token expired");
    assert!(cx.read_from_clipboard().is_none());

    cx.read(|app| app.write_clipboard_text_checked("keep me"))
        .unwrap();
    assert!(
        cx.read(|app| app.clear_clipboard_checked(crate::ClipboardClearBuilder::new("")))
            .is_err()
    );
    assert_eq!(cx.read_clipboard_text().as_deref(), Some("keep me"));
    assert!(
        crate::ClipboardClearBuilder::new("Temporary\ntoken")
            .validate()
            .is_err()
    );
}

#[cfg(any(test, feature = "test-support"))]
#[test]
fn clipboard_item_builder_round_trips_rich_payload() {
    let cx = crate::TestAppContext::single();
    let image = crate::Image::from_bytes(crate::ImageFormat::Png, vec![1, 2, 3, 4]);

    assert_eq!(crate::ImageFormat::Png.key(), "png");
    assert_eq!(image.format(), crate::ImageFormat::Png);
    assert_eq!(image.bytes(), &[1, 2, 3, 4]);
    assert_eq!(image.byte_len(), 4);
    assert!(image.has_bytes());
    assert_eq!(image.to_text(), "image: format png, bytes 4, empty false");
    assert!(!image.to_text().contains("1, 2, 3, 4"));

    cx.read(|app| {
        app.write_clipboard_item_checked(
            crate::ClipboardItem::builder()
                .text_with_json_metadata("formatted text", serde_json::json!({ "source": "test" }))
                .image_ref(&image),
        )
    })
    .unwrap();

    let item = cx.read_from_clipboard().expect("clipboard item");
    assert_eq!(item.text().as_deref(), Some("formatted text"));
    assert!(item.has_text());
    assert!(item.has_image());
    assert_eq!(item.first_image().unwrap(), &image);
    assert_eq!(item.strings().count(), 1);
    assert_eq!(item.images().count(), 1);
}

#[cfg(any(test, feature = "test-support"))]
#[test]
fn clipboard_html_helper_round_trips_plain_text_and_metadata() {
    let cx = crate::TestAppContext::single();

    cx.read(|app| app.write_clipboard_html("Hello world", "<strong>Hello world</strong>"))
        .unwrap();

    let item = cx.read_from_clipboard().expect("clipboard item");
    assert_eq!(item.text().as_deref(), Some("Hello world"));
    assert!(item.has_html());
    assert_eq!(item.html().as_deref(), Some("<strong>Hello world</strong>"));

    let string = item.strings().next().unwrap();
    assert!(string.has_html());
    assert_eq!(
        string.html().as_deref(),
        Some("<strong>Hello world</strong>")
    );
}

#[test]
fn clipboard_item_builder_validates_non_empty_payload() {
    assert!(crate::ClipboardItem::builder().validate().is_err());
    assert!(
        crate::ClipboardItem::builder()
            .text("copied text")
            .validate()
            .is_ok()
    );
    assert!(crate::ClipboardItem::builder().text("").validate().is_err());
    assert!(
        crate::ClipboardItem::builder()
            .text_with_metadata("copied text", "")
            .validate()
            .is_err()
    );
    assert!(
        crate::ClipboardItem::builder()
            .image(crate::Image::empty())
            .validate()
            .is_err()
    );
    assert!(crate::ClipboardItem::builder().html("plain", "").is_err());
}

#[test]
fn clipboard_item_builder_reports_json_metadata_errors() {
    struct FailingMetadata;

    impl serde::Serialize for FailingMetadata {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("metadata failed"))
        }
    }

    let item = crate::ClipboardItem::builder()
        .try_text_with_json_metadata("copied text", serde_json::json!({ "source": "test" }))
        .unwrap()
        .build_checked()
        .unwrap();
    let string = item.strings().next().unwrap();

    assert_eq!(string.text(), "copied text");
    assert!(string.metadata().unwrap().contains("\"source\":\"test\""));
    assert_eq!(
        string.metadata_json::<serde_json::Value>().unwrap()["source"],
        "test"
    );

    assert!(
        crate::ClipboardItem::builder()
            .try_text_with_json_metadata("copied text", FailingMetadata)
            .is_err()
    );
    assert!(
        crate::ClipboardString::new("copied text".to_string())
            .try_with_json_metadata(FailingMetadata)
            .is_err()
    );
}

/// **Validates: Requirements 14.1**
///
/// Verify that shell targets preserve the exact action a builder requested.
#[test]
fn shell_target_constructors_preserve_kind() {
    assert_eq!(
        crate::ShellTarget::url("https://example.com"),
        crate::ShellTarget::Url("https://example.com".to_string())
    );
    assert_eq!(
        crate::ShellTarget::path("/tmp/report.pdf"),
        crate::ShellTarget::Path(std::path::PathBuf::from("/tmp/report.pdf"))
    );
    assert_eq!(
        crate::ShellTarget::reveal_path("/tmp/report.pdf"),
        crate::ShellTarget::RevealPath(std::path::PathBuf::from("/tmp/report.pdf"))
    );
    assert!(
        crate::ShellTarget::url("https://example.com")
            .validate()
            .is_ok()
    );
    assert!(
        crate::ShellTarget::url("mailto:support@example.com")
            .validate()
            .is_ok()
    );
    assert!(
        crate::ShellTarget::url("javascript:alert(1)")
            .validate()
            .is_err()
    );
    assert!(
        crate::ShellTarget::url(" https://example.com")
            .validate()
            .is_err()
    );
    assert!(crate::ShellTarget::path("").validate().is_err());
    assert!(
        crate::ShellTargetsBuilder::new()
            .path("")
            .validate()
            .is_err()
    );
    assert!(
        crate::ShellTargetsBuilder::new()
            .path("/definitely/not/a/path")
            .require_existing_paths()
            .validate()
            .is_err()
    );
}

/// **Validates: Requirements 5.1**
///
/// Verify that builder-friendly global hotkeys parse shortcut strings and
/// preserve application-owned identifiers.
#[test]
fn global_hotkey_builder_parses_and_preserves_ids() {
    let hotkeys = crate::GlobalHotkeyBuilder::new()
        .parse_named_hotkey(1, "Command Palette", "cmd-shift-p")
        .unwrap()
        .parse_hotkey(2, "cmd-alt-i")
        .unwrap()
        .build();

    assert_eq!(hotkeys.hotkeys().len(), 2);
    assert_eq!(hotkeys.len(), 2);
    assert!(!hotkeys.is_empty());
    assert_eq!(hotkeys.ids(), vec![1, 2]);
    assert_eq!(hotkeys.named_count(), 1);
    assert_eq!(hotkeys.unnamed_count(), 1);
    assert_eq!(hotkeys.names(), vec![Some("Command Palette"), None]);
    assert!(hotkeys.contains_id(1));
    assert!(!hotkeys.contains_id(99));
    assert_eq!(
        hotkeys.to_text(),
        "global hotkey set: 2 hotkeys, 1 named, 1 unnamed, ids [1, 2]"
    );
    assert_eq!(hotkeys.hotkeys()[0].id(), 1);
    assert_eq!(
        hotkeys.hotkeys()[0].name().map(|name| name.as_ref()),
        Some("Command Palette")
    );
    assert!(hotkeys.hotkeys()[0].has_name());
    assert_eq!(
        hotkeys.hotkeys()[0].to_text(),
        "global hotkey: id 1, name true, shortcut true"
    );
    assert!(!hotkeys.hotkeys()[0].to_text().contains("Command Palette"));
    assert!(!hotkeys.hotkeys()[0].to_text().contains("cmd-shift-p"));
    assert_eq!(hotkeys.hotkeys()[1].id(), 2);
    assert!(!hotkeys.hotkeys()[1].has_name());

    let builder = crate::GlobalHotkeyBuilder::new()
        .parse_named_hotkey(3, "Open Capture", "cmd-alt-o")
        .unwrap()
        .parse_hotkey(4, "cmd-alt-p")
        .unwrap();
    assert_eq!(builder.len(), 2);
    assert!(!builder.is_empty());
    assert_eq!(builder.ids(), vec![3, 4]);
    assert_eq!(builder.named_count(), 1);
    assert_eq!(builder.unnamed_count(), 1);
    assert!(builder.contains_id(3));
    assert!(!builder.contains_id(99));
    assert_eq!(
        builder.to_text(),
        "global hotkey builder: 2 hotkeys, 1 named, 1 unnamed, ids [3, 4]"
    );
    assert!(!builder.to_text().contains("Open Capture"));
    assert!(!builder.to_text().contains("cmd-alt-o"));
}

#[test]
fn global_hotkeys_checked_previews_without_registering() {
    let cx = crate::TestAppContext::single();

    let hotkeys = cx
        .read(|app| {
            app.global_hotkeys_checked(
                crate::GlobalHotkeyBuilder::new()
                    .parse_named_hotkey(7, "Command Palette", "cmd-shift-p")
                    .unwrap()
                    .parse_named_hotkey(8, "Toggle Capture", "cmd-alt-c")
                    .unwrap(),
            )
        })
        .unwrap();

    assert_eq!(hotkeys.ids(), vec![7, 8]);
    assert_eq!(hotkeys.named_count(), 2);
    assert_eq!(hotkeys.unnamed_count(), 0);
    assert_eq!(
        hotkeys.names(),
        vec![Some("Command Palette"), Some("Toggle Capture")]
    );
    assert!(hotkeys.contains_id(8));
    assert_eq!(
        hotkeys.to_text(),
        "global hotkey set: 2 hotkeys, 2 named, 0 unnamed, ids [7, 8]"
    );
}

#[test]
fn global_hotkey_builder_rejects_ambiguous_registrations() {
    assert!(crate::GlobalHotkeyBuilder::new().validate().is_err());

    assert!(
        crate::GlobalHotkeyBuilder::new()
            .parse_hotkey(1, "cmd-shift-p")
            .unwrap()
            .parse_hotkey(1, "cmd-alt-i")
            .unwrap()
            .build_checked()
            .is_err()
    );

    assert!(
        crate::GlobalHotkeyBuilder::new()
            .parse_hotkey(1, "cmd-shift-p")
            .unwrap()
            .parse_hotkey(2, "cmd-shift-p")
            .unwrap()
            .build_checked()
            .is_err()
    );
    assert!(
        crate::GlobalHotkeyBuilder::new()
            .parse_named_hotkey(1, " Command Palette", "cmd-shift-p")
            .unwrap()
            .build_checked()
            .is_err()
    );
    assert!(
        crate::GlobalHotkeyBuilder::new()
            .parse_named_hotkey(1, "Command\nPalette", "cmd-shift-p")
            .unwrap()
            .build_checked()
            .is_err()
    );
    assert!(
        crate::GlobalHotkeyBuilder::new()
            .parse_named_hotkey(1, "", "cmd-shift-p")
            .unwrap()
            .build_checked()
            .is_err()
    );
}

#[test]
fn global_hotkey_unregistration_validates_cleanup_ids() {
    let hotkeys = crate::GlobalHotkeyBuilder::new()
        .parse_named_hotkey(7, "Command Palette", "cmd-shift-p")
        .unwrap()
        .parse_hotkey(8, "cmd-alt-i")
        .unwrap()
        .build_checked()
        .unwrap();

    let request = crate::GlobalHotkeyUnregistration::new().hotkey_set(&hotkeys);
    assert_eq!(request.ids(), &[7, 8]);
    assert_eq!(request.id_count(), 2);
    assert_eq!(
        request.to_text(),
        "global hotkey unregistration: 2 ids [7, 8]"
    );
    assert!(request.validate().is_ok());

    let single = crate::GlobalHotkeyUnregistration::new().id(9);
    assert_eq!(single.ids(), &[9]);
    assert_eq!(single.id_count(), 1);
    assert_eq!(single.to_text(), "global hotkey unregistration: 1 ids [9]");
    assert!(single.validate().is_ok());

    assert!(crate::GlobalHotkeyUnregistration::new().validate().is_err());
    assert!(
        crate::GlobalHotkeyUnregistration::new()
            .id(7)
            .id(7)
            .validate()
            .is_err()
    );
}

#[test]
fn unregister_global_hotkeys_checked_uses_request_validation() {
    let cx = crate::TestAppContext::single();

    assert!(
        cx.read(|app| {
            app.unregister_global_hotkeys_checked(crate::GlobalHotkeyUnregistration::new().id(42))
        })
        .is_ok()
    );

    assert!(
        cx.read(|app| {
            app.unregister_global_hotkeys_checked(crate::GlobalHotkeyUnregistration::new())
        })
        .is_err()
    );

    assert!(
        cx.read(|app| {
            app.unregister_global_hotkeys_checked(
                crate::GlobalHotkeyUnregistration::new().id(42).id(42),
            )
        })
        .is_err()
    );
}

/// **Validates: Requirements 20.1**
///
/// Verify that builder-friendly file dialogs preserve open/save options.
#[test]
fn file_dialog_builders_preserve_options() {
    let open = crate::OpenDialogBuilder::directory()
        .multiple(true)
        .prompt("Choose workspace")
        .image_files()
        .filter("Markdown", [".md", "markdown"]);
    let options = open.options();

    assert!(!options.files);
    assert!(options.directories);
    assert!(options.multiple);
    assert_eq!(
        options.prompt.as_ref().map(|prompt| prompt.as_ref()),
        Some("Choose workspace")
    );
    assert_eq!(options.filters.len(), 2);
    assert_eq!(options.filter_count(), 2);
    assert_eq!(options.filter_extension_count(), 9);
    assert!(!options.allows_files());
    assert!(options.allows_directories());
    assert!(options.allows_multiple());
    assert!(options.has_prompt());
    assert_eq!(
        options.to_text(),
        "path prompt options: mode directories, multiple true, prompt true, 2 filters, 9 extensions"
    );
    assert!(!options.to_text().contains("Choose workspace"));
    assert!(!options.to_text().contains("Markdown"));
    assert_eq!(options.filters[0], crate::FileDialogFilter::images());
    assert_eq!(options.filters[1].name.as_ref(), "Markdown");
    assert_eq!(options.filters[0].name(), "Images");
    assert_eq!(options.filters[0].extension_count(), 7);
    assert_eq!(
        options.filters[0].to_text(),
        "file dialog filter: name true, extensions 7"
    );
    assert_eq!(
        options.filters[1]
            .extensions
            .iter()
            .map(|extension| extension.as_ref())
            .collect::<Vec<_>>(),
        vec!["md", "markdown"]
    );
    assert_eq!(
        options.filters[1]
            .extensions()
            .iter()
            .map(|extension| extension.as_ref())
            .collect::<Vec<_>>(),
        vec!["md", "markdown"]
    );
    assert_eq!(options.filters[1].extension_count(), 2);
    assert!(!options.filters[1].to_text().contains("Markdown"));
    assert!(!options.filters[1].to_text().contains("markdown"));
    assert!(open.validate().is_ok());
    assert_eq!(
        open.to_text(),
        "path prompt options: mode directories, multiple true, prompt true, 2 filters, 9 extensions"
    );

    let open_plan = open.clone().build_checked().unwrap();
    assert!(!open_plan.allows_files());
    assert!(open_plan.allows_directories());
    assert!(open_plan.allows_multiple());
    assert_eq!(open_plan.filter_count(), 2);
    assert_eq!(open_plan.filter_extension_count(), 9);
    assert_eq!(open_plan.filter_names(), vec!["Images", "Markdown"]);
    assert_eq!(
        open_plan.to_text(),
        "open dialog: mode directories, multiple true, prompt true, 2 filters, 9 extensions"
    );
    assert!(!open_plan.to_text().contains("Choose workspace"));
    assert!(!open_plan.to_text().contains("Markdown"));

    assert!(
        crate::OpenDialogBuilder::file()
            .files_allowed(false)
            .directories_allowed(false)
            .validate()
            .is_err()
    );
    assert!(
        crate::OpenDialogBuilder::file()
            .file_filter(crate::FileDialogFilter::new("", ["txt"]))
            .validate()
            .is_err()
    );
    assert!(
        crate::OpenDialogBuilder::file()
            .prompt(" Pick file")
            .validate()
            .is_err()
    );
    assert!(
        crate::OpenDialogBuilder::file()
            .prompt("Pick\nfile")
            .validate()
            .is_err()
    );

    let save = crate::SaveDialogBuilder::new("/tmp")
        .suggested_name("report")
        .default_extension(".PDF");
    assert_eq!(save.directory_path(), std::path::Path::new("/tmp"));
    assert_eq!(save.suggested_name_value(), Some("report"));
    assert_eq!(save.default_extension_value(), Some("pdf"));
    assert!(save.validate().is_ok());

    let save_plan = save.clone().build_checked().unwrap();
    assert!(save_plan.has_suggested_name());
    assert!(save_plan.has_default_extension());
    assert!(save_plan.appended_default_extension());
    assert_eq!(
        save_plan.to_text(),
        "save dialog: directory set true, suggested name true, default extension true, appended default extension true"
    );
    assert!(!save_plan.to_text().contains("/tmp"));
    assert!(!save_plan.to_text().contains("report"));

    let (_, suggested_name) = save.into_parts();
    assert_eq!(suggested_name.as_deref(), Some("report.pdf"));

    let (_, existing_extension) = crate::SaveDialogBuilder::new("/tmp")
        .suggested_name("report.csv")
        .json()
        .into_parts();
    assert_eq!(existing_extension.as_deref(), Some("report.csv"));

    assert!(
        crate::SaveDialogBuilder::new("/tmp")
            .default_extension("")
            .validate()
            .is_err()
    );
    assert!(crate::SaveDialogBuilder::new("").validate().is_err());
    assert!(
        crate::SaveDialogBuilder::new("/tmp")
            .suggested_name(" report")
            .validate()
            .is_err()
    );
    assert!(
        crate::SaveDialogBuilder::new("/tmp")
            .suggested_name("reports/final")
            .validate()
            .is_err()
    );
    assert!(
        crate::SaveDialogBuilder::new("/tmp")
            .suggested_name("report")
            .default_extension("bad ext")
            .validate()
            .is_err()
    );
}

/// **Validates: Requirements 14.1**
///
/// Verify that builder-friendly window options preserve the same fields as the
/// raw `WindowOptions` struct while giving app builders a fluent setup path.
#[test]
fn window_options_builder_preserves_options() {
    let bounds = crate::Bounds::from_corners(
        crate::point(crate::px(10.0), crate::px(20.0)),
        crate::point(crate::px(810.0), crate::px(620.0)),
    );
    let min_size = crate::size(crate::px(320.0), crate::px(240.0));
    let traffic_lights = crate::point(crate::px(14.0), crate::px(16.0));

    let builder = crate::WindowOptionsBuilder::new()
        .windowed(bounds)
        .title("Inspector")
        .transparent_titlebar(true)
        .traffic_light_position(traffic_lights)
        .unfocused()
        .hidden();
    let builder = with_overlay(builder)
        .movable(false)
        .resizable(false)
        .minimizable(false)
        .display_id(crate::DisplayId(7))
        .blurred_background()
        .app_id("com.example.inspector")
        .min_size(min_size)
        .client_decorations()
        .tabbing_identifier("workspace")
        .mouse_passthrough(true);

    assert!(builder.has_bounds());
    assert_eq!(builder.bounds_mode(), "windowed");
    assert!(builder.has_titlebar());
    assert!(builder.has_title());
    assert!(builder.has_transparent_titlebar());
    assert!(builder.has_traffic_light_position());
    assert!(builder.starts_hidden());
    assert!(builder.starts_unfocused());
    assert!(builder.fixed_size());
    assert!(builder.has_display_id());
    assert!(builder.has_app_id());
    assert!(builder.has_min_size());
    assert!(builder.uses_client_decorations());
    assert!(builder.has_tabbing_identifier());
    assert!(!builder.has_parent());
    let summary = builder.to_text();
    assert!(summary.contains("kind overlay"));
    assert!(summary.contains("bounds windowed"));
    assert!(summary.contains("background blurred"));
    assert!(summary.contains("titlebar transparent"));
    assert!(summary.contains("decorations client"));
    assert!(summary.contains("title yes"));
    assert!(summary.contains("shown no"));
    assert!(summary.contains("focused no"));
    assert!(summary.contains("mouse-passthrough yes"));
    assert!(!summary.contains("Inspector"));
    assert!(!summary.contains("com.example.inspector"));
    assert!(!summary.contains("workspace"));

    let options = builder.build();

    assert_eq!(
        options.window_bounds,
        Some(crate::WindowBounds::Windowed(bounds))
    );
    assert_eq!(options.focus, false);
    assert_eq!(options.show, false);
    assert_eq!(options.kind, overlay_window_kind());
    assert_eq!(options.is_movable, false);
    assert_eq!(options.is_resizable, false);
    assert_eq!(options.is_minimizable, false);
    assert_eq!(options.display_id, Some(crate::DisplayId(7)));
    assert_eq!(
        options.window_background,
        crate::WindowBackgroundAppearance::Blurred
    );
    assert_eq!(options.app_id.as_deref(), Some("com.example.inspector"));
    assert_eq!(options.window_min_size, Some(min_size));
    assert_eq!(
        options.window_decorations,
        Some(crate::WindowDecorations::Client)
    );
    assert_eq!(options.tabbing_identifier.as_deref(), Some("workspace"));
    assert!(options.mouse_passthrough);
    assert_eq!(options.bounds_mode(), "windowed");
    assert!(options.starts_hidden());
    assert!(options.starts_unfocused());
    assert!(options.fixed_size());
    assert!(options.uses_client_decorations());
    assert!(!options.to_text().contains("Inspector"));

    let titlebar = options.titlebar.expect("titlebar should be configured");
    assert_eq!(
        titlebar.title.as_ref().map(|title| title.as_ref()),
        Some("Inspector")
    );
    assert!(titlebar.appears_transparent);
    assert_eq!(titlebar.traffic_light_position, Some(traffic_lights));
    assert!(titlebar.has_title());
    assert!(titlebar.is_transparent());
    assert!(titlebar.has_traffic_light_position());
    assert_eq!(
        titlebar.to_text(),
        "titlebar options: title yes, transparent yes, traffic-light-position yes"
    );
    assert!(!titlebar.to_text().contains("Inspector"));

    // Bottom/Wallpaper/Top go through WindowOptionsBuilder directly here (rather than
    // WindowIntentBuilder, covered above) - same platform-independence requirement:
    // WindowKind never changes shape, so these must build on every platform.
    let bottom_options = with_bottom(crate::WindowOptionsBuilder::new()).build();
    assert_eq!(bottom_options.kind, bottom_window_kind());

    let wallpaper_options = with_wallpaper(crate::WindowOptionsBuilder::new()).build();
    assert_eq!(wallpaper_options.kind, wallpaper_window_kind());

    let top_options = with_top(crate::WindowOptionsBuilder::new()).build();
    assert_eq!(top_options.kind, top_window_kind());
}

#[test]
fn window_intent_builder_builds_checked_presets() {
    let bounds = crate::Bounds::from_corners(
        crate::point(crate::px(10.0), crate::px(20.0)),
        crate::point(crate::px(610.0), crate::px(420.0)),
    );
    let min_size = crate::size(crate::px(320.0), crate::px(240.0));

    let main_intent = crate::WindowIntentBuilder::main()
        .title("Kael")
        .windowed(bounds)
        .min_size(min_size)
        .app_id("com.example.kael");
    assert_eq!(main_intent.kind(), crate::WindowIntentKind::Main);
    assert!(main_intent.has_bounds());
    assert!(!main_intent.has_transparent_titlebar());
    assert!(!main_intent.starts_hidden());
    assert!(!main_intent.starts_unfocused());
    assert!(!main_intent.has_parent());
    let main_summary = main_intent.to_text();
    assert!(main_summary.contains("window intent main"));
    assert!(main_summary.contains("kind normal"));
    assert!(main_summary.contains("title yes"));
    assert!(main_summary.contains("min-size yes"));
    assert!(main_summary.contains("app-id yes"));
    assert!(!main_summary.contains("Kael"));
    assert!(!main_summary.contains("com.example.kael"));

    let main = main_intent.build_checked().unwrap();

    assert_eq!(main.kind, crate::WindowKind::Normal);
    assert!(main.is_resizable);
    assert_eq!(main.window_min_size, Some(min_size));
    assert_eq!(main.app_id.as_deref(), Some("com.example.kael"));

    let palette_intent = crate::WindowIntentBuilder::palette()
        .title("Command Palette")
        .windowed(bounds);
    assert_eq!(palette_intent.kind(), crate::WindowIntentKind::Palette);
    assert!(palette_intent.has_transparent_titlebar());
    assert!(
        palette_intent
            .options_summary()
            .contains("decorations client")
    );
    assert!(!palette_intent.to_text().contains("Command Palette"));

    let palette = palette_intent.build_checked().unwrap();
    assert_eq!(palette.kind, crate::WindowKind::Floating);
    assert!(!palette.is_resizable);
    assert!(!palette.is_minimizable);
    assert!(
        palette
            .titlebar
            .as_ref()
            .is_some_and(|titlebar| titlebar.appears_transparent)
    );

    let popup = crate::WindowIntentBuilder::popup().build_checked().unwrap();
    assert_eq!(popup.kind, crate::WindowKind::PopUp);
    assert!(!popup.is_resizable);
    assert!(!popup.is_movable);

    let overlay = overlay_intent_builder().build_checked().unwrap();
    assert_eq!(overlay.kind, overlay_window_kind());
    assert_eq!(
        overlay.window_background,
        crate::WindowBackgroundAppearance::Transparent
    );
    assert!(overlay.titlebar.is_none());

    // Bottom/Wallpaper/Top are wlr-layer-shell-only kinds: their builders and
    // validate() rules must still hold even on platforms without a native
    // layer-shell backend. Bottom/Wallpaper are unit variants there (their
    // *_window_kind()/*_intent_builder() helpers above are cfg-split to match);
    // only Top's shape never changes by platform, since X11/Windows read its
    // LayerShellOptions for real.
    let bottom = bottom_intent_builder().build_checked().unwrap();
    assert_eq!(bottom.kind, bottom_window_kind());
    assert!(!bottom.is_minimizable);
    assert!(!bottom.is_movable);
    assert!(!bottom.focus);
    assert_eq!(
        bottom.window_background,
        crate::WindowBackgroundAppearance::Transparent
    );
    assert!(bottom.titlebar.is_none());

    let wallpaper = wallpaper_intent_builder().build_checked().unwrap();
    assert_eq!(wallpaper.kind, wallpaper_window_kind());
    assert!(!wallpaper.is_minimizable);
    assert!(!wallpaper.is_resizable);
    assert!(!wallpaper.is_movable);
    assert!(!wallpaper.focus);
    assert_eq!(
        wallpaper.window_background,
        crate::WindowBackgroundAppearance::Transparent
    );
    assert!(wallpaper.titlebar.is_none());

    let top = top_intent_builder().build_checked().unwrap();
    assert_eq!(top.kind, top_window_kind());
    assert!(!top.is_minimizable);
    assert!(!top.is_movable);
    assert_eq!(
        top.window_background,
        crate::WindowBackgroundAppearance::Transparent
    );
    assert!(top.titlebar.is_none());

    assert_eq!(overlay_intent_kind().to_text(), "overlay");
    assert_eq!(bottom_intent_kind().to_text(), "bottom");
    assert_eq!(wallpaper_intent_kind().to_text(), "wallpaper");
    assert_eq!(top_intent_kind().to_text(), "top");
    assert_eq!(bottom_window_kind().to_text(), "bottom");
    assert_eq!(wallpaper_window_kind().to_text(), "wallpaper");
    assert_eq!(top_window_kind().to_text(), "top");
    assert_eq!(crate::WindowKind::Floating.to_text(), "floating");
    assert_eq!(
        crate::WindowBackgroundAppearance::Transparent.to_text(),
        "transparent"
    );
    assert_eq!(crate::WindowDecorations::Server.to_text(), "server");
    assert_eq!(crate::ResizeEdge::BottomRight.to_text(), "bottom-right");
}

#[test]
fn window_chrome_support_summaries_are_content_safe() {
    let full_controls = crate::WindowControls::default();
    assert_eq!(full_controls.supported_count(), 4);
    assert!(full_controls.supports_all());
    assert!(!full_controls.supports_none());
    assert!(full_controls.has_zoom_control());
    assert_eq!(
        full_controls.to_text(),
        "window controls: supported 4, fullscreen yes, maximize yes, minimize yes, window-menu yes, zoom yes"
    );

    let limited_controls = crate::WindowControls {
        fullscreen: false,
        maximize: false,
        minimize: true,
        window_menu: false,
    };
    assert_eq!(limited_controls.supported_count(), 1);
    assert!(!limited_controls.supports_all());
    assert!(!limited_controls.supports_none());
    assert!(!limited_controls.has_zoom_control());
    assert_eq!(
        limited_controls.to_text(),
        "window controls: supported 1, fullscreen no, maximize no, minimize yes, window-menu no, zoom no"
    );

    assert_eq!(crate::WindowControlArea::Drag.to_text(), "drag");
    assert!(crate::WindowControlArea::Drag.is_drag_region());
    assert!(!crate::WindowControlArea::Drag.is_button());
    assert_eq!(crate::WindowControlArea::Close.to_text(), "close");
    assert!(crate::WindowControlArea::Close.is_button());
    assert_eq!(crate::WindowControlArea::Max.to_text(), "max");
    assert_eq!(crate::WindowControlArea::Min.to_text(), "min");
}

#[test]
fn window_intent_builder_rejects_incoherent_generated_options() {
    let zero_bounds = crate::Bounds::from_corners(
        crate::point(crate::px(10.0), crate::px(20.0)),
        crate::point(crate::px(10.0), crate::px(420.0)),
    );

    assert!(
        crate::WindowIntentBuilder::main()
            .configure(crate::WindowOptionsBuilder::popup)
            .build_checked()
            .is_err()
    );
    assert!(
        crate::WindowIntentBuilder::palette()
            .configure(|options| options.minimizable(true))
            .build_checked()
            .is_err()
    );
    assert!(
        crate::WindowIntentBuilder::new(crate::WindowIntentKind::Modal)
            .build_checked()
            .is_err()
    );
    assert!(
        crate::WindowIntentBuilder::popup()
            .configure(|options| options.resizable(true))
            .build_checked()
            .is_err()
    );
    assert!(
        overlay_intent_builder()
            .configure(crate::WindowOptionsBuilder::floating)
            .build_checked()
            .is_err()
    );
    assert!(
        crate::WindowIntentBuilder::main()
            .title(" Bad")
            .build_checked()
            .is_err()
    );
    assert!(
        crate::WindowIntentBuilder::main()
            .app_id("com/example/app")
            .build_checked()
            .is_err()
    );
    assert!(
        crate::WindowIntentBuilder::main()
            .windowed(zero_bounds)
            .build_checked()
            .is_err()
    );
}

/// **Validates: Requirements 14.1**
///
/// Verify that builder-friendly application menus produce the same owned menu
/// tree as the lower-level platform representation.
#[test]
fn menu_builders_preserve_owned_menu_tree() {
    let plan = crate::MenuBarBuilder::new()
        .menu(
            crate::MenuBuilder::new("File")
                .action("Open...", MenuBuilderOpen)
                .separator()
                .submenu(crate::MenuBuilder::new("Recent").action("Project", MenuBuilderOpen))
                .action("Save", MenuBuilderSave),
        )
        .menu(
            crate::MenuBuilder::new("Edit").os_submenu("Services", crate::SystemMenuType::Services),
        )
        .build_plan_checked()
        .unwrap();

    assert_eq!(plan.menu_count(), 2);
    assert_eq!(plan.top_level_names(), vec!["File", "Edit"]);
    assert_eq!(plan.item_count(), 6);
    assert_eq!(plan.action_count(), 3);
    assert!(!plan.has_os_actions());
    assert!(plan.has_system_menus());

    let menus = plan.into_menus();
    assert_eq!(menus.len(), 2);
    let owned = menus
        .into_iter()
        .map(|menu| menu.owned())
        .collect::<Vec<_>>();

    assert_eq!(owned[0].name.as_ref(), "File");
    assert_eq!(owned[0].items.len(), 4);
    assert!(matches!(owned[0].items[1], crate::OwnedMenuItem::Separator));
    assert!(matches!(
        owned[0].items[2],
        crate::OwnedMenuItem::Submenu(_)
    ));
    assert_eq!(owned[1].name.as_ref(), "Edit");
    assert!(matches!(
        owned[1].items[0],
        crate::OwnedMenuItem::SystemMenu(_)
    ));
}

#[test]
fn menu_builder_standard_edit_preserves_native_roles() {
    let menu = crate::MenuBuilder::standard_edit(
        "Edit",
        MenuBuilderUndo,
        MenuBuilderRedo,
        MenuBuilderCut,
        MenuBuilderCopy,
        MenuBuilderPaste,
        MenuBuilderSelectAll,
    )
    .build_checked()
    .unwrap()
    .owned();

    assert_eq!(menu.name.as_ref(), "Edit");
    assert_eq!(menu.items.len(), 8);
    assert!(matches!(menu.items[2], crate::OwnedMenuItem::Separator));
    assert!(matches!(menu.items[6], crate::OwnedMenuItem::Separator));

    let expected_roles = [
        ("Undo", crate::OsAction::Undo),
        ("Redo", crate::OsAction::Redo),
        ("Cut", crate::OsAction::Cut),
        ("Copy", crate::OsAction::Copy),
        ("Paste", crate::OsAction::Paste),
        ("Select All", crate::OsAction::SelectAll),
    ];
    let action_items = [0, 1, 3, 4, 5, 7];

    for (item_index, (expected_name, expected_role)) in action_items.into_iter().zip(expected_roles)
    {
        match &menu.items[item_index] {
            crate::OwnedMenuItem::Action {
                name, os_action, ..
            } => {
                assert_eq!(name, expected_name);
                assert_eq!(*os_action, Some(expected_role));
            }
            _ => panic!("expected action item at index {item_index}"),
        }
    }
}

#[test]
fn dock_menu_builder_preserves_owned_items() {
    let items = crate::DockMenuBuilder::new()
        .action("Show Window", MenuBuilderOpen)
        .separator()
        .submenu(crate::MenuBuilder::new("Recent").action("Project", MenuBuilderOpen))
        .action("Quit", MenuBuilderSave)
        .build_checked()
        .unwrap();

    assert_eq!(items.len(), 4);
    let owned = items
        .into_iter()
        .map(|item| item.owned())
        .collect::<Vec<_>>();

    match &owned[0] {
        crate::OwnedMenuItem::Action {
            name, os_action, ..
        } => {
            assert_eq!(name, "Show Window");
            assert_eq!(*os_action, None);
        }
        _ => panic!("expected first dock menu item to be an action"),
    }
    assert!(matches!(owned[1], crate::OwnedMenuItem::Separator));
    assert!(matches!(owned[2], crate::OwnedMenuItem::Submenu(_)));
    match &owned[3] {
        crate::OwnedMenuItem::Action { name, .. } => assert_eq!(name, "Quit"),
        _ => panic!("expected last dock menu item to be an action"),
    }
}

#[test]
fn dock_menu_builder_rejects_invalid_generated_menus() {
    assert!(crate::DockMenuBuilder::new().validate().is_err());
    assert!(
        crate::DockMenuBuilder::new()
            .separator()
            .validate()
            .is_err()
    );
    assert!(
        crate::DockMenuBuilder::new()
            .action(" Show", MenuBuilderOpen)
            .validate()
            .is_err()
    );
    assert!(
        crate::DockMenuBuilder::new()
            .action("Show\nWindow", MenuBuilderOpen)
            .validate()
            .is_err()
    );
    assert!(
        crate::DockMenuBuilder::new()
            .submenu(crate::MenuBuilder::new("Recent"))
            .validate()
            .is_err()
    );
}

#[test]
fn menu_builders_reject_invalid_menu_trees() {
    assert!(crate::MenuBarBuilder::new().validate().is_err());
    assert!(
        crate::MenuBuilder::new("")
            .action("Open", MenuBuilderOpen)
            .validate()
            .is_err()
    );
    assert!(
        crate::MenuBuilder::new("File")
            .action(" Open", MenuBuilderOpen)
            .validate()
            .is_err()
    );
    assert!(
        crate::MenuBuilder::new("File")
            .submenu(crate::MenuBuilder::new("Recent"))
            .validate()
            .is_err()
    );
    assert!(
        crate::MenuBarBuilder::new()
            .menu(crate::MenuBuilder::new("File").action("Open", MenuBuilderOpen))
            .menu(crate::MenuBuilder::new("File").action("Save", MenuBuilderSave))
            .validate()
            .is_err()
    );
    assert!(
        crate::MenuBuilder::new("File")
            .action("Open\nRecent", MenuBuilderOpen)
            .validate()
            .is_err()
    );
    assert!(
        crate::MenuBuilder::new("File")
            .action("Open", MenuBuilderOpen)
            .submenu(crate::MenuBuilder::new("Recent\0").action("Project", MenuBuilderOpen))
            .validate()
            .is_err()
    );
}

/// **Validates: Requirements 29.1**
///
/// Verify that font features can be constructed without panic.
#[test]
fn font_feature_constructs() {
    let feature = crate::FontFeature::new(*b"liga", 1);
    assert_eq!(feature.tag_str(), Some("liga"));
    assert_eq!(feature.value, 1);
}

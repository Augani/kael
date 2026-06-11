// Feature: platform-parity-electron-features, Cross-platform smoke tests

use std::sync::Mutex;

use crate::{BiometricStatus, NetworkStatus, PermissionStatus, TraceEvent, TracePhase};

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
    let action = crate::NotificationAction {
        id: "open".to_string(),
        label: "Open".to_string(),
    };
    let json = serde_json::to_string(&action);
    assert!(json.is_ok());
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

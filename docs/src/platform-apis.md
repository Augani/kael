# Platform APIs

Kael provides native platform integration matching (and exceeding) Electron's capabilities. All APIs work cross-platform on macOS, Windows, and Linux.

---

## File Dialogs

Native open/save file pickers:

```rust
// Open file dialog
let paths = cx.prompt_for_paths(PathPromptOptions {
    files: true,
    directories: false,
    multiple: true,
    prompt: Some("Open".into()),
}).await;

// Save file dialog
let path = cx.prompt_for_new_path(
    &std::env::current_dir()?,
    Some("document.txt"),
).await;
```

---

## Native Menus

Application menu bar (macOS menu bar, Windows/Linux window menu):

```rust
cx.set_menus(vec![
    Menu {
        name: "File".into(),
        items: vec![
            MenuItem::action("New", menu_action::New),
            MenuItem::action("Open...", menu_action::Open),
            MenuItem::separator(),
            MenuItem::action("Save", menu_action::Save),
            MenuItem::action("Save As...", menu_action::SaveAs),
            MenuItem::separator(),
            MenuItem::action("Quit", menu_action::Quit),
        ],
    },
    Menu {
        name: "Edit".into(),
        items: vec![
            MenuItem::action("Undo", menu_action::Undo),
            MenuItem::action("Redo", menu_action::Redo),
            MenuItem::separator(),
            MenuItem::action("Cut", menu_action::Cut),
            MenuItem::action("Copy", menu_action::Copy),
            MenuItem::action("Paste", menu_action::Paste),
        ],
    },
]);
```

---

## System Tray

Tray icon with menu and click handling:

```rust
// Set tray menu
cx.set_tray_menu(vec![
    TrayMenuItem::Action {
        label: "Show Window".into(),
        id: "show".into(),
    },
    TrayMenuItem::Separator,
    TrayMenuItem::Action {
        label: "Quit".into(),
        id: "quit".into(),
    },
]);

cx.set_tray_tooltip("My App — Running");

// Handle tray menu actions
cx.on_tray_menu_action(|action_id, cx| {
    if action_id.as_ref() == "show" {
        // bring window to front
    } else if action_id.as_ref() == "quit" {
        cx.quit();
    }
});

// Handle tray icon clicks
cx.on_tray_icon_event(|event, cx| {
    match event {
        TrayIconEvent::LeftClick => { /* toggle window */ },
        TrayIconEvent::DoubleClick => { /* show window */ },
        _ => {}
    }
});
```

---

## Clipboard

Read and write text and images:

```rust
// Write text
cx.write_to_clipboard(ClipboardItem::new_string("Hello, clipboard!".into()));

// Write text with metadata
cx.write_to_clipboard(ClipboardItem::new_string_with_metadata(
    "formatted text".into(),
    json!({"source": "my_app"}).to_string(),
));

// Read
if let Some(item) = cx.read_from_clipboard() {
    if let Some(text) = item.text() {
        println!("Got: {}", text);
    }
}
```

---

## Global Hotkeys

System-wide keyboard shortcuts (work even when app is unfocused):

```rust
cx.register_global_hotkey(1, &Keystroke::parse("cmd-shift-k")?)?;

cx.on_global_hotkey(|id| {
    match id {
        1 => { /* Cmd+Shift+K pressed anywhere */ },
        _ => {}
    }
});
```

---

## Notifications

OS-level notifications (not in-app toasts):

```rust
cx.show_notification("Build Complete", "All tests passed")?;

cx.show_notification_with_actions(
    "Update Available",
    "Version 2.0 is ready to install",
    &[
        NotificationAction { id: "install".into(), label: "Install Now".into() },
        NotificationAction { id: "later".into(), label: "Remind Later".into() },
    ],
    |action_id| {
        println!("User clicked: {}", action_id);
    },
)?;
```

---

## Deep Linking

Register and handle custom URL schemes. These methods are called on `Application` before `.run()`:

```rust
Application::new()
    // Handle all opened URLs
    .on_open_urls(|urls| {
        for url in urls {
            println!("Opened: {}", url);
        }
    })
    // Handle specific scheme with app context
    .on_deep_link("myapp", |url, cx| {
        // Handle myapp://path/to/resource
    })
    .run(|cx| {
        // ...
    });
```

---

## Multi-Window

Open multiple windows with independent views:

```rust
cx.open_window(
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
    },
    |_window, cx| cx.new(|_| SettingsView::new()),
).unwrap();
```

Kael windows follow native platform conventions automatically:

- **Scroll-to-focus** — scrolling over an unfocused Kael window activates it,
  matching standard macOS/Windows behavior
- **Smooth zoom** — double-clicking the titlebar animates the window to fill
  the screen using native Core Animation transitions
- **Live resize** — content reflows smoothly during window drag-resizing

---

## Auto-Update

Built-in application update pipeline that signs releases in CI and verifies them
on the client before anything touches disk. The chain is fail-closed by default:

1. **Sign** — `xtask generate-update-metadata` hashes each artifact and signs an
   `UpdateManifest` (version, channel, URL, SHA-256, size) with an ed25519 key.
2. **Fetch** — the client downloads the JSON feed over the workspace HTTP client.
3. **Verify feed** — the signature is checked against the embedded public key; an
   unsigned or wrongly-signed feed is rejected when the policy requires signing.
4. **Compare** — only strictly-newer semver versions are offered.
5. **Download** — the platform artifact streams to a private staging dir with a
   progress callback.
6. **Verify bytes** — the downloaded bytes must hash to the signed SHA-256 and
   match the signed size, or the package is discarded before install.
7. **Apply + rollback** — the new install is swapped in atomically; on any
   failure the previous version is restored.

### End-to-end client flow

```rust
use kael_release::update::UpdatePolicy;

let config = AutoUpdaterConfig {
    feed_url: "https://releases.myapp.com/update-feed.json".into(),
    check_interval: Duration::from_secs(86_400),
    allow_prerelease: false,
};

let mut updater = AutoUpdater::new(config, current_version, http_client);

// Embed the public key that pairs with the CI signing key.
updater.set_public_key_hex(RELEASE_PUBLIC_KEY_HEX)?;

// Honor a policy: channel + fail-closed signing requirement + check interval.
updater.apply_policy(&UpdatePolicy::default_stable());

if let Some(info) = updater.check_for_updates().await? {
    println!("New version: {}", info.version);

    // Streams in chunks; the callback fires repeatedly with running totals.
    let package = updater.download_update(|p| {
        if let Some(f) = p.fraction() {
            println!("downloading: {:.0}%", f * 100.0);
        }
    }).await?; // Err if the signature, size, or SHA-256 does not verify.

    // Hand off to the platform installer (macOS/Windows/Linux). The macOS path
    // runs `codesign --verify` then swaps the bundle in atomically with
    // rollback; install_and_restart relaunches the app.
    updater.set_installer(std::sync::Arc::new(MacInstaller));
    updater.install_and_restart()?;
    let _ = package;
}
```

### Update policy

`UpdatePolicy` (from `kael_release`) drives behavior. `apply_policy` adopts its
channel, maps `require_signed_feeds` onto signature enforcement, and sets the
check interval. The auto-check/download/install flags and interval are surfaced
via `updater.policy()` for the host app to schedule against.

```rust
let policy = UpdatePolicy {
    channel: UpdateChannel::Stable,
    auto_check: true,
    auto_download: false,
    auto_install: false,
    check_interval_secs: 86_400,
    require_signed_feeds: true, // fail closed: reject unsigned feeds
};
```

`UpdatePolicy::default_stable()` is the conservative default (check only, signed
feeds required).

### Generating signing keys

Generate an ed25519 keypair with the bundled helper:

```bash
cargo run -p xtask -- generate-update-key
```

It prints two 64-hex-character values:

- **Private key** — set as the `KAEL_UPDATE_SIGNING_KEY` repository secret. It is
  the 32-byte ed25519 secret seed, hex-encoded. Equivalent to
  `openssl rand -hex 32` used as a seed (the helper derives the matching public
  key for you).
- **Public key** — embed it in the client (`RELEASE_PUBLIC_KEY_HEX`) and in
  `kael.dist.toml` under `updater.public_key`.

Keep the private key in your CI secret store only; never commit it.

### Feed hosting

CI runs the feed step and uploads `update-feed.json` alongside the release
artifacts. Host it at the `feed_url` from `kael.dist.toml` (any static HTTPS
host: object storage, a CDN, or GitHub Releases). The feed is platform-keyed:

```json
{
  "version": "1.4.0",
  "channel": "stable",
  "url": "https://releases.myapp.com",
  "notes_url": "https://releases.myapp.com/notes/1.4.0",
  "pub_date": "2026-06-11T00:00:00Z",
  "platforms": [
    {
      "platform": "macos",
      "url": "https://releases.myapp.com/Kael-macos.zip",
      "signature": "<base64 ed25519 signature>",
      "checksum": "<sha256 hex>",
      "size_bytes": 12345678
    }
  ]
}
```

The client selects the entry matching the running OS. The signing key is wired
into CI as a guarded secret — when `KAEL_UPDATE_SIGNING_KEY` is absent the feed
step emits an **unsigned** feed (development only) instead of failing the build;
when present, a verification step round-trips the produced feed through
`verify_manifest`. To verify locally:

```bash
KAEL_UPDATE_SIGNING_KEY=<private-hex> \
  cargo run -p xtask -- verify-update-feed --feed dist/update-feed.json
```

> **HTTPS is mandatory.** Manifest URLs that are not `https://` are rejected, and
> verification fails closed: a missing public key, missing signature, wrong key,
> channel mismatch, size mismatch, or SHA-256 mismatch all refuse the install.

---

## Printing

Native print dialog and custom rendering:

```rust
let job = PrintJob::new("Document")
    .orientation(PrintOrientation::Portrait)
    .page(PrintPage::new(size(px(612.0), px(792.0)), |ctx| {
        ctx.draw_text("Hello, printed world!", point(72.0, 72.0), style);
    }));

window.show_print_dialog(job);
```

---

## Power Management

Prevent sleep and detect power state:

```rust
// Prevent display sleep during video playback
let blocker = cx.start_power_save_blocker(PowerSaveBlockerKind::PreventDisplaySleep);

// Check power mode
match cx.power_mode() {
    PowerMode::Performance => { /* full quality */ },
    PowerMode::LowPower => { /* reduce effects */ },
    _ => {}
}

// Detect idle time
if let Some(idle) = cx.system_idle_time() {
    if idle > Duration::from_secs(300) { /* user is away */ }
}

// Listen for sleep/wake
cx.on_system_power_event(|event, cx| {
    match event {
        SystemPowerEvent::WillSleep => { /* save state */ },
        SystemPowerEvent::DidWake => { /* refresh data */ },
        _ => {}
    }
});
```

---

## Session Persistence

Save and restore window positions across launches:

```rust
let store = SessionStore::new("my-app")?;

// Save current window layout
store.save_window_states(&window_states)?;

// Restore on next launch
if let Ok(states) = store.load_window_states() {
    for (id, state) in &states {
        cx.open_window(WindowOptions {
            window_bounds: Some(state.bounds),
            ..Default::default()
        }, |_, cx| cx.new(|_| MyView::new()));
    }
}
```

---

## Display Information

Enumerate monitors and get DPI:

```rust
let displays = cx.displays();
let primary = cx.primary_display();

for display in &displays {
    println!("Display {}: {:?}", display.id(), display.bounds());
}
```

---

## Crash Reporting

Automatic crash capture with remote submission:

```rust
use kael::CrashReporter;

let mut reporter = CrashReporter {
    app_id: "my-app".into(),
    crash_dir: std::env::temp_dir().join("crashes"),
    ..Default::default()
};

reporter.install_hook();
```

The panic hook above only captures Rust panics. To also capture native crashes
(segfaults, aborts, illegal instructions, and FFI/GPU-driver crashes) and submit
prior crashes on the next launch with user consent, use the `kael_diagnostics`
reporter and its `install_native()` / `check_and_submit_pending()` APIs. See
[Crash Reporting](crash-reporting.md) for installation, consent, the per-platform
capture matrix, and symbolication guidance.

## App Lifecycle

Launch at login and update the dock/taskbar:

```rust
cx.set_auto_launch("com.example.app", true)?;
let enabled = cx.is_auto_launch_enabled("com.example.app");

cx.set_dock_badge(Some("3"));   // None clears it
cx.set_dock_menu(dock_menu_items);
```

Enforce a single running instance — acquire a lock at startup and forward later launches to the existing process:

```rust
use kael::{SingleInstance, send_activate_to_existing};

match SingleInstance::acquire("com.example.app") {
    Ok(instance) => {
        instance.on_activate(Box::new(|| { /* focus the existing window */ }));
        // ... run the app ...
    }
    Err(_already_running) => {
        send_activate_to_existing("com.example.app")?;
        return; // this duplicate launch exits
    }
}
```

## Biometric Authentication

Gate sensitive actions behind Touch ID / Face ID / Windows Hello. Check availability, then prompt with a reason string and a completion callback:

```rust
use kael::BiometricStatus;

if let BiometricStatus::Available(_kind) = cx.biometric_status() {
    cx.authenticate_biometric("Unlock your vault", |success| {
        if success { /* proceed */ }
    });
}
```

`BiometricStatus` is `Available(BiometricKind)` or `Unavailable`; `BiometricKind` identifies the method (Touch ID, Face ID, fingerprint, Windows Hello).

## Screen & Media Capture

Enumerate capturable displays/windows and stream frames (build with the `screen-capture` feature):

```rust
if cx.is_screen_capture_supported() {
    // enumerate sources via cx.screen_capture_sources(..), then start a
    // capture stream whose frames arrive as ScreenCaptureFrame values
}
```

See `examples/capture_demo.rs`.

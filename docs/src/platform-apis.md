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
// Open a second window
cx.open_window(
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
    },
    |_window, cx| cx.new(|_| SettingsView::new()),
).unwrap();
```

---

## Auto-Update

Built-in application update pipeline:

```rust
let config = AutoUpdaterConfig {
    feed_url: "https://releases.myapp.com/appcast.xml".into(),
    ..Default::default()
};

let updater = AutoUpdater::new(config, current_version, http_client);

// Check for updates
let status = updater.check_for_updates().await;
match status {
    UpdateStatus::UpdateAvailable(info) => {
        println!("New version: {}", info.version);
    }
    UpdateStatus::UpToDate => println!("Already up to date"),
    _ => {}
}
```

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

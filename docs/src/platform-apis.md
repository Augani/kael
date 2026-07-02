# Platform APIs

Kael provides native platform integration for desktop apps without requiring an
Electron runtime. Coverage is intentionally broad, but support varies by OS and
by feature. Query `CapabilityReport::current()` before depending on platform
features that may be partial or unavailable on a target desktop.

```rust
let report = CapabilityReport::current();

// Strict gate: requires full support.
report.require(PlatformFeature::WebView)?;

// Usable gate: accepts Full, Partial, or RequiresInit and lets the app choose
// an explicit setup/fallback path.
if report.is_available(PlatformFeature::GlobalHotkeys) {
    cx.register_global_hotkeys(
        GlobalHotkeyBuilder::new()
            .parse_named_hotkey(1, "Command Palette", "cmd-shift-k")?,
    )?;
}

let readiness = CapabilityCheck::new()
    .require(PlatformFeature::WebView)
    .require_available(PlatformFeature::Notifications)
    .prefer_available(PlatformFeature::GlobalHotkeys)
    .evaluate(&report);

if let Some(summary) = readiness.required_failure_summary() {
    anyhow::bail!("this app cannot run on this desktop yet: {summary}");
}
```

For interaction-heavy apps, gate direct input assumptions explicitly:

```rust
let input = CapabilityCheck::new()
    .require(PlatformFeature::PrecisionPointerInput)
    .prefer_available(PlatformFeature::GestureInput)
    .prefer_available(PlatformFeature::TouchInput)
    .prefer_available(PlatformFeature::PenInput)
    .evaluate(&CapabilityReport::current());

if !input.missing_preferred().is_empty() {
    /* show mouse/keyboard controls alongside gesture affordances */
}
```

`PrecisionPointerInput`, `TouchInput`, `PenInput`, and `GestureInput` separate
mouse/trackpad support from direct touchscreen and stylus streams. This keeps
generated drawing, whiteboard, CAD, kiosk, and tablet-mode apps from assuming
Chromium-style pointer events exist on every native backend.

---

## File Dialogs

Native open/save file pickers:

```rust
// Open file dialog
let paths = cx
    .show_open_dialog(
        OpenDialogBuilder::files()
            .image_files()
            .filter("Markdown", ["md", "markdown"])
            .prompt("Open")
    )
    .await??;

// Directory picker
let projects = cx
    .show_open_dialog(
        OpenDialogBuilder::directory()
            .multiple(true)
            .prompt("Choose projects")
    )
    .await??;

// Save file dialog
let path = cx
    .show_save_dialog(
        SaveDialogBuilder::new(std::env::current_dir()?)
            .suggested_name("document")
            .text()
    )
    .await??;
```

Use `.image_files()`, `.audio_files()`, `.video_files()`, `.pdf_files()`,
`.text_files()`, or `.filter(name, extensions)` for named extension filters.
Use `SaveDialogBuilder::default_extension(...)`, `.pdf()`, `.text()`, or
`.json()` when a suggested save name should get a default extension only if the
user has not already supplied one.
Open prompts reject empty, padded, control-character, and overlong generated
labels. Save dialogs reject empty directories, empty or padded suggested names,
path separators in suggested names, and malformed default extensions.
The lower-level `prompt_for_paths(PathPromptOptions { ... })` and
`prompt_for_new_path(...)` calls remain available when you already have raw
options.

For apps that reopen documents, projects, or export locations later, convert
user-approved paths into checked file access bookmarks:

```rust
let bookmark = FileAccessBookmark::builder("project.main", project_dir)
    .scope(PathScope::UserSelected)
    .read_write()
    .require_existing_path()
    .canonicalize_path()
    .ttl_seconds(60 * 60 * 24)
    .build_checked()?;

let mut tokens = AccessTokenStore::new();
let token = bookmark.issue_token(&mut tokens, now_unix_seconds)?;
```

`FileAccessBookmarkBuilder` validates stable bookmark IDs, path text, optional
existence/canonicalization, read/write mode, and token TTL. Use
`bookmark.capabilities()` to translate a bookmark into filesystem capabilities
for the permission broker, and `AccessTokenStore` to validate/revoke temporary
file access grants.

For native file drops, convert raw dropped paths into a checked app intent
before importing, opening, or playing them:

```rust
let drop = cx.file_drop_intent_checked(
    FileDropIntentBuilder::media_source()
        .paths(dropped_paths)
        .max_paths(8)
        .canonicalize_paths(),
)?;

for path in drop.paths() {
    open_media(path)?;
}
```

`FileDropIntentBuilder` gives drops a semantic purpose such as open document,
import files, import folder, media source, project workspace, or a custom app
purpose. It validates non-empty paths, optional existence, file-vs-directory
policy, extension allowlists, max path count, canonicalization, and duplicate
paths before work starts. The lower-level drop-zone filter remains useful for
hover feedback; this intent builder is the app-owned gate after the user drops.

For outbound drags, generated exports, and Electron-style file promises, build
a checked export descriptor before starting a platform drag session:

```rust
let export = cx.file_export_drag_checked(
    FileExportDragIntentBuilder::generated_files("Drag generated image.")
        .virtual_file_with_mime("preview.png", "image/png", image_bytes)
        .max_virtual_file_bytes(32 * 1024 * 1024),
)?;

if CapabilityReport::current().is_available(PlatformFeature::FileExportDrag) {
    // Hand export.items() to the native drag-source backend.
}
```

`FileExportDragIntentBuilder` supports existing file paths and virtual files
with generated bytes. It validates user-facing purpose text, item count, safe
file names, optional MIME types, non-empty virtual bytes, virtual file size
limits, optional existence for existing paths, and deduplicates repeated path
items. Existing-path exports declare a `Capability::FilesystemRead` requirement
with `PathScope::UserSelected`; generated virtual files require no filesystem
capability. This gives designers, media tools, and AI artifact generators an
app-owned native export path without forcing a WebView download.

After a dialog, drop, recent-document restore, or file deep link, classify paths
before routing them to document, media, data, project, or archive handlers:

```rust
let intake = cx.file_intake_plan_checked(
    FileIntakePlanBuilder::new()
        .paths(paths)
        .canonicalize_paths()
        .reject_unknown(),
)?;

for video in intake.paths_of_kind(FileIntakeKind::Video) {
    open_video(video)?;
}
```

`FileIntakePlanBuilder` validates non-empty paths, optional existence, max path
count, canonicalization, deduplication, and optional rejection of unknown file
kinds. Entries expose normalized extensions and coarse `FileIntakeKind` values:
directory, project, image, audio, video, PDF, text, data, archive, or unknown.

Declare the document types an app owns before packaging or installer generation:

```rust
let associations = cx.file_associations_checked(
    FileAssociationSetBuilder::new()
        .association(
            FileAssociationBuilder::new("Markdown")
                .extensions(["md", "markdown"])
                .mime_type("text/markdown")
                .editor(),
        )
        .association(
            FileAssociationBuilder::new("Kael Project")
                .extension("kaelproj")
                .mime_type("application/x-kael-project")
                .editor(),
        ),
)?;

if associations.accepts_extension("md") {
    enable_markdown_open_flow();
}
```

`FileAssociationSetBuilder` is validation-only metadata for bundlers,
installers, docs, and agents. It normalizes extensions, validates MIME types,
and rejects duplicate extension or MIME claims across associations. Runtime file
opens still arrive through open requests, dialogs, recent documents, drops, or
platform-specific installer registration.

For Electron `app.getFileIcon(...)` style file explorers, recent-document rows,
upload pickers, and project launchers, build a checked native file icon request
before invoking a platform icon backend:

```rust
let icon = cx.file_icon_request_checked(
    FileIconRequestBuilder::new(project_path)
        .large()
        .require_existing_path(),
)?;

let planned = cx.file_icon_request_checked(
    FileIconRequestBuilder::new("Draft.kaelproj")
        .small(),
)?;
```

`FileIconRequestBuilder` validates non-empty/NUL-free paths, optional existence
requirements, optional canonicalization, small/normal/large/custom icon sizes,
and generic extension fallback for planned or missing paths. It does not render
the icon by itself; it is the typed handoff to platform icon extraction.

When setup code needs Electron `app.setAsDefaultProtocolClient(...)` or
document-default intent, build a checked default-handler plan before touching OS
registration APIs:

```rust
let defaults = cx.default_handler_plan_checked(
    DefaultHandlerPlanBuilder::new("com.example.kael-studio")
        .app_name("Kael Studio")
        .schemes(["kael", "kael-auth"])
        .file_association(
            FileAssociationBuilder::new("Kael Project")
                .extension("kaelproj")
                .mime_type("application/x-kael-project")
                .editor(),
        )
        .current_user_scope(),
)?;

if defaults.claims_scheme("kael") {
    // Safe to present a setup prompt or pass to platform registration glue.
}
```

`DefaultHandlerPlanBuilder` validates the app identifier, app name, URL schemes,
file associations, duplicate claims, and requested scope. It does not mutate OS
defaults by itself; use it as the typed handoff to installer code, first-run
setup, or platform-specific default-app registration.

Package identity, deep links, and document claims can be composed into a single
checked manifest for bundlers and installers:

```rust
let manifest = cx.package_manifest_checked(
    AppPackageManifestBuilder::new(
        AppMetadataBuilder::new("Kael Studio")
            .identifier("com.example.kael-studio")
            .version(env!("CARGO_PKG_VERSION")),
    )
    .url_schemes(UrlSchemeRegistrationBuilder::new().schemes(["kael", "kael-auth"]))
    .file_associations(
        FileAssociationSetBuilder::new().association(
            FileAssociationBuilder::new("Kael Project")
                .extension("kaelproj")
                .mime_type("application/x-kael-project")
                .editor(),
        ),
    )
    .icons(
        AppIconSetBuilder::new()
            .icon(AppIconAssetBuilder::app("assets/app.icns"))
            .icon(AppIconAssetBuilder::tray("assets/tray.svg").template())
            .icon(AppIconAssetBuilder::document("assets/document.png").size_px(128)),
    )
    .privacy_permissions(
        AppPrivacyManifestBuilder::new()
            .permission(AppPrivacyPermissionBuilder::camera(
                "Camera access records video notes.",
            ))
            .permission(AppPrivacyPermissionBuilder::microphone(
                "Microphone access records narration.",
            )),
    ),
)?;

let mac_documents = manifest.macos_document_types();
let linux_mime_types = manifest.linux_desktop_mime_types();
let windows_associations = manifest.windows_file_associations();
let tray_icons = manifest.icons_for(AppIconPurpose::Tray);
let mac_usage_descriptions = manifest.macos_usage_descriptions();

let readiness = manifest.readiness_report();
if !readiness.is_ready() {
    return Err(anyhow::anyhow!(readiness.summary()));
}

let dist = cx.distribution_plan_checked(
    AppDistributionPlanBuilder::new("/tmp/kael-dist")
        .target(AppDistributionTargetBuilder::dmg())
        .target(AppDistributionTargetBuilder::msi().channel("stable"))
        .target(AppDistributionTargetBuilder::appimage()),
)?;

let artifact_paths = dist.artifact_paths(&manifest);

let signing = cx.signing_plan_checked(
    AppSigningPlanBuilder::new()
        .target(
            AppSigningTargetBuilder::macos_developer_id(
                "Developer ID Application: Example, Inc.",
            )
            .team_id("ABCDE12345")
            .hardened_runtime()
            .notarize(),
        )
        .target(AppSigningTargetBuilder::windows_authenticode(
            "Example Code Signing Cert",
        ))
        .target(AppSigningTargetBuilder::linux_package("kael-release-key")),
)?;

if !signing.covers_distribution_plan(&dist) {
    return Err(anyhow::anyhow!("missing signing declaration for release target"));
}
```

`AppPackageManifestBuilder` requires a validated app identifier and reuses the
checked URL-scheme, file-association, icon-asset, and privacy-permission
builders. The result exposes platform-shaped metadata for `CFBundleURLTypes`,
`CFBundleDocumentTypes`, Linux `.desktop` `MimeType=` entries, Windows
installer/ProgID generation, app/tray/document/installer icon handoff, and
known macOS usage-description entries. `AppIconSetBuilder` validates icon paths,
supported formats (`png`, `ico`, `icns`, `svg`), optional pixel sizes,
template/monochrome tray intent, and duplicate declarations.
`AppPrivacyManifestBuilder` validates user-facing privacy reasons and rejects
duplicate permission declarations for camera, microphone, screen capture,
location, notifications, filesystem, network, USB, HID, serial-port, and
Bluetooth intent. These declarations do not grant runtime access; continue to
use the permission broker and capability checks for actual process permissions.
This is not a bundler by itself; it is the typed handoff point for packaging
tools, release scripts, and AI agents.
`AppPackageReadinessBuilder` provides the release gate over that handoff: by
default it reports blocking errors for missing app versions and primary app
icons, plus warnings for file associations without document icons, extension
claims without MIME metadata, and privacy declarations without known platform
usage-description exports. Use the explicit `allow_*` methods only when a
release script intentionally accepts one of those gaps.
`AppDistributionPlanBuilder` covers the Electron-builder target-list side of
the workflow. It validates an absolute output directory, known artifact formats
for macOS (`dmg`, `mac-zip`), Windows (`msi`, `nsis`), and Linux (`appimage`,
`deb`, `rpm`, `tar-gz`), duplicate format/channel pairs, and portable release
channel labels. The result derives predictable artifact paths from the checked
manifest, but still leaves the actual bundling/signing/notarization work to the
platform-specific packaging tool.

For native geolocation, declare packaging intent and build a checked runtime
request before prompting the OS:

```rust
let location = cx.location_request_checked(
    LocationRequestBuilder::new("Show nearby workspaces.")
        .balanced()
        .timeout(Duration::from_secs(10))
        .maximum_age(Duration::from_secs(300)),
)?;

let privacy = location.privacy_permission();
let capability = location.required_capability();
```

`LocationRequestBuilder` validates user-facing purpose text, timeout, maximum
cached-location age, and background/accuracy combinations. Gate execution with
`CapabilityReport::current().is_available(PlatformFeature::Geolocation)` and
the permission broker's `Capability::Location`. Packaging still uses
`AppPrivacyPermissionBuilder::location(...)` so installers and macOS usage
descriptions stay aligned with runtime access.

For WebUSB/WebHID/Web Serial/Web Bluetooth-style app features, prefer checked
native device descriptors instead of routing through hidden browser pages:

```rust
let usb = cx.device_access_request_checked(
    DeviceAccessRequest::usb("Read measurements from the USB scale.")
        .vendor_product(0x1234, 0xabcd)
        .timeout(Duration::from_secs(20)),
)?;

let bluetooth = cx.device_access_request_checked(
    DeviceAccessRequest::bluetooth("Pair with the heart-rate strap.")
        .service_uuid("180D")
        .allow_background(),
)?;
```

`DeviceAccessRequestBuilder` validates user-facing purpose text, timeouts,
USB/HID vendor/product filters, serial port hints, Bluetooth service UUIDs, and
rejects filters that belong to a different device family. Gate execution with
`PlatformFeature::UsbDevices`, `HidDevices`, `SerialPorts`, or
`BluetoothDevices`, request the matching capability (`Capability::UsbDevice`,
`HidDevice`, `SerialPort`, or `Bluetooth`) through the permission broker, and
include `request.privacy_permission()` in packaging metadata. Current platform
reports expose these as checked descriptors first; backend device discovery and
I/O can then be implemented per OS without hiding risk inside WebView code.

`AppSigningPlanBuilder` covers the release-trust side of the same workflow. It
validates one signing declaration per platform, optional identity and team
labels, macOS-only hardened-runtime/notarization flags, timestamp intent, and
the rule that notarization requires a macOS identity. Use
`covers_distribution_plan(&dist)` before release so scripts and agents fail
early when an artifact target has no signing policy. The checked plan expresses
intent only; platform signing, timestamping, and notarization commands still run
in the packaging backend.

---

## Message Dialogs

Native message boxes and confirmations:

```rust
// Alert-style message
let rx = cx.show_message_dialog(
    MessageDialogBuilder::info("Export Complete", "The report was saved.")
)?;
let button_index = rx.await?;

// Destructive confirmation: 0 = Cancel, 1 = Delete
let rx = cx.show_message_dialog(
    MessageDialogBuilder::destructive_confirm("Delete Draft?", "This cannot be undone", "Delete")
        .detail("The draft will be removed from this device.")
)?;

if rx.await? == 1 {
    delete_draft()?;
}

// Unsaved changes: 0 = Cancel, 1 = Don't Save, 2 = Save
let rx = cx.show_message_dialog(
    MessageDialogBuilder::save_discard_cancel(
        "Save changes?",
        "This document has unsaved changes.",
    )
)?;
let button_index = rx.await?;

// Error dialog
cx.show_message_dialog(
    MessageDialogBuilder::error("Export Failed", "Could not write the file")
        .detail(error.to_string())
)?;
```

`MessageDialogBuilder` rejects empty, padded, control-character, duplicate, too
many, and overly long generated labels before native dialogs show ambiguous
button copy. Use lower-level `show_dialog(DialogOptions { ... })` when you
already have raw platform dialog options. Use `.default_button(index)` and
`.cancel_button(index)` when you need to preserve Electron-style default or
escape-key intent for a custom button order.

---

## Native Menus

Application menu bar (macOS menu bar, Windows/Linux window menu):

```rust
cx.set_menus_checked(
    MenuBarBuilder::new()
        .menu(
            MenuBuilder::new("File")
                .action("New", menu_action::New)
                .action("Open...", menu_action::Open)
                .separator()
                .action("Save", menu_action::Save)
                .action("Save As...", menu_action::SaveAs)
                .separator()
                .action("Quit", menu_action::Quit),
        )
        .menu(
            MenuBuilder::standard_edit(
                "Edit",
                menu_action::Undo,
                menu_action::Redo,
                menu_action::Cut,
                menu_action::Copy,
                menu_action::Paste,
                menu_action::SelectAll,
            ),
        ),
)?;
```

`MenuBuilder::standard_edit(...)` provides the common Electron-style Edit menu
shape with native OS role mappings for Undo, Redo, Cut, Copy, Paste, and Select
All. Checked menu builders reject empty, padded, control-character, and overly
long labels, plus empty menus and duplicate top-level menu names before native
installation.

Raw `set_menus(...)`, `Vec<Menu>` values, and `MenuItem::action(...)` remain
available for code that already validates or constructs menu trees manually.

---

## Context Menus

Native context menus for right-click, secondary-click, or command surfaces:

```rust
cx.show_context_menu_checked(
    mouse_position,
    NativeContextMenuBuilder::new()
        .action("Open", "open")
        .separator()
        .submenu(
            "Sort",
            NativeContextMenuBuilder::new()
                .action("By Name", "sort-name")
                .toggle("Descending", false, "sort-desc"),
        )
        .action("Reveal in Folder", "reveal"),
    |action_id, _cx| match action_id.as_ref() {
        "open" => {}
        "reveal" => {}
        _ => {}
    },
)?;
```

Raw `Vec<TrayMenuItem>` values remain available because context menus and tray
menus share the same native item model. Use `show_context_menu(...)` when you
already validated a raw item tree yourself.

---

## System Tray

Tray icon with menu and click handling:

```rust
// Builder-friendly path
cx.configure_tray_app_checked(
    TrayAppBuilder::new()
        .action("Show Window", "show")
        .separator()
        .toggle("Pause Sync", false, "pause-sync")
        .submenu(
            "Status",
            TrayMenuBuilder::new()
                .toggle("Available", true, "available")
                .action("Set Away", "away"),
        )
        .action("Quit", "quit")
        .status_tooltip("My App - Running")
        .panel()
        .keep_alive_without_windows(true),
)?;

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

Use `TrayAppBuilder` when building a background/tray app: it validates and
applies menu items, tooltip text, panel-mode click behavior, and
`keep_alive_without_windows` together so startup cannot partially install an
invalid tray surface. Use `set_tray_menu_checked(...)`,
`set_tray_tooltip_checked(...)`, and `set_tray_panel_mode(...)` when those pieces
are owned by separate parts of the app.

Use `TrayTooltipBuilder::status(...)` or `text(...)` for short background-app
state and `clear()` when no tooltip should be shown. The checked path rejects
empty tooltips, padded text, control characters, and text longer than 256
characters before platform UI receives it. Raw `set_tray_tooltip(...)` remains
available for already-validated or platform-specific tooltip behavior.

The lower-level enum remains available when you already have menu items:

```rust
cx.set_tray_menu(vec![
    TrayMenuItem::action("Show Window", "show"),
    TrayMenuItem::separator(),
    TrayMenuItem::action("Quit", "quit"),
]);
```

---

## Clipboard

Read and write text and images. For plain text, use the convenience methods:

```rust
// Write checked text from generated commands or app-owned copy actions.
cx.write_clipboard_text_checked("Hello, clipboard!")?;

// Read text
if let Some(text) = cx.read_clipboard_text()? {
    println!("Got: {}", text);
}
```

Use `ClipboardItem::builder()` when you need metadata, images, or multi-entry
payloads:

```rust
// Electron-style rich HTML with plain-text fallback.
cx.write_clipboard_html(
    "Quarterly report",
    "<strong>Quarterly report</strong>",
)?;

cx.write_clipboard_item(
    ClipboardItem::builder()
        .try_text_with_json_metadata("formatted text", json!({"source": "my_app"}))?
        .image_ref(&image),
)?;

// Read
if let Some(item) = cx.read_from_clipboard()? {
    if let Some(text) = item.text() {
        println!("Got: {}", text);
    }
    if let Some(image) = item.first_image() {
        println!("Got image: {:?}", image.format());
    }
    if let Some(html) = item.html() {
        println!("Got HTML: {}", html);
    }
}
```

`ClipboardItem::builder().html(plain_text, html)?` stores rich HTML metadata
next to a paste-safe plain-text fallback. Checked clipboard writes reject empty
items, empty text, empty metadata, empty image bytes, empty HTML, and NUL/control
characters before platform clipboard code receives the payload. The lower-level
`write_clipboard_text(...)` and `write_to_clipboard(...)` methods remain
available for already-validated custom integrations.

---

## Share Sheet

Enable the `share` feature to hand text, links, images, and file attachments to
the operating system share UI.

```rust
let result = cx
    .show_share_sheet(
        ShareSheet::builder()
            .subject("Build report")
            .text("All checks passed")
            .url("https://example.com/report")
            .file(report_path)
            .exclude(ShareType::Social),
    )
    .await?;
```

Use `ShareItem::{text,url,file,files,image}` or
`ShareSheet::{text,url,file,files}` for one-line payloads, and
`ShareSheet::builder()` / `ShareSheetBuilder::new()` for export bundles. The
checked path validates at least one non-empty payload, URL schemes, image MIME
types and bytes, and file existence before invoking the platform backend.
`cx.share_support()` reports the current backend destinations, while
`cx.show_share_sheet_checked(sheet).await?` accepts a fully built `ShareSheet`.

---

## Secure Credentials

Store login tokens, refresh tokens, or service credentials in the platform
keychain / credential manager:

```rust
let write = cx.write_secure_credential(
    CredentialBuilder::new("https://api.example.com")
        .username("ada")
        .password(refresh_token),
)?;

write.await?;

if let Some(credential) = cx
    .read_secure_credential_checked(CredentialServiceBuilder::new("https://api.example.com"))?
    .await?
{
    println!("credential for {}", credential.username());
}

cx.delete_secure_credential_checked(
    CredentialServiceBuilder::new("https://api.example.com"),
)?.await?;
```

`CredentialBuilder` validates the service key, username, and secret before
calling the OS keychain API. Use `CredentialServiceBuilder` for read/delete
paths so generated service values are checked too. Service and username values
may not be empty, accidentally padded with whitespace, overly long, or contain
control characters, and secrets may not be empty. The lower-level
`write_credentials(...)`, `read_credentials(...)`, `delete_credentials(...)`,
`read_secure_credential(...)`, and `delete_secure_credential(...)` methods remain
available when an integration already manages raw keychain tuples.

---

## Permissions

Use `PermissionRequestBuilder` to check and request common OS permissions from a
single startup path. The returned snapshot reports the status before any prompt
was launched, and microphone/camera callbacks still receive the platform prompt
result asynchronously:

```rust
let permissions = cx.request_permissions(
    PermissionRequestBuilder::startup_privacy()
        .microphone_with_callback(|granted| {
            println!("microphone granted: {granted}");
        })
        .camera_with_callback(|granted| {
            println!("camera granted: {granted}");
        }),
)?;

if permissions.has_blocking_denial() {
    if let Some(summary) = permissions.blocking_denial_summary() {
        eprintln!("permissions blocked: {summary}");
    }
    for denial in permissions.blocking_denials() {
        // Route denial.key to settings guidance or a fallback feature path.
    }
}

if permissions.has_pending_permission() {
    for permission in permissions.pending_permissions() {
        // Route permission.key to waiting UI while the OS prompt is unresolved.
    }
}
```

Use `PermissionRequestBuilder::startup_privacy()` for accessibility,
microphone, and camera checks, `capture_studio()` for capture/recording startup
flows, and `media_devices()` when you only need microphone and camera. Inspect
`requested_permissions()`, `granted_permissions()`, `pending_permissions()`,
`granted_summary()`, and `blocking_denial_summary()` to drive setup screens,
fallbacks, and settings links without parsing OS-specific strings.

Use the lower-level `accessibility_status()`, `microphone_status()`,
`camera_status()`, and individual request methods when a feature needs to ask
for exactly one permission at the moment of use.

---

## Accessibility Semantics

Custom UI should declare semantic roles, labels, values, states, and actions
before it is exposed to the platform accessibility backend:

```rust
let attrs = AccessibilityAttributes::switch("Enable sync", enabled)
    .disabled(is_busy);
attrs.validate()?;

let report = attrs.audit_report();
if !report.is_ready() {
    anyhow::bail!(report.summary());
}

div()
    .track_focus(&focus)
    .tab_stop(true)
    .accessibility(attrs);
```

Recipes cover buttons, links, checkboxes, switches, radio buttons, sliders,
progress bars, and text inputs. Use `AccessibilityAttributes::validate()` for a
fail-fast component check, and `audit_report()` when generated UI or AI agents
need every issue at once. Full `AccessibilityTree::audit_report()` catches
tree-level problems such as missing children, parent mismatches, multiple
focused nodes, hidden focused nodes, missing interactive names/actions,
conflicting states, unknown roles, and invalid range values before emitting a
platform tree.

---

## Global Hotkeys

System-wide keyboard shortcuts (work even when app is unfocused):

```rust
cx.register_global_hotkeys_checked(
    GlobalHotkeyBuilder::new()
        .parse_named_hotkey(1, "Command Palette", "cmd-shift-k")?
        .parse_named_hotkey(2, "Toggle Capture", "cmd-alt-c")?,
)?;

cx.on_global_hotkey(|id| {
    match id {
        1 => { /* Command palette pressed anywhere */ },
        2 => { /* Toggle capture pressed anywhere */ },
        _ => {}
    }
});

cx.on_global_hotkey_up(|id| {
    if id == 1 { /* hotkey released */ }
});
```

The lower-level ID API remains available when you already parsed the keystroke:

```rust
cx.register_global_hotkey(1, &Keystroke::parse("cmd-shift-k")?)?;
```

Use `register_global_hotkeys(...)` for permissive raw sets. The checked builder
path rejects empty sets, duplicate IDs, and duplicate keystrokes before platform
registration begins.

### Platform behaviour

| Platform | Backend | Notes |
| --- | --- | --- |
| macOS | Carbon/`NSEvent` monitors | Immediate registration. |
| Windows | `RegisterHotKey` | Immediate registration. |
| Linux (X11) | `XGrabKey` on the root window | Immediate registration; fails if another client already holds the grab. |
| Linux (Wayland) | `org.freedesktop.portal.GlobalShortcuts` desktop portal | Interactive and asynchronous — see below. |

### Wayland: the GlobalShortcuts portal

Wayland compositors do not let arbitrary clients grab keys, so global hotkeys go
through [`org.freedesktop.portal.GlobalShortcuts`][portal] provided by
`xdg-desktop-portal` and a backend that implements the interface (GNOME, KDE
Plasma, Hyprland's portal, and others). The flow is `CreateSession` →
`BindShortcuts` → listen for `Activated`/`Deactivated` signals, which are routed
into the same `on_global_hotkey` / `on_global_hotkey_up` callbacks used on every
other platform.

Because of how the portal works, Wayland registration differs from the other
backends in three honest ways:

- **It is asynchronous.** `register_global_hotkey` records the request and starts
  the portal session in the background, returning `Ok(())` to mean
  *registered-pending* rather than *bound and live*. The shortcut becomes active
  once the portal confirms the binding.
- **It is interactive.** The first `BindShortcuts` call may show a system dialog
  asking the user to confirm or reassign the shortcuts. Nothing fires until the
  user responds.
- **The trigger may change.** The compositor is free to bind a different key
  combination than the one requested. The preferred trigger is sent as a hint in
  the XDG shortcuts format (e.g. `CTRL+SHIFT+k`, `LOGO+space`); the actual,
  user-facing trigger is reported back and can be read for display.

`register_global_hotkey` returns a descriptive `Err` only when it can fail
synchronously — currently when the keystroke cannot be mapped to an XDG trigger.
If the portal itself is unavailable (no `xdg-desktop-portal`, or a backend that
does not implement GlobalShortcuts), the background session fails and the
shortcut simply never activates; the failure is logged. Query
`CapabilityReport::current()` for `PlatformFeature::GlobalHotkeys`, which reports
`Partial` on Linux with a note describing the portal dependency.

> **Current limitation (v1):** shortcuts are bound to a single portal session.
> Registering additional hotkeys after the session is already bound re-binds the
> full set, which some compositors only honour at session creation. For the most
> predictable behaviour, register all Wayland hotkeys during startup.

[portal]: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html

---

## Focused Window Info

Launchers, capture tools, automation panels, and AI agents often need to inspect
the active desktop window before deciding what to do. Use a checked query when a
feature requires specific metadata instead of accepting any raw platform result:

```rust
if let Some(info) = cx.focused_window_info_checked(
    FocusedWindowQuery::builder()
        .external_only()
        .require_title()
        .require_pid()
        .app_name_contains("code"),
)? {
    println!(
        "focused app={} pid={:?} title={}",
        info.app_name, info.pid, info.window_title
    );
}
```

`FocusedWindowQuery` rejects contradictory generated filters, empty or padded
app names, control characters, zero PIDs, and exact-plus-contains app-name
filters before platform state is read. Use `.external_only()` when the feature
should act on another app, `.current_process_only()` for app-owned windows,
`.bundle_id(...)` for macOS app targeting, `.pid(...)` for exact process
matching, and `.require_title()` when an empty title should be treated as no
match.

Raw `focused_window_info()` remains available for diagnostics and custom
platform-specific handling. On macOS, window-title access may depend on the
Accessibility permission; if titles are missing, pair this with the permission
broker above.

---

## Notifications

OS-level notifications (not in-app toasts). For builder- and agent-authored apps,
prefer `NotificationBuilder`; it validates required fields and keeps action IDs
beside the labels they display:

```rust
cx.show_notification_checked("Build Complete", "All tests passed")?;

cx.show_desktop_notification(
    NotificationBuilder::new("Build Complete", "All tests passed")
)?;

cx.show_desktop_notification_with_actions(
    NotificationBuilder::new("Update Available", "Version 2.0 is ready to install")
        .open_and_dismiss_actions("Install Now", "Remind Later"),
    |action_id| {
        println!("User clicked: {}", action_id);
    },
)?;

cx.show_desktop_notification_with_actions(
    NotificationBuilder::new("Sync Failed", "Could not reach the server")
        .retry_action("Retry")
        .settings_action("Settings")
        .dismiss_action("Later"),
    |action_id| {
        println!("User clicked: {}", action_id);
    },
)?;
```

`NotificationBuilder` rejects empty, padded, control-character, overly long, and
duplicate action data before callback routing becomes ambiguous. Use
`.open_and_dismiss_actions(...)`, `.retry_action(...)`, and
`.settings_action(...)` for common native notification flows; use
`.action(id, label)` when the app owns stable custom action IDs.
For generated plain notifications, `show_notification_checked(...)` is the
shortest validated path and uses the same title/body rules as
`NotificationBuilder`.

The lower-level platform calls are still available when you already have raw
action arrays:

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

## Shell

Open URLs, files, and folders through the operating system:

```rust
// Open a URL in the default browser or registered URL handler.
cx.open_external_url("https://example.com/docs")?;

// Open a file or directory with the system default app.
cx.open_path(project_dir)?;

// Reveal a file or directory in Finder, Explorer, or the desktop file manager.
cx.show_item_in_folder(report_path)?;

// Use a typed target when a command can point at different shell destinations.
cx.open_shell_target(ShellTarget::reveal_path(report_path))?;

// Batch workflow outcomes such as opening help and revealing an export.
cx.open_shell_targets(
    ShellTargetsBuilder::new()
        .url("https://example.com/docs/export")
        .reveal_path(report_path)
        .require_existing_paths(),
)?;

// Validate a move-to-trash/recycle request before shell integration handles it.
let trash = cx.trash_request_checked(TrashRequest::builder(report_path).canonicalize_path())?;
```

`open_external_url(...)` uses the `OpenExternalUrl` capability. `open_path(...)`
and `show_item_in_folder(...)` use the higher-risk `ShellExecute` capability.
`ShellTarget::validate()` and `ShellTargetsBuilder` reject empty or padded URLs,
unsupported shell URL schemes, missing HTTP(S) hosts, empty paths, and NUL
characters before opening each target in order. Use
`.require_existing_paths()` for export/reveal workflows and
`.canonicalize_paths()` when generated paths should be normalized first. Shell
URL targets intentionally allow `http`, `https`, and `mailto`; custom app
schemes should use the deep-link registration APIs. The lower-level
`open_url(...)`, `open_with_system(...)`, and `reveal_path(...)` calls remain
available for platform integrations that already manage capability boundaries.
For Electron `shell.trashItem(...)` style flows, `TrashRequestBuilder` validates
empty paths, NUL bytes, filesystem roots, relative paths unless explicitly
allowed, and missing targets by default. The checked request does not permanently
delete anything; it is the typed handoff for a platform trash/recycle backend.

---

## App Paths

Resolve app-owned storage, cache, log, temp, and download locations without
hard-coding OS directory conventions:

```rust
let paths = cx.app_paths_checked(
    AppPathBuilder::new("com.example.app")
        .all_common()
        .create_dirs(),
)?;

let settings_path = paths.config_dir().unwrap().join("settings.json");
let cache_dir = paths.cache_dir().unwrap();
let log_dir = paths.logs_dir().unwrap();
let download_dir = paths.downloads_dir().unwrap();
```

`AppPathBuilder` validates the app id, rejects duplicate roles, and resolves
common Electron `app.getPath(...)` equivalents: `Data`, `Config`, `Cache`,
`Logs`, `Temp`, and `Downloads`. App-owned roles are scoped by the app id;
`Downloads` returns the user's downloads directory. Use `.create_dirs()` when a
startup path should exist before migrations, logs, databases, downloads, or
background workers begin.

When an app needs a native replacement for Chromium-origin storage, declare the
storage contract explicitly:

```rust
let storage = cx.app_storage_plan_checked(
    AppStoragePlanBuilder::new("com.example.app")
        .settings_json("settings", "settings.json")
        .sqlite_database("main-db", "state/app.sqlite")
        .key_value_store("kv", "kv")
        .blob_cache("thumbnails", "thumbnails")
        .log_file("main-log", "app.log")
        .temp_workspace("exports", "exports"),
)?;

let db_path = storage.entry("main-db").unwrap().absolute_path();
```

`AppStoragePlanBuilder` resolves the needed app path roles and checks every
entry before migrations, settings loads, caches, or background workers start.
Entries declare a kind (`SettingsJson`, `SqliteDatabase`, `KeyValueStore`,
`BlobCache`, `LogFile`, `TempWorkspace`, or custom), durability (`Durable`,
`Rebuildable`, or `Temporary`), relative path, optional max byte budget, and
sensitivity for diagnostics. Paths must stay relative to app-owned roles and
cannot target `Downloads`; duplicate ids, unsafe names, parent-directory
escapes, absolute paths, and invalid quotas fail early. Use entry
`read_capability()` / `write_capability()` when wiring worker or plugin
permissions.

---

## Launch Context

Capture startup arguments and selected environment values without dumping the
whole process environment:

```rust
let launch = cx.launch_context_checked(
    LaunchContextBuilder::new()
        .environment_keys(["KAEL_PROFILE", "APP_CHANNEL"])
        .require_executable()
        .require_current_dir(),
)?;

for arg in launch.args() {
    tracing::debug!(arg, "launch argument");
}

if launch.is_development_mode() {
    tracing::info!("running a development build");
}
```

`LaunchContextBuilder` captures command-line arguments by default and captures
environment variables only from an explicit allowlist. It validates environment
keys, rejects duplicate keys, and can require the executable path or current
directory when startup routing depends on them. Use `cx.launch_context()` for a
best-effort snapshot with args and no environment values.

---

## Helper Processes

Describe app-owned native helper processes without dropping to shell strings:

```rust
let launch = HelperProcessLaunch::utility(
    ProcessId(42),
    "video-transcoder",
    cx.path_for_auxiliary_executable("transcoder")?,
)
.arg("--input")
.arg(input_path.display().to_string())
.env("RUST_LOG", "info")
.inherit_environment_keys(["PATH"])
.capabilities(["media:transcode", "fs/app-data"])
.restart_on_failure(2, Duration::from_millis(250))
.heartbeat_interval(Duration::from_secs(1))
.build_checked()?;

let (info, options) = launch.into_spawn_parts();
supervisor.spawn_with_options(info, options)?;
```

`HelperProcessLaunchBuilder` covers Electron `utilityProcess` and
`child_process`-style app helpers while keeping launch validation outside the
renderer layer. It validates the process class, name, executable path,
arguments, explicit environment variables, inherited environment allowlist,
working directory, declared capability labels, and restart/heartbeat policy.
`ProcessClass::Utility` is the neutral bucket for app-owned tools that are not
UI, media, worker, or extension hosts. Environment inheritance is off by
default; opt into `.inherit_environment_keys(...)` when the helper needs a
small parent-env allowlist.

---

## Support Diagnostics

Collect a copy-paste support report without scraping process state or leaking
startup secrets by default:

```rust
let diagnostics = cx.support_diagnostics_checked(
    SupportDiagnosticsBuilder::new()
        .metadata(
            AppMetadataBuilder::new("Kael Studio")
                .version(env!("CARGO_PKG_VERSION"))
                .identifier("com.example.kael-studio"),
        )
        .app_paths(AppPathBuilder::new("com.example.kael-studio").app_storage()),
)?;

cx.write_clipboard_text(diagnostics.to_text());
```

`SupportDiagnosticsBuilder` includes OS info, best-effort locale, process
metrics, executable path, current directory, and zero command-line arguments or
environment values by default. Call `.include_launch_args()` only when argv is
safe to share, and use `.environment_keys([...])` for an explicit environment
allowlist. App paths are optional and must be side-effect free:
diagnostics reject `AppPathBuilder::create_dirs()` so a support action cannot
create storage directories.

---

## Locale

Read a native locale snapshot for formatting, catalog selection, onboarding, and
support diagnostics:

```rust
let locale = cx.locale_snapshot_checked(
    LocaleSnapshotBuilder::new()
        .locale("de_DE.UTF-8")
        .preferred_languages(["de-DE", "en-US"]),
)?;

if locale.is_rtl() {
    // Choose right-to-left layout defaults.
}
```

`LocaleSnapshotBuilder` accepts explicit locale candidates, then optionally
falls back to `LC_ALL`, `LC_MESSAGES`, `LANG`, and `LANGUAGE` environment
signals. Locale tags normalize underscores to hyphens, strip encoding/modifier
suffixes, and expose `locale()`, `language()`, `region()`,
`preferred_languages()`, `text_direction()`, and `source()`. `cx.locale_snapshot()`
returns a best-effort snapshot with `en-US` fallback.

For editor and form surfaces that need browser-style spelling policy, build a
checked text-checking request before handing text to a native or bundled
dictionary backend:

```rust
let request = cx.text_checking_request_checked(
    TextCheckingRequestBuilder::new(editor_text)
        .locale_snapshot(&locale)
        .check_grammar()
        .autocorrect()
        .custom_words(["Kael", "GPUI"])
        .max_suggestions(5),
)?;
```

`TextCheckingRequestBuilder` validates non-empty text, locale tags, enabled
features, custom dictionary words, duplicate words, and suggestion limits.
Pair it with `CapabilityReport::current().is_available(PlatformFeature::SpellChecking)`
to decide whether to use a native spellchecker, a bundled dictionary, or a
simple no-spellcheck fallback.

---

## Process Metrics

Inspect the current app process when tuning resource budgets or diagnosing
memory growth:

```rust
let metrics = cx.current_process_metrics();

tracing::info!(
    pid = metrics.process_id(),
    windows = metrics.window_count(),
    rss = ?metrics.resident_set_bytes(),
    virtual_memory = ?metrics.virtual_memory_bytes(),
    uptime_ms = metrics.uptime().as_millis(),
    "kael process metrics"
);
```

`ProcessMetricsSnapshot` always includes the current process id, uptime, open
Kael window count, and best-effort executable/current-directory paths. Memory
fields are best-effort and use cheap platform sources when available: Linux
reads `/proc/self/statm`, macOS shells out to `ps`, and unsupported platforms
return `None` for memory values instead of failing diagnostics. Use
`snapshot.memory().is_supported()` before enforcing memory budgets in tests or
agent audits.

---

## Resource Budgets

Evaluate the current app process against explicit resource budgets for
lightweight-runtime gates, smoke tests, and AI-agent audits:

```rust
let budget = cx.evaluate_resource_budget_checked(
    AppResourceBudgetBuilder::new()
        .max_resident_set_bytes(256 * 1024 * 1024)
        .max_virtual_memory_bytes(2 * 1024 * 1024 * 1024)
        .max_windows(4)
        .require_memory_metrics()
        .warn_when_power_constrained(),
)?;

if !budget.is_within_budget() {
    tracing::warn!(summary = budget.summary(), "resource budget exceeded");
}
```

`AppResourceBudgetBuilder` validates positive thresholds and requires at least
one configured check. `AppResourceBudgetEvaluation` includes the sampled process
metrics, runtime snapshot, structured issues, `is_within_budget()`,
`missing_required_metrics()`, and a compact `summary()`. Memory metrics remain
best-effort across OSes; use `.require_memory_metrics()` when a test or release
gate must fail if the platform cannot provide memory data.

---

## App Metadata

Declare validated app identity once and reuse it for About dialogs,
diagnostics, support links, and generated desktop chrome:

```rust
let metadata = AppMetadataBuilder::new("Kael Studio")
    .version(env!("CARGO_PKG_VERSION"))
    .build(option_env!("GIT_SHA").unwrap_or("dev"))
    .identifier("com.example.kael-studio")
    .website_url("https://example.com")
    .support_url("https://example.com/support")
    .license("Apache-2.0");

cx.show_about_dialog_checked(metadata)?;
```

`AppMetadataBuilder` validates user-facing names, version/build labels,
bundle-style identifiers, HTTP(S) website/support URLs, copyright, license, and
credits before they reach native chrome. `build_checked()` returns
`AppMetadata`, which exposes accessors plus `display_title()` and
`about_dialog()` when an app wants to route the metadata through its own menu or
custom dialog flow.

---

## App Update State

Model "Check for Updates", release banners, download progress, and restart
prompts with validated state before wiring a feed or platform updater backend:

```rust
let update = cx.app_update_state_checked(
    AppUpdateStateBuilder::new(env!("CARGO_PKG_VERSION"))
        .channel(AppUpdateChannel::Stable)
        .phase(AppUpdatePhase::Available)
        .release(
            AppUpdateReleaseBuilder::new("1.3.0")
                .channel(AppUpdateChannel::Stable)
                .title("Kael Studio 1.3")
                .notes_url("https://example.com/releases/1.3.0")
                .download_url("https://example.com/downloads/kael-studio-1.3.zip")
                .signed()
                .rollout_percentage(25),
        ),
)?;

let label = update.menu_label();
let action = update.recommended_action();

let decision = cx.app_update_offer_checked(
    AppUpdateOfferPolicyBuilder::stable()
        .cohort_key(machine_install_id)
        .require_signed_release(true),
    AppUpdateReleaseBuilder::new("1.3.0")
        .channel(AppUpdateChannel::Stable)
        .download_url("https://example.com/downloads/kael-studio-1.3.zip")
        .signed()
        .rollout_percentage(25),
)?;

if decision.should_offer() {
    show_update_banner();
}
```

`AppUpdateStateBuilder` is a side-effect-free state model, not an installer. It
validates release versions, channel labels, release-note/download URLs, progress
range, and error messages. Update phases that imply an available package require
release metadata, download progress is valid only while downloading, and failed
states require an error message. Use the resulting `menu_label()` and
`recommended_action()` for menus, settings rows, notifications, and AI-agent
audits while a platform-specific updater performs the actual check/download.
`AppUpdateOfferPolicyBuilder` adds the app-facing release gate that Electron
apps often hide inside updater glue code. It validates channel tracking,
signed-release requirements, download-URL requirements, stable rollout cohorts
or explicit rollout buckets, and critical/mandatory rollout bypass behavior.
The resulting decision is `Offer`, `Defer`, or `Block`, with a reason such as
channel mismatch, rollout exclusion, missing download URL, or unsigned release.
It does not verify signatures itself; pass `.signed()` only after the feed or
package verification layer has succeeded.

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
    // Handle typed open requests without ad-hoc URL parsing
    .on_open_request(|request, cx| {
        match request.kind() {
            OpenRequestKind::File { path } => {
                // Open a document passed by the OS, Finder, Explorer, or xdg-open.
            }
            OpenRequestKind::DeepLink { scheme } => {
                // Route an app-owned scheme such as myapp://settings.
            }
            OpenRequestKind::Url { scheme } => {
                // Inspect a normal external URL such as https://... or mailto:...
            }
            OpenRequestKind::Unknown => {}
        }
    })
    // Handle specific schemes with app context
    .deep_links_checked(
        DeepLinkRouterBuilder::new()
            .route("myapp", |url, cx| {
                // Handle myapp://path/to/resource
            })
            .route("oauth", |url, cx| {
                // Handle oauth://callback?code=...
            }),
    )?
    .run(|cx| {
        let tasks = cx.register_url_schemes(
            UrlSchemeRegistrationBuilder::new()
                .scheme("myapp")
                .scheme("oauth"),
        ).expect("valid URL schemes");

        for task in tasks {
            task.detach_and_log_err(cx);
        }
    });
```

`DeepLinkRouterBuilder` validates grouped route schemes and rejects duplicates.
`UrlSchemeRegistrationBuilder` validates scheme syntax, deduplicates repeated
schemes, and keeps startup code readable. Use `.on_open_requests(...)` or
`.on_open_request(...)` when the app needs to distinguish app-owned deep links,
external URLs, and `file://` document opens without custom parsing. The
lower-level `register_url_scheme("scheme")`, `.on_open_urls(...)`,
`.on_open_url(...)`, `.on_deep_link("scheme", callback)`, and `.deep_links(...)`
methods remain available for direct route management.

---

## Custom App Protocols

Use custom protocols for app-owned URLs such as packaged assets, preview
documents, or internal route content without passing raw filesystem paths
through UI code.

```rust
let app = Application::new();
app.custom_protocols_checked(
    CustomProtocolRouterBuilder::new()
        .route("app", |request, cx| {
            let body = format!("asset path: {}", request.path());
            CustomProtocolResponse::text(body)
        }),
)?;
app.run(|cx| {
    if let Some(response) = cx
        .handle_custom_protocol_url("app://assets/readme.txt")
        .expect("valid custom protocol URL")
    {
        println!("{} bytes", response.body.len());
    }
});
```

`CustomProtocolRouterBuilder` validates custom schemes, rejects duplicate
routes, and prevents shadowing standard schemes such as `http`, `https`,
`file`, `data`, or `javascript`. `CustomProtocolRequest::parse(...)` exposes
typed `scheme`, `host`, `path`, and `query` fields. Build responses with
`CustomProtocolResponse::{html,text,json,bytes}` or
`CustomProtocolResponseBuilder`; checked responses validate status codes, MIME
types, and headers before they are handed back to the app.

For packaged assets or offline documents, prefer the checked file resolver over
manual path joins:

```rust
let route = CustomProtocolFileResolver::builder("assets/app")
    .host("assets")
    .index_file("index.html")
    .cache_control("public, max-age=60")
    .require_existing_root()
    .canonicalize_root()
    .route_checked("app")?;

app.custom_protocols_checked(CustomProtocolRouterBuilder::from(route))?;
```

`CustomProtocolFileResolverBuilder` maps requests such as
`app://assets/icons/logo.svg` to files under one root, returns `404` for missing
files or host mismatches, infers common MIME types, and rejects parent-directory
traversal before reading bytes. Existing files are canonicalized against the
root, so symlink escapes are rejected as well.

---

## Multi-Window

Open multiple windows with independent views:

```rust
cx.open_window(
    WindowIntentBuilder::main()
        .title("Kael Studio")
        .windowed(bounds)
        .min_size(size(px(720.0), px(480.0)))
        .build_checked()?,
    |_window, cx| cx.new(|_| MainView::new()),
).unwrap();

cx.open_window(
    WindowIntentBuilder::palette()
        .title("Settings")
        .windowed(bounds)
        .min_size(size(px(480.0), px(320.0)))
        .build_checked()?,
    |_window, cx| cx.new(|_| SettingsView::new()),
).unwrap();

cx.open_window(
    WindowOptionsBuilder::new()
        .title("Command Palette")
        .centered(size(px(720.0), px(420.0)), cx)
        .floating()
        .transparent_titlebar(true)
        .blurred_background()
        .client_decorations(),
    |_window, cx| cx.new(|_| CommandPalette::new()),
).unwrap();
```

Prefer `WindowIntentBuilder` for generated BrowserWindow-style intent: `main`,
`palette`, `utility`, `modal(parent)`, `popup`, and `overlay` presets compose
coherent window kinds, resize/minimize/move flags, titlebar/background defaults,
and parent requirements before opening a window. It validates finite positive
bounds/minimum sizes, titles, app IDs, tab identifiers, and intent-specific
invariants such as modal parent handles, non-minimizable palettes, non-resizable
popups, and overlay window kind. Drop to `WindowOptionsBuilder` when an app
needs the full native option surface directly.

Raw `WindowOptions { ... }` values remain available when constructing options
manually.

For document/editor windows, apply title and unsaved-change chrome together with
a checked document state:

```rust
window.set_document_state_checked(
    WindowDocumentStateBuilder::document(project_path.join("Report.md"))
        .require_existing_path()
        .unsaved_changes(),
)?;
```

`WindowDocumentStateBuilder` validates explicit titles, derives a title from the
document path when needed, rejects empty or NUL-containing paths, can require or
canonicalize existing paths, and applies the platform edited marker with the
same state update. Raw `set_window_title(...)` and `set_window_edited(...)`
remain available when an app owns the validation.

Kael windows follow native platform conventions automatically:

- **Scroll-to-focus** — scrolling over an unfocused Kael window activates it,
  matching standard macOS/Windows behavior
- **Smooth zoom** — double-clicking the titlebar animates the window to fill
  the screen using native Core Animation transitions
- **Live resize** — content reflows smoothly during window drag-resizing

---

## File Watching

Watch project folders, theme files, config files, generated assets, and logs
without polling:

```rust
let mut watcher = FileWatcher::new(cx, |event| {
    println!("file change: {event:?}");
})?;

let watch_set = watcher.watch_set(
    FileWatchSetBuilder::new()
        .paths([project_dir, config_file, log_dir])
        .max_depth(3)
)?;

watcher.watch_with_options(
    single_file,
    FileWatchOptionsBuilder::new()
        .non_recursive()
        .build_checked()?,
)?;
```

Use `FileWatchSetBuilder::new().paths([...]).recursive()` or
`.max_depth(depth)` when one feature needs to watch several project, config, log,
or generated-asset roots with shared options. The checked path rejects empty
sets, empty paths, missing paths, raw non-recursive depth limits, and zero-depth
watches before platform registration starts, then canonicalizes and deduplicates
the paths. Use `FileWatchOptionsBuilder::new().recursive()` for all descendants,
`.max_depth(depth)` for bounded project-folder watches, and `.non_recursive()`
for single files or direct children. Raw `FileWatchOptions { ... }`,
`FileWatchOptions::recursive()`, `watch_with_options(...)`, and
`watch(path, recursive)` remain available for low-level use.

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

let config = AutoUpdaterConfigBuilder::new("https://releases.myapp.com/update-feed.json")
    .check_interval(Duration::from_secs(86_400))
    .stable_only()
    .build_checked()?;

let mut updater = AutoUpdater::new_checked(config, current_version, http_client)?;

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

Use `AutoUpdaterConfigBuilder` for generated updater setup so feed URL syntax,
HTTP(S) scheme, host presence, and non-zero check intervals are validated before
network work begins. Raw `AutoUpdaterConfig { ... }` and `AutoUpdater::new(...)`
remain available for callers that already validate configuration.

When building release feeds or test fixtures programmatically, use
`UpdateInfoBuilder` to validate update metadata before publishing or injecting
it:

```rust
let update = UpdateInfoBuilder::new(
    SemanticVersion::new(2, 5, 1),
    "https://releases.myapp.com/MyApp-2.5.1.zip",
)
.sha256(package_sha256)
.size_bytes(package_size)
.signature(ed25519_signature_base64)
.build_signed_checked()?;
```

`build_checked()` validates the download URL, optional SHA-256, optional package
size, and optional signature shape. `build_signed_checked()` additionally
requires signature, SHA-256, and size metadata for fail-closed update
verification.

When a relaunch should use a custom binary path, validate it before storing it
for the next `cx.restart()`:

```rust
let restart_path = cx.set_restart_path_checked(
    RestartPathBuilder::new("/Applications/MyApp.app/Contents/MacOS/MyApp")
        .require_existing_file()
        .canonicalize(),
)?;
println!("will restart with {}", restart_path.display());
```

Use `RestartPathBuilder::current_exe()?` for the current executable,
`.require_existing_file()` for update/install flows that must relaunch a real
binary, and `.allow_missing()` only when a lower-level platform launcher will
resolve the path later. Raw `set_restart_path(path)` remains available for
custom relaunch integrations.

### App-owned downloads

Use `DownloadRequest` for downloads that do not originate inside a WebView:
exports, model/artifact fetches, offline packs, plugin assets, background
workers, and native command-palette actions. It validates the URL, destination,
optional integrity metadata, parent-directory behavior, and outbound network
policy before the app hands work to an HTTP client.

```rust
let policy = NetworkPolicyBuilder::new()
    .allow_host("cdn.myapp.com")
    .build_checked()?;

let request = DownloadRequest::builder(
    "https://cdn.myapp.com/exports/report.pdf",
    dirs.download_dir().join("report.pdf"),
)
.sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
.size_bytes(42_000)
.network_policy(policy)
.create_parent_dirs()
.build_checked()?;

request.validate()?;
```

This is separate from WebView downloads. WebView download handlers preserve
browser behavior for hosted pages; `DownloadRequest` is the native descriptor to
queue, audit, and execute app-owned downloads consistently across workers,
plugins, and generated automation.

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
let job = PrintJob::letter("Document", |ctx, cx| {
    ctx.draw_text(
        "Hello, printed world!",
        point(px(72.0), px(72.0)),
        PrintTextStyle::default(),
    );
})
    .orientation(PrintOrientation::Portrait)
    .margins(Edges::all(px(36.0)));

job.validate()?;
window.print_checked(PrintRequest::dialog(job), cx)?;

// WebView-hosted documents can use the same checked request surface.
window.print_checked(PrintRequest::webview("invoice-preview"), cx)?;
```

Use `PrintJob::letter(...)`, `PrintJob::a4(...)`, `PrintPage::letter(...)`, and
`PrintPage::a4(...)` for common paper sizes instead of repeating point values.
`PrintJob::validate()` catches empty, padded, control-character, or overly long
titles, missing pages, invalid page sizes, mixed page sizes, negative margins,
and margins that leave no drawable content area before native print UI opens.
Print drawing helpers drop invalid generated commands with non-finite points,
empty rectangles, invalid stroke widths, invalid font sizes, or unsupported
control characters in text.
Use `PrintRequest::dialog(job)` for the normal native print UI,
`PrintRequest::silent(job)` only when the app intentionally owns direct printer
dispatch, and `PrintRequest::webview(id)` for Electron
`webContents.print(...)` style hosted documents. `Window::print_checked(...)`
validates native jobs or WebView ids before dispatching.

---

## Power Management

Prevent sleep and detect power state:

```rust
// Prevent display sleep during video playback
let blocker = cx.start_power_save_blocker_checked(
    PowerSaveBlockerBuilder::prevent_display_sleep()
        .reason("video playback"),
)?;

let monitor = cx.watch_system_power_checked(
    SystemPowerMonitorBuilder::new()
        .on_power_mode_changed(|snapshot, cx| {
            if snapshot.should_reduce_work() {
                /* lower animation, polling, or render quality */
            }
        })
        .on_suspend(|snapshot, cx| {
            /* save state */
        })
        .on_resume(|snapshot, cx| {
            /* refresh data */
        }),
)?;

if monitor.initially_should_reduce_work() {
    /* start in a lighter mode */
}
```

The raw `start_power_save_blocker(PowerSaveBlockerKind::...)` and
`stop_power_save_blocker(id)` methods remain available when you already store
platform IDs yourself. Prefer the checked builder path for media playback,
presentations, capture tools, and long-running tasks because it validates
generated reasons and returns a typed
handle with the blocker kind and optional reason.

Use `cx.system_power_snapshot()` when you only need a synchronous view of
`power_mode`, `reduce_motion`, and `system_idle_time`. Use
`watch_system_power(...)` when you only need the initial snapshot without
callbacks. The raw `cx.on_system_power_event(...)`, `cx.power_mode()`,
`cx.reduce_motion()`, and `cx.system_idle_time()` hooks remain available for
custom routers.

For Electron-style native theme decisions, use `cx.native_theme_snapshot()`:

```rust
let theme = cx.native_theme_snapshot();
let background = theme.choose(dark_background, light_background);

if theme.should_reduce_effects() {
    /* disable decorative blur, motion, or expensive effects */
}
```

`NativeThemeSnapshot` combines `window_appearance()`, `reduce_motion()`, and
`power_mode()` into one small value with `is_dark()`, `is_light()`,
`is_vibrant()`, and `should_reduce_effects()` helpers. Raw platform calls remain
available when a feature needs a single signal.

For Electron-style idle gating, use a checked `SystemIdlePolicy` instead of
open-coded duration comparisons:

```rust
let idle = cx.system_idle_evaluation_checked(
    SystemIdlePolicyBuilder::minutes(5)
        .require_known_idle_time(),
)?;

if idle.is_idle() {
    /* run indexing, sync compaction, or expensive preview generation */
}
```

`SystemIdlePolicyBuilder` rejects zero thresholds and contradictory unknown-idle
behavior. By default, platforms that cannot report idle time evaluate to
`Unknown` and do not match; add `.treat_unknown_as_idle()` only for features that
are safe to run when idle telemetry is unavailable.

---

## Native Media

For URL/file/bytes video players, prefer checked media sources before wiring a
controller, native element, or `kael_ui::VideoPlayer`:

```rust
let source = MediaSourceBuilder::url("https://cdn.example.com/movie.mp4")
    .build_checked()?;

let video = VideoController::new(source)
    .volume(0.8)
    .playback_rate(1.0)
    .webvtt_text_track("en", "English", Some("en"), captions)
    .selected_text_track("en");

video.load_metadata()?;
video.play()?;
video.fast_seek(Duration::from_secs(42))?;
```

Use `MediaSourceBuilder::file(path).require_existing_file().canonicalize_file()`
for local files, `MediaSourceBuilder::bytes(bytes)` for memory-backed clips,
and `MediaSourceBuilder::reader(key, open)` for generated reader sources with a
stable cache key. The raw `MediaSource::url(...)`, `.file(...)`, `.bytes(...)`,
and `.reader(...)` constructors remain available for custom FFmpeg inputs.

For generated Electron-style video players, prefer a checked playback plan so
source validation, content-type routing, native `canPlayType` confidence, and
WebView fallback page creation happen in one place:

```rust
let plan = VideoPlaybackPlanBuilder::url(video_url)
    .content_type(content_type_header)
    .webview_options(WebViewVideoOptions::default().controls(true))
    .build_checked()?;

match plan.target() {
    VideoPlaybackPlanTarget::Native => {
        let video = plan.controller();
        video.load_metadata()?;
        video.play()?;
    }
    VideoPlaybackPlanTarget::WebViewFallback {
        page_url,
        element_id,
        ..
    } => {
        return webview(element_id.clone(), page_url.clone())
            .size_full()
            .into_any_element();
    }
}
```

`VideoPlaybackPlanBuilder` validates URLs/files/bytes/readers through
`MediaSourceBuilder`, validates optional MIME/content types, validates
`WebViewVideoOptions`, and rejects memory-backed sources when a WebView fallback
is requested because browsers need a URL/file source.

For HLS/DASH or extensionless CDN URLs that should use browser media behavior,
validate the fallback page options before embedding them:

```rust
let options = WebViewVideoOptions::default()
    .controls(true)
    .poster("https://cdn.example.com/poster.jpg")
    .preload(WebViewVideoPreload::Metadata)
    .controls_list(["nodownload"])
    .object_fit("cover")
    .webvtt_text_track("English", Some("en"), captions)
    .checked()?;

let page_url = webview_video_player_url(&source, &options)
    .expect("URL/file media can be wrapped for WebView fallback");
```

`WebViewVideoOptions::validate()` rejects empty/padded poster or track URLs,
unsupported URL schemes, invalid `controlslist` tokens, invalid text-track
metadata, and unsafe `object-fit` values before generated media UI reaches the
embedded browser.

---

## Media Keys

Route hardware media keys and OS media controls to audio or video playback:

```rust
let video = MediaSourceBuilder::url(video_url).controller_checked()?;

MediaKeyBindingBuilder::new()
    .video(video.clone())
    .playlist(
        VideoPlaylist::new([
            MediaSource::url("https://cdn.example.com/intro.mp4"),
            MediaSource::url("https://cdn.example.com/lesson.mp4"),
            MediaSource::url("https://cdn.example.com/outro.mp4"),
        ])
        .repeat(true),
    )
    .install_checked(cx)?;
```

`Play`, `Pause`, `PlayPause`, and `Stop` are routed to the configured
`AudioHandle` or `VideoController`. `NextTrack` and `PreviousTrack` can replace
the bound `VideoController` source through `VideoPlaylist`, or call
`on_next_track(...)` / `on_previous_track(...)` when an app owns a custom queue.
`VideoPlaylist::validate()` / `checked()` rejects empty playlists and invalid
media sources, while `MediaKeyBindingBuilder::install_checked(...)` also rejects
playlist routing without a bound video controller. Use raw `install(...)` or the
lower-level `on_media_key_event(...)` callback when you need a custom event
router.

---

## User Attention

Bounce the dock icon, flash the taskbar, or request equivalent desktop
attention for background work:

```rust
let request = cx.request_user_attention_checked(
    UserAttentionBuilder::informational()
        .reason("download complete"),
)?;

// Later, when the user opens the app or the condition is resolved:
request.cancel(cx);
```

Use `UserAttentionBuilder::critical()` for urgent conditions that should keep
requesting attention until cancelled. The checked path rejects empty reasons;
the raw `request_user_attention(AttentionType::...)`,
`request_user_attention_with(...)`, and `cancel_user_attention()` methods remain
available when you already manage the attention lifecycle.

---

## Window Progress

Show download, export, install, or sync progress in the platform window
representation:

```rust
window.set_progress_bar_checked(ProgressBarState::normal(0.42)?)?;
window.set_progress_bar_checked(ProgressBarState::Indeterminate)?;

// Later, when work completes or the user cancels:
window.set_progress_bar_checked(ProgressBarState::None)?;
```

Use `ProgressBarState::normal(...)`, `error(...)`, and `paused(...)` for
checked determinate states. The checked constructors and
`set_progress_bar_checked(...)` reject NaN, infinity, and fractions outside
`0.0..=1.0` before values reach dock/taskbar APIs. The raw
`set_progress_bar(...)` method remains available when the caller has already
validated platform-specific state.

---

## Network Status

Use a monitor for sync, presence, upload queues, and offline-mode UX:

```rust
let monitor = cx.watch_network_status_checked(
    NetworkStatusMonitorBuilder::new()
        .on_offline(|cx| {
            // Pause sync and show offline UI.
        })
        .on_online(|cx| {
            // Resume queued work.
        })
        .on_change(|status, cx| {
            println!("network status: {status:?}");
        }),
)?;

if !monitor.initially_online() {
    // Start in offline mode.
}
if monitor.initially_offline() {
    // Defer network-heavy work.
}
```

Use `watch_network_status(...)` when you only need an initial snapshot without
callbacks.

The raw `network_status()` and `on_network_status_change(...)` methods remain
available for custom routers.

For outbound requests from workers, extensions, sync clients, or generated HTTP
integrations, pair network status with a checked host policy:

```rust
let policy = NetworkPolicyBuilder::new()
    .allow_host("api.example.com")
    .allow_url("https://cdn.example.com/assets/app.js")?
    .build_checked()?;

if policy.check_url("https://api.example.com/v1/sync")? {
    // Safe to hand this URL to the app HTTP client.
}

let request = AppNetworkRequestBuilder::post("https://api.example.com/v1/sync")
    .header("Content-Type", "application/json")
    .header("X-Client-Version", env!("CARGO_PKG_VERSION"))
    .body_size_bytes(512)
    .network_policy(policy.clone())
    .build_checked()?;
```

`NetworkPolicyBuilder` validates host strings and URL-derived hosts, rejects
non-HTTP(S) URLs, duplicate hosts, and mixed allow/deny lists, and defaults to
`DenyAll` when no hosts are configured.
Use `AppNetworkRequestBuilder` when generated workers, plugin hosts, sync
clients, or export flows need checked request metadata before using the app HTTP
client. It validates HTTP(S) URLs, host policy, request methods, duplicate or
malformed headers, CR/LF header injection, optional body sizes, and body/method
shape. It does not send the request; it is the typed handoff to your transport.

For long-lived realtime transports, use `AppRealtimeConnection` as the checked
descriptor before opening a WebSocket or server-sent events stream:

```rust
let realtime = AppRealtimeConnection::websocket("wss://events.example.com/socket")
    .protocol("kael.v1")
    .heartbeat_interval(std::time::Duration::from_secs(30))
    .max_message_bytes(64 * 1024)
    .network_policy(policy)
    .build_checked()?;
```

`AppRealtimeConnection` validates WebSocket `ws`/`wss` URLs, EventSource
`http`/`https` URLs, duplicate or malformed headers, WebSocket subprotocol
tokens, heartbeat bounds, inbound message budgets, and attached network policy.
It does not open the socket; it gives generated agents and native workers a
typed, auditable handoff to the app realtime transport.

---

## Session Persistence

Save and restore window positions and app session data across launches:

```rust
let store = SessionStore::new_checked("com.example.my-app")?;

let snapshot = store.save_snapshot_checked(
    SessionSnapshotBuilder::new()
        .window_state("main", main_window.window_state())
        .app_data(serde_json::json!({
            "workspace": workspace_id,
            "sidebar": "files",
        }))?,
)?;

// Restore on next launch
if let Ok(snapshot) = store.load_snapshot() {
    for (id, state) in &snapshot.window_states {
        cx.open_window(
            WindowOptionsBuilder::new().bounds(state.bounds),
            |_, cx| cx.new(|_| MyView::new()),
        );
    }

    if let Some(app_data) = snapshot.app_data {
        restore_workspace(app_data)?;
    }
}
```

Use `SessionStore::new_checked(...)` for generated app IDs and
`save_snapshot_checked(SessionSnapshotBuilder::new()...)` for generated session
snapshots. The checked path rejects empty, padded, path-like, control-character,
and overly long app/window IDs, and rejects JSON `null` app data so callers use
`clear_app_data()` intentionally. Raw `SessionStore::new(...)`,
`save_snapshot(...)`, `SessionSnapshotBuilder::build()`, `save_window_states(...)`,
and `load_window_states(...)` remain available for compatibility or
geometry-only apps.

Use `restore_window_states(...)` when reopening windows after monitor changes:

```rust
let displays = cx.displays().iter().map(|display| display.id()).collect::<Vec<_>>();
let primary = cx.primary_display().map(|display| display.id());
let restored = store.restore_window_states(&displays, primary)?;
```

Use `save_window_states(...)` / `load_window_states(...)` when only window
geometry needs persistence.

---

## Display Information

Resolve window and panel positions across monitors:

```rust
let placement = cx.resolve_window_placement(
    WindowPlacementBuilder::new(size(px(420.), px(320.)))
        .bottom_right(px(16.)),
)?;

cx.open_window(
    WindowOptionsBuilder::new()
        .title("Downloads")
        .placement(&placement)
        .floating(),
    |_window, cx| cx.new(|_| DownloadsView::new()),
)?;

let display_id = placement.display_id();
```

For Electron `screen`-style display queries, use `DisplayQueryBuilder`:

```rust
let primary = cx
    .query_displays_checked(DisplayQueryBuilder::primary())?
    .first()
    .cloned();

let cursor_display = cx
    .query_displays_checked(DisplayQueryBuilder::cursor().fallback_to_primary())?
    .first()
    .cloned();

let displays = cx.query_displays_checked(DisplayQueryBuilder::all())?;
```

`DisplaySnapshot` exposes display id, optional stable UUID, bounds, default
window bounds, refresh rate, whether it is primary, and whether it contains the
cursor. Checked queries can require a match, allow empty results, or fall back to
the primary display for cursor/id lookups.

Enumerate monitors and get DPI when you need raw display metadata:

```rust
let displays = cx.displays();
let primary = cx.primary_display();

for display in &displays {
    println!("Display {}: {:?}", display.id(), display.bounds());
}
```

The raw `cx.compute_window_bounds(size, &WindowPosition::...)` helper remains
available when you already have a semantic `WindowPosition`.

---

## Text Antialiasing

Kael renders text with the best antialiasing each platform's font rasterizer can
produce. When a glyph is drawn with an opaque color at full opacity and without a
transform, Kael requests **subpixel (LCD RGB) antialiasing**; otherwise it falls
back to **grayscale** antialiasing. Subpixel coverage is rasterized per channel and
uploaded into the polychrome (RGBA) glyph atlas, then blended in the sprite shader
by collapsing the per-channel coverage to a dominant alpha while preserving the RGB
fringing — the same math across every backend.

| Platform | Subpixel text | Rasterizer |
|----------|---------------|-----------|
| macOS    | Yes           | Core Text / `font-kit` (BGRA coverage) |
| Windows  | Yes           | DirectWrite ClearType (`DWRITE_TEXTURE_CLEARTYPE_3x1`) with the OS gamma/contrast correction |
| Linux    | Grayscale only | cosmic-text / swash |

Subpixel selection is decided by the text system's capability, not a compile-time
target check, so a backend that gains LCD support is picked up automatically.

### Why Linux stays grayscale

Linux text shaping and rasterization go through `cosmic-text`, whose `SwashCache`
renders every non-color glyph with `Format::Alpha` — a single-channel grayscale
mask. Its `Content::SubpixelMask` branch is an unimplemented `TODO`, so the cache
never emits RGB coverage. The underlying `swash` scaler *can* produce
`Format::Subpixel`, but reaching it means bypassing `SwashCache` and driving
`swash`'s `ScaleContext`/`Render` directly (or adding a FreeType path with an LCD
filter), plus matching the glyph-cache keys and fractional-offset handling. That is
a separate rasterizer effort; until then Linux uses high-quality grayscale
antialiasing, which is visually correct (just without the horizontal-resolution
gain of LCD subpixel rendering).

---

## Crash Reporting

Automatic crash capture with remote submission:

```rust
use kael::CrashReporterBuilder;

let mut reporter = CrashReporterBuilder::new("com.example.my-app")
    .endpoint("https://crashes.example.com/reports")
    .http_client(http_client.clone())
    .build_checked()?;

reporter.install_hook();

// On the next launch, after checking user consent:
reporter.submit_pending_reports().await?;
```

For startup code that wants the hook installed immediately, use
`cx.install_crash_reporter_checked(CrashReporterBuilder::new(app_id))?`.
Checked crash reporter builders reject empty, padded, path-like, control-character,
or overly long app IDs, custom report directories that are not absolute, and
submission endpoints that are not HTTP(S) URLs with hosts. The lower-level
`CrashReporter::new(app_id)`, `set_endpoint(...)`, `set_http_client(...)`, and
`install_hook()` APIs remain available when an app owns validation.

The panic hook above only captures Rust panics. To also capture native crashes
(segfaults, aborts, illegal instructions, and FFI/GPU-driver crashes) and submit
prior crashes on the next launch with user consent, use the `kael_diagnostics`
reporter and its `install_native()` / `check_and_submit_pending()` APIs. See
[Crash Reporting](crash-reporting.md) for installation, consent, the per-platform
capture matrix, and symbolication guidance.

## App Lifecycle

Configure app lifetime, launch at login, and update the dock/taskbar:

```rust
let lifecycle = cx.configure_lifecycle_policy_checked(
    AppLifecyclePolicyBuilder::new()
        .quit_when_all_windows_close()
        .quit_cleanup_timeout(Duration::from_millis(250))
        .reason("flush workspace state"),
)?;

let launch = cx.configure_auto_launch(
    AutoLaunchBuilder::enable("com.example.app"),
)?;
let enabled = launch.enabled();

// App ids are validated before platform registration.
assert_eq!(launch.app_id(), "com.example.app");

cx.perform_lifecycle_command_checked(
    AppLifecycleCommand::activate_with_options(true)
        .reason("show existing project window"),
)?;

cx.perform_lifecycle_command_checked(
    AppLifecycleCommand::restart("apply downloaded update"),
)?;

cx.set_dock_badge_checked(DockBadgeBuilder::count(3))?;
cx.set_dock_badge_checked(DockBadgeBuilder::label("sync"))?;
cx.set_dock_badge_checked(DockBadgeBuilder::clear())?;
cx.set_dock_menu_checked(
    DockMenuBuilder::new()
        .action("Show Window", menu_action::ShowWindow)
        .separator()
        .action("Quit", menu_action::Quit),
)?;
window.set_progress_bar_checked(ProgressBarState::normal(0.7)?)?;
cx.add_recent_documents(
    RecentDocumentsBuilder::new()
        .require_existing_files()
        .canonicalize()
        .document("/path/to/report.pdf")
        .document("/path/to/notes.md"),
).expect("recent document paths");
cx.update_jump_list_checked(
    JumpListBuilder::new()
        .action("Open Project", menu_action::Open)
        .workspace_path("/path/to/project")
        .workspace(["/path/to/project", "/path/to/workspace.code-workspace"]),
)?;
```

Use `AppLifecyclePolicyBuilder::new().quit_when_all_windows_close()` for normal
document or utility apps, and `.keep_alive_without_windows()` for tray,
background sync, menubar, or agent apps that should survive after their last
window closes. `.quit_cleanup_timeout(duration)` controls how long futures
registered with `on_app_quit(...)` may run before shutdown continues; the
checked builder rejects zero and longer-than-30-second cleanup timeouts plus
invalid diagnostic reasons. Raw `set_keep_alive_without_windows(...)`,
`on_app_quit(...)`, `on_app_restart(...)`, and `on_window_closed(...)` remain
available for lower-level lifecycle integrations.
Use `AppLifecycleCommand::activate()`, `.activate_with_options(...)`, `.hide()`,
`.hide_other_apps()`, `.unhide_other_apps()`, `.quit(reason)`, and
`.restart(reason)` for Electron `app.focus(...)`, hide/show, quit, and relaunch
flows. `perform_lifecycle_command_checked(...)` validates optional diagnostics
and requires an explicit reason before quit or restart dispatches.

Use `cx.runtime_snapshot()` when startup gates, diagnostics, or AI agents need
a single read-only view of app readiness and lifecycle state:

```rust
let runtime = cx.runtime_snapshot();

if runtime.is_background_runtime() {
    tracing::info!("running without visible windows");
}

if runtime.power().should_reduce_work() {
    schedule_lightweight_sync();
}
```

`AppRuntimeSnapshot` includes the capability process id, uptime, window count,
keep-alive policy, quit-cleanup timeout, quitting flag, network status, system
power snapshot, and native theme snapshot. This is the app-runtime companion to
`CapabilityReport::current()`: use the capability report for platform support,
and the runtime snapshot for current process/app state.

For Electron-style `capturePage()` workflows, build a checked app-window capture
request before invoking a platform snapshot backend:

```rust
let capture = cx.app_window_capture_request_checked(
    AppWindowCaptureRequest::focused_window("Capture visual regression evidence.")
        .png()
        .max_dimensions(1920, 1080)
        .max_pixels(2_073_600),
)?;
```

`AppWindowCaptureRequestBuilder` targets the focused window, a specific app
window, or all visible app windows. It validates purpose text, requested PNG or
raw RGBA output, optional window chrome/cursor flags, max dimensions, max pixel
count, and the multi-window rule that cursor capture is ambiguous. Gate capture
backends with `CapabilityReport::current().is_available(PlatformFeature::AppWindowCapture)`.
Requests that allow occluded/minimized OS-level capture expose
`required_capability() == Some(Capability::ScreenCapture)`; visible app-owned
window render snapshots do not require that capability.

For Electron `BrowserWindow.show()`, `.hide()`, `.focus()`, `.minimize()`, and
`setIgnoreMouseEvents(...)` style flows, use a checked window interaction
command:

```rust
window.perform_window_interaction_checked(WindowInteractionCommand::show())?;
window.perform_window_interaction_checked(WindowInteractionCommand::activate())?;
window.perform_window_interaction_checked(
    WindowInteractionCommand::mouse_passthrough("Heads-up overlay should not block clicks"),
)?;
window.perform_window_interaction_checked(WindowInteractionCommand::receive_mouse_events())?;
```

`WindowInteractionCommand` validates optional diagnostics and requires an
explicit reason before enabling mouse pass-through, since click-through windows
can be hard for users to recover if generated accidentally. Raw
`show_window()`, `hide_window()`, `activate_window()`, `minimize_window()`,
`is_window_visible()`, and `set_mouse_passthrough(...)` remain available for
already-validated custom integrations.

For long-running native apps with heavy text, icon, or sprite churn, set a
checked renderer atlas budget instead of leaving glyph/sprite atlases unbounded:

```rust
window.set_atlas_byte_budget_checked(
    WindowAtlasBudgetBuilder::bytes(128 * 1024 * 1024)
        .reason("Large editor keeps many documents and symbols warm"),
)?;

window.set_atlas_byte_budget_checked(WindowAtlasBudgetBuilder::clear())?;
```

`WindowAtlasBudgetBuilder` rejects zero-byte budgets, excessively large budgets,
and invalid diagnostic text before the renderer backend receives the request.
The lower-level `window.set_atlas_byte_budget(...)` remains available for
already-validated platform-specific memory policy.

For frameless windows and custom titlebars, use checked chrome commands instead
of calling compositor hooks directly:

```rust
window.perform_window_chrome_command_checked(
    WindowChromeCommand::request_decorations(WindowDecorations::Client)
        .reason("custom titlebar owns drag regions"),
)?;
window.perform_window_chrome_command_checked(WindowChromeCommand::start_move())?;
window.perform_window_chrome_command_checked(
    WindowChromeCommand::start_resize(ResizeEdge::BottomRight),
)?;
window.perform_window_chrome_command_checked(
    WindowChromeCommand::show_window_menu(point(px(12.0), px(32.0))),
)?;
```

`WindowChromeCommand` validates optional diagnostics and rejects non-finite
window-menu positions before platform backends receive them. Raw
`request_decorations(...)`, `show_window_menu(...)`, `start_window_move()`, and
`start_window_resize(...)` remain available for already-validated custom chrome.

For presentation, media, POS, dashboard, and kiosk windows, use a checked
presentation policy instead of raw fullscreen toggles:

```rust
window.set_presentation_policy_checked(
    WindowPresentationPolicyBuilder::fullscreen("Present launch deck"),
)?;

window.set_presentation_policy_checked(
    WindowPresentationPolicyBuilder::kiosk("Point of sale checkout"),
)?;
```

`WindowPresentationPolicyBuilder::fullscreen(...)` keeps normal user exit
behavior, while `kiosk(...)` records hidden-chrome and restricted-exit intent
for platform backends that can enforce it. The checked path validates reasons,
applies platform fullscreen state, and `clear_presentation_policy_checked()`
returns the window to normal windowed behavior.

For windows that should not appear in screenshots, screen sharing, or app-owned
visual regression capture, record checked content-protection intent on the
native window:

```rust
window.set_content_protection_checked(
    WindowContentProtectionBuilder::exclude_from_capture("Protect checkout secrets"),
)?;
```

Use `WindowContentProtectionBuilder::obscure_when_captured(...)` when a platform
can blur/blank captured output but may not support full exclusion, and
`clear_content_protection_checked()` when the private flow ends. The checked
policy validates a user-facing reason, records whether app-owned window capture
should skip the window, and gives platform backends one authoritative intent to
map onto OS content-protection APIs.

Use `DockBadgeBuilder::count(...)` for unread counts, `label(...)` for short
sync/export status, and `clear()` when the state is resolved. The checked path
rejects empty labels, padded labels, control characters, and overly long badge
text before the platform tries to render a dock badge or taskbar overlay. Raw
`set_dock_badge(Some(label))` and `set_dock_badge(None)` remain available when
you already own validation.

Use `DockMenuBuilder` for app icon context menus so generated action labels,
submenus, and separators are validated before platform installation. The checked
path rejects empty menus, separator-only menus, empty submenu trees, padded
labels, control characters, and overly long labels. Raw `set_dock_menu(items)`
remains available for apps that already own menu validation.

Use `JumpListBuilder` for Windows taskbar jump lists and Electron-style recent
workspace groups. Task entries are validated as action menu items, workspace
entries must contain at least one non-empty path, and optional
`.require_existing_paths().canonicalize()` gives generated apps a safer path for
project launchers. Raw `update_jump_list(menus, entries)` remains available for
custom Windows integrations.

Enforce a single running instance — acquire a lock at startup and forward later launches to the existing process:

```rust
use kael::{SingleInstanceBuilder, SingleInstanceLaunch};

match SingleInstanceBuilder::new("com.example.app").launch()? {
    SingleInstanceLaunch::Primary(instance) => {
        tracing::info!(app_id = instance.app_id(), "primary app instance");
        instance.on_activate(Box::new(|| { /* focus the existing window */ }));
        // ... run the app ...
    }
    SingleInstanceLaunch::Duplicate { notified, .. } => {
        debug_assert!(notified);
        return; // this duplicate launch exits
    }
}
```

The builder validates startup IDs before creating platform lock names, rejecting
empty, padded, control-character, path-like, and overly long values. Use
`launch.app_id()`, `launch.is_primary()`, `launch.is_duplicate()`, and
`launch.notified_existing()` for telemetry and branch-free startup plumbing.
Use `SingleInstance::acquire(...)` and `send_activate_to_existing(...)` directly
when you need lower-level lock/notification control.

## Biometric Authentication

Gate sensitive actions behind Touch ID / Face ID / Windows Hello. The builder
validates the user-facing reason string, rejects accidental leading/trailing
whitespace, snapshots availability, and only shows the OS prompt when biometrics
are available by default:

```rust
use kael::BiometricAuthBuilder;

let request = cx.authenticate_biometric_with(
    BiometricAuthBuilder::unlock_vault(),
    |success| {
        if success { /* proceed */ }
    },
)?;

if !request.prompted() {
    // Fall back to a password or PIN.
}
```

Use `cx.biometric_status()` when you only need a synchronous availability
check. `BiometricStatus` is `Available(BiometricKind)` or `Unavailable`;
`BiometricKind` identifies the method (Touch ID, Face ID, fingerprint, Windows
Hello). Use `BiometricAuthBuilder::approve_payment()` for payment or transfer
confirmation flows, or `BiometricAuthBuilder::new(reason)` for custom copy.
Checked prompts reject empty, padded, control-character, and overly long
generated reason strings. The raw `cx.authenticate_biometric(reason, callback)`
hook remains available for platform-specific flows.

## Screen & Media Capture

Enumerate capturable displays/windows and stream frames. Use the app helper when
you want platform-default backends plus the current permission broker:

```rust
let manager = cx.capture_manager();
let sources = manager.sources(
    CaptureSourceQueryBuilder::screens_and_windows()
        .name_contains("Display")
        .limit(4),
)?;

if let Some(source) = sources.first() {
    tracing::info!("capturable source: {} ({:?})", source.name, source.kind);
}

let configs = manager.configs(
    CaptureConfigSetBuilder::screen_with_microphone()
        .video_frame_rate(30.0)
        .video_resolution(1920, 1080),
)?;

let mut pipeline = CapturePipeline::new();
for config in configs {
    let mut session = manager.create_session(&config)?;
    session.start(config, std::sync::Arc::new(|frame| {
        // Handle CaptureFrame::Video or CaptureFrame::Audio.
    }))?;
    pipeline.add_session(session);
}
```

Use `CaptureConfigSetBuilder::screen_with_microphone()`,
`camera_with_microphone()`, or `screen_with_system_audio()` for common app
flows, then apply `.video_frame_rate(...)` and `.video_resolution(...)` to every
video source in the set. Use
`CaptureConfigBuilder::{screen, window, camera, microphone, system_audio}()`
for a single source, `.device_name_contains(...)` for remembered preferences, or
`.device_id(...)` after presenting `manager.devices(kind)` in a custom source
picker. The lower-level `CaptureConfig::new(...)`, `create_session(...)`, and
`create_session_with(...)` APIs remain available when the app needs direct
control.
Use `CaptureSourceQueryBuilder` when you need an Electron
`desktopCapturer.getSources(...)`-style source catalog before constructing
capture configs. It validates source kinds, availability filtering,
case-insensitive name filters, and result limits, and returns a
`CaptureSourceCatalog` with stable `CaptureDeviceInfo` values for picker UI,
saved preferences, diagnostics, and agents. The catalog can produce a basic
config for the first selected device via `first_config(kind)`, while frame-rate,
resolution, audio, and multi-source constraints remain on the capture config
builders.

Use `CapturePipeline` when you need coordinated screen, microphone, camera, or
system-audio sessions with backpressure accounting. The lower-level
`is_screen_capture_supported()` and `screen_capture_sources()` methods remain
available when you need platform source metadata or a direct `ScreenCaptureFrame`
stream.

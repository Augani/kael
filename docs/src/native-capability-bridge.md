# Native Capability Bridge

Kael is a native-first desktop framework, not a bundled browser runtime.
That distinction matters. Browser-runtime desktop stacks start from Chromium,
Node.js, the DOM, CSS, browser media, WebGL, WebCodecs, WebRTC, service workers,
and the npm UI ecosystem by default. Kael starts from the opposite end: one Rust
application, native windows, a GPU-rendered retained UI tree, and explicit
platform APIs.

The capability bar is therefore not "copy a browser runtime". The bar is:

1. Make common desktop-app workflows easier and more resource-efficient.
2. Provide native high-performance equivalents for the web APIs app builders
   reach for most often.
3. Offer clear escape hatches when the whole web platform is the correct tool.
4. Document the current capability level honestly so developers and AI agents
   pick the right primitive on the first try.

## The builder ladder

When an app needs more control, builders should move down this ladder:

| Need | Kael primitive | Current status |
| --- | --- | --- |
| Standard app UI | `kael_ui` components | Broad coverage; best starting point |
| Custom visual design | styled `div()`, theme tokens, custom variants | Good coverage; expand recipes |
| Custom behavior with custom markup | headless controllers, semantic accessibility recipes | Available for common patterns; focus/a11y recipes now cover common custom controls |
| Custom drawing | canvas, paths, images, SVG, Lottie | Useful today; missing some browser canvas parity |
| Web-standard surface | `webview(id, url)` | Available; should be documented as an intentional escape hatch |
| Low-level GPU effects | render targets, custom shaders, render graph | Design exists; public API is still roadmap work |
| OS integration or isolated workload | platform APIs, worker processes, extensions | Good base; capability varies by OS |

This ladder is the core answer to "can I design any app?" Kael should not force
every problem through one abstraction. It should make the right abstraction
obvious.

## Native Capability Matrix

Use this matrix before choosing an implementation path. It keeps WebView in its
proper place: a compatibility island for web-shaped surfaces, not the default
answer for desktop app development.

| Area | Common desktop app stacks provide | Kael answer today | Status | Next gap to close |
| --- | --- | --- | --- | --- |
| App chrome and product UI | DOM/CSS plus Chromium layout everywhere | `DesktopPrimitive::NativeAppChromeComponents`, `kael_ui`, styled elements, themes, headless controllers, navigation, overlays, data grids, editors, and accessibility recipes | Strong native path | More end-to-end native app shell templates |
| Embedded views and panes | Hosted embedded views, preview panes, inspectors, hosted subviews, and split browser surfaces | `DesktopPrimitive::EmbeddedHostedViews`, native `div`/`splitter`/`tabs`/`Navigator`/`LayerStack`/`Surface` composition, `cached` and `deferred` panes, plus scoped `webview_with_options` and per-pane `webview_controller` handles for hosted islands | Native-first bridge | Pane ownership/docking templates, focus restoration recipes, native inspector/preview scaffolds |
| Layout, styling, and animation | CSS layout/cascade, design tokens, transitions, effects, images, SVG, canvas, and web animation libraries | `DesktopPrimitive::LayoutStylingAnimation`, styled native containers, `ThemeTokens`, transitions, layer stacks, effect layers, cached/deferred subtrees, native image/SVG/Lottie/canvas surfaces, and content-safe render summaries | Strong native path | Responsive layout recipes, CSS-token migration helpers, animation timeline/keyframe templates |
| Windows and presentation | `BrowserWindow`, window management, frameless windows, always-on-top, kiosk, fullscreen, custom titlebars, native tabs, and child/popup windows | `DesktopPrimitive::WindowManagement`, `WindowManagementHandoffBuilder`, `WindowManagementNextAction`, checked window intents, placement, focused-window queries, chrome commands, presentation policies, interactions, z-order, opacity, content protection, document state, runtime snapshots, and explicit hosted popup/fullscreen islands | Strong native path | Stronger child-window/docking templates, per-OS polish tests, and kiosk enforcement |
| Screen and display topology | Desktop `screen`, display bounds, scale factor, refresh rate, cursor display, monitor-aware placement, and browser `devicePixelRatio` assumptions | `DesktopPrimitive::ScreenDisplayTopology`, `DisplayTopologyHandoffBuilder`, `DisplayTopologyNextAction`, `DisplayQueryBuilder`, `DisplaySnapshot`, topology summaries, checked window placement/restore, focused-window/runtime checks, and explicit hosted screen islands | Strong native path | Richer display-change and complex multi-monitor restore fixtures |
| Video/audio players | `<video>` / `<audio>` with browser controls, MSE/HLS/DASH where Chromium supports it | `MediaSource`, `VideoController`, `VideoPlayer::url`, native rendering for direct media, WebView fallback for adaptive manifests | Usable bridge | Hardware decode, native adaptive streaming, richer track selection |
| Audio graph and recording | Web Audio `AudioContext`, media streams, recorders, meters, waveforms, and effect chains | `DesktopPrimitive::WebAudioRecording`, `DesktopSurfaceArea::AudioGraphRecording`, `AudioWorkflowHandoffBuilder`, `AudioWorkflowNextAction`, `AudioPlayer`, `Waveform`, checked microphone/system-audio capture configs, capture pipelines, permissions, and privacy manifests | Native with guardrails for playback/capture; WebView or roadmap for arbitrary graphs | Native AudioContext-style node graph, low-latency effects/meters, offline rendering, and sample-accurate scheduling |
| Files, dialogs, and native image/icon assets | `dialog`, `native image`, `file icon request`, app/tray/document icons, file associations, document intake, and browser image-pipeline assumptions | Checked open/save dialogs, `DesktopPrimitive::ImageIconAssets`, `ImageIconAssetHandoffBuilder`, `ImageIconAssetNextAction`, `AppIconSetBuilder`, `AppIconAssetBuilder`, `FileIconRequestBuilder`, `TrayIconBuilder`, `ImageSource`, `RenderImage`, clipboard/canvas/print/drop image routing, document handlers, and explicit hosted image islands | Strong native path | Native resize/crop/encode helpers, richer app-icon conversion pipeline, more end-to-end app templates |
| Message dialogs and prompts | Desktop `native message dialog`, alert-style info/warning/error boxes, destructive confirmations, unsaved-change prompts, about dialogs, and browser alert/confirm/prompt/beforeunload semantics | `DesktopPrimitive::MessageDialogsPrompts`, `MessageDialogHandoffBuilder`, `MessageDialogNextAction`, `MessageDialogBuilder`, `MessageDialogPlan`, `message_dialog_checked`, `show_message_dialog`, `show_about_dialog_checked`, `DialogOptions`, and explicit hosted browser dialog islands | Native with guardrails | Prompt policy templates for plugins/agents, platform sheet/modal recipes, localized button-role and destructive-action templates |
| Filesystem and workspace access | Node `fs`, `path`, `fs.watch`, chokidar, workspace trees, project caches, recent projects, and shell reveal/open/trash actions | `DesktopPrimitive::FilesystemWorkspaceAccess`, `WorkspaceOpenHandoffBuilder`, `FileOperationHandoffBuilder`, `FileOperationNextAction`, `FileIntakePlanBuilder`, `AppPathBuilder`, `FileWatchOptionsBuilder`, `FileWatchSetBuilder`, `FileWatcher`, `FileWatchEvent`, `RecentDocumentsBuilder`, storage migration/cleanup plans, and `ShellTargetsBuilder` | Native with guardrails | Workspace tree/indexer templates, watcher debounce/backpressure recipes, bulk file operation transactions and undo recipes |
| App identity and metadata | Desktop `app identity name`, `app identity name`, `setAppUserModelId`, About metadata, file/protocol identity, badges, and builder/package manifests | `DesktopPrimitive::AppIdentityMetadata`, `AppIdentityMetadataHandoffBuilder`, `AppIdentityMetadataNextAction`, `AppMetadataBuilder`, `AppPackageManifestBuilder`, `AppPackageReadinessBuilder`, `DefaultHandlerPlanBuilder`, `FileAssociationSetBuilder`, `UrlSchemeRegistrationBuilder`, `WindowAppIdBuilder`, `DockBadgeBuilder`, and checked About dialogs | Native with guardrails | Platform registration backends, store listing metadata generators, localized about/legal templates |
| Drag/drop and DataTransfer | DOM `DragEvent`, `DataTransfer`, external drops, rich payloads, drag-out exports, and drop zones | `DesktopPrimitive::DragDropDataTransfer`, `ExternalDropData`, `FileDropFilter`, `FileDropEvent`, `FileDropIntentBuilder`, `FileExportDragIntentBuilder`, `DragDropTransferHandoffBuilder`, `DragDropTransferHandoff`, `DragDropTransferNextAction`, pointer policy, clipboard-compatible payloads, and scoped WebView drag/drop for hosted islands | Native with guardrails | Internal reorder/dropzone recipes, native drag-preview customization, richer non-file payload adapters |
| Clipboard, editing, selection | Browser clipboard/edit commands and OS menus | Native clipboard items, clipboard/editing handoffs, edit-state snapshots, focused edit commands, WebView clipboard bridge for hosted editors | Strong with hosted-editor fallback | Broader MIME coverage and rich-editor recipes |
| Menus, tray, dock/taskbar | App/menu roles, tray, jump lists, recent docs | `DesktopShellChromeHandoffBuilder`, `MenuCommandHandoffBuilder`, checked app menus, context menus, tray menus/icons, dock/taskbar helpers, progress, attention, window placement, recent documents, and jump lists | Broad native path | More role parity and Linux/Windows variance docs |
| Input, IME, and shortcuts | Chromium keyboard/pointer/input/composition events, focus rules, shortcut handling, global shortcuts, touch, pen, gamepad, MIDI, and raw device input | `DesktopPrimitive::InputImeShortcuts`, `ShortcutInputHandoffBuilder`, `ShortcutInputNextAction`, checked keymaps, global hotkeys, keyboard layout snapshots, IME summaries, pointer/gesture policies, touch/stylus surfaces, advanced input handoffs, focus traversal, and edit-command state | Native with guardrails | Native gamepad, MIDI, raw-input backends, and more IME fixtures |
| Localization and text system | `locale snapshot`, preferred languages, browser text direction, and browser spellcheck policy | `DesktopPrimitive::LocalizationTextSystem`, `DesktopSurfaceArea::LocalizationTextSystem`, `LocalizationTextHandoffBuilder`, `LocalizationTextNextAction`, `LocaleSnapshotBuilder`, locale direction, preferred-language snapshots, checked text-checking requests, capability reports, and scoped hosted text islands | Native with guardrails | Native Intl-style formatting helpers, language-pack recipes, and grammar backend adapters |
| Forms, validation, and autofill | HTML form controls, constraint validation, submit/reset, file inputs, browser autofill, and password managers | `DesktopPrimitive::FormsValidationAutofill`, native form controls, field/focus validation, file upload/drop descriptors, WebView form/file-input bridges only for hosted islands | Native with guardrails | Higher-level native form schemas, autofill/password-manager templates, and form wizard scaffolds |
| Notifications and shell integration | Notifications, protocol handlers, shell open/trash, paths | Checked notifications, URL schemes/deep links, shell open/trash, app paths, default handlers | Broad native path | More platform-specific action behavior docs |
| Printing, document export, protocols, app paths | `hosted document print`, `hosted PDF export`, `hosted save-page export`, `protocol.handle`, `app path lookup`, default protocol/file handlers | `PrintJob`, `PrintRequest`, `DocumentExportRequest`, `DocumentExportFormat`, `DocumentExportDestination`, `DocumentOutputHandoffBuilder`, `DocumentOutputHandoff`, `DocumentOutputNextAction`, `CustomProtocolRouterBuilder`, `CustomProtocolFileResolverBuilder`, `DefaultHandlerPlanBuilder`, `FileAssociationSetBuilder`, and `AppPathBuilder` | Native-first bridge | Native PDF byte rendering backend, save-page archive writers, more print-layout templates, and platform registration backends |
| App storage and sessions | Chromium profile folders, cookies, localStorage, IndexedDB, caches, app settings | `DesktopPrimitive::AppStorageSessions`, `App::app_storage_session_handoff_checked`, `AppStoragePlanBuilder`, `AppStorageSessionHandoffBuilder`, `AppStorageSessionHandoff`, `AppStorageSessionNextAction`, app paths, settings/SQLite/cache entries, credential storage, migrations, cleanup, and WebView session boundaries | Native-first bridge | Higher-level settings and migration templates |
| Safe storage and credentials | Desktop `secure storage`, encrypted token files, credential persistence, refresh secrets, API keys, logout cleanup | `DesktopPrimitive::SecureStorageCredentials`, `SecureCredentialHandoffBuilder`, `SecureCredentialNextAction`, `CredentialBuilder`, `CredentialServiceBuilder`, `CredentialWriteRequest`, `StoredCredential`, checked secure credential read/write/delete calls, secure-keychain feature checks, and redacted support diagnostics | Native with guardrails | App-scoped encrypt/decrypt byte helpers, credential rotation and multi-account templates, passkey/WebAuthn native recipes |
| App lifecycle and single instance | App readiness, final-window behavior, activation, quit/relaunch, login items, recent docs, and single-instance ownership | `DesktopPrimitive::AppLifecycleSingleInstance`, `App::app_lifecycle_startup_handoff_checked`, `AppLifecycleStartupHandoffBuilder`, `AppLifecycleStartupHandoff`, `AppLifecycleStartupNextAction`, lifecycle policies/commands, runtime snapshots, single-instance launch, auto-launch, and recent documents | Native with guardrails | Richer per-OS lifecycle events and activation templates |
| Launch arguments and environment | Startup arguments, env-derived mode, current directory, executable identity, document/deep-link opens, and duplicate-launch payloads | `DesktopPrimitive::LaunchEnvironmentConfig`, `LaunchEnvironmentHandoffBuilder`, `LaunchEnvironmentNextAction`, `LaunchContextBuilder`, launch argument policies, environment allowlists, startup diagnostics, and duplicate-launch handoff descriptors | Native with guardrails | Startup-source classification, deep-link/document-open normalization, and redacted diagnostics export templates |
| IPC and command messaging | `typed IPC host`, `typed IPC client`, `explicit bridge`, preload APIs, command routing, helper/extension messages | `DesktopPrimitive::IpcCommandMessaging`, `App::command_ipc_handoff_checked`, `CommandIpcHandoffBuilder`, `CommandIpcHandoff`, `CommandIpcNextAction`, checked command registry, command palette descriptors, typed IPC transport, extension RPC envelopes, and WebView bridge messages only for hosted pages | Native with guardrails | Higher-level typed command bus and schema-derived IPC validators |
| Security and permission policy | `contextIsolation`, sandbox, permission handlers, session permission requests, preload trust boundaries | `DesktopPrimitive::SecurityPermissionsPolicy`, `App::security_permission_handoff_checked`, `SecurityPermissionHandoffBuilder`, `PermissionBrokerInstallBuilder`, `ThreatModel`, `Capability`, `PermissionRequestBuilder`, privacy manifests, network policy, keychain storage, process context, and WebView permission bridges only for hosted pages | Native with guardrails | Least-privilege templates, WebView CSP/navigation policy recipes, and platform entitlement backends |
| Background tasks and workers | Node `worker threads`, `utility process`, async queues, progress, cancellation, retries | `DesktopPrimitive::BackgroundTasksWorkers`, `App::background_work_handoff_checked`, `BackgroundWorkHandoffBuilder`, `BackgroundWorkHandoff`, `BackgroundWorkNextAction`, `JobScheduler`, `BackgroundJob`, `JobDescriptor`, `WorkerPool`, `WorkerHandle`, cancellation tokens, retry policies, progress events, and typed worker IPC | Native with guardrails | Durable persisted queues and resume-after-restart templates |
| Navigation, history, and routing | Browser history, location/title/favicon, reload/back/forward, target-blank, SPA routes | `DesktopPrimitive::NavigationHistoryRouting`, `NavigationHandoffBuilder`, `NavigationRouteDescriptorBuilder`, `Navigator`, `Route`, tabs, links, breadcrumbs, session snapshots, and WebView navigation bridges for hosted pages | Native with guardrails | Richer document history, tab restore, and preview/favicons templates |
| Find, page search, and zoom | `hosted find`, `found-in-page`, `stopFinding`, `setZoomFactor`, `getZoomFactor`, browser zoom shortcuts/gestures, and viewer search | `DesktopPrimitive::FindInPageZoom`, `FindZoomHandoffBuilder`, native command/search panels, editor/markdown/rich-text result summaries, route/list scroll integration, document zoom buckets, and WebView find/zoom bridges only for hosted pages | Native with guardrails | Native document-search index helpers, zoom controller templates, cross-surface result aggregation |
| Network, realtime, downloads | `fetch`, WebSocket, EventSource, Chromium downloads, Node networking | `NetworkRealtimeHandoffBuilder`, `AppNetworkRequestBuilder`, `NetworkPolicyBuilder`, `AppRealtimeConnection`, `DownloadRequest`, `DownloadExecutionPlan`, and `DownloadHandoff` for app-owned traffic; WebView only for browser-owned auth/widgets/cookies/downloads | Native-first bridge | Higher-level REST/GraphQL/WebSocket templates and richer download UI |
| Permissions, capture, power, theme | Browser permission prompts plus Desktop `power-save blocker`, `power monitor`, `nativeTheme`, and idle APIs | Permission broker and capture builders plus `PowerThemeIdleHandoffBuilder`, `PowerSaveBlockerBuilder`, `SystemPowerMonitorBuilder`, `NativeThemeSnapshot`, and `SystemIdlePolicyBuilder` | Strong native path | More system permission and adaptive-work recipes |
| Hardware and device APIs | WebUSB, WebHID, Web Serial, Web Bluetooth, gamepad, and MIDI-style app requirements | `HardwareDeviceHandoffBuilder`, checked `DeviceAccessRequest` descriptors, permission broker mapping, privacy manifest declarations, hosted vendor setup, and native backend work tracking | Partial | Native discovery and IO backends beyond checked descriptors |
| Packaging, updater, signing | desktop-builder/desktop-updater ecosystem | `PackagingUpdateHandoffBuilder`, `PackagingUpdateNextAction`, distribution/signing/update policy builders, icon/entitlement checks, crash reporter setup, restart paths, and download request summaries | Practical bridge | Complete release templates per target |
| Accessibility and automation | DOM accessibility tree, roles, focus, actions, selectors, and test/agent actionability | `DesktopPrimitive::AccessibilityDomTree`, `AccessibilityAutomationHandoffBuilder`, `DesktopSurfaceArea::AccessibilityAutomation`, semantic attributes, accessibility tree audits, action routing, announcements, focus handoffs, and content-safe automation summaries | Native with guardrails | Cross-process automation harness templates and richer custom-role recipes |
| Performance and resource claims | Chromium process model, task manager, devtools memory visibility, and profiling timelines | `DesktopPrimitive::PerformanceMemoryDiagnostics`, `PerformanceEvidenceHandoffBuilder`, `PerformanceEvidenceNextAction`, native runtime/process snapshots, resource budgets, benchmark sample pairs, baseline comparisons, bounded traces, support diagnostics, and explicit hosted profiler islands | Evidence-first | Larger shared benchmark suite, cross-platform CPU/GPU memory collectors, native timeline inspectors |
| Crash reporting and support diagnostics | Desktop `crash reporting`, pending crash uploads, crash dumps, release symbolication, and support bundles | `DesktopPrimitive::CrashReportingDiagnostics`, `CrashReportingHandoffBuilder`, `CrashReportingNextAction`, `CrashReporterBuilder`, `CrashReporter`, `CrashReport`, `App::install_crash_reporter_checked`, pending report submission, `SupportDiagnosticsBuilder`, `NetworkPolicyBuilder`, hosted crash dashboards, and roadmap tracking | Native-first bridge | Native minidump and symbolication pipeline, release-symbol upload recipes, privacy-review templates for crash attachments |
| Developer tools and observability | `devtools inspector`, console, network/protocol inspection, traces | `DesktopPrimitive::DeveloperToolsObservability`, `DeveloperObservabilityHandoffBuilder`, native diagnostics panels, structured log sinks, trace sessions, WebView DevTools bridges for browser islands, and redacted support bundles | Native-first bridge | Richer native layout/network/timeline inspectors |
| Web compatibility | Full Chromium DOM/CSS/JS/browser platform | Explicit WebView islands with bridges, storage, cookies, devtools, media/form/dialog/network events | Compatibility fallback | Better recipes for auth, payments, maps, rich editors |
| Low-level graphics escape hatch | Canvas/WebGL/WebGPU and compositor effects | `GraphicsCanvasHandoffBuilder`, native canvas/path/image/SVG/Lottie, effect layers, headless rendering; render targets/custom shaders marked roadmap | Partial | Public render target and shader APIs |
| Visual capture and snapshots | `hosted page controller.visual capture`, offscreen rendering, screenshots, thumbnails, media frames, and visual test evidence | `VisualCaptureHandoffBuilder`, checked app-window capture requests, headless renderer evidence, cached/effect snapshots, scoped WebView DOM/media frame capture, redacted support diagnostics, and roadmap capture work tracking | Native with guardrails | Native screenshot backend dispatch, full-page stitched capture recipes, visual diff/thumbnail templates |
| Native escape hatches | Node native modules, child processes, utility processes | `App::helper_plugin_handoff_checked`, `HelperPluginHandoffBuilder`, worker/extension/helper process plans with permission checks and summaries | Practical bridge | More templates for FFmpeg, language servers, plugin hosts |

The rule of thumb is simple: start native when the app owns the experience,
choose WebView when the dependency is the web platform itself, and require a
capability/reporting check before marking native desktop capability ready.

This audit should not be WebView-first. WebView is one compatibility lane for
browser-native dependencies such as OAuth pages, maps, payments, embedded docs,
legacy rich editors, browser-only media pipelines, and WebGL/WebGPU content. A
credible native desktop capability also needs native answers for the rest of the app:
windowing, screen/display topology, app chrome, layout/styling/animation, menus,
input/IME/shortcuts, localization/text behavior, audio graph/recording, tray/dock/taskbar integration, files, drag/drop data transfer,
app storage/session state, app lifecycle/single-instance, launch environment/config, IPC/command messaging, security/permissions policy, background tasks/workers, navigation/history/routing,
clipboard, capture, notifications, shell integration, updater flows, helper
processes, extension hosts, accessibility/automation, graphics, printing,
packaging, developer tools, telemetry, resource budgets, and AI-agent-readable summaries. When a
builder asks "can I build any app?", classify the app by those surfaces first;
only reach for WebView when the required surface is actually the web platform.

Use this app-type ladder before choosing an implementation route:

| App type | Primary Kael route | WebView role |
| --- | --- | --- |
| Productivity tools, dashboards, CRMs, IDE-like shells | Native windows, `kael_ui`, menus, command registry, files, plugins, background jobs | Hosted docs, auth, third-party widgets, or legacy editors |
| Media apps | Native `VideoPlayer` / `AudioPlayer`, playlists, captions, audio capture/waveforms, shell/share/export, resource budgets | Adaptive streams, browser-only DRM/player SDKs, or arbitrary Web Audio graphs until native backends land |
| Creative/canvas/design tools | Native canvas, paths, SVG, Lottie, surfaces, layers, headless renderer | Browser-only WebGL/WebGPU effects or imported web editors |
| Communication/collaboration apps | Native realtime/network plans, notifications, tray, permissions, capture | Embedded web calls, account pages, external team widgets |
| Developer tools | Native file watching, subprocesses, extension hosts, terminals, command palette, updater | Browser previews, docs, inspector-like panels |
| Consumer/commerce apps | Native chrome, menus, downloads, share sheets, notifications, app metadata | Payments, maps, auth, vendor widgets |

## App-Type Capability Recipes

Use these recipes when deciding whether a requested app should be native,
WebView-backed, or mixed. The goal is not to clone Desktop's implementation
model; it is to give builders the same practical reach with clearer ownership
boundaries and lower default resource cost.

| App category | Common expectations | Native Kael path | WebView only when | Still explicit gap |
| --- | --- | --- | --- | --- |
| Media player/editor | `<video>` / `<audio>`, playlist state, fullscreen/PiP, file open, hardware keys, downloads/exports, share sheet | `VideoPlayer`, `AudioPlayer`, `VideoPlaylist`, media-key bindings, file dialogs/intake, `DownloadBatch`, `ShareSheet`, power blockers, resource budgets | Adaptive HLS/DASH/DRM player SDKs or browser-only controls are required | Native hardware decode, adaptive streaming, stream selection, pitch-preserving speed |
| Project/file explorer or IDE shell | `window management`, menus, command palette, file watching, recent projects, session restore, helper processes, language server, terminals, extension host | `WindowIntentBuilder`, `kael_ui` navigation/data/editor components, app menus, command registry, `FileWatchSet`, `SessionStore`, `JumpListPlan`, `HelperProcessLaunch::language_server`, plugin manifests/RPC | Embedded web preview, docs, devtools-like inspector, or imported web editor is central | More end-to-end templates for IDE/project shells |
| Collaboration/chat app | WebSocket/EventSource, notifications, tray, permissions, capture, reconnecting background work, app badge/status | `AppRealtimeConnectionSet`, `NotificationBuilder`, tray/dock/taskbar helpers, permission broker, capture manager, background jobs, app lifecycle and power snapshots | External account pages, hosted meeting/call widgets, or web-only team widgets are required | More permission UX recipes and platform notification variance docs |
| Creative/canvas/design tool | Canvas/SVG/WebGL, drag/drop, clipboard images, undo/redo, asset caches, export/download, window inspectors | Native canvas/path/image/SVG/Lottie, effect layers, `HeadlessRenderer`, drag/drop payloads, rich clipboard, undo/redo, image caches, `DownloadBatch`, utility windows | Browser-only WebGL/WebGPU engines, imported design editors, or cross-origin asset widgets are required | Public render targets/custom shaders |
| Consumer commerce or dashboard app | Native shell, auth, payments, maps, downloads, notifications, settings, app metadata, updater | `kael_ui`, app metadata/about, settings/storage plans, notifications, shell/deep links, app network/download descriptors, update policy/readiness | Payment provider widgets, maps, OAuth/SSO, embedded docs, or vendor dashboards are required | More complete release templates per target |
| Plugin-heavy app | Native modules, child processes, extension manifests, command/menu contributions, permissions, IPC | `App::helper_plugin_handoff_checked`, `HelperPluginHandoffBuilder`, `PluginManifest`, extension host/runtime, permission broker, `HelperProcessLaunch::plugin_host`, typed IPC/RPC, command registry, file bookmarks | Existing plugin UI is web-only or the plugin ecosystem expects a browser page | More templates for migrated Desktop/Node plugin hosts |

Use the typed planner before committing to a native/WebView split:

```rust
let portfolio = DesktopCapabilityPortfolioAudit::all(&CapabilityReport::current());
let plan = DesktopAppCategory::MediaPlayerEditor
    .capability_plan(&CapabilityReport::current());
let category_brief = DesktopAppCategory::MediaPlayerEditor
    .generation_brief(&CapabilityReport::current());
let contract = DesktopAppCategory::MediaPlayerEditor
    .generation_contract(&CapabilityReport::current());
let category_handoff = contract.builder_handoff();
if let Some(media_packet) =
    category_handoff.requirement_work_packet_for(DesktopAppRequirement::MediaPlayback)
{
    tracing::info!(summary = media_packet.to_text(), "desktop category media packet");
}
if let Some(batch) = contract.current_execution_batch() {
    tracing::info!(
        summary = batch.to_text(),
        "desktop category native-first execution batch"
    );
}
let manifest = DesktopAppCategory::MediaPlayerEditor
    .generation_manifest(&CapabilityReport::current());
let queue = manifest.generation_queue();
let brief = plan.brief();
let remediation = plan.remediation_summary();
let primitive_audit = DesktopAppCategory::MediaPlayerEditor
    .primitive_bridge_audit(&CapabilityReport::current());
let matrix = DesktopAppCategory::MediaPlayerEditor
    .capability_matrix(&CapabilityReport::current());
let blueprint = DesktopAppCategory::MediaPlayerEditor
    .generation_blueprint(&CapabilityReport::current());

tracing::info!(summary = portfolio.to_text(), "desktop capability portfolio audit");
if let Some(focus) = portfolio.recommended_focus() {
    tracing::info!(summary = focus.to_text(), "desktop capability portfolio focus");
}
if let Some(handoff) = portfolio.recommended_handoff() {
    tracing::info!(summary = handoff.to_text(), "desktop capability portfolio handoff");
    let evidence = handoff.acceptance_evidence_report();
    let review = handoff.review_evidence(evidence);
    tracing::info!(summary = review.to_text(), "desktop capability portfolio evidence review");
}
tracing::info!(summary = category_brief.to_text(), "desktop category generation brief");
tracing::info!(summary = contract.to_text(), "desktop category generation contract");
tracing::info!(summary = contract.next_step().to_text(), "desktop generation next step");
tracing::info!(summary = category_handoff.to_text(), "desktop category builder handoff");
tracing::info!(summary = manifest.to_text(), "desktop category generation manifest");
tracing::info!(summary = queue.to_text(), "desktop category generation queue");
tracing::info!(summary = brief.to_text(), "desktop capability brief");
tracing::info!(summary = matrix.to_text(), "desktop capability matrix");
tracing::info!(summary = primitive_audit.to_text(), "desktop primitive audit");
tracing::info!(summary = blueprint.to_text(), "desktop generation blueprint");

if let Some(item) = matrix.current_backlog_item() {
    tracing::info!(summary = item.to_text(), "desktop capability backlog item");
}
if let Some(ticket) = matrix.current_backlog_ticket() {
    tracing::info!(summary = ticket.to_text(), "desktop capability backlog ticket");
    let scaffold = &ticket.scaffold_hint;
    let starters = &ticket.starter_recipe.native_starters;
    let evidence = &ticket.evidence_checklist;
    let review = ticket.ready_review();
    tracing::info!(summary = review.to_text(), "desktop capability ticket review");
    /* generate or verify the ticket without guessing placement or criteria */
}

match plan.readiness() {
    DesktopCapabilityReadiness::NativeReady => {
        /* generate the native app path */
    }
    DesktopCapabilityReadiness::NativeWithGuardrails
    | DesktopCapabilityReadiness::WebViewIsland
    | DesktopCapabilityReadiness::RoadmapGap => {
        /* brief caveats, isolate WebView islands, or state roadmap items first */
    }
    DesktopCapabilityReadiness::BlockedByFeatureGaps => {
        /* do not mark readiness; inspect feature gaps before generating */
    }
}

tracing::info!(summary = remediation.to_text(), "desktop capability remediation");

for action in plan.action_items() {
    tracing::info!(summary = action.to_text(), "desktop capability action");
    if action.blocks_generation() {
        let missing = &action.feature_gaps;
        let details = &action.feature_gap_details;
        let needs_fallback = action.needs_fallback_or_roadmap();
        /* resolve missing native capability, choose a narrower app shape, or state not ready */
    }
}

for step in blueprint.native_steps() {
    let recipe = step.starter_recipe();
    /* generate the native Kael primitive from recipe.native_starters */
}

for row in matrix.non_webview_gap_rows() {
    tracing::info!(summary = row.to_text(), "non-webview desktop capability gap");
    /* resolve native/platform capability work; do not treat this as a WebView answer */
}

for item in matrix.backlog_items() {
    match item.kind {
        DesktopCapabilityBacklogKind::ResolveNativeCapabilityGap => {
            /* resolve the native/platform blocker before generating dependent work */
        }
        DesktopCapabilityBacklogKind::BuildReadyNativePrimitive => {
            /* generate from native starters and setup checks */
        }
        DesktopCapabilityBacklogKind::CollectAcceptanceEvidence => {
            /* run smoke, parity, and resource checks */
        }
        DesktopCapabilityBacklogKind::IsolateBrowserDependency => {
            /* keep the browser-shaped dependency as a named WebView island */
        }
        DesktopCapabilityBacklogKind::StateRoadmapCaveat => {
            /* brief the remaining roadmap caveat before marking readiness */
        }
    }
}

for ticket in matrix.backlog_tickets() {
    tracing::info!(summary = ticket.to_text(), "ready desktop capability ticket");
    let review = ticket.ready_review();
    if !review.can_close_ticket {
        /* generate, fix, collect evidence, isolate WebView, or brief caveats based on review.next_kind */
    }
    /* ticket carries the item, starter recipe, scaffold hint, acceptance criteria, and evidence checklist */
}

let review = matrix.ready_backlog_review();
tracing::info!(summary = review.to_text(), "desktop capability backlog review");
let native_first = matrix.native_first_plan();
tracing::info!(summary = native_first.to_text(), "desktop native-first work plan");
if let Some(phase) = native_first.current_phase() {
    tracing::info!(summary = phase.to_text(), "desktop native-first work phase");
    tracing::info!(
        summary = phase.assignment().to_text(),
        "desktop native-first phase assignment"
    );
    let checklists = phase.evidence_checklists();
    let kits = phase.execution_kits();
    let batch = phase.execution_batch();
    let continuation = native_first.continue_with_execution_batch_evidence(&batch, checklists);
    tracing::info!(
        summary = continuation.to_text(),
        "desktop native-first execution continuation"
    );
    /* assign the whole phase without flattening WebView work ahead of native work */
}
if native_first.should_defer_webview() {
    /* resolve blockers, build native primitives, and collect evidence before WebView islands */
}
if let Some(pass) = native_first.current_generation_pass() {
    /* execute the first native-first ticket */
}
let claim = native_first.ready_readiness_decision();
tracing::info!(summary = claim.to_text(), "desktop matrix parity decision");
if let Some(ticket) = review.current_ticket() {
    let next_kind = review.next_kind();
    let evidence = review.current_evidence_checklist();
    /* continue with this ticket and next kind across the matrix */
}
if let Some(pass) = review.current_generation_pass() {
    tracing::info!(summary = pass.to_text(), "desktop capability generation pass");
    tracing::info!(summary = pass.brief().to_text(), "desktop capability generation pass brief");
    if pass.resolves_blocker() {
        let gaps = pass.feature_gaps();
        /* resolve native/platform blockers */
    } else if pass.can_generate_native() {
        let starters = pass.native_starters();
        let scaffold = pass.scaffold_hint();
        /* generate the native primitive from starters and scaffold */
    } else if pass.should_collect_evidence() {
        let criteria = pass.acceptance_criteria();
        let evidence = pass.expected_evidence_checklist();
        /* run smoke, parity, and resource checks */
        let outcome = pass.complete_with_evidence(evidence);
        tracing::info!(summary = outcome.to_text(), "desktop pass outcome");
        let refreshed = matrix.review_backlog_evidence([outcome.evidence_checklist().clone()]);
        tracing::info!(summary = refreshed.to_text(), "desktop refreshed backlog review");
    }
}

for entry in manifest.native_entries() {
    let starters = &entry.native_starters;
    let setup = &entry.setup_checks;
    let scaffold = entry.scaffold_hint();
    let acceptance = entry.acceptance_criteria();
    /* generate the category primitive and run its setup checks */
}

if queue.is_blocked() {
    let blockers = queue.blocking_phase().unwrap();
    /* resolve blockers before generating */
} else {
    for entry in queue.ready_entries() {
        let starters = &entry.native_starters;
        let scaffold = entry.scaffold_hint();
        let acceptance = entry.acceptance_criteria();
        /* generate the next native primitive */
    }
    for hint in queue.ready_scaffold_hints() {
        /* place generated modules, state, views, commands, and verification hooks */
    }
    for criteria in queue.ready_acceptance_criteria() {
        /* verify smoke, parity, and resource criteria before marking readiness */
    }
    for checklist in queue.ready_acceptance_evidence_checklists() {
        tracing::info!(summary = checklist.to_text(), "desktop acceptance evidence");
        if !checklist.is_complete() || checklist.has_failures() {
            /* keep the readiness claim gated until evidence passes */
        }
    }
    let evidence = queue.ready_acceptance_evidence_report();
    tracing::info!(summary = evidence.to_text(), "desktop acceptance evidence report");
    let claim = queue.readiness_decision(&evidence);
    tracing::info!(summary = claim.to_text(), "desktop readiness decision");
    if !claim.can_claim_parity {
        /* show blocked primitives and keep readiness decision gated */
    }
    let refreshed = contract.with_ready_evidence(evidence);
    tracing::info!(summary = refreshed.to_text(), "desktop updated generation contract");
    match refreshed.next_action() {
        DesktopGenerationNextAction::ResolveGenerationBlockers => {
            /* resolve required primitives before generating */
        }
        DesktopGenerationNextAction::GenerateReadyNativeWork => {
            /* generate from ready entries, scaffold hints, and starter recipes */
        }
        DesktopGenerationNextAction::FixFailedEvidence => {
            /* fix failed checks before collecting fresh evidence */
        }
        DesktopGenerationNextAction::CollectReadyEvidence => {
            /* run the remaining smoke, parity, and resource checks */
        }
        DesktopGenerationNextAction::BriefCaveats => {
            /* state WebView islands and roadmap gaps before claiming readiness */
        }
        DesktopGenerationNextAction::ClaimParity => {
            /* ready-work readiness can be marked for this evaluated scope */
        }
    }
}

for entry in manifest.webview_entries() {
    let islands = &entry.webview_islands;
    /* keep browser-shaped behavior isolated and named */
}
```

For preset categories, use `requirements()`, `capability_intake()`, or
`builder_handoff(&report)` when the scheduler needs the same app-wide
requirement packets custom intakes expose. `DesktopAppRequirement::from_primitive(...)`
maps a primitive family back to the requirement name, and
`DesktopCategoryGenerationContract::builder_handoff()` exposes
`requirement_work_packet_for(...)`, focus-filtered requirement packets, native
starters, setup checks, acceptance criteria, evidence checklists, strategic
bridge tracks/priorities, and the next native/WebView/roadmap batch from the
already-evaluated category contract. Use
`requirement_packets_for_bridge_track(track)`,
`requirement_packets_for_bridge_priority(priority)`, and
`critical_bridge_packets()` to route design freedom, canvas/graphics,
performance/memory, production readiness, media/audio, native desktop APIs, and
agent developer-experience work without flattening it into a WebView queue. Use
`active_bridge_track_summaries()` and `active_bridge_priority_summaries()` when
a scheduler needs count-level packets, kits, blockers, native work, evidence,
WebView, roadmap, and highest-priority summaries before assigning parallel
workstreams. Use `active_bridge_track_workstreams()`,
`bridge_track_workstream(track)`, or `critical_bridge_workstreams()` when the
scheduler needs the matching packets plus missing-by-default evidence checklists
as one dispatchable bundle. Use
`contract.continue_with_bridge_track_workstream_evidence(&workstream,
checklists)` when a graphics, media, performance, production, native desktop
API, or developer-experience worker returns evidence and needs the refreshed
contract plus next app-wide builder handoff without passing through a
WebView-specific queue. Use
`contract.run_bridge_track_workstream_loop([(track, checklists)], max_steps)`
when a coordinator receives multiple returned strategic tracks and needs one
final handoff/report across the bridge pass. Use
`handoff.replacement_scorecard()` before making an Electron-replacement claim:
it joins current evidence coverage with bridge-track/priority scope, reports the
next blocking track and priority, and keeps deferred WebView islands separate
from native-first blockers. Use `handoff.replacement_work_order()` to get the
priority-sorted immediate packets plus deferred WebView and roadmap packets
before assigning track owners. Use
`handoff.first_replacement_assignment_packet()` or
`work_order.first_assignment_packet()` when an implementation worker needs the
first packet's kits, scaffold hints, native starters, quick starts, setup
checks, acceptance criteria, and evidence checklists in one build-ready object.
Use `handoff.replacement_assignment_packets()` or
`work_order.assignment_packets()` to dispatch all immediate assignments in
parallel, and `critical_replacement_assignment_packets()` /
`critical_assignment_packets()` when only critical bridge work should fan out.
Call
`contract.continue_with_replacement_assignment_batch_evidence([(assignment,
checklists)])` to fan those parallel evidence returns back into one refreshed
contract and next handoff.
Use `contract.run_replacement_assignment_batch_loop(evidence_groups, max_steps)`
when a coordinator wants to repeat the same parallel fan-out/fan-in cycle across
refreshed immediate assignment packets.
Call `contract.continue_with_replacement_assignment_evidence(&assignment,
checklists)` when that implementation worker returns evidence so the refreshed
contract and next handoff stay connected to the assignment that ran.
Use `contract.run_replacement_assignment_loop(evidence_batches, max_steps)`
when a coordinator wants to keep taking the refreshed first replacement
assignment until evidence is exhausted, the step cap is reached, or readiness is
available.

`DesktopCapabilityPlan` combines the app-category recipe, full desktop-surface
audit, and implementation briefs. `DesktopCapabilityBrief` is the content-safe
pre-generation summary to show first; it includes category, readiness, generation
permission, caveat requirement, surface/action/blocker counts, remediation
counts, WebView presence, and roadmap presence without exposing URLs, file
paths, project names, captions, account ids, extension ids, device identifiers,
or arbitrary plugin metadata. Use `plan.readiness()`, `plan.can_generate()`,
`plan.needs_briefing()`, `plan.action_items()`, `plan.blocking_action_items()`,
`plan.remediation_summary()`, `plan.recipe`, `plan.surface_audit`, and
`plan.implementation_briefs` when an agent needs the lower-level checklist.
Action items are ordered as blocking feature gaps, roadmap declarations, WebView
islands, then native guardrails. Blocking action items expose safe
`PlatformFeature` identifiers through `feature_gaps` and support levels through
`feature_gap_details`, while `to_text()` reports counts only. Each
`PlatformFeatureGap` exposes `remediation()` so agents can distinguish
permission/setup work from guarded native paths, policy/configuration blocks, and
fallback-or-roadmap gaps without parsing prose.
Use `DesktopAppCategory::primitives()`, `primitive_bridge_audit(&report)`,
`capability_matrix(&report)`, and `generation_blueprint(&report)` when the user
has already named an app shape; these presets choose the usual Desktop
primitive families for media players, project/file explorers, collaboration
apps, creative tools, dashboards, and plugin-heavy apps before an agent writes
code. For bespoke app briefs, build an `DesktopCapabilityIntake` from
`DesktopAppRequirement` values such as `MediaPlayback`, `FilesAndDocuments`,
`HardwareDevices`, `PluginsAndProcesses`, `AccessibilityAutomation`, and
`PerformanceDiagnostics`. The intake deduplicates requirements into Desktop
primitive families and exposes `primitive_bridge_audit(&report)`,
`generation_brief(&report)`, `capability_matrix(&report)`,
`native_first_plan(&report)`, `generation_contract(&report)`, and
`ready_readiness_decision(&report)` so agents can plan a custom video player,
developer tool, editor, hardware utility, or plugin host without pretending it
belongs to one preset category.
`DesktopCapabilityIntakeBrief` is the first count-level summary to show for a
custom brief: it reports requirement and primitive counts, native blockers,
ready native tickets, evidence tickets, deferred WebView islands, roadmap
caveats, current primitive, and readiness status without logging app content. Use
`DesktopCapabilityIntakeContract` when a worker needs the full handoff:
brief, matrix, native-first plan, backlog review, claim decision,
`current_generation_pass()`, `next_step()`, `next_action()`, and
`with_evidence_checklists(...)` for refreshing evidence after generated work.
When workers return `DesktopCapabilityGenerationPassOutcome`, call
`contract.with_pass_outcomes(outcomes)`; the refreshed contract updates backlog
review, readiness decision, next action, and the count-level brief without manually
extracting checklists. `current_pass_brief()` and
`current_evidence_checklist()` are available when a UI or worker queue needs a
small assignment payload before executing the pass.
`DesktopCapabilityIntakeNextStep` is the count-level dispatch summary for
custom briefs: it includes action, current primitive, blocker/native/evidence
ticket counts, deferred WebView/roadmap counts, evidence totals, and whether
caveats still need briefing. Use `contract.current_work_order()` when a worker
queue needs one serializable handoff containing the next-step summary plus the
focused pass; call `work_order.assignment()` when the scheduler only needs the
compact counts and flags for resolving native blockers, generating native work,
collecting evidence, isolating a browser dependency, or briefing a roadmap
caveat. Call `work_order.execution_kit()` when the worker needs concrete
built-in starter APIs, setup checks, scaffold placement, acceptance criteria,
and the evidence checklist for the native-first pass. For app-wide audits, call
`contract.open_execution_kits()` for unresolved review-state work,
`contract.planned_execution_kits()` for the full planned capability map, or the
filtered
`blocker_execution_kits()`, `native_execution_kits()`,
`webview_execution_kits()`, and `roadmap_execution_kits()` helpers before
assigning work so devices, files, capture, accessibility, diagnostics,
packaging, media, and browser islands remain visible together. Start broad with
`contract.execution_coverage()` when the scheduler needs count-level coverage
across planned/open kits, blockers, native work, evidence, WebView islands,
roadmap caveats, feature gaps, starter APIs, setup checks, criteria, and
evidence totals before assigning focused workers. Use
`contract.requirement_coverage()` when the scheduler must keep every requested
surface visible as a row, including media, files, windows, devices, packaging,
accessibility, diagnostics, WebView islands, and roadmap caveats, or
`contract.requirement_coverage_for(requirement)` for one surface. Use
`contract.requirement_work_packets()` when workers need those rows plus concrete
execution kits per requested surface, or
`contract.requirement_work_packet_for(requirement)` to assign one surface owner.
Packets preserve native blockers, native starters, evidence, WebView islands,
roadmap caveats, bridge track, and bridge priority together and can complete
matching evidence checklists. Use
`contract.builder_handoff()` when the scheduler needs the intake brief,
execution coverage, recommended next batch, and all requirement packets as one
app-wide handoff before splitting work across agents. Use
`handoff.next_requirement_packets()` for the surfaces matching the current focus,
or `handoff.requirement_packets_for_focus(focus)` to route native blockers,
native work, evidence, browser islands, and roadmap caveats explicitly. Use
`handoff.next_scaffold_hints()`, `handoff.next_native_starters()`,
`handoff.next_quick_start_steps()`, `handoff.next_setup_checks()`,
`handoff.next_acceptance_criteria()`, and `handoff.next_evidence_checklists()`
as the concrete next-batch build payload. For media apps, quick starts make the
direct URL path explicit with `VideoElementHandoffBuilder::url(url)`,
`VideoUrlPlaybackHandoff::url(url)?`, `MediaSourceBuilder`,
`VideoPlaybackPlanBuilder`, `VideoElementHandoff`,
`VideoElementHandoffNextAction`, `VideoPlaybackRequirementPlan`,
`video_capability_report()`, `VideoPlaybackControlsBuilder`,
`TextTrackBuilder`, `VideoPlaylist`, `MediaKeyBindingBuilder`,
`WebViewVideoOptions`, `WebViewVideoCommandBuilder`, and
`kael_ui::VideoPlayer::url(...)`, so ordinary URL/file/bytes/reader media starts
native while HLS/DASH/DRM/browser-only SDK needs become explicit WebView media
islands with checked commands. For file-heavy apps, quick starts point agents at
`OpenDialogBuilder::files()`, `OpenDialogBuilder::directory()`,
`SaveDialogBuilder::new(dir).suggested_name(name)`, `FileIntakePlanBuilder`,
`FileExportDragIntentBuilder`, and `RecentDocumentsBuilder` before considering a
hosted file picker; filesystem/workspace quick starts point at
`FileIntakePlanBuilder`, `AppPathBuilder`, `FileWatchSetBuilder`,
`FileWatcher`, `FileWatchEvent`, storage migration/cleanup plans,
`AppStorageSessionHandoffBuilder`, `AppStorageSessionHandoff`,
`AppStorageSessionNextAction`, and `ShellTargetsBuilder` before reaching for
Node `fs`, `path`, `fs.watch`,
chokidar, browser file handles, or hosted file-manager widgets. For editor-heavy apps, quick starts point at
`ClipboardItem::builder`, `ClipboardReadRequestBuilder`,
`ClipboardEditingHandoffBuilder`, `ClipboardEditingHandoff`,
`ClipboardEditingNextAction`, `MenuBuilder::standard_edit`,
`cx.edit_command_state_snapshot_checked()`, and `ClipboardClearBuilder` before
falling back to browser selection or clipboard APIs. For notification and shell flows, quick starts point at
`NotificationBuilder`, `NotificationFlowHandoffBuilder`,
`show_desktop_notification_with_action_router`, `ShellTargetsBuilder`,
`DeepLinkRouterBuilder`, `DeepLinkSetupPlan`, and `UserAttentionBuilder` before
falling back to hosted account or permission pages. For capture and permission flows, quick starts point at
`PermissionRequestBuilder::capture_studio()`, `AppPrivacyManifestBuilder`,
`CaptureSourceQueryBuilder::screens_and_windows()`, `CaptureConfigBuilder`,
`CaptureConfigSetBuilder`, `CaptureHandoffBuilder`, `CaptureHandoff`,
`CaptureHandoffNextAction`, `CaptureManager`, and `CapturePipeline` before
falling back to a hosted meeting widget. For graphics/canvas flows, quick starts
point at `graphics_capability_report()`, `canvas(size, draw)`, `DrawContext`,
`PathBuilder`, `ImageSource`, `svg()`, `Lottie`, `effect_layer(...)`, and
`HeadlessRenderer` before isolating browser-only WebGL/WebGPU engines as WebView
islands. After workers return evidence, call
`contract.continue_with_builder_handoff_evidence(&handoff, checklists)` to get
the refreshed contract and next app-wide handoff in one step. Use
`contract.run_builder_handoff_loop(evidence_batches, max_steps)` for bounded
multi-step builder loops with stop reason, final handoff, and final report. Use
`coverage.recommendation()` to route the next worker to native blockers, native
work, failed evidence, evidence collection, browser islands, roadmap caveats, or
a readiness decision without scanning app content. Then call
`contract.recommended_execution_kits()` or
`contract.execution_kits_for_recommendation(&recommendation)` to hand workers
the matching executable kits directly, or `contract.recommended_execution_batch()`
when the queue needs the recommendation, selected kits, and selected-kit counts
as one serializable handoff. After workers fill evidence, call
`batch.complete_with_evidence_checklists(checklists)` and feed the returned
outcomes into `contract.with_pass_outcomes(...)`, or use
`contract.continue_with_execution_batch_evidence(&batch, checklists)` to receive
the refreshed contract and next recommended batch directly. Use
`continuation.should_continue()`, `has_next_batch()`, and `next_focus()` to
drive repeated worker loops without guessing from counts. Use
`continuation.remaining_work()` when the scheduler needs count-level reasons for
the next loop: blockers, native work, evidence, WebView islands, roadmap
caveats, failed/missing evidence, and readiness state. Use
`contract.run_execution_evidence_loop(evidence_batches, max_steps)` when the
agent has multiple evidence batches and needs a bounded result with step count,
explicit stop reason, step-limit status, final readiness state, and final
remaining work. Call `loop_result.final_report()` for the compact final action,
focus, continue/parity flags, next-kit count, and failed/missing evidence counts,
or `loop_result.final_handoff()` when the scheduler needs that report plus the
next recommended batch as one serializable handoff. The report exposes route
helpers for native work, browser islands, roadmap caveats, and readiness decisions.
`DesktopCapabilityIntakeWorkOrder::to_text()` reports action, pass presence,
current primitive, and evidence counts without logging app content. A worker can
call `work_order.complete_with_evidence(checklist)` to return an
`DesktopCapabilityGenerationPassOutcome`, then feed that outcome to
`contract.with_pass_outcomes(...)`.
the resulting matrix as the compact non-WebView audit surface:
`ready_native_rows()` shows primitives agents can generate now,
`non_webview_gap_rows()` keeps native/platform blockers like USB, serial,
capture, packaging, or plugin isolation visible, `webview_rows()` names the
browser-shaped compatibility islands, and `roadmap_rows()` names the caveats
that must be briefed. Then call `backlog_items()` or `current_backlog_item()` to
turn the matrix into a prioritized work queue: resolve native capability gaps
first, build ready native primitives, collect acceptance evidence, isolate
browser dependencies, and state roadmap caveats. Call `backlog_tickets()`,
`current_backlog_ticket()`, or `blocking_backlog_tickets()` when the agent needs
the concrete starter recipe, scaffold hint, acceptance criteria, and evidence
checklist attached to each ordered item. Call `matrix.native_first_plan()` when
the agent or UI needs that queue grouped into blockers, native primitive tickets,
evidence tickets, deferred WebView islands, and roadmap caveats; its
`phases()`, `current_phase()`, `current_phase_assignment()`,
`current_execution_kit()`, `current_execution_batch()`, `current_ticket()`, and `current_generation_pass()`
helpers keep the next handoff native-first, while
`DesktopNativeFirstWorkPhase::assignment()`, `execution_kits()`,
`execution_batch()`, and `evidence_checklists()` give a whole phase of worker
counts, concrete starter payloads, and proof work without flattening WebView
tickets ahead of native work. After workers return proof, call
`native_first.continue_with_execution_batch_evidence(&batch, checklists)` to get
the refreshed review, claim, and next native-first batch.
`should_defer_webview()` makes browser islands an explicit later phase instead
of the default route. Use
`matrix.ready_readiness_decision()` before evidence exists, or
`matrix.readiness_decision(checklists)` after verification, to get a
matrix-level `DesktopParityClaimStatus` that blocks on native generation gaps,
failed evidence, missing evidence, deferred WebView islands, and roadmap caveats
before an agent claims Desktop readiness. After running verification, call
`ticket.review_evidence(checklist)`; before verification, `ticket.ready_review()`
keeps the ticket blocked by missing evidence and reports the next backlog kind
without logging criterion text. Use `matrix.ready_backlog_review()` before a
run and `matrix.review_backlog_evidence(checklists)` after a run when the agent
needs cross-ticket counts for closable, open, blocking, failed, and missing
work; call `review.current_ticket()` and `review.current_evidence_checklist()`
to continue from the first open ticket without rescanning the ticket list. Use
`review.current_generation_pass()` when the agent needs a single focused pass
with helpers for blocker resolution, native generation, evidence collection,
WebView isolation, and roadmap briefing. The pass exposes `feature_gaps()`,
`native_starters()`, `setup_checks()`, `scaffold_hint()`,
`acceptance_criteria()`, `expected_evidence_checklist()`, and
`review_evidence(checklist)` so generated work can start and report results
without digging through nested ticket fields. Use
`pass.complete_with_evidence(checklist)` when a worker returns evidence; the
outcome reports whether the ticket can close and exposes the evidence checklist
to feed back into `matrix.review_backlog_evidence(...)`. Use `pass.brief()` as
the count-level worker assignment summary when logging, delegating, or showing
the next pass in UI. Use
`DesktopAppCategory::generation_brief(&report)` as the first summary shown for
category-driven generation; it combines capability readiness, primitive
blueprint counts, blockers, native/WebView/roadmap step counts, feature-gap
counts, and remediation counts without logging app content.
Use `DesktopAppCategory::generation_manifest(&report)` next when agents need a
machine-readable handoff: each entry stays attached to its Desktop primitive
and desktop surface area while exposing blocker feature details, native starter
APIs, setup checks, WebView island starters, and roadmap items. Manifest
summaries remain count-only for safe logging. Call `manifest.generation_queue()`
when the agent needs the ordered execution loop: blockers first, then native
generation, then explicit WebView islands, then roadmap declarations. Use
`queue.ready_native_starters()` and `queue.ready_setup_checks()` to give
builders concrete native APIs and setup checks for the current pass; use
`queue.queued_webview_islands()` and `queue.queued_roadmap_items()` to show
which browser-backed slices and missing platform work remain outside the native
pass. Use
`entry.scaffold_hint()` or `queue.ready_scaffold_hints()` to place generated
modules, state owners, view components, command hooks, and verification hooks
without inferring project structure from prose. Use
`entry.acceptance_criteria()` or `queue.ready_acceptance_criteria()` to verify
smoke, parity, and resource criteria before claiming native desktop capability
parity. Use `entry.acceptance_evidence_checklist()` or
`queue.ready_acceptance_evidence_checklists()` to record pass/fail/missing
evidence; inspect each item with `expected_evidence_artifact()` and each
checklist with `expected_evidence_artifacts()` so smoke criteria get runtime or
checked-API proof, readiness criteria get desktop-behavior comparison proof, and
resource criteria get snapshot, budget, or benchmark proof before status is set
to passed. Use `queue.ready_acceptance_evidence_report()` as the aggregate
readiness evidence for the current native work. Use
`queue.readiness_decision(&evidence)` or `queue.ready_readiness_decision()`
as the final allow/block decision before claiming readiness.
Use `DesktopAppCategory::generation_contract(&report)` when an agent needs the
single category handoff object containing the brief, manifest, queue,
capability matrix, native-first plan, ready evidence report, initial ready-work
claim decision, native-first claim decision, and recommended next step.
Call `contract.next_step()` or `contract.next_action()` to choose whether to
resolve blockers, generate ready native work, fix failed evidence, collect
remaining evidence, brief caveats, or mark readiness. After verification, call
`contract.with_ready_evidence(evidence)` or
`contract.with_ready_evidence_checklists(checklists)` to refresh the claim
decision and next-step recommendation without rebuilding the handoff. For the
native-first worker loop, call `contract.current_phase_assignment()`,
`contract.current_execution_kit()`, or `contract.current_execution_batch()`;
after workers return proof, call
`contract.continue_with_execution_batch_evidence(&batch, checklists)` to get a
refreshed category contract plus the next native-first batch.
Use `DesktopCapabilityPortfolioAudit::all(&report)` for broad framework
audits across every standard category. It rolls up category contracts,
next-action counts, blocked categories, ready native work, mark-ready scopes,
briefing needs, feature gaps, and missing evidence without collapsing the result
into a WebView-only answer. Use `portfolio.prioritized_entries()` for the
ordered category work queue and `portfolio.recommended_focus()` when an agent
needs one category/action to handle next. Use `portfolio.recommended_handoff()`
when the agent needs the focused contract plus the manifest entries, scaffold
hints, acceptance criteria, evidence report, and native-first execution batch
relevant to that action. Use `portfolio.recommended_execution_batch()` or
`portfolio.recommended_phase_assignment()` when a scheduler only needs the
recommended batch payload. After
verification, call `handoff.review_evidence(evidence)` or
`handoff.review_evidence_checklists(checklists)` to refresh the claim decision
and next action for the focused scope, or
`handoff.continue_with_execution_batch_evidence(&batch, checklists)` to refresh
the focused category contract and next native-first batch.

When a developer asks about a specific Desktop API instead of an app category,
start with the primitive bridge. This avoids treating every missing expectation
as a WebView problem:

```rust
let report = CapabilityReport::current();
let media = DesktopPrimitive::MediaPlaybackSurface.bridge(&report);
let hardware = DesktopPrimitive::HardwareDeviceApis.bridge(&report);
let all_primitives = DesktopPrimitiveBridgeAudit::all(&report);
let matrix = all_primitives.capability_matrix();
let blueprint = all_primitives.generation_blueprint();

tracing::info!(summary = media.to_text(), "desktop media primitive bridge");
tracing::info!(summary = hardware.to_text(), "desktop hardware primitive bridge");
tracing::info!(summary = all_primitives.to_text(), "desktop primitive audit");
tracing::info!(summary = matrix.to_text(), "desktop capability matrix");
tracing::info!(summary = blueprint.to_text(), "desktop generation blueprint");

if let Some(item) = matrix.current_backlog_item() {
    tracing::info!(summary = item.to_text(), "desktop primitive backlog item");
}
if let Some(ticket) = matrix.current_backlog_ticket() {
    tracing::info!(summary = ticket.to_text(), "desktop primitive backlog ticket");
    tracing::info!(
        summary = ticket.ready_review().to_text(),
        "desktop primitive ticket review"
    );
}

if !hardware.feature_checks_passed() {
    let gaps = hardware.feature_gaps();
    let remediation = hardware.remediation_summary();
    /* resolve native device support, request setup, or state the roadmap gap */
}

for row in matrix.non_webview_gap_rows() {
    tracing::info!(summary = row.to_text(), "non-webview capability gap");
    /* resolve native/platform support instead of defaulting to WebView */
}

for item in matrix.blocking_backlog_items() {
    tracing::info!(summary = item.to_text(), "blocking capability backlog item");
}

for ticket in matrix.blocking_backlog_tickets() {
    tracing::info!(summary = ticket.to_text(), "blocking capability backlog ticket");
    tracing::info!(summary = ticket.ready_review().to_text(), "blocking ticket review");
}
tracing::info!(
    summary = matrix.ready_backlog_review().to_text(),
    "desktop primitive backlog review"
);

if !blueprint.can_generate() {
    for step in blueprint.blocking_steps() {
        tracing::info!(summary = step.to_text(), "blocked desktop generation step");
    }
}

for step in blueprint.native_steps() {
    let recipe = step.starter_recipe();
    tracing::info!(summary = recipe.to_text(), "desktop primitive starter recipe");
    /* generate the native Kael surface first */
}

for step in blueprint.webview_steps() {
    /* isolate browser-shaped behavior into a small WebView island */
}

for step in blueprint.roadmap_steps() {
    /* state product or backend work that remains roadmap */
}
```

`DesktopPrimitive` covers the Kael desktop capability families builders usually name first:
window management, embedded hosted views, screen/display topology, media playback, menus/tray/dock/taskbar, files/dialogs,
message dialogs, filesystem/workspace access, app identity, clipboard/editing, input/IME/shortcuts, media capture,
notifications/shell, canvas/SVG/WebGL, hardware devices,
helper processes/plugins, updater/packaging, accessibility/automation,
performance diagnostics, developer tools/observability,
app storage/sessions, app lifecycle/single-instance, IPC/command messaging, secure storage/credentials, and navigation/history/routing. Each bridge maps the primitive to an owning
`DesktopSurfaceArea`, route, feature checks, feature gaps, native primitive
count, WebView condition count, roadmap count, and remediation summary without
logging URLs, local paths, device identifiers, extension IDs, plugin metadata,
or document contents. `DesktopCapabilityMatrix` is the compact inspection view
for agents that need to answer "what else besides WebView is missing?" before
generating: rows expose `can_start_native_work()`, `has_non_webview_gap()`,
`feature_gaps()`, route, area, native starter count, setup check count, WebView
condition count, roadmap count, and acceptance criterion count while keeping
summaries count-only. `DesktopCapabilityBacklogItem` is the actionable queue
item form of that matrix; use `backlog_items()`, `blocking_backlog_items()`, and
`backlog_for_kind(kind)` when an agent needs a stable order for native blockers,
native generation, evidence collection, browser-island isolation, and roadmap
briefing. Use `DesktopCapabilityBacklogTicket` through `backlog_tickets()` or
`current_backlog_ticket()` when the agent is ready to act: the ticket carries
the backlog item, starter recipe, scaffold hint, acceptance criteria, and
missing-by-default evidence checklist without logging app content. Use
`ticket.ready_review()` for the initial missing-evidence state and
`ticket.review_evidence(checklist)` after smoke, parity, and resource checks;
the review reports `next_kind`, failure/missing counts, and whether the ticket
can close. Use `DesktopCapabilityBacklogReview` through
`matrix.ready_backlog_review()` or `matrix.review_backlog_evidence(checklists)`
when the agent needs one summary of open tickets, closable tickets, blocking
tickets, and the next backlog kind across the matrix. Use
`review.current_ticket()`, `review.open_tickets()`, and
`review.closable_tickets()` for the concrete follow-up handoff after each run;
use `review.current_generation_pass()` for the one-pass command surface agents
should execute next. Its pass-scoped accessors expose the feature gaps, native
starters, setup checks, WebView islands, roadmap items, scaffold hint,
acceptance criteria, expected evidence checklist, and evidence-review helper for
that one pass. `DesktopCapabilityGenerationPassBrief` is the compact worker
handoff for that pass and reports counts and booleans only.
`DesktopCapabilityGenerationPassOutcome` is the return path after a worker
fills the pass evidence; it carries the reviewed ticket and the evidence to
refresh the matrix-level review.
`DesktopNativeFirstWorkPlan` is the grouped version of the same queue: call
`phases()` or `current_phase()` to assign blockers, native generation, evidence,
deferred WebView islands, and roadmap caveats as separate phases; call
`current_phase_assignment()` for a count-only scheduler payload and
`current_execution_kit()` / phase `execution_kits()` for concrete starter APIs,
scaffold hints, acceptance criteria, and evidence checklists; call
`current_execution_batch()` / phase `execution_batch()` when the scheduler needs
one serializable batch, then
`continue_with_execution_batch_evidence(&batch, checklists)` to refresh the
review, claim, and next batch; call
`ready_review()` / `review_evidence(checklists)` or
`ready_readiness_decision()` / `readiness_decision(checklists)` to refresh the
matrix-level claim from the ordered plan itself.
`DesktopGenerationBlueprint` is the ordered handoff for builders:
feature-gap blockers first, native surfaces next, narrow WebView islands after
that, and roadmap declarations last. Its `to_text()` and step summaries report
counts and route names only, so generated apps can plan without leaking app
content. Call `DesktopPrimitive::starter_recipe()` or
`DesktopGenerationStep::starter_recipe()` to choose concrete starter API
families such as checked window builders, `VideoPlaybackPlanBuilder`,
display topology queries, `kael_ui::VideoPlayer`, capture source/config builders, tray/menu/dock builders,
file dialog/intake/export builders, native navigation/history/tabs/breadcrumbs,
device access requests, plugin/runtime builders, accessibility tree audits, or benchmark/resource-budget APIs. Starter
recipe summaries report counts only; inspect their vectors when generating code,
but do not put user paths, URLs, device identifiers, extension IDs, captions, or
document text into logs.

When the question is broader than one recipe, audit the full desktop surface.
Use `DesktopSurfaceArea::all()` when tooling needs the complete inventory, and
`DesktopSurfaceAuditPlan::all(&CapabilityReport::current())` when the audit
must prove that no desktop area was skipped before work is assigned:

```rust
let surfaces = DesktopSurfaceArea::all();
let audit = DesktopSurfaceAuditPlan::all(&CapabilityReport::current());
let work_queue = audit.work_queue();

if audit.has_feature_gaps() {
    /* inspect areas_with_feature_gaps() and pick native fallback or roadmap copy */
}

assert_eq!(audit.area_count(), surfaces.len());
tracing::info!(summary = work_queue.to_text(), "desktop surface audit work queue");
if let Some(batch) = work_queue.current_batch() {
    tracing::info!(summary = batch.assignment.to_text(), "desktop surface audit assignment");
    tracing::info!(summary = batch.to_text(), "desktop surface audit execution batch");
    /* assign batch.handoff and batch.evidence_checklists to the next native-first worker */
    let continuation =
        audit.continue_with_execution_batch_evidence(&batch, batch.evidence_checklists.clone());
    tracing::info!(
        summary = continuation.to_text(),
        "desktop surface audit batch continuation"
    );
    let loop_result = audit.run_execution_batch_loop([batch.evidence_checklists], 4);
    tracing::info!(
        summary = loop_result.final_dossier().to_text(),
        "desktop surface audit batch loop dossier"
    );
}

if let Some(handoff) = audit.recommended_handoff() {
    tracing::info!(summary = handoff.to_text(), "desktop surface audit handoff");
    /* use handoff.native_primitives, feature_gap_details, webview_when, and roadmap_items */

    let evidence = handoff.evidence_checklists();
    let review = handoff.review_evidence_checklists(evidence);
    tracing::info!(summary = review.to_text(), "desktop surface audit evidence review");

    let continuation = audit.continue_with_handoff_evidence(&handoff, review.evidence);
    tracing::info!(summary = continuation.to_text(), "desktop surface audit continuation");

    let loop_result = audit.run_handoff_evidence_loop([handoff.evidence_checklists()], 4);
    tracing::info!(summary = loop_result.final_report().to_text(), "desktop surface audit loop");
    tracing::info!(
        summary = loop_result.readiness_decision().to_text(),
        "desktop surface audit readiness decision"
    );
    tracing::info!(
        summary = loop_result.final_loop_handoff().to_text(),
        "desktop surface audit loop handoff"
    );
    tracing::info!(
        summary = loop_result.final_dossier().to_text(),
        "desktop surface audit dossier"
    );
}

for area in audit.areas_with_feature_gaps() {
    let brief = area.implementation_brief();
    tracing::info!(summary = brief.to_text(), "desktop surface implementation brief");
    /* use brief.native_primitives, brief.webview_when, and brief.roadmap_items */
}
```

For app-specific planning, use
`DesktopSurfaceAuditPlan::for_category(category, &report)` after the full
inventory is understood. `DesktopSurfaceArea` covers app chrome, embedded view
composition, layout/styling/animation, windows, screen/display topology, media,
audio graph/recording, files, message dialogs, filesystem/workspace access,
image/icon assets, app identity, drag/drop, clipboard/editing,
menus/tray/dock/taskbar, input/IME/shortcuts, localization/text, forms,
notifications/shell, printing/protocols/paths, app storage/sessions, secure
storage/credentials, app lifecycle/single-instance, launch environment/config,
IPC/command messaging, security/permissions policy, background tasks/workers,
navigation/history/routing, find/zoom document tools, network/realtime
downloads, capture/permissions, power/theme/idle, graphics/canvas, visual
capture/snapshots, hardware devices, plugins/processes, packaging/updates,
accessibility/automation, performance diagnostics, crash reporting diagnostics,
developer tools/observability, WebView compatibility, and the low-level GPU
escape hatch. Use `audit.action_items()`, `audit.blocking_action_items()`,
`audit.work_queue()`, `audit.recommended_action_item()`, and
`audit.recommended_handoff()` when a broad framework audit needs serializable
worker assignments instead of just counts. `DesktopSurfaceAuditWorkQueue`
groups the prioritized handoffs into blockers, native/platform work, explicit
WebView islands, roadmap items, and evidence totals so schedulers can assign
non-WebView native work before caveat-only tasks. Use
`work_queue.current_assignment()` for the count-level worker focus and
`work_queue.current_batch()` when a runner needs the handoff plus evidence
checklists in one object. Use `work_queue.execution_batches()`,
`work_queue.blocking_batches()`, `work_queue.native_batches()`,
`work_queue.webview_batches()`, and `work_queue.roadmap_batches()` when a
scheduler should fan out parallel workers by lane without treating WebView as
the default route. Use `work_queue.lane_summaries()` and
`work_queue.recommended_lane_summary()` when dashboards or agent routers need
count-level batch, gap, starter, caveat, and evidence totals per lane before
allocating workers; use `work_queue.recommended_lane_batches()` to get the
executable batch set, `work_queue.recommended_lane_evidence_batches()` when a
runner needs one proof bundle per batch, and
`work_queue.recommended_lane_evidence_checklists()` to get the flattened
missing-by-default proof bundle without re-filtering the queue, or
`work_queue.recommended_lane_evidence_report()` when a worker needs the
aggregate pass/fail/missing proof summary for that lane. Use
`work_queue.recommended_lane_decision()` as the pre-dispatch gate when an
agent needs one object for lane focus, next action, readiness status, evidence
counts, and whether to dispatch work.
`DesktopSurfaceAuditExecutionBatch` can review
submitted evidence with `complete_with_evidence_checklists(...)`; call
`audit.continue_with_execution_batch_evidence(&batch, checklists)` when the
runner should receive the next assignment and next batch immediately after
evidence review. `DesktopSurfaceAuditWorkerPacket` exposes the same return
path with `packet.review_evidence_checklists(...)`,
`packet.ready_evidence_review()`, and
`audit.continue_with_worker_packet_evidence(&packet, checklists)` so an agent
can complete native, WebView-island, or roadmap work from the packet it was
assigned without reconstructing the original batch. Use
`audit.run_execution_batch_loop(evidence_batches, max_steps)` when the broad
worker should consume multiple queue-batch evidence
submissions and return the same final loop report, handoff, readiness decision,
and dossier surfaces as the lower-level handoff loop. Use
`audit.run_recommended_lane_evidence_loop(max_steps)` when an agent wants the
recommended lane's default proof bundles, final readiness decision, and
builder-facing dossier without manually passing grouped evidence batches, or
`audit.recommended_lane_handoff(max_steps)` when a scheduler needs one
serializable package containing the lane decision, batches, worker packets,
grouped evidence, aggregate evidence report, dry-run loop, and final dossier.
Use `batch.worker_packet()` or `recommended_handoff.worker_packets` when the worker needs
the implementation brief, concrete feature gaps, native primitives, WebView
caveats, roadmap items, primitive starter recipes, scaffold hints, quick-start
steps, setup checks, acceptance criteria, and evidence checklists without
traversing nested handoffs.
`DesktopSurfaceAuditHandoff` carries the action, area plan,
implementation brief, native primitive starters, feature-gap details, justified
WebView conditions, roadmap items, acceptance criteria, and evidence checklists.
Call `handoff.evidence_checklists()`, `handoff.evidence_report()`,
`handoff.review_evidence(...)`, or `handoff.review_evidence_checklists(...)`
after the worker run; `DesktopSurfaceAuditEvidenceReview` reports whether the
surface remains blocked, needs failed evidence fixed, needs missing evidence
collected, should brief caveats, or can close toward a readiness claim.
Call `audit.continue_with_handoff_evidence(...)` or
`audit.continue_with_handoff_evidence_checklists(...)` when the broad audit
itself should advance: `DesktopSurfaceAuditContinuation` returns the next
action, whether to continue, and the next handoff after a reviewed surface
closes or remains open. Use `audit.run_handoff_evidence_loop(evidence_batches,
max_steps)` when an agent has multiple broad-surface evidence batches and needs
a bounded worker loop; `DesktopSurfaceAuditLoop` and
`DesktopSurfaceAuditLoopReport` expose step count, stop reason, final action,
final handoff presence, target helpers, and continue/stop booleans. Use
`loop_result.readiness_decision()` before claiming broad desktop readiness; it
returns `Allowed`, `BlockedByGeneration`, `BlockedByFailedEvidence`,
`BlockedByMissingEvidence`, or `RequiresBriefing` for the full surface-audit
loop result. Use
`loop_result.final_loop_handoff()` when an agent runner needs the final report
paired with the next broad-surface handoff in one serializable object.
Use `loop_result.final_dossier()` for a builder-facing summary of the readiness
status, final action, next area/action, target flags, audited area count,
feature gaps, and evidence counts without exposing app payloads.
`to_text()` reports only counts and booleans. This keeps audits
from collapsing into "use WebView" when the missing area is actually multi-monitor placement,
USB/HID, serial/Bluetooth, browser history/location assumptions, capture consent, packaging, plugin isolation, or custom GPU
extensibility.
For hardware apps, the starter recipe now points directly at
`DeviceAccessRequest::{usb,hid,serial,bluetooth}`,
`DeviceAccessRequestBuilder`, `cx.device_access_request_checked(...)`,
`PermissionBroker`, matching `Capability::*` grants, platform feature gates, and
`request.privacy_permission()` so agents model WebUSB/WebHID/Web Serial/Web
Bluetooth requirements as native descriptors, permissions, and packaging
metadata before any vendor WebView island is considered.
For plugin-heavy and helper-process apps, the starter recipe points at
`PluginManifest`, `PluginPermissionManifest`, `HelperProcessLaunch`,
`HelperProcessLaunch::plugin_host(...)`, `ProcessSpawnOptionsBuilder`,
`PermissionBrokerInstallBuilder`, `ProcessContextBuilder`, `IpcSchema`,
`WorkerRequest`/`WorkerResponse`, extension request/response/handshake
messages, and `CrashPolicy`, so Desktop `helper process`, `utility process`, and
plugin-host requirements become validated native launches, brokered
capabilities, typed IPC, and supervised restart policy before a WebView island is
reserved for an existing browser-only plugin UI.
For packaging and updater flows, the starter recipe points at
`AppPackageManifestBuilder`, `AppPackageReadinessBuilder`,
`AppDistributionPlanBuilder`, `AppSigningPlanBuilder`,
`AutoUpdaterConfigBuilder`, `UpdateInfoBuilder::build_signed_checked()`,
`AppUpdateOfferPolicyBuilder`, `AppUpdateStateBuilder`, `DownloadExecutionPlan`,
`DownloadHandoffBuilder`, `DownloadHandoff`, and `RestartPathBuilder`, so Desktop-builder and `updater` requirements are
modeled as manifest/readiness/signing/feed/download/relaunch contracts before a
vendor release portal or platform backend is embedded.
For accessibility and automation flows, the starter recipe points at
`AccessibilityAttributes` recipes, `AccessibilityTree::audit_report()`,
`AccessibilityActionRouter`, `AccessibilityAnnouncementBuilder`,
`AccessibilityFocusBuilder`, semantic `MenuEntry`/`Link`/`TreeItem`/`Navigator`
summaries, and content-safe `to_text()` methods, so Desktop DOM-like role,
action, focus, live-region, and audit needs stay native unless a legacy web UI
must keep browser accessibility semantics.
For window-management work, the starter recipe now points at
`WindowIntentBuilder`, `WindowPlacementBuilder`, `WindowControls`,
`WindowChromeCommand`, `WindowPresentationPolicyBuilder`,
`WindowRuntimeSnapshotQueryBuilder`, `WindowInteractionCommand`,
`WindowZOrderPolicyBuilder`, `WindowOpacityBuilder`,
`WindowContentProtectionBuilder`, `SessionStore`, and `SessionSnapshotBuilder`,
so main, palette, utility, modal, popup, overlay, custom chrome, fullscreen,
kiosk, always-on-top, translucent, protected, and restored windows start from
native checked APIs before target-blank or browser-owned popup islands are
considered.
For native app chrome, the starter recipe points at
`AppChromeSurfaceHandoffBuilder`, `AppChromeSurfaceNextAction`,
`kael_ui::init`, `ThemeTokens`, `Navigator`, `Route`, `CommandPalette`,
`DataTable`, `DataGrid`, `Editor`, `Markdown`, and
`AccessibilityAttributes`, so app shell, data/document surfaces, command
surfaces, and accessibility checks start from one checked native workflow before
legacy web UI or third-party web component libraries become WebView islands.
For embedded hosted views, the starter recipe points at
`EmbeddedHostedViewHandoffBuilder`, `EmbeddedHostedViewNextAction`,
`EmbeddedHostedPaneProfile`, native panes, cached/deferred panes, floating
panes, explicit hosted pane profiles, pane-scoped controllers, and support
preflight, so OAuth, maps, payments, docs, browser graphics, and legacy web
widgets become isolated owned panes instead of untracked global browser state.
For layout, styling, and animation, the starter recipe points at
`LayoutStylingHandoffBuilder`, `LayoutStylingNextAction`,
`CssTokenMigrationBuilder`, `AnimationTimelineBuilder`,
`ResponsiveLayoutPlanBuilder`, styled `div()`, `ThemeTokens`, `Transition`,
`Navigator`, `Route`, `cached`, `deferred`, `effect_layer`, `LayerStack`,
native image/SVG/Lottie/canvas primitives, `UniformList`, `RecyclingList`, and
content-safe render evidence, so CSS-like layout, media-query breakpoint
planning, design-token migration, keyframe/timeline motion, route motion,
overlays, effects, and large lists start from a checked native workflow before
exact browser CSS or animation engines become WebView islands.
For menus, tray, dock, taskbar, jump lists, hotkeys, and progress, the starter
recipe points at `MenuBarBuilder`, `MenuBarPlan`, `MenuBuilder::standard_edit`,
`TrayAppBuilder`, `TrayIconBuilder`, `TrayMenuBuilder`, `TrayTooltipBuilder`,
`NativeContextMenuBuilder`, `DockBadgeBuilder`, `DockMenuBuilder`,
`DockMenuActionBuilder`, `JumpListBuilder`, `JumpListPlan`,
`GlobalHotkeyBuilder`, `GlobalHotkeyUnregistration`, and
`WindowProgressBuilder`, so app menus, Edit roles, tray apps, context menus,
dock/taskbar badges and menus, Windows taskbar tasks, global shortcuts, and
progress indicators start from native checked command surfaces before hosted
context-menu semantics are treated as a WebView island.
For performance and memory diagnostics, the starter recipe points at
`AppRuntimeSnapshotQueryBuilder`, `current_process_metrics()`,
`AppResourceBudgetBuilder`, `BenchmarkHarness`, `BenchmarkSampleApp`,
`BenchmarkSamplePair`, and
`BaselineComparisonReport::generate_with_sample_pairs(...)`, so memory, CPU,
startup, idle, and "lighter than Desktop" claims are blocked until budgets and
comparable Desktop/Kael evidence are present.
After the audit, call `DesktopSurfaceArea::implementation_brief()` or
`DesktopSurfaceAreaPlan::implementation_brief()` to turn a flagged area into
builder guidance: native primitives to try first, WebView conditions that are
actually justified, and roadmap items that should be stated honestly. The broad
briefs now name concrete starters such as `WindowChromeCommand`,
`VideoUrlPlaybackHandoff`, `TrayAppBuilder`, `HelperProcessLaunch::plugin_host`,
and `BaselineComparisonReport`, so agents can move from an area-level audit to
real native APIs without dropping back to WebView or stale placeholders. The
brief's `to_text()` reports counts only, so agents can log the plan without
leaking paths, URLs, project names, plugin metadata, device identifiers, or
generated copy.

For each recipe, write the implementation brief in this order:

1. Classify every required surface as native-ready, native-with-guardrails,
   WebView compatibility fallback, or roadmap.
2. Choose native `kael_ui` and platform APIs for app-owned chrome, data, files,
   windows, processes, permissions, and diagnostics.
3. Add WebView islands only for dependencies that are actually browser
   platform products: OAuth, payments, maps, hosted docs/editors, DRM players,
   WebGL/WebGPU engines, or third-party widgets.
4. Add `to_text()` or `to_safe_text()` traces around every generated builder so
   agents can inspect the plan without logging user content, paths, tokens, URLs,
   file names, captions, workspace ids, or arbitrary JSON.
5. State the remaining roadmap items explicitly instead of implying Desktop
   readiness where the native backend is not ready.

## Media is the first bridge to build

Desktop inherits the browser media element. A developer can create a video
player by setting a URL on a `<video>` element and controlling it with
JavaScript:

```js
video.src = url
await video.play()
video.currentTime = 42
video.playbackRate = 1.5
```

Kael currently has useful media primitives, but not this level of convenience.
`kael-media` can load `MediaSource::Url`, `File`, `Bytes`, and `Reader`, and
decode video through FFmpeg. The core `video(source)` element can display
frames. `VideoController` now provides the browser-shaped control layer:
metadata loading, play/pause/stop/seek, volume/mute, loop, audio playback-rate
control, ready-state changes, buffered ranges, WebVTT/SRT text tracks,
snapshots, and events.
`kael_ui::VideoPlayer::source(...)` now wires that controller into the existing
player chrome, renders selected text-track cues as captions, and renders the
core `video(source)` element behind the controls. `VideoPlayer::url(...)`
defaults to `VideoPlayerRoute::Auto`, so ordinary URL/file media uses the
native element while HLS/DASH-style manifest sources are routed through a
WebView-hosted browser `<video controls>` fallback. The built-in controls
expose loaded caption/subtitle tracks through a captions menu. The remaining
gap is moving decode, buffering, and rendering onto stronger media backends.
For generated player UIs and AI-agent audits, inspect
`VideoPlayer::to_text()`, `VideoPlayerState::to_text()`,
`VideoCaptionStyle::to_text()`, `AudioPlayer::to_text()`,
`AudioPlayerState::to_text()`, and `Waveform::to_text()` so traces report
source kind, route, size, controls/captions/poster/source/title presence,
progress/volume buckets, handler counts, and waveform shape without logging
media URLs, file paths, titles, caption text, exact seek times, volume/rate
values, waveform amplitudes, or colors.

For generated Web Audio-style flows, use one checked native handoff before
playback, recording, waveform UI, permissions, package metadata, and browser
fallbacks drift into separate partial implementations:

```rust
let audio = cx.audio_workflow_handoff_checked(
    AudioWorkflowHandoffBuilder::new()
        .playback_source(AudioPlaybackSourceBuilder::url("https://example.com/preview.mp3"))
        .permission_preflight(
            AudioPermissionPreflightBuilder::new()
                .microphone()
                .system_audio(),
        )
        .privacy_manifest(
            AppPrivacyManifestBuilder::new()
                .permission(AppPrivacyPermissionBuilder::microphone(
                    "Microphone access records narration.",
                ))
                .permission(AppPrivacyPermissionBuilder::screen_capture(
                    "Screen capture records system audio.",
                )),
        )
        .recording_capture(CaptureConfigSetBuilder::new().microphone().system_audio())
        .waveform_evidence(1, 2048),
)?;

match audio.next_action() {
    AudioWorkflowNextAction::CheckAudioCapabilities => {}
    AudioWorkflowNextAction::PreparePlayback => {}
    AudioWorkflowNextAction::PreflightRecordingPermissions => {}
    AudioWorkflowNextAction::AddPrivacyMetadata => {}
    AudioWorkflowNextAction::PrepareRecordingCapture => {}
    AudioWorkflowNextAction::RenderWaveformEvidence => {}
    AudioWorkflowNextAction::UseHostedAudioIsland => {}
    AudioWorkflowNextAction::TrackAudioGraphRoadmap => {}
}

tracing::info!(summary = audio.to_text(), "audio workflow");
```

`AudioWorkflowHandoffBuilder` validates playback source descriptors,
microphone/system-audio capture descriptors, permission preflight, privacy
manifest coverage, waveform evidence, hosted audio islands, and native audio
graph roadmap work together. It keeps arbitrary `AudioContext` graphs,
`AudioWorklet` processors, offline rendering, and sample-accurate scheduling as
explicit hosted islands or roadmap work until native graph APIs land. Summaries
avoid logging media URLs, file paths, reader keys, byte payloads, device
filters, permission reasons, waveform samples, capability notes, or roadmap
text.

The implemented control layer looks like this:

```rust
use kael::{
    can_play_video_type, recommended_video_playback_route,
    recommended_video_playback_route_for_type, webview, webview_video_player_url,
    video_capability_report, MediaSource, MediaSourceBuilder, VideoCanPlay, VideoController, VideoEvent,
    VideoElementHandoffBuilder, VideoPlaybackControlsBuilder, VideoPlaybackPlanBuilder,
    VideoPlaybackPlanTarget, VideoPlaybackRoute, VideoPlaylist, WebViewVideoOptions,
};
use std::time::Duration;

let capabilities = video_capability_report();
println!("hardware decode: {:?}", capabilities.hardware_decode);
tracing::info!(summary = capabilities.to_text(), "video capabilities");
if capabilities.has_native_gaps() && capabilities.has_webview_fallback() {
    tracing::info!(
        native_gaps = capabilities.native_gap_count(),
        roadmap = capabilities.roadmap_count(),
        "browser-video fallback covers current native media gaps"
    );
}

let handoff = cx.video_element_handoff_checked(
    VideoElementHandoffBuilder::url(video_url.clone())
        .initial_controls(VideoPlaybackControlsBuilder::new().volume(0.6).playback_rate(1.0))
        .playlist(VideoPlaylist::new([MediaSource::url(video_url.clone())])),
)?;
tracing::info!(summary = handoff.to_text(), "video element handoff");
let baseline = handoff.requirement_plan();
tracing::info!(summary = baseline.to_text(), "video element requirements");

let plan = VideoPlaybackPlanBuilder::url(video_url.clone())
    .content_type(content_type_header)
    .webview_options(WebViewVideoOptions::default().controls(true));
tracing::info!(summary = plan.to_text(), "video playback plan builder");
let plan = plan.build_checked()?;
tracing::info!(summary = plan.to_text(), "video playback plan");

match plan.target() {
    VideoPlaybackPlanTarget::Native => {
        let video = plan.controller();
        video.load_metadata()?;
        video.play()?;
    }
    VideoPlaybackPlanTarget::WebViewFallback { page_url, element_id, .. } => {
        return webview(element_id.clone(), page_url.clone()).size_full().into_any_element();
    }
}

let support = can_play_video_type("video/mp4; codecs=\"avc1.42E01E\"");
if support < VideoCanPlay::Maybe {
    // Pick a WebView island or ask the user for a different source.
}

if matches!(
    recommended_video_playback_route(&MediaSourceBuilder::url(video_url.clone()).build_checked()?),
    VideoPlaybackRoute::WebViewRecommended { .. }
) {
    let source = MediaSourceBuilder::url(video_url.clone()).build_checked()?;
    let options = WebViewVideoOptions::default()
        .autoplay(true)
        .muted(true)
        .checked()?;
    tracing::info!(summary = options.to_text(), "webview video options");
    let page = webview_video_player_url(
        &source,
        &options,
    )
    .expect("URL-backed media can be wrapped for WebView fallback");
    return webview("streaming-video", page).size_full().into_any_element();
}

let route_from_header = recommended_video_playback_route_for_type(content_type_header);
if route_from_header.should_use_webview() {
    // Extensionless HLS/DASH CDN URLs should follow the browser fallback too.
}

let video = MediaSourceBuilder::url(video_url)
    .controller_checked()?
    .volume(0.8)
    .muted(false)
    .playback_rate(1.0)
    .looping(false);

video.add_webvtt_text_track_checked("en", "English", Some("en"), webvtt_source)?;

video.select_text_track_checked("en")?;

video.load_metadata()?;
video.play()?;
let controls = VideoPlaybackControlsBuilder::new()
    .volume(0.7)
    .playback_rate(1.25)
    .fast_seek_secs(42.0);
tracing::info!(summary = controls.to_text(), "video controls");
video.apply_controls_checked(controls)?;
video.set_url_checked(next_video_url)?;

for event in video.drain_events() {
    match event {
        VideoEvent::LoadedMetadata {
            duration,
            width,
            height,
        } => {
            println!("loaded {duration:?} at {width}x{height}");
        }
        VideoEvent::TimeUpdate { current_time } => {
            println!("time {current_time:?}");
        }
        VideoEvent::Progress { buffered_ranges } => {
            println!("buffered: {buffered_ranges:?}");
        }
        VideoEvent::CanPlay => {
            println!("ready to play");
        }
        VideoEvent::Error(error) => eprintln!("{error}"),
        _ => {}
    }
}

println!("state: {:?}", video.playback_state());
println!("time: {:?}", video.current_time());
println!("time seconds: {:?}", video.current_time_secs());
println!("duration: {:?}", video.duration());
println!("duration seconds: {:?}", video.duration_secs());
println!("ready: {:?}", video.ready_state());
println!("buffered: {:?}", video.buffered_ranges());
println!("muted: {:?}", video.is_muted());
println!("rate: {:?}", video.rate());

let snapshot = video.snapshot();
println!("snapshot cues: {:?}", snapshot.active_text_cues);
```

For web-familiar naming, `current_time_secs()`, `set_current_time_secs(...)`,
`fast_seek_secs(...)`, `duration_secs()`, `paused()`, `set_position(...)`,
`muted_state()`, `rate()`, and `looping_enabled()` are aliases around the
canonical Rust getters.
Use `VideoPlaybackControlsBuilder` for generated controls, media-key handlers,
and AI-authored player chrome that should batch volume, muted, playback-rate,
looping, and seek changes. The checked path rejects empty updates,
NaN/infinite values, volume outside `0.0..=1.0`, playback rates outside
`0.0625..=16.0`, negative seek seconds, and extremely large seek positions
before mutating the controller; raw setters remain available for hand-validated
custom integrations.
Use `VideoPlaybackControlsBuilder::to_text()` for content-safe control traces
before applying generated batches; it reports configured fields without logging
exact positions, volume values, or rates.
For source replacement, prefer `set_source_checked(MediaSourceBuilder::...)`,
`set_url_checked(...)`, `set_file_checked(...)`, `set_bytes_checked(...)`, and
`set_reader_checked(...)` for generated runtime `src` swaps. Checked
replacement validates the new source before mutating the controller, then resets
media-derived state while preserving volume, mute, playback-rate, loop, and
text-track configuration. Raw `set_source(...)`, `set_url(...)`, `set_file(...)`,
`set_bytes(...)`, and `set_reader(...)` remain available for hand-validated
integrations. Use `MediaSourceBuilder::to_text()` to summarize source kind and
file checks without logging signed URLs, local paths, reader keys, or media
bytes.
For generated caption/subtitle controls, use `select_text_track_checked(id)` and
`disable_text_track_checked()` so unknown or malformed ids do not silently change
the active cues. Raw `select_text_track(...)` and `disable_text_track()` remain
available for permissive app-owned flows.
Use `TextTrackBuilder`, `add_text_track_checked(...)`,
`add_srt_text_track_checked(...)`, and `add_webvtt_text_track_checked(...)` for
generated caption/subtitle setup so empty metadata, empty parsed cue sets,
invalid cue ranges, and duplicate track ids fail before controller state changes.
Use `recommended_video_playback_route(...)`,
`recommended_video_playback_route_for_type(...)`, or
`VideoController::recommended_route()` before constructing advanced media UIs:
direct files/URLs default to native playback, while HLS (`.m3u8`) and DASH
(`.mpd`) manifests or adaptive streaming MIME types recommend an explicit
WebView island until native streaming backends land. The MIME helper is useful
for extensionless CDN URLs where the `Content-Type` header is the only reliable
signal.
Prefer `cx.video_element_handoff_checked(VideoElementHandoffBuilder::url(url))`
for generated URL players that want the closest Kael replacement for
`<video src="...">` plus custom controls: the handoff validates the URL, returns
a controller or fallback render instruction, carries optional initial controls
and playlist/media-key intent, and exposes `requirement_plan()`, `next_action()`,
`controller_checked()`, `media_key_binding_builder_checked()`, and `to_text()`
without logging the URL.
Use `VideoUrlPlaybackHandoff::url(url)?` when a smaller source-to-render handoff
is enough; it exposes `baseline_requirement_plan()`, `full_requirement_plan()`,
`baseline_next_action()`, and `baseline_ready()` without logging the URL.
Drop to `VideoPlaybackPlanBuilder` when a generator needs optional MIME type
routing or custom `WebViewVideoOptions`; it validates the source, optional MIME
type, and fallback options, then returns a single target (`Native` or
`WebViewFallback`) plus `can_play`, route, controller, and fallback page/id
accessors.
Before claiming a generated player is as tweakable as Electron's DOM `<video>`,
use `cx.video_element_customization_plan_checked(...)` with
`VideoElementCustomizationPlanBuilder::new(handoff).html_video_baseline()` plus
the requested controls, timeline, captions, fullscreen, PiP, hardware decode,
source-switching, and playlist/media-key features. The resulting
`VideoElementCustomizationPlan` separates satisfied customizations from
documented native limits, browser-only fallback needs, missing playlist/handler
wiring, and native backend work without logging URLs, file paths, MIME strings,
caption text, seek positions, volume/rate values, fallback reasons, or generated
data URLs.
Use `VideoPlaybackPlanBuilder::to_text()` before build and
`VideoPlaybackPlan::to_text()` before rendering generated players; they
summarize source kind, selected target, route, support confidence, content-type
presence, WebView preference, fallback track count, and start-position presence
without logging URLs, file paths, MIME strings, or inline captions. Use
`VideoPlaybackRenderInstruction::to_text()` after planning when dispatch logs
should distinguish native controller rendering from browser fallback without
logging generated data URLs or fallback reasons.
When a builder asks for a specific set of player capabilities, evaluate the
checked route before rendering:

```rust
let requirements = plan.requirement_plan([
    VideoPlaybackRequirement::BasicPlayback,
    VideoPlaybackRequirement::TextTracks,
    VideoPlaybackRequirement::AdaptiveStreaming,
    VideoPlaybackRequirement::PictureInPicture,
    VideoPlaybackRequirement::HardwareDecode,
]);

tracing::info!(summary = requirements.to_text(), "video requirements");
```

`VideoPlaybackRequirementPlan` is the honest audit layer for desktop-app
video expectations. It marks requirements as satisfied, limited, or missing for
the selected native/WebView route, so generated apps can decide whether to ship
native playback, route through browser `<video>`, or present a clear roadmap
gap for hardware decode, native adaptive streaming, or stream selection. Its
summary reports counts, target, and next action only; use exact requirement
getters when a test or setup screen intentionally needs the detailed checklist.
Use `next_action()` for a builder-safe handoff: render the planned route,
accept an explicit limitation, use a WebView fallback for browser-only media
affordances, or build the missing native backend. Use
`requires_webview_fallback()`, `webview_fallback_requirements()`,
`requires_native_backend_work()`, and `native_backend_work_requirements()` when
the generator needs separate browser-island and native-runtime work queues.
Use `can_play_video_type(...)`, `can_play_video_source(...)`, or
`VideoController::can_play_source()` when you need a browser-style support
confidence (`No`, `Maybe`, or `Probably`) before showing a native player.
Use `webview_video_player_url(...)` with `WebViewVideoOptions` to build a
browser `<video>` page for URL/file sources that should be routed through
WebView. The fallback accepts common HTML-video attributes such as poster,
preload, crossorigin, controlslist, disabled picture-in-picture, initial
current time, object-fit, and WebVTT `<track>` tags. The fallback page posts
browser media events back through the WebView bridge, and `VideoPlayer` maps
loaded metadata, readiness, progress, play/pause, time, seek, volume, rate,
text-track selection, active cue changes, browser fullscreen, picture-in-picture,
ended, and error messages into the same callback surface as native playback.
Use `WebViewVideoOptions::to_text()` for fallback-page summaries without
logging poster URLs, track URLs, or inline caption text.
Drive custom fallback chrome with
`VideoController::dispatch_webview_command_checked(window, WebViewVideoCommandBuilder::...)`:
commands cover play/pause/toggle/stop, exact and fast seek, volume, mute,
playback rate, loop, text-track selection/disablement, browser fullscreen,
picture-in-picture, and snapshot requests while rejecting invalid generated
volumes, playback rates, seek targets, and text-track selectors before running
JavaScript in the WebView. Use `WebViewVideoCommandBuilder::to_text()` and
`command_kind()` before dispatching browser fallback controls so generated
player chrome can log play, seek, audio, text-track, fullscreen,
picture-in-picture, or snapshot command classes without exposing exact seek
positions, volume/rate values, or text-track selectors.
Use `video_capability_report()` when an app or agent needs an honest feature
matrix: source types, controller events, source replacement, can-play checks,
route recommendation, WebView fallback, text tracks, fast seek, playback rate,
fullscreen, adaptive streaming, hardware decode, and native stream selection.
Use `full_count()`, `partial_count()`, `roadmap_count()`,
`native_gap_count()`, `has_native_gaps()`, and `has_webview_fallback()` to
gate generated players, release audits, and capability readiness claims without
parsing prose.

The high-level player API is:

```rust
use kael::{ObjectFit, WebViewVideoCommandBuilder, WebViewVideoCrossOrigin, WebViewVideoOptions};
use kael_ui::prelude::*;
use std::time::Duration;

VideoPlayer::url(video_url, cx)
    .object_fit(ObjectFit::Contain)
    .playback_route(VideoPlayerRoute::Auto)
    .content_type(content_type_header)
    .preload(VideoPreload::Metadata)
    .controls(true)
    .volume(0.8)
    .muted(false)
    .playback_rate(1.0)
    .looping(false)
    .start_at(Duration::from_secs(0))
    .poster(poster_url)
    .webview_options(
        WebViewVideoOptions::default()
            .preload(kael::WebViewVideoPreload::Metadata)
            .cross_origin(WebViewVideoCrossOrigin::Anonymous)
            .controls_list(["nodownload"])
            .disable_picture_in_picture(true),
    )
    .show_captions(true)
    .webvtt_text_track("en", "English", Some("en"), webvtt_source)
    .select_text_track("en")
    .caption_style(
        VideoCaptionStyle::default()
            .background(kael::black().opacity(0.82))
            .font_size(px(16.0)),
    )
    .on_loaded_metadata(|duration, width, height, _window, _cx| {
        println!("loaded {duration:?} at {width}x{height}");
    })
    .on_can_play(|_window, _cx| {
        println!("ready to play");
    })
    .on_progress(|buffered_ranges, _window, _cx| {
        println!("buffered: {buffered_ranges:?}");
    })
    .on_time_update(|current_time, _window, _cx| {
        println!("time {current_time:?}");
    })
    .on_seeked(|current_time, _window, _cx| {
        println!("seeked to {current_time:?}");
    })
    .on_rate_change(|rate, _window, _cx| {
        println!("rate {rate}x");
    })
    .on_cue_change(|cues, _window, _cx| {
        println!("active cues: {}", cues.len());
    })
    .on_error(|error, _window, _cx| eprintln!("{error}"))
```

Use `VideoPlayer::file(...)`, `VideoPlayer::bytes(...)`,
`VideoPlayer::reader(...)`, or `VideoPlayer::source(MediaSource::...)` when the
source is not a URL. Auto routing is the default; use `.native_playback()` to
force Kael's native element, `.webview_fallback()` to force a browser `<video>`
island for URL/file sources, or `.webview_options(...)` to tune the fallback
page. Use `.content_type(...)` when an extensionless URL is known to be HLS,
DASH, or another adaptive streaming type from a response header. The high-level
`.poster(...)`, `.preload(...)`, `.start_at(...)`, and `.webvtt_text_track(...)`
builders are mirrored into the fallback options.
When Auto selects WebView, `.on_loaded_metadata(...)`, `.on_can_play(...)`,
`.on_progress(...)`, `.on_playing(...)`, `.on_paused(...)`,
`.on_time_update(...)`, `.on_seeked(...)`, `.on_volume_changed(...)`,
`.on_rate_change(...)`, `.on_ended(...)`, `.on_error(...)`, and `.on_event(...)`
still receive browser video events.

When an app needs to drive the browser fallback directly, use the same WebView
command channel Kael exposes for other embedded web surfaces:

```rust
let controller = player.controller().expect("source-backed player");
controller.dispatch_webview_command_checked(
    window,
    WebViewVideoCommandBuilder::fast_seek(Duration::from_secs(90)),
)?;
controller.dispatch_webview_command_checked(window, WebViewVideoCommandBuilder::playback_rate(1.5))?;
```

Parsed tracks can still be configured directly when needed:

```rust
let player = VideoPlayer::url(video_url, cx)
    .text_track(custom_track)
    .select_text_track("en");
```

For source-backed players, `.on_play(...)`, `.on_pause(...)`, `.on_seek(...)`,
`.on_volume_change(...)`, `.on_playback_speed_change(...)`,
`.on_source_changed(...)`, `.on_loaded_metadata(...)`,
`.on_ready_state_change(...)`, `.on_can_play(...)`, `.on_can_play_through(...)`,
`.on_waiting(...)`, `.on_progress(...)`, `.on_playing(...)`, `.on_paused(...)`,
`.on_stopped(...)`, `.on_time_update(...)`, `.on_seeked(...)`,
`.on_volume_changed(...)`, `.on_rate_change(...)`, `.on_loop_change(...)`,
`.on_text_track_added(...)`, `.on_text_track_changed(...)`, `.on_cue_change(...)`,
`.on_ended(...)`, and `.on_error(...)` are additive user hooks. Source-backed configuration methods such as `.controls(...)`,
`.autoplay()`, `.preload(...)`, `.volume(...)`, `.muted(...)`,
`.playback_rate(...)`, `.looping(...)`, `.start_at(...)`, `.srt_text_track(...)`,
`.webvtt_text_track(...)`, `.select_text_track(...)`, and
`.disable_text_track(...)` configure the internal `VideoController` or built-in
chrome directly. `VideoPreload::Metadata` and `VideoPreload::Auto` load metadata
up front; `Auto` will grow into deeper buffering as the backend gains accurate
streaming ranges. The internal controller still receives user commands first, so
custom analytics or app-state callbacks do not accidentally disconnect playback.
Keyboard controls and visible chrome use the same callbacks: space toggles
play/pause, arrows seek or adjust volume, `m` toggles mute, and `f` toggles real
window fullscreen.

and advanced apps should be able to split the rendering and controls:

```rust
let video = VideoController::url(video_url);

div()
    .child(VideoView::new(video.clone()).object_fit(ObjectFit::Cover))
    .child(VideoControls::new(video).overlay())
```

### Required media capabilities

Ship these as the public media contract before claiming Desktop-like media:

- Source types: file path, URL, bytes, custom reader.
- Controls: play, pause, stop, seek, fast seek, rate, volume, mute, loop.
  Initial playback-rate support is implemented for audio output through the
  current software sink; pitch preservation and a stronger decoded-video clock
  remain backend work. `VideoController::fast_seek(...)` and
  `.fast_seek_secs(...)` are available for scrubbers; the current software
  backend uses the same stream-level seek path as exact seek until platform
  backends can prefer keyframe seeks.
- State: duration, current time, buffered ranges, ready state, dimensions,
  playback state, error state. Initial `VideoSnapshot::buffered_ranges` and
  `VideoReadyState` support is implemented; local file/bytes/reader sources
  report the full duration after metadata loads, while URL-backed streaming
  ranges remain unknown until native streaming backends can report them.
- Events: loaded metadata, can play, playing, pause, seeked, waiting, time
  update, source changed, ended, error. Initial `Progress`,
  `ReadyStateChange`, `CanPlay`, `CanPlayThrough`, and `Waiting` events are
  implemented.
- Rendering: object-fit, poster, placeholder, rounded clipping, overlays,
  real window fullscreen toggles, and fullscreen hooks. `VideoPlayer` uses
  `Window::toggle_fullscreen()` for its `f` keybinding and fullscreen button,
  then calls `.on_fullscreen(...)` with the platform-reported state.
- Audio/video sync: one controller owns the clock; UI reads state from it.
- Subtitles and tracks: WebVTT/SRT basics are implemented at the
  `VideoController` text-track layer, and `VideoPlayer::source(...)` renders
  selected text cues with customizable caption styling and built-in caption
  track selection. Native audio/video stream selection remains roadmap work.
- Streaming reality: progressive HTTP first; HLS/DASH either native-backed or
  automatically routed through WebView by `VideoPlayerRoute::Auto` until native
  support exists. `VideoPlaybackRoute` helpers flag HLS/DASH manifests and
  adaptive streaming MIME types as WebView-recommended. Initial `VideoCanPlay`
  helpers mirror
  `canPlayType`-style confidence for MIME types and sources, and
  `webview_video_player_url(...)` creates a WebView-hosted browser video page
  for URL/file fallbacks that posts media events back to `VideoPlayer`.
- Backend ladder: current FFmpeg software decode first, stream-level seek and
  prefetch, then platform hardware decode with low-copy or zero-copy textures.

### Media implementation slices

1. Add `VideoController`/`MediaController` as the stateful owner of playback.
   Initial `VideoController` is implemented.
2. Add `VideoHandle` commands and `VideoEvent` notifications. Initial command
   and event surface is implemented on `VideoController`, including
   source-change, ready-state, and buffered-range events.
3. Make `VideoPlayer::source(...)` create and wire a controller automatically.
   Initial wrapper is implemented.
4. Add source-backed `VideoPlayer` builder methods for common video-element
   attributes such as controls, autoplay, volume, muted, playback rate, loop,
   preload, SRT/WebVTT text tracks, selected captions, caption styling,
   object-fit, poster, and initial seek. Initial attribute-like configuration
   and built-in caption selection are implemented.
5. Keep `VideoPlayer::new(state)` for custom/legacy control overlays.
6. Move decode, buffering, and clock management off paint-time paths.
7. Add true seek on `VideoFrameStream` instead of restart-on-backward-position.
   Initial FFmpeg stream seek is implemented; exact-frame seeking still depends
   on decoding forward from the nearest keyframe.
8. Add platform surface backends:
   - macOS: AVFoundation/CoreVideo/Metal.
   - Windows: Media Foundation/D3D texture.
   - Linux: GStreamer/VAAPI/DMABUF where available.
9. Render selected text-track cues over `VideoPlayer::source(...)` with
   customizable caption styling. Implemented.

## WebView islands are a feature, not a failure

Some requirements are web-shaped: maps, hosted payments, rich third-party
editors, SSO flows, complex video streaming, embedded documentation,
browser-only graphics, and customer-provided web widgets. Kael should make
these first-class through `webview(id, url)`, `webview_with_options(...)`,
`WebViewOptions`, `webview_controller(id)`, JavaScript evaluation, injected
CSS/JS, message passing, and navigation handlers.

```rust
use kael::{
    webview_controller, webview_file_with_options, webview_html_with_options, webview_with_options,
    NavigationPolicy, WebViewBridgeMessage, WebViewDownloadPolicy, WebViewDragDropPolicy,
    WebViewNewWindowPolicy, WebViewOptions, WebViewPageLoadEvent,
};

let browser = webview_controller("checkout");
let downloads_dir = std::env::current_dir()?.join("downloads");
std::fs::create_dir_all(&downloads_dir)?;
let mut auth_headers = http_client::http::HeaderMap::new();
auth_headers.insert(
    http_client::http::header::AUTHORIZATION,
    http_client::http::HeaderValue::from_static("Bearer preview-token"),
);

div().child(
    webview_with_options(
        browser.id(),
        checkout_url,
        WebViewOptions::embedded_widget()
            .user_agent("MyApp/1.0")
            .devtools()
            .zoom_hotkeys()
            .media_autoplay()
            .focused()
            .clipboard_access()
            .transparent_background()
            .request_headers(auth_headers.clone())
            .general_autofill_enabled(false)
            .bridge_script()
            .on_bridge_message({
                let browser = browser.clone();
                move |message, window, _cx| {
                    if message.is_kind("pick-video") {
                        browser.respond_to_bridge_message(
                            window,
                            &message,
                            serde_json::json!({ "path": "/movies/trailer.mp4" }),
                        )
                        .ok();
                    }
                }
            })
            .on_bridge_message(|message, _window, _cx| {
                if message.is_kind("checkout-complete") {
                    println!("checkout payload: {}", message.payload);
                }
            })
            .on_navigate(|url, _window, _cx| {
                if url.starts_with("https://trusted.example") {
                    NavigationPolicy::Allow
                } else {
                    NavigationPolicy::Deny
                }
            })
            .on_new_window(|url, _window, _cx| {
                if url.starts_with("https://trusted.example") {
                    WebViewNewWindowPolicy::NavigateCurrent
                } else {
                    WebViewNewWindowPolicy::Deny
                }
            })
            .on_download_started({
                let downloads_dir = downloads_dir.clone();
                move |url, suggested_path, _window, _cx| {
                    if url.starts_with("https://trusted.example/reports/") {
                        let filename = suggested_path
                            .and_then(|path| path.file_name().map(|name| name.to_owned()))
                            .unwrap_or_else(|| "download.bin".into());
                        WebViewDownloadPolicy::SaveTo(downloads_dir.join(filename))
                    } else {
                        WebViewDownloadPolicy::Deny
                    }
                }
            })
            .on_download_completed(|download, _window, _cx| {
                println!(
                    "download {}: {:?}",
                    if download.success { "complete" } else { "failed" },
                    download.path
                );
            })
            .on_drag_drop(|event, _window, _cx| {
                println!("webview file drag/drop event: {event:?}");
                WebViewDragDropPolicy::AllowBrowserDefault
            })
            .on_document_title_changed(|title, window, _cx| {
                window
                    .set_window_title_checked(WindowTitleBuilder::new(format!(
                        "Checkout - {title}"
                    )))
                    .expect("validated checkout title");
            })
            .on_page_load(|event, url, _window, _cx| {
                match event {
                    WebViewPageLoadEvent::Started => println!("loading {url}"),
                    WebViewPageLoadEvent::Finished => println!("loaded {url}"),
                }
            }),
    )
    .size_full(),
);

browser.navigate_with_headers(window, checkout_url, auth_headers)?;
browser.post_bridge_message(window, WebViewBridgeMessage::new("host-ready"))?;
browser.open_devtools(window)?;
browser.is_devtools_open(window, |result| {
    println!("devtools open: {result:?}");
})?;
browser.set_zoom_factor(window, 1.1)?;
browser.focus(window)?;
browser.evaluate_javascript(window, "window.dispatchEvent(new Event('host-ready'))")?;
browser.evaluate_javascript_with_result(window, "document.title", |result| {
    println!("document title result: {result:?}");
})?;
browser.url(window, |result| {
    println!("current WebView URL: {result:?}");
})?;
browser.reload(window)?;
browser.print(window)?;
browser.clear_browsing_data(window)?; // Logout, account switching, or test cleanup.
```

For app-rendered documents, use the native print request path instead of
round-tripping through HTML:

```rust
let job = PrintJob::letter("Invoice", |ctx, cx| {
    ctx.draw_text(
        "Invoice #1042",
        point(px(72.0), px(72.0)),
        PrintTextStyle::default(),
    );
    tracing::info!(summary = ctx.to_text(), "print page");
});

let request = PrintRequest::dialog(job);
tracing::info!(summary = request.to_text(), "print request");
window.print_checked(request, cx)?;

let output = cx.document_output_handoff_checked(
    DocumentOutputHandoffBuilder::save_webview_page(
        "invoice-preview",
        DocumentExportFormat::HtmlComplete,
        "/tmp/invoice-preview.html",
    ),
)?;
tracing::info!(summary = output.to_text(), "document output handoff");
assert_eq!(output.next_action(), DocumentOutputNextAction::SaveHostedPage);
```

`PrintRequest::dialog(job)` is the safe default because it shows the platform
print UI. Use `PrintRequest::silent(job)` only for deliberate direct printer
dispatch, and `PrintRequest::webview(id)` when an existing WebView-hosted
document should follow Desktop `hosted document print(...)` behavior. The checked
path validates native print titles, pages, page sizes, margins, drawing
commands, and WebView ids before dispatch. Use `request.to_text()` for
document-safe logs, tests, and AI-agent summaries before print UI or silent
printer dispatch. Use `PrintContext::to_text()` plus `command_count()`,
`fill_count()`, `stroke_count()`, `text_count()`, `image_count()`, and
`is_empty()` inside native render callbacks to confirm generated invoices,
labels, reports, and proofs contain the expected categories of content without
logging document text, image bytes, or drawing coordinates.
For builder and agent handoff loops, wrap `PrintRequest` or
`DocumentExportRequest` in `DocumentOutputHandoffBuilder`, pass it through
`cx.document_output_handoff_checked(...)`, and inspect
`DocumentOutputNextAction` before dispatch. The checked handoff separates native
print, hosted print, native PDF export, hosted PDF export, and hosted save-page
work without logging document titles, generated bytes, WebView ids, output
paths, selectors, or URLs.

For a local HTML file, use `webview_file(...)` /
`webview_file_with_options(...)`, the native analogue of Desktop's `loadFile`:

```rust
webview_file_with_options(
    "local-docs",
    "assets/docs/index.html",
    WebViewOptions::embedded_widget()
        .bridge_script()
        .allow_navigation_schemes(["file", "data", "https"]),
)?
.size_full();
```

For controlled browser islands that do not need a separate server or asset file,
use `webview_html(...)` / `webview_html_with_options(...)`, the native analogue
of Desktop's `loadHTML`. These load the HTML string directly into the native
WebView; use `webview_html_url(...)` only when you explicitly need a data URL
string for another API:

```rust
let preview = webview_controller("preview");

webview_html_with_options(
    preview.id(),
    r#"<!doctype html>
<button onclick="window.kael.post('clicked')">Click</button>"#,
    WebViewOptions::embedded_widget()
        .bridge_script()
        .on_bridge_message(|message, _window, _cx| {
            if message.is_kind("clicked") {
                println!("inline widget clicked");
            }
        }),
)
.size_full();
```

Use `WebViewController::load_html(window, html)` when an existing browser island
should replace its document at runtime, such as live previews, generated
reports, template editors, or local documentation panes.

When you control the page, inject `.bridge_script()` and use
`window.kael.post(kind, payload, id)` for fire-and-forget messages or
`await window.kael.invoke(kind, payload)` for request/response calls. On the
Rust side, `WebViewBridgeMessage { kind, id, payload }`,
`.on_bridge_message(...)`, `WebViewController::post_bridge_message(...)`,
`WebViewController::respond_to_bridge_message(...)`, and
`WebViewController::reject_bridge_message(...)` give builders the same
message-envelope habit they expect from Desktop IPC. Raw `.on_message(...)` /
`.post_message(...)` remain available for custom protocols; lower-level
`WebViewBridgeMessage::response_to(...)` and `.error_to(...)` remain available
when a custom router owns the controller.

When the native app needs desktop-app `hosted script evaluation(...)`,
use `WebViewController::evaluate_javascript_with_result(window, script,
callback)`. Linux/Windows Wry-backed WebViews return the backend's JSON string
serialization of the JavaScript result through `Result<SharedString,
SharedString>`, so app code can decide whether to parse JSON or keep the raw
browser value. The existing `evaluate_javascript(...)` remains available for
fire-and-forget commands. Kael's custom macOS WebView backend uses WKWebView's
JavaScript completion handler and returns a JSON string serialization as well.

For desktop-app `hosted page controller.getURL()`, use
`WebViewController::url(window, callback)`. Linux/Windows Wry-backed WebViews
return Wry's current URL, and Kael's custom macOS WebView backend reads
`WKWebView.URL.absoluteString`. Before a macOS WebView has committed a page,
Kael falls back to the URL last declared through Kael navigation/load APIs.

During development, request inspector support with `.devtools()`. On
Linux/Windows debug/devtools builds, `WebViewController::open_devtools(window)`,
`.close_devtools(window)`, and `.is_devtools_open(window, callback)` map to
Wry-backed WebView devtools controls. On macOS, `.devtools()` marks the
underlying `WKWebView` as inspectable so it can appear in Safari/Web Inspector,
and `open_devtools(window)` ensures inspectability is enabled. WKWebView does
not expose public APIs to programmatically open, close, or report the inspector
window state, so custom devtool chrome should treat those runtime controls as
Linux/Windows-only for now.

For app-owned diagnostics, hosted-widget health checks, automated tests, and
AI-agent observability, use
`.on_console_message("console:message", |event, window, cx| { ... })`. It
preserves normal browser console behavior while forwarding typed
`WebViewConsoleEvent { level, message, args, source, line, column }` values for
`console.debug/log/info/warn/error`, uncaught errors, and unhandled promise
rejections. Use `.console_bridge("console:message")` with
`.on_bridge_message(...)` plus
`WebViewConsoleEvent::from_bridge_message(&message, "console:message")` when a
custom router owns multiple bridge message kinds.

For hosted documents, maps, editors, and dashboards that should own browser
zoom shortcuts, request backend zoom handling with `.zoom_hotkeys()` or
`.zoom_hotkeys_enabled(true)`. This maps to Wry's browser zoom hotkey/gesture
setting; Windows/WebView2 honors it today, and Kael's custom macOS WebView
backend handles standard `Command` + `+`, `Command` + `-`, and `Command` + `0`
keyboard zoom plus trackpad magnification gestures through WKWebView
`pageZoom`. Linux does not expose this behavior through the backend API yet. Use
`WebViewController::set_zoom_factor(window, factor)` when the native app should
drive zoom through its own controls instead; Linux/Windows Wry-backed WebViews
and Kael's custom macOS WKWebView backend honor that runtime command.

For native chrome that needs to observe hosted editor shortcuts or browser
before-input activity, use
`.on_keyboard_event("keyboard:event", |event, window, cx| { ... })`. It
forwards typed `WebViewKeyboardEvent` values for `keydown`, `keyup`, and
`beforeinput` with key/code, modifiers, repeat/composition state, editable-target
state, input type, data, and `defaultPrevented`. This is a portable WebView
island bridge for desktop-app `before-input-event` diagnostics and shortcut
coordination. Commands that must cancel input before a page sees it should still
use native Kael shortcut/keymap handling around the WebView boundary.

For native tabs, breadcrumbs, app-owned Back/Forward controls, and agents that
need hosted SPA route awareness, use
`.on_location_changed("location:changed", |event, window, cx| { ... })`. It
injects the standard bridge and forwards typed
`WebViewLocationEvent { url, title, ready_state, can_go_back, can_go_forward }`
values on `pushState`, `replaceState`, `popstate`, `hashchange`, `pageshow`, DOM
ready, load, and title-related DOM mutations. Use
`.location_bridge("location:changed")` with `.on_bridge_message(...)` plus
`WebViewLocationEvent::from_bridge_message(&message, "location:changed")` when a
custom router owns multiple bridge message kinds. Pair it with
`.navigation_state_bridge()` when native Forward state should be accurate for
app-owned same-document routes; otherwise `can_go_forward` stays conservative.

For resource throttling, active-tab chrome, hosted-player pause/resume, and
automation that needs to know whether browser content is active, use
`.on_lifecycle_event("lifecycle:event", |event, window, cx| { ... })`. It
forwards typed
`WebViewLifecycleEvent { event, visibility_state, hidden, has_focus, fullscreen, persisted }`
values for `focus`, `blur`, `visibilitychange`, `pageshow`, `pagehide`, and
browser fullscreen changes. Use `.lifecycle_bridge("lifecycle:event")` with
`.on_bridge_message(...)` plus
`WebViewLifecycleEvent::from_bridge_message(&message, "lifecycle:event")` when a
custom router owns multiple bridge message kinds. This is the portable
browser-side companion to native window focus and visibility handling; app code
should still use native Kael window lifecycle hooks for whole-window state.

For hosted documents, readers, dashboards, and editor panes whose scroll
position should drive native chrome or automation, use
`.on_scroll_event("scroll:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewScrollEvent { event, x, y, max_x, max_y, viewport_width, viewport_height, scroll_width, scroll_height, progress_x, progress_y }`
values for initial, scroll, and viewport resize snapshots. Use
`.scroll_bridge("scroll:event")` with `.on_bridge_message(...)` plus
`WebViewScrollEvent::from_bridge_message(&message, "scroll:event")` when a
custom router owns multiple bridge message kinds. The script throttles updates
with `requestAnimationFrame`, so native progress bars, hiding toolbars, and
AI-agent viewport checks can observe browser scroll state without polling.

For hosted rich editors, documents, and preview panes whose selection should
drive native edit menus or floating formatting chrome, use
`.on_selection_event("selection:event", |event, window, cx| { ... })`. It
forwards typed
`WebViewSelectionEvent { event, selected_text, selected_html, collapsed, editable, input_kind }`
values for initial, document `selectionchange`, input `select`, key/mouse/touch
selection updates, and focus/blur snapshots. Use
`.selection_bridge("selection:event")` with `.on_bridge_message(...)` plus
`WebViewSelectionEvent::from_bridge_message(&message, "selection:event")` when a
custom router owns multiple bridge message kinds. This is the event-driven
companion to `selected_text(...)` and `selected_html(...)`: native Edit,
Format, Copy/Cut/Paste, and AI-agent inspection flows can react as the browser
selection changes.

For browser-media islands, demos, and hosted players that should start media
without a user gesture, request the browser autoplay policy explicitly with
`.media_autoplay()` or `.media_autoplay_enabled(true)`. Use
`.media_autoplay_enabled(false)` when an embedded third-party page should keep a
stricter gesture requirement. Linux/Windows Wry-backed WebViews and Kael's
custom macOS WKWebView backend honor this construction option today.

For custom native controls around WebView-hosted `<audio>` or `<video>`, prefer
`.on_media_event("media:event", |event, window, cx| { ... })`. It injects the
bridge, observes current and future media elements, and passes typed
`WebViewMediaEvent { event, state }` values for play/pause/seek/time/volume/
rate/metadata/buffering/error events. Use `.media_event_bridge("media:event")`
with `.on_bridge_message(...)` plus
`WebViewMediaEvent::from_bridge_message(&message, "media:event")` when you need
to share one bridge handler across several message kinds. Use
`webview_media_event_bridge_script(kind)` directly only when composing a custom
injection bundle.

For native right-click / secondary-click menus over WebView content, prefer
`.on_context_menu("context:menu", |event, window, cx| { ... })`. It injects the
standard bridge, prevents the browser default context menu, and passes typed
`WebViewContextMenuEvent` values with viewport coordinates, selected text,
nearest link href, image source, media source, editable state, and input kind.
Use `.context_menu_bridge("context:menu")` with `.on_bridge_message(...)` plus
`WebViewContextMenuEvent::from_bridge_message(&message, "context:menu")` when
one bridge handler needs to route multiple message kinds. This is the
desktop-app path for app-owned context menus around hosted editors,
documents, media previews, and browser widgets: collect page context from the
WebView, then call the native context-menu builder from the handler.

For hover status bars, link previews, click telemetry, and AI-agent pointer
inspection over hosted content, use
`.on_pointer_event("pointer:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewPointerEvent { event, x, y, buttons, pointer_type, target_tag, link_href, image_src, media_src, editable, input_kind }`
values for pointer movement, pointer down/up, click, double-click, and pointer
leave. Use `.pointer_bridge("pointer:event")` with `.on_bridge_message(...)`
plus `WebViewPointerEvent::from_bridge_message(&message, "pointer:event")` when
a custom router owns multiple bridge message kinds. This is the lightweight
hover/click companion to `.on_context_menu(...)`; it does not prevent browser
defaults.

For hosted auth, checkout, settings, admin, and browser-widget forms, use
`.on_form_event("form:event", |event, window, cx| { ... })`. It forwards typed
`WebViewFormEvent { event, form_id, form_name, action, method, target, enctype, field, fields, default_prevented }`
values for submit, reset, change, and input events. Field snapshots include
name, id, tag, input kind, non-sensitive value, checked state, disabled state,
and required state; password and file input values are intentionally omitted.
Use `.form_bridge("form:event")` with `.on_bridge_message(...)` plus
`WebViewFormEvent::from_bridge_message(&message, "form:event")` when a custom
router owns multiple bridge kinds. This gives native validation, progress
chrome, tests, and AI agents a structured form surface without bespoke page
JavaScript, and it does not prevent browser defaults.

For app-owned forms, keep the surface native and describe the handoff before
rendering or mutating state:

```rust
let schema = cx.native_form_schema_checked(
    NativeFormSchemaBuilder::new("signup")
        .field(
            FormFieldDescriptorBuilder::new("email", FormFieldKind::Email)
                .label("Email address")
                .required(),
        )
        .step("account", ["email"])
        .dirty_state_tracking()
        .disable_submit_until_valid()
        .autofill_enabled(false),
)?;

let handoff = cx.form_validation_handoff_checked(
    schema
        .validation_handoff_builder()
        .text_checking(TextCheckingRequestBuilder::new(editor_text).check_grammar())
        .submit("signup")
        .reset("signup")
        .hosted_form_bridge("checkout"),
)?;
```

`NativeFormSchemaBuilder` covers generated app-owned form schemas, wizard steps,
dirty-state policy, submit gating, and autofill intent before UI renders.
`FormValidationHandoffBuilder` covers native field descriptors, text-checking,
submit/reset, autofill policy, and explicit hosted-form fallback. Its checked
handoff validates ids, labels, required/disabled state, ranges, patterns,
choice counts, text-checking policy, and WebView bridge scope while `to_text()`
summarizes request kinds without field values, credentials, filenames, or form
payloads.

For lower-level flows, build the validation handoff directly:

```rust
let handoff = cx.form_validation_handoff_checked(
    FormValidationHandoffBuilder::new()
        .field(
            FormFieldDescriptorBuilder::new("email", FormFieldKind::Email)
                .label("Email address")
                .required(),
        )
        .text_checking(TextCheckingRequestBuilder::new(editor_text).check_grammar())
        .submit("signup")
        .reset("signup")
        .autofill_policy("signup", false)
        .hosted_form_bridge("checkout"),
)?;
```

For hosted upload flows that use `<input type="file">`, add
`.on_file_input_event("file:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewFileInputEvent { event, input_name, input_id, accept, multiple, form_id, form_name, action, method, files }`
values when file inputs emit `change` or `input`. Each file entry includes the
browser-exposed file name, size, MIME type, and last-modified timestamp. Local
paths are not exposed by browsers, so this bridge is for native upload chrome,
validation, tests, and AI-agent observability rather than path access. Use
`.file_input_bridge("file:event")` with `.on_bridge_message(...)` plus
`WebViewFileInputEvent::from_bridge_message(&message, "file:event")` when a
custom router owns multiple bridge kinds.

For native diagnostics, loading UI, tests, and AI-agent observability around
hosted subresources, use
`.on_resource_event("resource:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewResourceEvent { event, url, initiator_type, target_tag, success, start_time, duration, transfer_size, encoded_body_size, decoded_body_size, next_hop_protocol, render_blocking_status }`
values from browser `PerformanceResourceTiming` entries plus captured element
`load` / `error` events. Use `.resource_bridge("resource:event")` with
`.on_bridge_message(...)` plus
`WebViewResourceEvent::from_bridge_message(&message, "resource:event")` when a
custom router owns multiple bridge kinds. This is resource observability, not
request interception: use it to see what loaded or failed, while main-frame
headers/navigation remain controlled by `.request_headers(...)`,
`navigate_with_headers(...)`, and `.on_navigate(...)`.

For hosted apps that call `fetch(...)` or `XMLHttpRequest`, use
`.on_network_event("network:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewNetworkEvent { event, api, method, url, status, status_text, ok, duration_ms, error_name, error_message, response_type, document_url }`
values for fetch completion/rejection and XHR load/error/abort/timeout. Use
`.network_bridge("network:event")` with `.on_bridge_message(...)` plus
`WebViewNetworkEvent::from_bridge_message(&message, "network:event")` when a
custom router owns multiple bridge kinds. This is JavaScript network API
observability, not request interception: it cannot rewrite headers, block
requests, inspect bodies, or observe browser requests that do not go through
page `fetch`/XHR.

For hosted pages that may call `alert`, `confirm`, `prompt`, or register
`beforeunload` prompts, use
`.on_dialog_event("dialog:event", |event, window, cx| { ... })`. It forwards
typed
`WebViewDialogEvent { event, message, default_value, result, url, default_prevented }`
values after the browser produces its normal synchronous dialog result. Use
`.dialog_bridge("dialog:event")` with `.on_bridge_message(...)` plus
`WebViewDialogEvent::from_bridge_message(&message, "dialog:event")` when a
custom router owns multiple bridge kinds. This is dialog observability, not a
replacement for the browser's synchronous `confirm()` / `prompt()` return path.

For auth, checkout, editor, and browser-widget islands that should take
keyboard focus immediately, request initial focus with `.focused()` or
`.focused_enabled(true)`. Use `WebViewController::focus(window)` and
`.focus_parent(window)` to move focus across the native/WebView boundary after
modals, route changes, completed auth, or embedded-editor handoff. Wry-backed
Linux/Windows WebViews and Kael's custom macOS WKWebView backend honor these
focus commands today.

For rich hosted editors, document widgets, and browser pages that call
`navigator.clipboard` or `document.execCommand("copy")`, request JavaScript
clipboard access with `.clipboard_access()` or
`.clipboard_access_enabled(true)`. Wry-backed Linux/Windows WebViews honor this
construction option today. Kael's custom macOS WebView backend injects an
opt-in native bridge for `navigator.clipboard.readText()` and
`navigator.clipboard.writeText(...)` backed by `NSPasteboard`, supports the
`text/plain` subset of `navigator.clipboard.read()` / `write(...)`, and maps
legacy `document.execCommand("copy")` / `"cut"` text selection calls onto the
same bridge. Broader clipboard item MIME types remain controlled by WebKit,
macOS permissions, and app menu accelerators.

When native chrome, tests, or AI agents need to observe clipboard activity
inside hosted editors, add
`.on_clipboard_event("clipboard:event", |event, window, cx| { ... })`. It
forwards typed
`WebViewClipboardEvent { event, types, text, html, target_editable, url, default_prevented }`
values for browser `copy`, `cut`, and `paste` events when the browser exposes
clipboard data to the page event. Use `.clipboard_event_bridge("clipboard:event")`
with `.on_bridge_message(...)` plus
`WebViewClipboardEvent::from_bridge_message(&message, "clipboard:event")` when
a custom router owns multiple bridge kinds. This bridge does not prevent
browser defaults and should be treated as explicit opt-in because paste payloads
can contain user data.

For hosted calls, screen-share flows, maps, local-device widgets, and other
pages that may request camera, microphone, display capture, geolocation, or
notification access, add
`.on_permission_request("permission:request", |request, window, cx| { ... })`.
It forwards typed
`WebViewPermissionRequest { permission, permissions, api, url, origin, user_gesture, details }`
values before wrapped browser APIs continue. Return
`WebViewPermissionDecision::Deny` to block the page call before it reaches the
browser, or `Allow` / `Default` to continue to the embedded browser's native
permission flow. Use `.permission_bridge("permission:request")` with
`.on_bridge_message(...)` plus
`WebViewPermissionRequest::from_bridge_message(&message, "permission:request")`
when a custom router owns multiple bridge kinds. This is an app policy
preflight for WebView islands; the browser engine and operating system remain
the final authority for native prompts.

For hosted auth, settings, carts, drafts, and embedded widgets that use Web
Storage, add `.on_storage_event("storage:event", |event, window, cx| { ... })`.
It forwards typed
`WebViewStorageEvent { event, area, key, old_value, new_value, length, url, local }`
values when hosted content mutates `localStorage` or `sessionStorage`, and when
the browser emits cross-document `storage` events. Use
`.storage_bridge("storage:event")` with `.on_bridge_message(...)` plus
`WebViewStorageEvent::from_bridge_message(&message, "storage:event")` when a
custom router owns multiple bridge kinds. For on-demand inspection and setup,
use `WebViewController::storage_snapshot(window, callback)`,
`.set_storage_item(window, WebViewStorageArea::Local, key, value, callback)`,
`.remove_storage_item(window, WebViewStorageArea::Session, key, callback)`, and
`.clear_storage_area(window, area, callback)`. Snapshot callbacks report
readable entries plus per-area `available` / `error` fields, and mutation
callbacks report `WebViewStorageMutationResult { ok, area, key, length, error }`.
These helpers are storage observability and current-document mutation, not a
replacement for the browser storage engine; use controller
`clear_browsing_data(window)` for profile cleanup.

For native-looking browser islands, set the host surface with
`.background_color(color)` or request `.transparent_background()`. This is
useful for inline previews, shaped widgets, WebGL/canvas overlays, and embeds
that should inherit native chrome instead of showing a default white rectangle.
Linux/Windows Wry-backed WebViews honor this construction option and runtime
updates today; Kael's custom macOS WKWebView backend honors it for the native
WebView host surface as well.

For untrusted static docs, preview panes, or sanitised customer-provided HTML
that should not execute scripts, use `.javascript_disabled()` or
`.javascript_disabled_enabled(true)`. Linux/Windows Wry-backed WebViews and
Kael's custom macOS WKWebView backend honor this construction option today. Do
not combine this with
`.bridge_script()`, `.inject_javascript(...)`, or hosted widgets that require
page JavaScript.

For hosted account forms, profile pages, and privacy-sensitive embeds on
Windows, tune browser-level general form suggestions with
`.general_autofill_enabled(false)` or `.general_autofill_disabled()`. This maps
to WebView2's general autofill setting and does not disable password or
credit-card autofill. Wry reports this option as unsupported on Linux/macOS, so
Kael treats it as a Windows-only WebView preference today.

For hosted pages that call `window.open(...)` or use `target="_blank"`, set an
explicit new-window policy. `.deny_new_windows()` blocks popups,
`.open_new_windows_in_current_webview()` keeps the flow inside the current
island, `.allow_new_windows()` delegates to the backend default, and
`.on_new_window(...)` lets the app choose per URL with
`WebViewNewWindowPolicy::{Deny, NavigateCurrent, Allow}`. Wry-backed
Linux/Windows WebViews honor this policy today. Kael's custom macOS WKWebView
backend honors all three policies for target-blank requests. On macOS, `Allow`
creates a WebKit-managed popup child WebView inside the same native window;
prefer `NavigateCurrent` or a custom handler when the app should own the
resulting route, chrome, or window lifecycle.

For authenticated previews, localized docs, test fixtures, and hosted tools
that need request metadata, use `.request_headers(headers)` on
`WebViewOptions` for the first load and
`WebViewController::navigate_with_headers(window, url, headers)` for later
navigations. Both accept `http_client::http::HeaderMap`. This is the
browser-island equivalent of hosted main-frame header injection: it
applies to the main navigation request, while subresource requests remain
controlled by the embedded browser engine. Linux/Windows Wry-backed WebViews
and Kael's custom macOS WKWebView backend honor request headers today.

For pages that trigger browser downloads, set a download policy too.
`.allow_downloads()` keeps the backend default, `.deny_downloads()` blocks all
downloads, and `.on_download_started(...)` can return
`WebViewDownloadPolicy::{Allow, Deny, SaveTo(path)}`. `SaveTo` must use an
absolute destination path because that is what the Wry backends require.
`.on_download_completed(...)` receives `WebViewDownloadCompleted { url, path,
success }` for progress handoff into the native app. Linux/Windows Wry-backed
WebViews and Kael's custom macOS WKWebView backend honor these handlers today.
On macOS, `Allow` resolves to the user's `~/Downloads` folder using WebKit's
suggested filename, while `SaveTo(path)` still requires an absolute path.
When a native context menu, command palette, or agent receives a `linkHref`,
`imageSrc`, or `mediaSrc` from the WebView bridges, call
`WebViewController::trigger_download(window, url, filename, callback)` or the
alias `download_url(...)` to dispatch a browser `<a download>` action inside
the hosted document. The callback returns `WebViewDownloadTriggerResult` with
the resolved URL and requested filename hint; the final destination and success
still come from the download policy and completion handlers. Cross-origin
responses and `Content-Disposition` headers may ignore the filename hint.

For app-owned downloads that do not start in browser content, use a checked
`DownloadRequest` instead of routing through a hidden WebView. This covers
exports, offline packs, model/artifact fetches, plugin assets, installer
helpers, and background worker queues:

```rust
let destination = cx.download_destination_plan_checked(
    DownloadDestinationPlanBuilder::new(url)
        .suggested_file_name("bundle.zip")
        .download_dir(dirs.download_dir())
        .network_policy(policy.clone())
        .sha256(expected_sha256)
        .size_bytes(expected_size)
        .create_parent_dirs(),
)?;

tracing::info!(summary = destination.to_text(), "download destination");

match destination.next_action() {
    DownloadDestinationNextAction::PromptForDestination => {
        // open the app-owned Save As dialog, then rebuild with .destination(path)
    }
    DownloadDestinationNextAction::ReviewOverwritePolicy => {
        // ask before replacing the existing file, then rebuild with .overwrite_existing()
    }
    DownloadDestinationNextAction::BuildRequest => {}
}

let request = destination.build_request_checked()?;
```

`cx.download_destination_plan_checked(DownloadDestinationPlanBuilder::...)` is
the native bridge for browser-like download destination behavior: suggested
filenames, download-directory defaults, explicit Save As destinations,
parent-directory policy, overwrite review, network policy, and integrity
metadata are all validated before a generated worker or agent can queue the
transfer. Its summaries avoid URL, destination path, suggested filename, and
exact size.

If the destination is already known, build the request directly:

```rust
let download = DownloadRequest::builder(url, destination)
    .network_policy(policy)
    .sha256(expected_sha256)
    .size_bytes(expected_size)
    .create_parent_dirs();

tracing::info!(summary = download.to_text(), "download request");
tracing::debug!(summary = download.to_safe_text(), "download request shape");
let request = cx.download_request_checked(download)?;
tracing::info!(summary = request.to_text(), "download request");
```

`DownloadRequest` rejects empty or non-HTTP(S) URLs, missing hosts, relative or
directory destinations, invalid SHA-256 or zero sizes, missing parent
directories unless `.create_parent_dirs()` is set, and URLs denied by the
attached `NetworkPolicy`. Use `cx.download_request_checked(...)` before queueing
generated downloads so the app can show or log a safe plan before the builder is
consumed. The descriptor is transport-agnostic:
hand it to an HTTP client, worker pool, plugin host, or export manager after
validation. Use `to_safe_text()` when an AI agent, shared trace, or telemetry
event should avoid hosts, destination paths, and exact sizes.

For multi-file exports, offline packs, model bundles, plugin asset sync, and
updater-adjacent artifact queues, group checked requests with
`cx.download_batch_checked(...)`:

```rust
let batch = DownloadBatch::builder()
    .request_builder(DownloadRequest::builder(index_url, index_path).create_parent_dirs())?
    .request_builder(
        DownloadRequest::builder(model_url, model_path)
            .sha256(model_sha256)
            .size_bytes(model_size)
            .create_parent_dirs(),
    )?;

tracing::info!(summary = batch.to_text(), "download batch");
let batch = cx.download_batch_checked(batch)?;
```

`cx.download_batch_checked(...)` rejects empty queues and duplicate destinations,
then reuses each request's URL, destination, integrity, parent-dir, and
network-policy validation. Inspect `request_count()`, `is_empty()`,
`DownloadBatch::requests()`, `into_requests()`, `sha256_count()`, `size_count()`,
`create_parent_dirs_count()`, `network_policy_count()`, `to_text()`, and
`to_safe_text()` before handing work to a background job or worker process.

Before the worker starts, create a checked execution policy with
`cx.download_execution_plan_checked(...)` for the native queue:

```rust
let plan = cx.download_execution_plan_checked(
    DownloadExecutionPlan::builder(batch)
        .max_parallel(2)
        .retry_attempts(3)
        .temporary_file_extension("partial"),
)?;

tracing::info!(summary = plan.to_text(), "download execution plan");
```

This is the small native-download-manager contract builders need before an
actual transfer backend runs: bounded parallelism, bounded retries, optional
temporary files, and explicit existing-file behavior. The checked path rejects
zero or excessive parallelism, more than ten retries, unsafe temporary-file
extensions, and existing destinations unless `.overwrite_existing()` is set.

When an AI agent or builder needs a single bridge object for a native download
surface, create a `DownloadHandoff`:

```rust
let handoff = cx.download_handoff_checked(
    DownloadHandoffBuilder::new()
        .request_builder(
            DownloadRequest::builder(asset_url, asset_path)
                .network_policy(policy)
                .sha256(asset_sha256)
                .size_bytes(asset_size)
                .create_parent_dirs(),
        )?
        .max_parallel(2)
        .retry_attempts(2),
)?;

tracing::info!(summary = handoff.to_text(), "download handoff");
```

`DownloadHandoffNextAction` reports the next missing step:
`ReviewOverwritePolicy`, `AddNetworkPolicy`, `AddIntegrityMetadata`, or
`QueueDownloads`. Use `has_complete_network_policy()`,
`has_complete_integrity_metadata()`, `needs_overwrite_review()`, and
`is_queue_ready()` to drive setup UI, agent repair loops, and worker queueing
without using a hidden WebView for app-owned downloads.

For app-owned HTTP requests that are not downloads, use
`AppNetworkRequestBuilder` as the Desktop `net.request`-style descriptor before
handing work to the app HTTP client:

```rust
let handoff = cx.network_realtime_handoff_checked(
    NetworkRealtimeHandoffBuilder::new()
        .request_builder(AppNetworkRequestBuilder::post("https://api.example.com/v1/sync"))?
        .realtime_connection_builder(
            AppRealtimeConnection::websocket("wss://events.example.com/socket")
                .protocol("app.v1")
                .reconnect_conservative(),
        )?
        .network_policy_builder(NetworkPolicyBuilder::new().allow_host("api.example.com"))?
        .hosted_network_bridge("checkout"),
)?;

tracing::info!(summary = handoff.to_text(), "network/realtime handoff");
```

`cx.network_realtime_handoff_checked(...)` and the underlying
`NetworkRealtimeHandoffBuilder` validate app-owned HTTP requests, realtime
connections or sets, outbound network policies, and explicit hosted
network/resource bridge scope before generated workers, plugins, or agents
dispatch side effects. `to_text()` reports request kinds and next action without
logging URLs, hosts, headers, body contents, credentials, cookies, destination
paths, byte sizes, hashes, subprotocols, or reconnect timings.

```rust
let descriptor = cx.set_http_client_checked(
    AppHttpClientInstallBuilder::new(client)
        .require_user_agent()
        .disallow_proxy(),
)?;

tracing::info!("installed HTTP client {}", descriptor.type_name());
```

The checked install path validates the app-wide client metadata: type name,
optional user-agent, and optional proxy URL. It complements request descriptors;
it does not replace per-request host policies, retry rules, body handling, or
response handling.

```rust
let request = AppNetworkRequestBuilder::post("https://api.example.com/v1/sync")
    .header("Content-Type", "application/json")
    .body_size_bytes(512)
    .network_policy(policy);

tracing::info!(summary = request.to_text(), "app network request");
tracing::debug!(summary = request.to_safe_text(), "app network request shape");
let request = request.build_checked()?;
```

The descriptor validates HTTP(S) URLs, host policy, method/body shape, duplicate
or malformed headers, and CR/LF header injection. It stays transport-agnostic:
use `AppNetworkRequestBuilder::validate()` and `to_text()` before queueing
generated request work, then let the app choose the HTTP client, retry policy,
body bytes, and response handling. Use `to_safe_text()` for agent or telemetry
summaries that should avoid hosts and exact body sizes.

For desktop-app background work such as indexing, sync, exports, and
agent-managed tasks, use the app-level checked scheduler:

```rust
let descriptor = JobDescriptor::new("export/video").with_priority(JobPriority::High);
tracing::info!(summary = descriptor.to_text(), "background job");
let handoff = cx.background_work_handoff_checked(
    BackgroundWorkHandoffBuilder::descriptor(descriptor.clone()),
)?;
tracing::info!(summary = handoff.to_text(), "background work handoff");
match handoff.next_action() {
    BackgroundWorkNextAction::ScheduleJob => {}
    BackgroundWorkNextAction::WaitForDependencies => {}
    BackgroundWorkNextAction::ReportProgress => {}
    BackgroundWorkNextAction::CancelJob => {}
    BackgroundWorkNextAction::PauseJob => {}
    BackgroundWorkNextAction::ResumeJob => {}
    BackgroundWorkNextAction::UseWorkerPool => {}
    BackgroundWorkNextAction::UseHelperProcess => {}
}
let job_id = cx.schedule_job_with_descriptor_checked(ExportVideoJob::new(project_id), descriptor)?;
```

Use `cx.schedule_job_checked(job)?` for default metadata. Checked scheduling
rejects invalid job IDs, descriptor/job ID mismatches, bad dependency lists, and
invalid retry policy before the queue changes. Use `RetryPolicy::to_text()`,
`JobDescriptor::to_text()`, `JobStatus::to_text()`, `JobProgress::to_text()`,
and `JobInfo::to_text()` when generated status UI, diagnostics, or agents need
priority, retry, dependency, progress, started/completed, and terminal-state
summaries without logging job ids, dependency ids, progress messages, exact
percentages, retry attempts, or timing details. Raw `schedule_job(...)` remains
available when the app owns job validation.
Use `cx.background_work_handoff_checked(...)` with
`BackgroundWorkHandoffBuilder::job(...)`, `.descriptor(...)`, `.progress(...)`,
`.cancel(...)`, `.pause(...)`, `.resume(...)`, `.worker_pool(...)`, or
`.helper_process(...)` before generated queues, progress reporting,
cancellation controls, worker-pool dispatch, or helper process escalation.
Inspect `BackgroundWorkNextAction`, `is_job()`,
`is_progress()`, `is_cancel()`, `is_pause()`, `is_resume()`,
`is_worker_pool()`, `is_helper_process()`, typed accessors, and `to_text()`
without logging job ids, dependency ids, progress messages, percentages, worker
reasons, helper-process reasons, or payloads.

For Desktop `WebSocket` and `EventSource` parity outside hosted browser pages,
use `AppRealtimeConnection` as the checked realtime descriptor:

```rust
let realtime = cx.realtime_connection_checked(
    AppRealtimeConnection::websocket("wss://events.example.com/socket")
        .protocol("kael.v1")
        .heartbeat_interval(std::time::Duration::from_secs(30))
        .reconnect_policy(AppRealtimeReconnectPolicy::conservative())
        .network_policy(policy),
)?;

tracing::info!(summary = realtime.to_text(), "app realtime connection");
tracing::debug!(summary = realtime.to_safe_text(), "app realtime shape");
```

Use `AppRealtimeConnection::server_sent_events(url)` for EventSource-style
streams. The descriptor validates transport-specific URL schemes, host policy,
headers, WebSocket subprotocols, heartbeat intervals, and inbound message
budgets before the app opens its chosen realtime client. Use
`AppRealtimeReconnectPolicy::conservative()` for ordinary chat/presence flows,
`.persistent()` for critical background sync, or a custom checked policy when a
worker needs explicit attempts and backoff bounds. Checked reconnect policies
reject more than 100 attempts, sub-100ms initial delays, max delays below
initial delays, and max delays above one hour. Use
`cx.realtime_connection_checked(AppRealtimeConnection::...)` before opening
generated realtime work. Use `to_safe_text()` for agent or telemetry summaries
that should avoid hosts, heartbeat timing, reconnect timing, and exact
message-size budgets.
For apps that open several live channels together, such as presence,
notifications, collaboration, and background sync, build an
`AppRealtimeConnectionSet` before connecting:

```rust
let realtime = cx.realtime_connection_set_checked(
    AppRealtimeConnectionSet::builder()
        .connection_builder(AppRealtimeConnection::websocket(presence_url).protocol("app.v1"))?
        .connection_builder(AppRealtimeConnection::server_sent_events(events_url))?,
)?;

tracing::info!(summary = realtime.to_text(), "app realtime connection set");
```

`cx.realtime_connection_set_checked(...)` rejects empty sets and exact duplicate
connection descriptors, while each connection still validates its URL, headers,
protocols, heartbeat, message budget, and network policy. Inspect
`connection_count()`, `websocket_count()`, `server_sent_events_count()`,
`protocol_count()`, `header_count()`, `heartbeat_count()`, `max_message_count()`,
`reconnect_policy_count()`, `network_policy_count()`, `connections()`,
`into_connections()`, `to_text()`, and `to_safe_text()` before opening native
realtime transports or handing the plan to a worker.

For pages that accept dragged files, use `.on_drag_drop(...)` to observe file
drag/drop events entering, moving over, dropping on, or leaving the WebView.
Return `WebViewDragDropPolicy::AllowBrowserDefault` when the page should keep
normal browser behavior, including drops onto `<input type="file">`. Return
`WebViewDragDropPolicy::BlockBrowserDefault`, or use `.block_drag_drop()`, when
hosted content should not receive local file drops. Linux/Windows Wry-backed
WebViews and Kael's custom macOS WKWebView backend honor this handler today.
On macOS, returning `AllowBrowserDefault` forwards the drag/drop operation to
WebKit so browser inputs and page handlers can still receive it; returning
`BlockBrowserDefault` prevents WebKit's default handling after the Kael handler
runs.

For hosted docs, auth, checkout, and editor islands that change
`document.title`, use `.on_document_title_changed(...)` to synchronize native
window titles, tabs, breadcrumbs, or inspector labels. Linux/Windows Wry-backed
WebViews and Kael's custom macOS WKWebView backend honor this handler today.
Prefer `window.set_window_title_checked(WindowTitleBuilder::new(title))?` when
the title comes from generated code, hosted pages, documents, or routes; the
checked path rejects empty, padded, control-character, and overly long platform
chrome text. Use `WindowTitleBuilder::to_text()` before applying generated or
hosted titles when logs or agents need length/blank state without the title
contents. Raw `window.set_window_title(...)` remains available for already
validated titles.
For hosted pages that expose favicons, use
`WebViewController::favicons(window, callback)` for an on-demand snapshot or
`.on_favicon_changed("favicon:changed", |event, window, cx| { ... })` for
event-driven native tab icons. The bridge reports resolved URLs from
`<link rel="icon">`, shortcut icons, Apple touch icons, and mask icons; it does
not fetch or decode image bytes for the app.

For browser content that should use familiar web commands, keep the
`WebViewController` next to the element id. In addition to navigation, reload,
JavaScript evaluation, and bridge messages, the controller exposes
`.navigate_with_headers(window, url, headers)`, `.load_html(window, html)`, `.focus(window)`,
`.focus_parent(window)`, `.set_zoom_factor(window, factor)`, `.print(window)`,
`.insert_css(window, key, css)`, `.remove_inserted_css(window, key)`,
`.find_text(window, query, WebViewFindOptions::forward(), callback)`,
`.find_text_result(window, query, WebViewFindOptions::forward(), callback)`,
`.stop_finding(window)`,
`.stop_finding_with_action(window, WebViewStopFindAction::KeepSelection)`,
`.copy(window, callback)`, `.cut(window, callback)`,
`.paste(window, callback)`, `.select_all(window, callback)`,
`.undo(window, callback)`, `.redo(window, callback)`,
`.delete_selection(window, callback)`,
`.insert_text(window, text, callback)`,
`.focus_selector(window, selector, callback)`,
`.click_selector(window, selector, callback)`,
`.add_class(window, selector, class_name, callback)`,
`.remove_class(window, selector, class_name, callback)`,
`.toggle_class(window, selector, class_name, force, callback)`,
`.set_attribute(window, selector, name, value, callback)`,
`.remove_attribute(window, selector, name, callback)`,
`.set_style_property(window, selector, name, value, callback)`,
`.remove_style_property(window, selector, name, callback)`,
`.set_form_value(window, selector, value, callback)`,
`.submit_form(window, selector, callback)`,
`.reset_form(window, selector, callback)`,
`.selected_text(window, callback)`,
`.selected_html(window, callback)`,
`.document_html(window, callback)`,
`.document_snapshot(window, callback)`,
`.element_snapshot(window, selector, callback)`,
`.capture_dom_image(window, selector, options, callback)`,
`.trigger_download(window, url, filename, callback)`,
`.download_url(window, url, filename, callback)`,
`.favicons(window, callback)`,
`.edit_command(window, WebViewEditCommand::Copy, callback)`,
`.title(window, callback)`,
`.user_agent(window, callback)`,
`.is_loading(window, callback)`,
`.can_go_back(window, callback)`, `.can_go_forward(window, callback)`,
`.viewport_snapshot(window, callback)`,
`.scroll_to(window, x, y, callback)`, `.scroll_by(window, dx, dy, callback)`,
`.scroll_selector_into_view(window, selector, callback)`,
`.cookies(window, callback)`, `.cookies_for_url(window, url, callback)`,
`.set_cookie(window, cookie, callback)`, `.delete_cookie(window, cookie, callback)`,
`.storage_snapshot(window, callback)`,
`.set_storage_item(window, area, key, value, callback)`,
`.remove_storage_item(window, area, key, callback)`,
`.clear_storage_area(window, area, callback)`,
`.stop_loading(window)`, `.play_media(window)`, `.pause_media(window)`,
`.set_media_muted(window, muted)`, `.set_media_volume(window, volume)`,
`.set_media_playback_rate(window, rate)`, `.seek_media_secs(window, seconds)`,
`.media_command(window, selector, command, callback)`,
`.set_media_source(window, selector, source, callback)`,
`.set_media_options(window, selector, options, callback)`,
`.capture_media_frame(window, selector, options, callback)`,
`.add_media_text_track(window, selector, track, callback)`,
`.remove_media_text_track(window, selector, track_selector, callback)`,
`.select_media_text_track(window, selector)`,
`.disable_media_text_tracks(window)`,
`.request_media_fullscreen(window)`, `.exit_media_fullscreen(window)`,
`.request_media_picture_in_picture(window)`,
`.exit_media_picture_in_picture(window)`,
`.media_state(window, callback)`, `.mute_media(window)`, `.unmute_media(window)`, and
`.clear_browsing_data(window)` for
`hosted navigation(...)` with extra
headers, `hosted HTML load(...)`, `hosted view focus`,
`hosted zoom factor(...)`, `hosted document print(...)`,
runtime `hosted CSS injection(...)` / `removeInsertedCSS(...)` styling,
basic `hosted find(...)` / `stopFindInPage(...)` flows,
`hosted copy` / `cut()` / `paste()` / `selectAll()` / `undo()` /
`redo()` edit flows, `hosted page controller.insertText(...)` hosted-editor typing,
selector-driven focus/click helpers for hosted controls, selector-driven hosted
DOM class/attribute/style customization, form value setting, and form
submission,
`hosted selected-text query`, rich selection HTML inspection, common
`executeJavaScript("document.documentElement.outerHTML")` export/diagnostic
flows, selector-scoped element inspection,
`hosted title query`,
desktop-app page favicon update flows for native tabs,
`hosted user-agent query`,
`hosted loading query`,
`hosted back-state query` / `hosted forward-state query`,
hosted document viewport inspection and app-owned scrolling,
`hosted load stop`,
app-owned play/pause/mute/volume/rate/seek controls and state snapshots for
browser `<audio>` and `<video>` elements, including browser fullscreen and
picture-in-picture requests,
`session.cookies.get(...)`, `session.cookies.set(...)`,
`session.cookies.remove(...)`, local/session Web Storage inspection and seeding,
and session cleanup workflows. Read callbacks
receive `Result<Vec<WebViewCookie>, SharedString>` with name, value, domain,
path, secure, and http-only metadata. Set/delete callbacks receive
`Result<(), SharedString>`. `storage_snapshot(...)` returns
`Result<WebViewStorageSnapshot, SharedString>` with URL, origin, readable
`localStorage` entries, and readable `sessionStorage` entries.
`set_storage_item(...)`, `remove_storage_item(...)`, and
`clear_storage_area(...)` take `WebViewStorageArea::{Local, Session}` and return
`Result<WebViewStorageMutationResult, SharedString>` with `ok`, area, key,
length, and browser error text when the current document cannot access storage.
Use these helpers for auth/session debugging, hosted settings, carts, draft
seeding, tests, and AI-agent state inspection without custom page JavaScript.
Browser origin, sandboxing, private-mode, and storage quota rules still apply;
for full profile cleanup use `clear_browsing_data(window)`. Find callbacks receive `Result<bool, SharedString>`
and report whether the browser found and selected a match. Use
`.find_text_result(...)` when native find chrome also needs a result count; it
returns `Result<WebViewFindResult, SharedString>` with `found` plus a portable
DOM text match count for the current document. That count does not inspect
cross-origin frames or backend-native hidden match state. Use
`.find_result_bridge("find:result")` or
`.on_find_result("find:result", |event, window, cx| { ... })` when native find
chrome, tests, or agents need desktop-app `found-in-page` updates after
browser `window.find(...)` calls, including the query, options, found flag,
match count, selected text, and page URL. `stop_finding(window)` maps to
Desktop's clear-selection default, and
`stop_finding_with_action(...)` accepts
`WebViewStopFindAction::{ClearSelection, KeepSelection, ActivateSelection}` for
desktop-app `stopFindInPage(action)` find-bar behavior.
Edit-command callbacks also receive `Result<bool, SharedString>` with the
browser's `document.execCommand(...)` success flag. `insert_text(...)` returns
`Result<bool, SharedString>` and tries browser `insertText` first, then falls
back to replacing the focused input/textarea selection or the current
contenteditable range while dispatching an input event. Use it for command
palettes, native editor chrome, tests, and AI agents that need to type into a
hosted editor without handwritten page JavaScript. It does not bypass browser
focus, disabled/read-only fields, or page-level validation.
`focus_selector(...)` and `click_selector(...)` return
`Result<bool, SharedString>` after querying the first matching element in the
top document, scrolling it into view, and calling the browser's normal
`focus(...)` or `click()` method. Use them with `insert_text(...)` for simple
agent/test flows such as focus a hosted search box, type, then click submit.
`add_class(...)`, `remove_class(...)`, and `toggle_class(...)` return
`Result<bool, SharedString>` after mutating `classList` on the first matching
top-document element. `set_attribute(...)` / `remove_attribute(...)` and
`set_style_property(...)` / `remove_style_property(...)` do the same for DOM
attributes and inline CSS properties. Use these for app-owned hosted widgets,
third-party embeds with stable selectors, visual test setup, and AI-agent
customization without writing raw JavaScript. They do not pierce cross-origin
frames or shadow roots, and page script/browser policy can still reinterpret or
override sensitive attributes and styles.
`set_form_value(...)` returns `Result<bool, SharedString>` after setting the
first matching input, textarea, select, checkbox, radio, or contenteditable
element and dispatching normal `input` and `change` events. Use it for hosted
settings/auth/checkout widgets where the app owns the selector and wants a
small Rust-side fill helper. `submit_form(...)` returns
`Result<bool, SharedString>` after finding a matching form or nearest ancestor
form from a selected control, then using `requestSubmit()` where available so
browser validation and submit handlers run. It falls back to a cancelable submit
event and `form.submit()` for older engines. `reset_form(...)` returns
`Result<bool, SharedString>` after finding a matching form or nearest ancestor
form and calling normal `form.reset()` so hosted default values and reset
listeners run. These are convenience helpers, not a full Playwright-style
automation engine: they do not pierce cross-origin frames, shadow roots, or
browser permission prompts. `selected_text(...)`
returns `Result<SharedString, SharedString>` for the current browser document or
focused input/textarea selection. `selected_html(...)` serializes cloned
document selection ranges as HTML and returns escaped selected text for focused
input/textarea controls. `document_html(...)` returns
`document.documentElement.outerHTML` for inspectors, export flows, bug reports,
and AI-agent page understanding; cross-origin frames remain browser-owned and
are not expanded into that string. `document_snapshot(...)` returns structured
same-document metadata and page-understanding data: URL, title, ready state,
language, direction, truncated visible text, total text length, headings, links,
images, and forms. Use it for diagnostics, browser inspectors, tests, and
AI-agent planning when raw HTML is too noisy. `element_snapshot(...)` returns
`Result<Option<WebViewElementSnapshot>, SharedString>` for the first matching
top-document element. It captures tag name, id, classes, normalized text,
form-control value/checked/disabled state, editable/hidden flags, nearest link
or media/image source, viewport rectangle, attributes, and a few computed style
signals. Use it before selector mutations, context-menu actions, visual tests,
and AI-agent plans that need to inspect one hosted control without raw
JavaScript. It returns `Ok(None)` for no match and does not inspect
cross-origin frames or shadow roots. `capture_dom_image(...)` returns
`Result<Option<SharedString>, SharedString>` with an SVG data URL for a selected
same-document element. It clones the element, inlines computed styles, mirrors
common form values, and wraps the clone in an SVG `foreignObject`; use
`WebViewDomImageCaptureOptions` to set width, height, background, and maximum
pixel area. This is useful for app-owned widget thumbnails, receipts, previews,
visual test artifacts, and AI-agent page previews. It is not a native pixel
screenshot or full hosted page capture equivalent: it does not pierce
cross-origin frames or shadow roots, and browser media, canvas, WebGL, plugin
surfaces, external fonts, and remote images may not serialize with visual
fidelity. Use `capture_media_frame(...)` for the current frame of browser
`<video>` elements. Use `.edit_command(...)` when a builder or agent wants to route
through the generic command enum instead of the named helpers.

For native app windows, use a checked app-window capture request instead of a
WebView DOM image:

```rust
let handoff = cx.visual_capture_handoff_checked(
    VisualCaptureHandoffBuilder::new()
        .app_window_capture_builder(
            AppWindowCaptureRequest::focused_window("Capture visual regression evidence.")
                .png()
                .max_dimensions(1920, 1080)
                .max_pixels(2_073_600),
        )?
        .native_snapshot_evidence("headless-render", 2)
        .hosted_capture(HostedVisualCaptureDescriptor::dom_image("preview", "#receipt"))
        .support_diagnostics(SupportDiagnosticsBuilder::new())
        .roadmap_work("full-page stitched capture"),
)?;
tracing::info!(summary = handoff.to_text(), "visual capture handoff");

let capture = cx.app_window_capture_request_checked(
    AppWindowCaptureRequest::focused_window("Capture visual regression evidence.")
        .png()
        .max_dimensions(1920, 1080)
        .max_pixels(2_073_600),
)?;
```

This is Kael's native `visual capture`-style contract for tests, support
diagnostics, and AI agents. `AppWindowCaptureRequestBuilder` can target the
focused app window, a specific app window, or all visible app windows, and it
validates purpose text, PNG vs raw RGBA output, window chrome/cursor flags,
dimension and pixel limits, plus the rule that multi-window captures cannot
include one cursor. Gate backend use with `PlatformFeature::AppWindowCapture`.
Visible app-owned render snapshots do not require screen-capture permission;
requests that allow occluded/minimized OS-level capture expose
`Some(Capability::ScreenCapture)` from `required_capability()`.
`VisualCaptureHandoffBuilder` is the native-first route planner for screenshots,
thumbnails, support bundles, visual tests, hosted DOM/media evidence, and
AI-agent visual inspection. It validates app-window capture, native
headless/cached/effect evidence counts, scoped hosted element/DOM/media capture,
support diagnostics, and roadmap capture work before any backend dispatch.
`VisualCaptureNextAction` tells builders whether to capture an app window,
collect native evidence, capture a hosted surface, export diagnostics, or track
roadmap work. `VisualCaptureHandoff::to_text()` reports request kinds and
booleans without logging pixels, paths, URLs, selectors, document text, window
titles, bounds, coordinates, image bytes, exact dimensions, hosted ids, roadmap
reason text, or generated preview contents.
Context-menu bridge callbacks receive `WebViewContextMenuEvent` with page
coordinates, selection text, nearest link/image/media sources, and editable
field metadata so native menus can enable actions such as Open Link, Copy
Image, Save Media, Paste, or Inspect without writing bespoke page scripts for
every embed.
Pointer bridge callbacks receive `WebViewPointerEvent` with page coordinates,
buttons, pointer type, target tag, nearest link/image/media sources, and
editable field metadata so native status bars, hover previews, click handling,
tests, and agents can inspect hosted content without preventing browser
defaults.
Runtime CSS insertion creates or replaces a named
`<style data-kael-style-key="...">` block; use app-owned keys such as
`"checkout-theme"` or `"reader-overrides"` so the same block can be updated or
removed later.
Title callbacks receive `Result<SharedString, SharedString>` and read
`document.title` on demand; use `.on_document_title_changed(...)` when you need
continuous synchronization.
Favicon callbacks receive `Result<Vec<SharedString>, SharedString>` from
`favicons(...)` or `WebViewFaviconEvent { urls }` from `.on_favicon_changed(...)`;
use these URLs for native tab icons, breadcrumbs, history rows, and hosted-app
switchers.
User-agent callbacks receive `Result<SharedString, SharedString>` and read
`navigator.userAgent`; use `WebViewOptions::user_agent(...)` or
`webview(...).user_agent(...)` when you need to set the initial user agent.
Loading-state callbacks receive `Result<bool, SharedString>` and use
`document.readyState !== "complete"` for app-owned spinners and route guards;
use `.on_page_load(...)` when you need event-driven lifecycle updates.
Back-state callbacks receive `Result<bool, SharedString>` and use
`history.length > 1` for portable Back button gating. The browser History API
does not expose a reliable forward-stack read. `can_go_forward(...)` therefore
reads `window.__kaelNavigationState.canGoForward` or
`window.kaelNavigationState.canGoForward` when present and otherwise returns
`false` conservatively. Use `.navigation_state_bridge()` for app-owned
same-document WebView navigation; it tracks `pushState`, `replaceState`, and
`popstate` entries created after injection and publishes that marker for native
Forward buttons. Keep backend-native forward stack reads on the hardening
roadmap for cross-page and third-party navigation.
Location bridge callbacks receive
`WebViewLocationEvent { url, title, ready_state, can_go_back, can_go_forward }`
from `.on_location_changed(...)` and are the event-driven route-sync path for
hosted SPAs, native breadcrumbs, tab labels, and AI-agent state tracking.
Lifecycle bridge callbacks receive
`WebViewLifecycleEvent { event, visibility_state, hidden, has_focus, fullscreen, persisted }`
from `.on_lifecycle_event(...)` and are the event-driven browser-page path for
pausing hosted work, marking tabs inactive, reacting to browser fullscreen, or
letting agents know whether embedded content is focused or visible.
For app-owned editor, markdown, rich-text, list, or document-viewer search and
zoom, keep the state native and validate it with a checked handoff:

```rust
let handoff = cx.find_zoom_handoff_checked(
    FindZoomHandoffBuilder::new()
        .search("needle")
        .result_summary(12, Some(3))
        .next_result()
        .fit_width()
        .hosted_find_zoom_bridge("docs"),
)?;
```

`FindZoomHandoffBuilder` covers native search, result summaries, result
navigation, clear-selection, document zoom modes, custom scale bounds,
persistence policy, and explicit hosted find/zoom fallback. Its summary reports
request kinds and next action without logging queries, selected text, matched
snippets, document contents, selectors, URLs, route ids, exact zoom factors,
coordinates, or viewport geometry. Use WebView `find_text`, `find_text_result`,
`stop_finding`, `set_zoom_factor`, `find_result_bridge`, and `zoom_hotkeys`
only when browser-owned text matching, selection highlighting, iframe/shadow-DOM
search, or exact page zoom is required.
Scroll bridge callbacks receive
`WebViewScrollEvent { event, x, y, max_x, max_y, viewport_width, viewport_height, scroll_width, scroll_height, progress_x, progress_y }`
from `.on_scroll_event(...)` and are the event-driven browser-page path for
reader progress, sticky native chrome, scroll restoration, and agent viewport
inspection. For on-demand viewport work, keep the controller and call
`viewport_snapshot(window, callback)`, `scroll_to(window, x, y, callback)`,
`scroll_by(window, dx, dy, callback)`, or
`scroll_selector_into_view(window, selector, callback)`. These helpers return
the same `WebViewScrollEvent` shape after reading or moving the top document.
`scroll_selector_into_view(...)` returns `Ok(None)` when no top-document element
matches. Cross-origin frames and shadow roots remain browser-owned.
Selection bridge callbacks receive
`WebViewSelectionEvent { event, selected_text, selected_html, collapsed, editable, input_kind }`
from `.on_selection_event(...)` and are the event-driven browser-page path for
native Edit menu enablement, floating formatting bars, rich-editor integrations,
and agent selection inspection.
The browser-media helpers operate on current
`document.querySelectorAll("audio,video")` elements. `play_media(window)` calls
each element's `play()` and swallows rejected promises, so browser autoplay and
user-gesture policies still apply; it is a convenient app-owned command, not a
cross-browser autoplay bypass. `set_media_volume(...)` clamps to `0.0..=1.0`,
`set_media_playback_rate(...)` sends the requested non-negative rate, and
`seek_media_secs(...)` clamps negative or non-finite input to `0.0`.
`media_command(...)` takes `WebViewMediaCommand` when native chrome, tests, or
agents need to play, pause, toggle, stop, mute, change volume, change playback
rate, or seek one matching media element or descendant instead of every media
element on the page.
`set_media_source(...)` returns `Result<bool, SharedString>` after finding a
matching `<audio>`, `<video>`, or nested `<source>`, assigning the new browser
media `src`, and calling `load()` so normal metadata, buffering, and media
events run. `set_media_options(...)` applies
`WebViewMediaElementOptions` to a matching `<audio>` or `<video>` so native
chrome and agents can toggle browser controls, loop, autoplay, muted,
playsinline, poster, preload, controlslist, and picture-in-picture disablement
without custom page JavaScript. `capture_media_frame(...)` returns
`Result<Option<SharedString>, SharedString>` with a browser canvas data URL for
the current frame of a matching `<video>`; it returns `Ok(None)` when no frame
is drawable or browser CORS/tainted-canvas rules block capture. Use
`WebViewMediaFrameCaptureOptions` to request size, MIME type, and quality.
`add_media_text_track(...)` appends a real browser `<track>` from
`WebViewMediaTextTrackOptions`, usually a WebVTT URL or data URL, so the
embedded browser owns cue loading/parsing and the resulting track appears in
`media_state(...)`. `remove_media_text_track(...)` removes
matching `<track>` children from a hosted media element by track id, label,
language, kind, src, or zero-based index so apps can swap subtitle sets without
reloading the WebView.
`select_media_text_track(...)` matches text-track id, label, language, or
zero-based index across current media elements and sets matching tracks to
`showing` while disabling the rest; `disable_media_text_tracks(...)` disables
all browser text tracks. Browser
fullscreen and picture-in-picture helpers call the standard element/document
APIs when present and swallow rejected promises; browser support, page
attributes, embedding policy, permissions, and user-gesture requirements still
apply.
`media_state(window, callback)` returns
`Result<Vec<WebViewMediaElementState>, SharedString>` with tag name, DOM id,
source, paused/ended/muted/seeking flags, volume, playback rate, current time,
optional duration, ready/network state, fullscreen and picture-in-picture
booleans, buffered ranges, browser text-track metadata, and active cue text for
native controls that drive browser-hosted players. `WebViewMediaEvent` uses the
same `WebViewMediaElementState` shape for event-driven updates.
Use browsing-data cleanup for logout, account switching, demo reset buttons,
and test isolation; with a persistent
`.storage_key(...)` / `WebViewOptions::auth_flow(...)`, the cleanup applies to
that WebView profile. Linux/Windows Wry-backed WebViews and Kael's custom
macOS WKWebView backend honor these commands through their profile cookie
stores today.

For lifecycle coordination, use `.on_page_load(...)` to receive
`WebViewPageLoadEvent::{Started, Finished}` plus the URL. This covers common
Desktop `did-start-loading` / `did-finish-load` style flows such as showing
spinners, deferring host messages until the page is ready, or observing hosted
auth redirects. Wry-backed Linux/Windows WebViews and Kael's custom macOS
WKWebView backend honor this handler today.

```js
const result = await window.kael.invoke("pick-video", { accept: ["video/*"] });
video.src = result.path;
window.kael.post("checkout-complete", { id: checkoutId });
```

Use the named option presets when they match the intent:

- `WebViewOptions::auth_flow(storage_key)` for OAuth, SSO, and account pages
  that need persistent cookies/session storage.
- `WebViewOptions::embedded_widget()` for payments, maps, docs, customer
  widgets, and other ephemeral third-party surfaces.
- `WebViewOptions::web_graphics()` for WebGL/WebGPU/canvas islands that should
  fill their element without browser scroll chrome.
- Add `.devtools()` to any option bundle while developing WebView islands.
- Add `.console_bridge(...)` or `.on_console_message(...)` when native
  diagnostics, tests, or agents should receive hosted-page console output.
- Add `.zoom_hotkeys()` / `.zoom_hotkeys_enabled(...)` when browser content
  should own zoom keyboard shortcuts or gestures.
- Add `.keyboard_event_bridge(...)` or `.on_keyboard_event(...)` when native
  chrome should observe hosted keydown/keyup/beforeinput activity.
- Add `.location_bridge(...)` or `.on_location_changed(...)` when native tabs,
  breadcrumbs, Back/Forward chrome, or agents should observe hosted SPA route
  changes.
- Add `.lifecycle_bridge(...)` or `.on_lifecycle_event(...)` when native chrome,
  resource throttling, tests, or agents should observe hosted focus, visibility,
  page show/hide, and browser fullscreen changes.
- Add `.scroll_bridge(...)` or `.on_scroll_event(...)` when native progress,
  sticky chrome, scroll restoration, tests, or agents should observe hosted
  scroll and viewport state.
- Keep a controller and call `viewport_snapshot(...)`, `scroll_to(...)`,
  `scroll_by(...)`, or `scroll_selector_into_view(...)` when native chrome,
  tests, or agents need to move or inspect the hosted top-document viewport.
- Keep a controller and call `add_class(...)`, `remove_class(...)`,
  `toggle_class(...)`, `set_attribute(...)`, `remove_attribute(...)`,
  `set_style_property(...)`, or `remove_style_property(...)` when hosted
  widgets need selector-scoped DOM customization without bespoke JavaScript.
- Call `element_snapshot(...)` first when native chrome, tests, or agents need
  to inspect one hosted element before deciding whether to focus, click, fill,
  style, or save related content.
- Add `.selection_bridge(...)` or `.on_selection_event(...)` when native edit
  menus, formatting chrome, tests, or agents should observe hosted selection
  state.
- Add `.media_autoplay()` / `.media_autoplay_enabled(...)` for browser-media
  islands that need an explicit autoplay policy.
- Add `.context_menu_bridge(...)` or `.on_context_menu(...)` when native chrome
  should own right-click / secondary-click menus for hosted WebView content.
- Add `.pointer_bridge(...)` or `.on_pointer_event(...)` when native status
  bars, hover previews, click handling, tests, or agents need lightweight
  link/image/media/editable context for hosted content.
- Add `.form_bridge(...)` or `.on_form_event(...)` when native validation,
  progress chrome, tests, or agents should observe hosted submit, reset, change,
  and input activity without bespoke page JavaScript.
- Add `.file_input_bridge(...)` or `.on_file_input_event(...)` when native
  upload chrome, tests, or agents should observe browser file-input selections
  with file names, sizes, MIME types, and last-modified timestamps.
- Add `.resource_bridge(...)` or `.on_resource_event(...)` when native
  diagnostics, loading UI, tests, or agents should observe hosted subresource
  timing plus element load/error activity without request interception.
- Add `.network_bridge(...)` or `.on_network_event(...)` when native
  diagnostics, loading UI, tests, or agents should observe hosted fetch/XHR
  outcomes without opening devtools.
- Add `.dialog_bridge(...)` or `.on_dialog_event(...)` when native diagnostics,
  tests, or agents should observe hosted `alert`, `confirm`, `prompt`, and
  `beforeunload` activity while preserving browser behavior.
- Add `.clipboard_event_bridge(...)` or `.on_clipboard_event(...)` when native
  editor chrome, tests, or agents should observe hosted copy/cut/paste events
  and browser-exposed clipboard data.
- Add `.permission_bridge(...)` or `.on_permission_request(...)` when native
  app policy should preflight hosted camera, microphone, display-capture,
  geolocation, or notification requests before browser permission prompts
  continue.
- Add `.storage_bridge(...)` or `.on_storage_event(...)` when native account
  chrome, settings sync, tests, or agents should observe hosted local/session
  storage changes without polling JavaScript.
- Add `.navigation_state_bridge()` when native chrome should enable or disable
  a Forward button for app-owned same-document WebView navigation.
- Add `.focused()` / `.focused_enabled(...)` when a WebView island should take
  keyboard focus as soon as it is created.
- Add `.clipboard_access()` / `.clipboard_access_enabled(...)` for hosted rich
  editors and browser widgets that need JavaScript clipboard APIs.
- Add `.javascript_disabled()` / `.javascript_disabled_enabled(...)` for
  untrusted static docs and previews that should not run page scripts.
- Add `.general_autofill_enabled(...)` / `.general_autofill_disabled()` for
  Windows/WebView2 general form suggestions.
- Add `.request_headers(...)` / `.clear_request_headers()` when the first
  navigation needs desktop-app extra request headers.
- Add `.html(...)` / `.clear_html()` when an option bundle should provide an
  initial raw HTML document instead of a URL.
- Add `.deny_new_windows()`, `.open_new_windows_in_current_webview()`,
  `.allow_new_windows()`, or `.on_new_window(...)` for popup and target-blank
  behavior.
- Add `.allow_downloads()`, `.deny_downloads()`, `.on_download_started(...)`,
  and `.on_download_completed(...)` when browser content can download files.
  Pair this with `WebViewController::trigger_download(...)` for native "Save
  link/image/media" commands sourced from context-menu or pointer bridge URLs.
- Add `.on_drag_drop(...)` or `.block_drag_drop()` when browser content can
  receive local file drops and the app needs to preserve or block browser
  defaults explicitly.
- Add `.on_document_title_changed(...)` when hosted content should drive native
  titles, tabs, or breadcrumbs.
- Add `.favicon_bridge(...)` or `.on_favicon_changed(...)` when hosted content
  should drive native tab icons, breadcrumbs, history rows, or app switchers.
- Add `.on_page_load(...)` when hosted content needs native loading state,
  ready coordination, or auth-flow progress.

All presets can be refined with `.storage_key(...)`, `.user_agent(...)`,
`.inject_css(...)`, `.inject_javascript(...)`, `.bridge_script()`,
`.devtools()`, `.console_bridge(...)`, `.on_console_message(...)`,
`.zoom_hotkeys()`, `.zoom_hotkeys_enabled(...)`,
`.find_result_bridge(...)`, `.on_find_result(...)`,
`.keyboard_event_bridge(...)`, `.on_keyboard_event(...)`,
`.location_bridge(...)`, `.on_location_changed(...)`,
`.lifecycle_bridge(...)`, `.on_lifecycle_event(...)`,
`.scroll_bridge(...)`, `.on_scroll_event(...)`,
`.selection_bridge(...)`, `.on_selection_event(...)`,
`.media_autoplay()`, `.media_autoplay_enabled(...)`, `.media_event_bridge(...)`,
`.on_media_event(...)`, `.context_menu_bridge(...)`, `.on_context_menu(...)`,
`.pointer_bridge(...)`, `.on_pointer_event(...)`,
`.form_bridge(...)`, `.on_form_event(...)`,
`.file_input_bridge(...)`, `.on_file_input_event(...)`,
`.resource_bridge(...)`, `.on_resource_event(...)`,
`.network_bridge(...)`, `.on_network_event(...)`,
`.dialog_bridge(...)`, `.on_dialog_event(...)`,
`.clipboard_event_bridge(...)`, `.on_clipboard_event(...)`,
`.permission_bridge(...)`, `.on_permission_request(...)`,
`.storage_bridge(...)`, `.on_storage_event(...)`,
`.navigation_state_bridge()`,
`.html(...)`, `.clear_html(...)`, `.focused()`, `.focused_enabled(...)`, `.clipboard_access()`,
`.clipboard_access_enabled(...)`, `.javascript_disabled()`,
`.javascript_disabled_enabled(...)`, `.general_autofill_enabled(...)`,
`.general_autofill_disabled()`, `.request_headers(...)`,
`.clear_request_headers()`, `.deny_new_windows()`,
`.open_new_windows_in_current_webview()`,
`.allow_new_windows()`, `.allow_downloads()`, `.deny_downloads()`,
`.on_download_started(...)`, `.on_download_completed(...)`,
`.on_drag_drop(...)`, `.block_drag_drop()`, `.on_message(...)`,
`.on_bridge_message(...)`, `.favicon_bridge(...)`, `.on_favicon_changed(...)`,
`.on_document_title_changed(...)`,
`.on_page_load(...)`, `.on_navigate(...)`, `.on_new_window(...)`, and
`.allow_navigation_schemes(...)`. For tiny one-off embeds, the existing fluent
methods on `webview(id, url)` remain available.

Recommended rule:

- Use native Kael for app chrome, navigation, panes, toolbars, forms, lists, and
  performance-sensitive surfaces.
- Use WebView islands when the value comes from web compatibility itself.
- Keep WebView boundaries explicit and message-based so apps do not become a
  hidden browser app by accident.

## Platform APIs need builder-shaped affordances

Desktop feels productive partly because common desktop capabilities are one
obvious call away. Kael already has many of the native backends, but the public
surface should increasingly expose builder-shaped APIs that are easy for humans
and AI agents to compose without remembering raw arrays or callback plumbing.

Notifications are the pattern:

```rust
cx.show_notification_checked("Build Complete", "All tests passed")?;

let notification = NotificationBuilder::new("Build Complete", "All tests passed");
tracing::info!(summary = notification.to_text(), "notification");
cx.show_desktop_notification(notification)?;

let update_notification =
    NotificationBuilder::new("Update Available", "Version 2.0 is ready to install")
        .critical()
        .tag("update-available")
        .group("updates")
        .timeout_secs(30)
        .open_action("Install Now")
        .dismiss_action("Remind Later");
tracing::info!(summary = update_notification.to_text(), "notification");
let plan = update_notification
    .clone()
    .delivery_plan(NotificationFeatureSupport::rich())?;
tracing::info!(summary = plan.to_text(), "notification delivery plan");
cx.show_desktop_notification_with_action_router(
    update_notification,
    |action| {
        if action.is_open() {
            install_update();
        } else if action.is_dismiss() {
            remind_later();
        }
    },
)?;
```

This does not replace lower-level calls such as
`show_notification(...)` or `show_notification_with_actions(...)`; it gives
builders a safer default path. Use `show_notification_checked(...)` for plain
notifications and `.action(id, label)` for custom routing. Prefer
`show_desktop_notification_with_action_router(...)` for generated action
callbacks so platform IDs are classified as known or unknown
`NotificationActionEvent` values before routing. Builder validation rejects
duplicate action IDs before callbacks become ambiguous. Use `.low_priority()`,
`.critical()`, `.silent(...)`, `.deliver_silently()`, `.tag(...)`, `.group(...)`,
`.timeout_ms(...)`, and `.timeout_secs(...)` to preserve desktop-app delivery
intent such as urgency, silent delivery, replacement, grouping, and expiration
hints in builder-authored flows; platform backends can honor those hints
incrementally. Use
`notification.to_text()` before dispatch and `action.to_text()` inside callback
routers for content-safe logs and agent summaries; summaries report
urgency, metadata presence, counts, and booleans without logging title, body,
tags, groups, action labels, raw action IDs, unknown platform action IDs, or
timeout values. Use `NotificationFeatureSupport` and
`notification.delivery_plan(...)` when the backend support profile is known:
`NotificationFeatureSupport::basic()` is title/body-only, `.actions()` adds
action buttons, and `.rich()` marks all builder metadata as supported.
`NotificationDeliveryPlan` reports `missing_features()`,
`missing_feature_count()`, `is_fully_supported()`, and `requires_fallback()` so
agents can fall back to in-app inboxes, simpler notifications, or explicit UI
when an desktop-app rich notification would degrade on a platform. Use
`action_ids()`, `tag_value()`, and `group_value()` only for deliberate exact-ID
or exact-metadata inspection.
When a notification action also opens URLs, reveals exports, or requests dock
or taskbar attention, use `NotificationFlowHandoffBuilder` before dispatch. It
combines `NotificationDeliveryPlan`, optional `ShellTargetsBuilder`, optional
`UserAttentionBuilder`, and `NotificationFlowNextAction` so generated code can
choose plain dispatch, action-router dispatch, attention-then-dispatch, or
in-app fallback without logging notification copy, action labels, URLs, paths,
tags, groups, or attention reasons.
Inside action callbacks, use `NotificationActionFollowUpBuilder` before running
commands or shell side effects:

```rust
let follow_up = NotificationActionFollowUpBuilder::new(notification.clone(), action)
    .app_command_for_action(NotificationAction::OPEN_ID, "exports.open-latest")
    .shell_targets(
        ShellTargetsBuilder::new()
            .url("https://example.com/help/export")
            .reveal_path(report_path)
            .require_existing_paths(),
    )
    .fallback_unknown_actions()
    .build_checked()?;

tracing::info!(summary = follow_up.to_text(), "notification action follow-up");

match follow_up.next_action() {
    NotificationActionFollowUpNextAction::RunAppCommand => {}
    NotificationActionFollowUpNextAction::OpenShellTargets => {}
    NotificationActionFollowUpNextAction::RequestAttention => {}
    NotificationActionFollowUpNextAction::ShowFallbackUi => {}
    NotificationActionFollowUpNextAction::IgnoreUnknownAction => {}
    NotificationActionFollowUpNextAction::AcknowledgeAction => {}
}
```

This closes the notification-button gap that browser-style apps often leave to
ad hoc JavaScript callbacks: known action ids must be declared, app-command
mappings must target declared actions, shell targets and attention are validated,
and unknown platform action ids have an explicit ignore or fallback policy.
Summaries avoid notification copy, action labels, command ids, URLs, paths, and
attention reasons.
Tray menus follow the same direction:

```rust
cx.set_tray_icon_checked(TrayIconBuilder::png(include_bytes!("icon.png").to_vec()))?;
cx.set_tray_menu_checked(
    TrayMenuBuilder::new()
        .action("Show Window", "show")
        .separator()
        .toggle("Pause Sync", false, "pause-sync")
        .submenu(
            "Status",
            TrayMenuBuilder::new()
                .toggle("Available", true, "available")
                .action("Set Away", "away"),
        )
        .action("Quit", "quit"),
)?;
cx.set_tray_tooltip_checked(TrayTooltipBuilder::status("Sync complete"))?;

let tray_plan = cx.tray_app_checked(
    TrayAppBuilder::new()
        .action("Show Window", "show")
        .toggle("Pause Sync", false, "pause-sync")
        .status_tooltip("Sync complete")
        .panel()
        .keep_alive_without_windows(true),
)?;
tracing::info!(summary = tray_plan.to_text(), "tray app");
```

Use `TrayIconBuilder::png(...)`, `ico(...)`, `bytes(...)`, or `clear()` for
generated tray icons. The checked path rejects empty, too-small, oversized, and
unknown-format byte buffers before platform tray APIs receive them; raw
`cx.set_tray_icon(...)` remains available for already-validated platform icon
handling. Use `TrayIconBuilder::to_text()` for generated logs and AI-agent
summaries without recording encoded icon bytes.

Use `TrayTooltipBuilder::status(...)`, `text(...)`, or `clear()` for generated
tray/background app status. The checked path rejects empty tooltips, padded text,
control characters, and text longer than 256 characters; raw
`cx.set_tray_tooltip(...)` remains available for already-validated platform
behavior. Use tooltip `to_text()` / `has_tooltip()` / `is_clear()` for
content-safe traces that avoid logging tooltip text.
Use `tray_app_checked(...)` before `configure_tray_app_checked(...)` when
generated background apps or plugin systems need to inspect action IDs,
item/toggle counts, tooltip presence, panel mode, and keep-alive policy before
mutating the live tray surface. Use tray app `to_text()` for a single
content-safe summary that avoids logging labels, action IDs, or tooltip text.

Generated desktop shell work can also be coordinated through one checked native
handoff before tray, placement, progress, badges, and attention requests drift
into separate partial states:

```rust
let shell = cx.desktop_shell_chrome_handoff_checked(
    DesktopShellChromeHandoffBuilder::new()
        .runtime_snapshot(AppRuntimeSnapshotQueryBuilder::new().require_not_quitting())
        .tray_app(
            TrayAppBuilder::new()
                .action("Show Window", "show")
                .toggle("Pause Sync", false, "pause-sync")
                .status_tooltip("Sync running")
                .panel()
                .keep_alive_without_windows(true),
        )
        .window_placement(WindowPlacementBuilder::new(size(px(360.0), px(240.0))))
        .progress_indicator(ProgressIndicatorBuilder::normal("sync", 0.5).window())
        .dock_badge(DockBadgeBuilder::count(3))
        .user_attention(UserAttentionBuilder::informational().reason("sync complete")),
)?;

match shell.next_action() {
    DesktopShellChromeNextAction::ValidateRuntimeState => {}
    DesktopShellChromeNextAction::ConfigureTrayApp => {}
    DesktopShellChromeNextAction::ResolveWindowPlacement => {}
    DesktopShellChromeNextAction::InstallProgressIndicator => {}
    DesktopShellChromeNextAction::SetDockBadge => {}
    DesktopShellChromeNextAction::RequestUserAttention => {}
    DesktopShellChromeNextAction::TrackRoadmapWork => {}
}

tracing::info!(summary = shell.to_text(), "desktop shell chrome");
```

`DesktopShellChromeHandoffBuilder` validates runtime state, tray/background app
configuration, semantic window placement, progress plans, dock/taskbar badges,
user attention, and roadmap shell work together. `DesktopShellChromeHandoff`
reports request kinds and the next action without logging labels, action IDs,
tooltip text, progress scopes, badge labels, attention reasons, geometry, or
roadmap text.

Use the lower-level tray, dock, progress, and placement builders directly when
one subsystem owns only one shell piece.
When tray menus are configured separately, inspect `TrayMenuBuilder::to_text()`
or `TrayMenuItem::items_to_text(...)` before calling `set_tray_menu_checked(...)`.
These summaries expose item, action, toggle, checked-toggle, submenu, separator,
and max-depth counts without logging labels or action IDs.

Context menus use the same native item model with a context-specific builder:

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
        ),
    |action_id, _cx| {
        println!("context menu action: {action_id}");
    },
)?;
```

Both checked paths validate empty labels, empty action IDs, empty submenus, and
duplicate action IDs across nested menu trees before the OS menu is installed.
Use `NativeContextMenuBuilder::to_text()` plus the matching item/action/toggle
count helpers before showing generated context menus so tests and agents can
verify menu shape without logging labels or action IDs.

Apply this pattern to deep-link setup and other remaining platform surfaces:
keep the native platform capability explicit, but make the 80% path one fluent
object with validation, docs, and capability-report guidance.

Clipboard text now follows that rule too:

```rust
cx.write_clipboard_text_checked("Copied from Kael")?;

let read = ClipboardReadRequestBuilder::text("Paste into editor");
tracing::info!(summary = read.to_text(), "clipboard read");
if let Some(text) = cx.read_clipboard_text_checked(read)? {
    println!("clipboard: {text}");
}
```

For richer paste/copy workflows, use the validated clipboard builder instead of
manually constructing entry arrays:

```rust
let item = ClipboardItem::builder()
    .try_text_with_json_metadata(
        "formatted text",
        serde_json::json!({ "source": "my_app" }),
    )?
    .image_ref(&preview_image);
tracing::info!(summary = item.to_text(), "clipboard write");
cx.write_clipboard_item_checked(item)?;

let handoff = ClipboardEditingHandoffBuilder::read_any("Inspect paste payload")
    .build_checked()?;
tracing::info!(summary = handoff.to_text(), "clipboard editing handoff");
if handoff.next_action() == ClipboardEditingNextAction::ReadClipboard {
    tracing::info!("read clipboard through a checked clipboard request");
}

let read = ClipboardReadRequestBuilder::any("Inspect paste payload");
tracing::info!(summary = read.to_text(), "clipboard read");
if let Some(item) = cx.read_clipboard_item_checked(read)? {
    tracing::info!(summary = item.to_text(), "clipboard item");

    if item.has_text() {
        println!("text: {:?}", item.text());
    }
    if let Some(image) = item.first_image() {
        println!("image format: {:?}", image.format());
    }
}

let clear_clipboard = ClipboardClearBuilder::new("Copied token expired");
tracing::info!(summary = clear_clipboard.to_text(), "clipboard clear");
cx.clear_clipboard_checked(clear_clipboard)?;
```

Raw `ClipboardItem` constructors remain available, but the builder path gives
agents a safer way to combine text, JSON metadata, and images. The older
`write_clipboard_text(...)` convenience method remains available for
already-validated text, while generated code should prefer
`write_clipboard_text_checked(...)`, `write_clipboard_item_checked(...)`,
`read_clipboard_text_checked(...)`, `read_clipboard_item_checked(...)`, and
`ClipboardClearBuilder::to_text()` when clearing sensitive clipboard contents
without logging the clear reason;
checked writes require the current process to hold `Capability::ClipboardWrite`.
Checked reads require the current process to hold `Capability::ClipboardRead`
and use `ClipboardReadRequestBuilder::{text, html, image, any}(reason)` to state
the expected clipboard content class before inspecting user-visible clipboard
data.
Use `ClipboardEditingHandoffBuilder` when generated editor, command-palette, or
agent flows need one checked setup packet for clipboard writes, reads, clears,
or edit-command snapshots. `ClipboardEditingNextAction` separates write, read,
clear, and snapshot work without logging clipboard text, HTML, metadata, image
bytes, read/clear reasons, command labels, selectors, or URLs.
Use `item.to_text()` for content-safe logs, tests, and AI-agent summaries before
inspecting clipboard text, HTML, metadata, or image bytes. Runtime `Image`
values expose `format()`, `byte_len()`, `has_bytes()`, and `to_text()` so agents
can inspect native-image payload shape without logging raw bytes. On the
builder, use
`entry_count()`, `text_count()`, `image_count()`, `metadata_count()`,
`text_len_bytes()`, `has_text()`, `has_html()`, `has_image()`, and `to_text()`
before writing generated copy payloads.
For Desktop `clipboard.clear()` privacy and reset flows, prefer
`clear_clipboard_checked(...)` so generated code must explain why it is removing
user-visible clipboard contents; raw `clear_clipboard()` remains available for
already-validated integrations.

For formatted text that the app owns, prefer native `rich_text()` before
embedding a browser editor just for selection, links, mentions, hashtags,
inline code, or inline chips:

```rust
let body = rich_text()
    .selectable()
    .text("Open ")
    .link("docs", "https://example.com/docs", |_, _| {})
    .mention("@owner", "user-42", |_, _| {})
    .code("cargo test")
    .build();
tracing::info!(summary = body.to_text(), "rich text");
```

`RichText::to_text()` reports segment, text-byte, inline-element, highlighted,
code, entity, link, mention, hashtag, click-handler, selectability,
selection-color, and element-id metadata without logging text, URLs, mentions,
hashtags, code contents, or entity payloads. Use `segment_count()`,
`text_segment_count()`, `inline_element_count()`, `entity_count()`,
`link_count()`, `mention_count()`, `hashtag_count()`, and
`click_handler_count()` when generated app chrome or tests need to verify native
formatted text composition.

For editable fields and custom native input chrome, `text_input(...).render_with`
receives a `TextInputRenderState` with content-safe inspection helpers:

```rust
text_input("search", self.query.clone())
    .placeholder("Search")
    .render_with(|state, window, cx| {
        tracing::info!(summary = state.to_text(), "text input render");
        state.paint_default_contents(window, cx);
    });
```

Use `value_len_bytes()`, `display_text_len_bytes()`,
`placeholder_len_bytes()`, `has_placeholder()`, `is_empty()`,
`is_masked_display()`, `line_count()`, `selection_rect_count()`,
`has_selection()`, and `has_cursor()` for generated tests and custom renderers.
The summary reports focus, placeholder, multiline, selection, caret, masking,
and line shape without logging field values, placeholder text, selected text, or
geometry coordinates.

For desktop-app editors that would normally reach for Monaco, CodeMirror, or
DOM textarea APIs, prefer the native `Editor` when the app needs a lighter code,
markdown, log, SQL, or prompt-editing surface. `Position::to_text()`,
`Selection::to_text()`, `FoldRange::to_text()`,
`EditorDiagnostic::to_text()`, `EditorState::to_text()`, and
`Editor::to_text()` give builders and agents a structured view of language,
line/content byte counts, cursor and selection geometry, modified/file-path
presence, undo/redo depth, tree-sitter readiness, search counts/options, folds,
readonly state, diagnostics, and visual customization without logging document
text, file paths, selected text, search terms, diagnostic messages, or callback
internals.

For basic native form chrome, `ButtonRenderState`, `CheckboxRenderState`,
`ToggleRenderState`, `RadioItemRenderState`, `SliderRenderState`,
`ProgressRenderState`, `TabRenderState`, `DisclosureRenderState`,
`ModalRenderState`, `PopoverAnchorRenderState`, and
`PopoverPopupRenderState`, `MenuButtonTriggerRenderState`,
`MenuButtonItemRenderState`, `SplitterRenderState`, `ScrollBarRenderState`,
`Toast`, `Label`, `ListState`, `ListScrollEvent`, `UniformList`,
`UniformListScrollHandle`, and `RecyclingList` expose `to_text()` helpers
for content-safe diagnostics inside custom renderers, callbacks, or before
rendering. Label-based helpers report label/text presence or byte length without
logging label text. Radio, tab, menu-item, and list summaries report item
position, ranges, wrapper configuration, scroll-to-item intent, reorder intent,
or counts without logging row contents, measured heights, exact overdraw pixels,
scroll offsets, or values; use `sortable_reorder_plan(...).to_text()` before
generated reorder mutations. Modal and popover summaries report dismissal policy without logging
labels or geometry. Slider, splitter, scroll bar, and progress summaries report
coarse position, thumb, visibility, or completion classes without logging exact
values, maximums, bounds, pixels, ratios, or fractions. Toast summaries report
body presence, text lengths, duration class, and position without logging
title/body text or exact seconds.

For desktop-app `<select>` and searchable combo boxes, prefer native
`select(...)` with render-state summaries before embedding browser form
widgets:

```rust
select("workspace", self.workspace, workspaces)
    .placeholder("Choose workspace")
    .searchable()
    .render_with(|state, _window, _cx| {
        tracing::info!(summary = state.to_text(), "select trigger");
        div().child(state.display_text).into_any_element()
    });
```

Use `SelectRenderState::to_text()`, `SelectOptionRenderState::to_text()`,
`SelectPopupRenderState::to_text()`, and `SelectSearchRenderState::to_text()`
inside custom trigger, option, popup, and search renderers. The summaries expose
open/focus state, placeholder and selected-label presence, option index,
selected/highlighted state, filtered count, highlighted/selected index
presence, search activity, and byte lengths without logging displayed labels,
placeholder text, search queries, option values, popup widths, or coordinates.

For native date inputs and calendar pickers, prefer `date_picker(...)` with the
same render-state pattern:

```rust
date_picker("delivery", self.delivery_date)
    .render_days_with(|state, _window, _cx| {
        tracing::info!(summary = state.to_text(), "date picker day");
        div().child(state.day.to_string()).into_any_element()
    });
```

Use `DatePickerRenderState::to_text()`, `DatePickerDayRenderState::to_text()`,
`DatePickerPopupRenderState::to_text()`,
`DatePickerHeaderRenderState::to_text()`,
`DatePickerNavButtonRenderState::to_text()`, and
`DatePickerWeekdayRenderState::to_text()` inside custom trigger, day, popup,
header, navigation, and weekday renderers. The summaries expose open/focus
state, label byte lengths, selectable/selected/highlighted day state,
selected-highlighted relation, navigation availability, button direction and
enabled state, and weekday index without logging exact dates, month names,
weekday labels, button labels, popup widths, or coordinates.

Share sheets follow the same checked-builder shape when apps need Desktop-like
"share/export this" handoff to the operating system:

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

let sheet = ShareSheet::builder()
    .subject("Export bundle")
    .text("Assets are ready")
    .file(export_path)
    .build_checked()?;
tracing::info!(summary = sheet.to_text(), "share sheet");
cx.show_share_sheet_checked(sheet).await?;
```

`ShareItem::{text,url,file,files,image}` and
`ShareSheet::{text,url,file,files}` cover one-line cases, while
`ShareSheetBuilder` handles export bundles. The checked path validates non-empty
payloads, URL schemes, image data, and file existence before invoking the
platform backend; `cx.share_support()` reports available destination families.
Use `ShareSheetBuilder::to_text()` before building and `sheet.to_text()` after
building when generated export UI or agents need stable item, text, URL, file,
image, exclusion, and subject counts before opening native share UI.
`ShareItem::to_text()`, `ShareImage::to_text()`,
`PlatformShareSupport::to_text()`, and `ShareResult::to_text()` cover
per-payload, destination-support, and completion traces without logging text
bodies, URLs, file paths, image MIME strings, suggested names, subjects, or
platform activity identifiers.

Operating-system file drops should also use the typed drag/drop path instead
of platform-event bookkeeping:

```rust
let filter = FileDropFilter::video().max_files(1);

div()
    .id("video-drop-zone")
    .can_drop_external(filter.clone())
    .on_external_drop(move |data, _window, _cx| {
        tracing::info!(summary = filter.to_text(), "file drop filter");
        tracing::info!(summary = data.to_text(), "external drop");
        if let Some(paths) = data.accepted_paths_by(&filter) {
            for path in paths {
                // Open or import the dropped file.
            }
        }
        for url in data.urls() {
            // Import or embed the dropped URL.
        }
    });
```

This mirrors the Desktop mental model of dropping files into an editor,
uploader, or media player, while keeping the native typed drag/drop system.
Use `FileDropFilter::images()`, `.audio()`, `.video()`, `.media()`, or
`.single_file()` for common drop zones before falling back to custom
`.extensions([...])` filters. `can_drop_external(filter)` applies the filter to
file paths and still accepts text/URL-only payloads. Use
`FileDropFilter::to_text()`, `ExternalDropData::to_text()`,
`ExternalPaths::to_text()`, and `FileDropMatch::to_text()` when generated drop
zones, logs, or tests need counts for path/text/URL payloads, accepted/rejected
paths, extension filters, or max-file policies without logging local paths,
filenames, dragged text, URLs, or extension labels.

For generated builders and AI agents, use a drag/drop transfer handoff before
accepting the transfer. When the incoming payload may contain paths, URLs, text,
or mixed browser-style data, build a checked intake plan first:

```rust
let intake = DataTransferDropIntakePlanBuilder::new(drop_data)
    .file_filter(FileDropFilter::media().max_files(4))
    .max_urls(2)
    .max_text_bytes(16 * 1024)
    .allow_missing_paths()
    .build_checked()?;

tracing::info!(summary = intake.to_text(), "data transfer drop intake");

let handoff = DragDropTransferHandoffBuilder::media_drop(paths)
    .build_checked()?;
tracing::info!(summary = handoff.to_text(), "drag/drop handoff");
if handoff.next_action() == DragDropTransferNextAction::AcceptIncomingDrop {
    let intent = handoff.incoming_drop_builder().unwrap().clone().build_checked()?;
    tracing::info!(summary = intent.to_text(), "file drop intent");
}
```

`DataTransferDropIntakePlanBuilder` routes paths, URLs, text, mixed payloads,
unknown files, and hosted DOM fallback before app state changes. Use
`DataTransferDropIntakeNextAction` and `file_intake()` to decide whether to
open/import paths, route links, route text, review mixed payloads, review
unknown paths, reject a drop, or let a WebView island own DOM `DataTransfer`
semantics. Its summaries report only counts and booleans, never dropped paths,
file names, text, URLs, MIME strings, coordinates, selectors, hosted ids, or
payload contents.

`DragDropTransferHandoffBuilder` separates incoming drops, file-export drags,
internal drag routing, and hosted DOM `DataTransfer` delegation. Use
`DragDropTransferNextAction` to decide whether to accept an app-owned drop,
start a native export drag, configure internal drag handles/drop targets, or
isolate browser-only drag behavior in a WebView without logging dropped paths,
file names, text, URLs, MIME strings, generated bytes, route ids, WebView ids,
coordinates, selectors, or payload contents.

After the user drops files, convert accepted paths into an app-owned intent
before importing or opening them:

```rust
let intent = cx.file_drop_intent_checked(
    FileDropIntentBuilder::media_source()
        .paths(paths)
        .max_paths(4)
        .canonicalize_paths(),
)?;
tracing::info!(summary = intent.to_text(), "file drop intent");

for path in intent.paths() {
    open_media(path)?;
}
```

`FileDropIntentBuilder` validates the semantic purpose, max path count,
file-vs-directory policy, extension allowlists, optional existence,
canonicalization, and deduplication. Use it for desktop-app drag-to-open,
project import, folder import, media-player drops, and AI-agent file intake. Use
`FileDropIntentBuilder::to_text()` and `FileDropIntent::to_text()` for
content-safe summaries before import/open work; they expose counts and policies
without logging local file paths.

For drag-out/export workflows, use a checked file export drag descriptor rather
than generating a temporary WebView download:

```rust
let export = cx.file_export_drag_checked(
    FileExportDragIntentBuilder::generated_files("Drag rendered poster.")
        .virtual_file_with_mime("poster.png", "image/png", poster_bytes)
        .max_virtual_file_bytes(32 * 1024 * 1024),
)?;
assert_eq!(export.display_names(), vec!["poster.png"]);
```

`FileExportDragIntentBuilder` covers existing file paths and generated virtual
files/file promises. It validates purpose text, item limits, safe file names,
MIME type shape, non-empty generated bytes, byte limits, and optional existence
for existing paths. Existing-path exports declare a
`Capability::FilesystemRead { scope: PathScope::UserSelected }` requirement, and
`file_export_drag_checked(...)` verifies that capability before native drag
handoff; virtual/generated exports do not need filesystem access. Inspect
`item_count()`, `display_names()`, `existing_path_count()`, and
`virtual_file_count()` for generated export previews. Gate the platform
backend with `PlatformFeature::FileExportDrag`. This gives design tools, media
editors, report builders, and AI artifact apps an desktop-app drag-to-desktop
story without depending on browser download behavior.

When the same accepted paths may open a project, documents, media, archives,
and workspace watchers, build a single checked handoff:

```rust
let handoff = cx.workspace_open_handoff_checked(
    WorkspaceOpenHandoffBuilder::paths(intent.paths().iter().cloned())
        .canonicalize_paths()
        .watch_depth(2),
)?;
tracing::info!(summary = handoff.to_text(), "workspace open handoff");

if handoff.needs_unknown_review() {
    return show_unknown_file_review(handoff.intake());
}

if let Some(watch_set) = handoff.watch_set() {
    watcher.watch_set(watch_set.clone())?;
}
```

`WorkspaceOpenHandoffBuilder` is the file/project-app bridge for Desktop-style
path intake: it wraps `FileIntakePlanBuilder`, prepares `FileWatchSetBuilder`
roots from directories and project-file parents, reports
`WorkspaceOpenNextAction`, and keeps summaries path-safe. Use
`workspace_entry_count()`, `watch_root_count()`, `has_watch_set()`,
`needs_unknown_review()`, and `can_open_known_entries()` before mutating
project state or registering watchers.

When the same accepted paths only need routing, classify them once:

```rust
let intake = cx.file_intake_plan_checked(
    FileIntakePlanBuilder::new()
        .paths(intent.paths().iter().cloned())
    .canonicalize_paths(),
)?;

tracing::info!(summary = intake.to_text(), "file intake");

for project in intake.paths_of_kind(FileIntakeKind::Project) {
    open_project(project)?;
}

for media in intake.media_paths() {
    open_player(media)?;
}
```

`FileIntakePlanBuilder` covers the common extension-based branch that Desktop
apps often hand-roll after file dialogs, drops, recent documents, or
file-opening events: directories, project/workspace files, images, audio, video,
PDFs, text, structured data, archives, and unknowns. Add `.reject_unknown()` for
strict importers. The checked `FileIntakePlan` also exposes `entry_count()`,
`kind_count(kind)`, `media_paths()`, `document_paths()`, `project_paths()`,
`archive_paths()`, and `unknown_paths()` so builders and agents can route mixed
selections without duplicating extension tables. Use `to_text()` when logs,
tests, or AI agents need a path-safe summary before local files are opened.

For document apps, declare the file types the app owns as checked metadata:

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
tracing::info!(summary = associations.to_text(), "file associations");
```

This is Kael's bridge for Desktop packaging document-type metadata. It gives
bundlers, installers, docs, and AI agents a validated declaration of supported
extensions and MIME types, while runtime opens still flow through open requests,
recent documents, file dialogs, drops, and file intake. Extensions are
normalized, MIME types are validated, and duplicate claims are rejected before a
generated app ships contradictory metadata. Use association builder/set
`to_text()` for content-safe setup traces that avoid logging association names,
extensions, MIME types, or descriptions.

When runtime file intake has an explicit extension allowlist, compare it with
packaging and default-handler metadata before shipping:

```rust
let intake = FileDropIntentBuilder::open_document().extensions(["kaelproj", "md"]);
let setup = intake.setup_plan_with_default_handler(&associations, &defaults);

tracing::info!(summary = setup.to_text(), "file handling setup");
assert!(setup.is_ready());
```

`FileHandlingSetupPlan` reports extensions accepted by the runtime path that
are missing from checked file associations or optional default-handler claims.
Use the exact missing-extension getters for tests, release tooling, and setup
screens; use `to_text()` for content-safe agent traces because it reports only
counts, path kind, default-handler coverage, and readiness. This closes a
non-WebView Desktop readiness gap: document apps need runtime file intake,
package metadata, and OS default-handler intent to agree, not just a WebView
that can parse a path after launch.

For Desktop `file icon request` parity in file explorers, recent files,
upload pickers, and project launchers, use a checked file-icon request before
calling a platform icon backend:

```rust
let icon = cx.file_icon_request_checked(
    FileIconRequestBuilder::new(project_path)
        .large()
        .require_existing_path(),
)?;
tracing::info!(summary = icon.to_text(), "file icon request");
```

Use `.small()`, `.normal()`, `.large()`, or `.custom_size_px(size)` to request
the desired native icon size. Missing planned paths such as `"Draft.kaelproj"`
are allowed only when generic extension fallback is enabled and an extension
hint is present; concrete user paths can opt into `.require_existing_path()` and
`.canonicalize_path()`. Use `request.to_text()` for path-safe logs, tests, and
AI-agent summaries before handing the request to a platform icon backend.

For Desktop `default protocol registration` and default document-handler
intent, build a checked plan before any OS registration work:

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

tracing::info!(summary = defaults.to_text(), "default handler plan");
```

`DefaultHandlerPlanBuilder::from_package_manifest(&manifest)` seeds the same
runtime/setup intent from checked package metadata. The plan validates app
identity, schemes, document claims, duplicate claims, scope, and user-facing
prompt text, but does not mutate OS defaults by itself. Hand it to installer
code, first-run setup, or platform-specific registry/default-app glue. Use
`to_text()` when native onboarding, release automation, or an agent needs a
single content-safe summary of claimed scheme/document counts, scope, and
confirmation policy before asking the operating system to change defaults,
without logging app IDs, app names, scheme names, extensions, or MIME types.

When a generator needs the broader Desktop-builder style package contract,
compose identity, schemes, and document types into one checked manifest:

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

let icon_coverage = manifest.icons().coverage_summary();
tracing::info!(summary = icon_coverage.to_text(), "package icon coverage");

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
```

`AppPackageManifestBuilder` exports platform-shaped declarations for macOS
bundle URL/document entries, Linux desktop MIME types, and Windows installer
file associations. It also carries checked app, tray, document, and installer
icon declarations so desktop-app `native image`/packaging icon metadata has a
typed home before platform conversion. Use `icons().coverage_summary()` to
audit app, tray, document, and installer coverage before bundling or platform
icon conversion starts. Privacy declarations cover the
packaging-time side of Desktop permission work: camera, microphone, screen
capture, location, notifications, filesystem, network, USB, HID, serial-port,
and Bluetooth intent get validated user-facing reasons and known macOS
usage-description entries where applicable.
Runtime access still goes through Kael's capability broker. That gives
packaging tools and AI agents a stable typed handoff without smuggling installer
metadata through ad hoc strings.

For generated policy setup, start with a checked security handoff that can span
broker installation, process identity, network policy, runtime OS permission
preflight, and hosted-page permission bridge routing:

```rust
let handoff = cx.security_permission_handoff_checked(
    SecurityPermissionHandoffBuilder::new()
        .permission_broker_install(
            ProcessId(42),
            PermissionBrokerInstallBuilder::new()
                .grant(Capability::Network {
                    hosts: vec!["api.example.com".into()],
                })
                .deny_ungranted(),
        )
        .process_context(ProcessContextBuilder::worker(ProcessId(7)))
        .network_policy(NetworkPolicyBuilder::new().allow_host("api.example.com"))
        .hosted_webview_permission("media"),
)?;

tracing::info!(summary = handoff.to_text(), "security handoff");
```

Inspect `SecurityPermissionNextAction` before mutating app state so builders can
separate native broker setup, process-context switching, network allowlists,
capability checks, runtime permission preflight, and explicit WebView permission
bridges. Handoff summaries expose request counts and route shape without
logging capability labels, hosts, paths, permission reasons, process names, or
prompt details.

Configure that runtime broker through the checked install builder at startup:

```rust
let broker = cx.configure_permission_broker_checked(
    PermissionBrokerInstallBuilder::new()
        .grant(Capability::ShellExecute)
        .grant(Capability::Network {
            hosts: vec!["api.example.com".into()],
        })
        .deny_ungranted(),
)?;
tracing::info!(summary = broker.to_text(), "permission broker installed");

assert!(broker.grants(&Capability::ShellExecute));
```

The builder applies the selected `ThreatModel`, registers the current process
class, validates generated direct grants, and swaps the broker only after the
policy is valid. This gives desktop-app desktop features such as shell
handoffs, clipboard reads, notifications, downloads, helper processes, and
plugin/worker actions a single auditable runtime gate. Network grants reject
empty hosts, URL strings, paths, duplicates, and oversized generated host lists.
Use `broker.to_text()` for one stable startup audit line, and use raw
`set_permission_broker(...)` only when the app already constructs and audits its
broker directly.

When an desktop-app helper, worker, plugin host, or test harness needs to run
native actions under a different capability identity, switch with
`ProcessContextBuilder`:

```rust
let context = cx.set_current_process_id_checked(
    ProcessContextBuilder::utility(ProcessId(42)),
)?;
assert_eq!(context.process_class(), ProcessClass::Utility);
tracing::info!(summary = context.to_text(), "process context switched");
```

`ProcessContextBuilder::existing(id)` refuses unregistered ids, while
`worker(...)`, `utility(...)`, `media(...)`, `extension(...)`, and
`register(id, class)` register the process class before switching. The report
lists the previous process id, active id/class, whether registration happened,
and the capabilities visible after the switch. Use `context.to_text()` for one
stable helper/plugin audit line, and use raw `set_current_process_id(...)` only
when the app already owns process registry bookkeeping.

For Desktop `typed IPC host` / `typed IPC client` style helper and extension traffic, keep
messages typed and inspect their envelopes rather than logging payloads. Use
`cx.command_ipc_handoff_checked(CommandIpcHandoffBuilder::register_command(...))`,
`.palette_descriptor(...)`, `.ipc_request(...)`, `.ipc_response(...)`,
`.ipc_progress(...)`, `.ipc_cancel(...)`, `.extension_rpc(...)`, or
`.hosted_bridge(...)` before generated command and IPC routes dispatch, then
inspect `CommandIpcNextAction` to decide whether to register app commands,
publish palette metadata, send IPC, route extension RPC, or use a hosted page
bridge:

```rust
let handoff = cx.command_ipc_handoff_checked(CommandIpcHandoffBuilder::ipc_request(42))?;
tracing::info!(summary = handoff.to_text(), "command ipc handoff");
match handoff.next_action() {
    CommandIpcNextAction::RegisterCommand => {}
    CommandIpcNextAction::PublishPaletteDescriptor => {}
    CommandIpcNextAction::SendIpcRequest => {}
    CommandIpcNextAction::SendIpcResponse => {}
    CommandIpcNextAction::SendIpcProgress => {}
    CommandIpcNextAction::SendIpcCancel => {}
    CommandIpcNextAction::RouteExtensionRpc => {}
    CommandIpcNextAction::UseHostedBridge => {}
}
```

Use handoff `is_*` helpers, typed accessors, and `to_text()` without logging
command ids, labels, categories, shortcuts, icon names, correlation ids,
payloads, bridge message kinds, or error text. For lower-level transport traces,
use
`IpcMessage::to_text()`, `WorkerRequest::to_text()`,
`WorkerResponse::to_text()`, `WorkerProgress::to_text()`,
`WorkerError::to_text()`, `BootstrapMessage::to_text()`,
`frame_summary(frame).to_text()`, and extension RPC summaries such as
`ExtensionRequest::to_text()`, `ExtensionResponse::to_text()`,
`ExtensionNotification::to_text()`, `ExtensionHandshake::to_text()`, and
`ExtensionMessage::to_text()`. These expose message kind, correlation id,
success/error shape, JSON payload class/item count, bootstrap version/counts,
frame completeness, and extension notification/request shape without logging
JSON payloads, command ids, settings keys, panel ids, capability labels, payload
strings, or error messages.

For native geolocation, use a checked request descriptor instead of relying on
browser geolocation from a hidden WebView:

```rust
let location = cx.location_request_checked(
    LocationRequestBuilder::new("Show nearby workspaces.")
        .balanced()
        .timeout(Duration::from_secs(10))
        .maximum_age(Duration::from_secs(300)),
)?;
```

`LocationRequestBuilder` validates purpose text, timeout, cached-location age,
and background/accuracy combinations. Gate execution with
`PlatformFeature::Geolocation`, request `Capability::Location` through the
permission broker, and include `location.privacy_permission()` in packaging
metadata. WebView geolocation permission bridges remain useful for hosted
browser content; app-owned native features should use the native descriptor.

For WebUSB, WebHID, Web Serial, and Web Bluetooth parity, Kael exposes checked
native request descriptors so hardware access is not treated as a WebView-only
feature:

```rust
let device = cx.device_access_request_checked(
    DeviceAccessRequest::hid("Read shortcut events from the editing console.")
        .vendor_product(0x1234, 0xabcd),
)?;
tracing::info!(summary = device.to_text(), "device access request");

let plan = CapabilityReport::current().device_access_plan(&device);
tracing::info!(summary = plan.to_text(), "device access plan");

let handoff = cx.hardware_device_handoff_checked(
    HardwareDeviceHandoffBuilder::new()
        .device_access(device.clone())
        .policy_for_request(&device)
        .hosted_vendor_config("device-setup")
        .native_backend_work(DeviceAccessKind::Hid, "native report stream"),
)?;
tracing::info!(summary = handoff.to_text(), "hardware device handoff");
```

Use `DeviceAccessRequest::usb(...)`, `hid(...)`, `serial(...)`, or
`bluetooth(...)` to declare the app-owned device family, then add the relevant
filter: USB/HID vendor/product ids, serial `port_name_hint(...)`, or Bluetooth
`service_uuid(...)`. Checked builders reject empty/padded/control-character
reasons, zero or longer-than-120-second timeouts, product ids without vendor
ids, invalid Bluetooth UUIDs, and filters that belong to another device family.
Gate execution with `PlatformFeature::UsbDevices`, `HidDevices`, `SerialPorts`,
or `BluetoothDevices`, request `Capability::UsbDevice`, `HidDevice`,
`SerialPort`, or `Bluetooth`, and pass `device.privacy_permission()` into the
package manifest. Inspect `DeviceAccessRequestBuilder::to_text()`,
`DeviceAccessRequest::to_text()`, `has_vendor_id()`, `has_product_id()`,
`has_service_uuid()`, and `has_port_name_hint()` before prompting or invoking
backend IO; the summaries expose device family, filter presence, timeout
presence, and background intent without logging purpose text, vendor/product
IDs, service UUIDs, port hints, or exact timeout values. Use
`CapabilityReport::device_access_plan(&device)` or
`device_access_plan_checked(builder)` to classify the request before a generator
promises hardware support. `DeviceAccessExecutionPlan::next_action()` tells the
builder to open the native path, request permission or metadata, use a guarded
native descriptor, change policy/configuration, or build the missing native
backend; `requires_permission_or_metadata()`,
`requires_guarded_native_descriptor()`, `requires_policy_change()`, and
`requires_native_backend_work()` split that work without hiding privileged
hardware access in browser JavaScript.
`HardwareDeviceHandoffBuilder` groups checked device descriptors, broker
capabilities, privacy declarations, hosted vendor setup, and native backend
work before generated hardware apps prompt users or open privileged IO. Inspect
`HardwareDeviceNextAction` to prepare native descriptors, request broker
capability, add packaging metadata, use a scoped hosted setup page, or queue
missing native discovery/IO work. `HardwareDeviceHandoff::to_text()` reports
request kinds and booleans without logging purpose text, vendor/product ids,
service UUIDs, serial port hints, hosted surface ids, backend reason text, or
exact timeout values.

Run `manifest.readiness_report()` or
`cx.package_readiness_checked(AppPackageReadinessBuilder::new(manifest))` before
emitting installer files. The readiness report catches blocking gaps such as a
missing app version or primary icon, and non-blocking packaging warnings such as
document associations without document icons, extension-only file associations,
or privacy declarations that have no known platform usage-description export.
Use `AppDistributionPlanBuilder` for the Desktop-builder target-list part of
the flow: declare `dmg`, `mac-zip`, `msi`, `nsis`, `appimage`, `deb`, `rpm`, or
`tar-gz` targets, optional release channels, and an absolute artifact output
directory. The plan validates target shape and derives artifact paths from the
checked manifest; platform bundlers still own signing, notarization, and archive
creation.
Pair it with `AppSigningPlanBuilder` when release scripts need Desktop-builder
style signing/notarization intent. The checked plan rejects empty signing sets,
duplicate platforms, invalid identity/team labels, non-macOS notarization or
hardened-runtime flags, and notarization without a macOS signing identity. Call
`signing.covers_distribution_plan(&dist)` before release to catch an unsigned
target before a platform bundler starts.

Generated release scripts should coordinate those pieces through one checked
handoff before claiming desktop-builder or updater parity:

```rust
let release_handoff = cx.packaging_update_handoff_checked(
    PackagingUpdateHandoffBuilder::new()
        .package_readiness(AppPackageReadinessBuilder::new(manifest.clone()))
        .distribution_plan(
            AppDistributionPlanBuilder::new("/tmp/kael-dist")
                .target(AppDistributionTargetBuilder::dmg())
                .target(AppDistributionTargetBuilder::msi()),
        )
        .signing_plan(
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
                )),
        )
        .update_offer(
            AppUpdateOfferPolicyBuilder::stable().cohort_key(install_id),
            AppUpdateReleaseBuilder::new("1.3.0")
                .download_url("https://example.com/downloads/kael-1.3.0.dmg")
                .signed(),
        )
        .update_state(AppUpdateStateBuilder::new(env!("CARGO_PKG_VERSION")))
        .crash_reporter(CrashReporterBuilder::new("com.example.kael"))
        .restart_path(RestartPathBuilder::current_exe()?),
)?;
```

`PackagingUpdateHandoffBuilder` validates package readiness, distribution
targets, signing coverage, update offer policy, update UI state, crash reporter
setup, and restart paths. `PackagingUpdateHandoff::to_text()` reports request
kinds and next action without logging app names, app IDs, versions, artifact
paths, signing identities, release URLs, crash endpoints, restart paths, or
roadmap text.

For WebView bridges or custom platform integrations that need the browser
`DataTransfer` shape, normalize to `ExternalDropData`: it can carry file paths,
plain text, and URLs together, and still exposes `accepted_paths_by(...)` for
the file portion. Use `ExternalDropData::from_drag_value(value)` to normalize an
active drag payload, `ExternalDropData::from_uri_list(...)` for `text/uri-list`
payloads, and `ExternalDropData::from_plain_text(...)` for plain-text drops that
may contain URLs. File-only OS drops still emit
`ExternalPaths` for compatibility; macOS and Windows native text/URL drops and
Linux URI-list drops emit `ExternalDropData` when there is non-file data to
preserve.

Secure credentials should have a keychain-shaped 80% path for auth-heavy apps:

```rust
let handoff = cx.secure_credential_handoff_checked(
    SecureCredentialHandoffBuilder::new(cx.current_process_id())
        .feature_preflight(CapabilityReport::current())
        .permission_broker(PermissionBrokerInstallBuilder::new().deny_ungranted())
        .write(
            CredentialBuilder::new("https://api.example.com")
                .username("ada")
                .password(refresh_token),
        )
        .read(CredentialServiceBuilder::new("https://api.example.com"))
        .delete(CredentialServiceBuilder::new("https://api.example.com"))
        .support_diagnostics(SupportDiagnosticsBuilder::new())
        .hosted_auth_profile("browser-login-profile")
        .roadmap_work("native passkey credential recipe"),
)?;

match handoff.next_action() {
    SecureCredentialNextAction::CheckSecureKeychain => {}
    SecureCredentialNextAction::InstallPermissionBroker => {}
    SecureCredentialNextAction::WriteCredential => {}
    SecureCredentialNextAction::ReadCredential => {}
    SecureCredentialNextAction::DeleteCredential => {}
    SecureCredentialNextAction::ExportSupportDiagnostics => {}
    SecureCredentialNextAction::UseHostedAuthProfile => {}
    SecureCredentialNextAction::TrackRoadmapWork => {}
}

let credential = CredentialBuilder::new("https://api.example.com")
    .username("ada")
    .password(refresh_token);
tracing::info!(
    summary = credential.clone().build()?.to_text(),
    "credential write"
);

cx.write_secure_credential(credential)?.await?;

if let Some(credential) = cx
    .read_secure_credential("https://api.example.com")
    .await?
{
    tracing::info!(summary = credential.to_text(), "credential read");
    println!("credential account: {}", credential.username());
}
```

The wrapper validates service, username, and secret before delegating to the
platform keychain / credential manager, including rejecting accidentally padded
service or username strings. `SecureCredentialHandoffBuilder` is the safer
generated-code route when login/logout flows need keychain support preflight,
permission-broker setup, write/read/delete routing, support diagnostics,
explicit hosted auth fallback, or roadmap work. Its `to_text()` summary reports
request kinds and next action without logging service keys, usernames, token
bytes, hosted profile IDs, or roadmap text. Raw `write_credentials(...)`,
`read_credentials(...)`, and `delete_credentials(...)` remain available for
lower-level integrations.

Permissions now have a grouped startup path for apps that need desktop-app
access to media devices or accessibility automation:

```rust
let permissions = cx.request_permissions(
    PermissionRequestBuilder::capture_studio(),
)?;
tracing::info!(summary = permissions.to_text(), "permission preflight");

let privacy = AppPrivacyManifestBuilder::new()
    .permission(AppPrivacyPermissionBuilder::camera("Camera records video notes."))
    .permission(AppPrivacyPermissionBuilder::microphone("Microphone records voice notes."))
    .permission(AppPrivacyPermissionBuilder::screen_capture("Screen capture shares your workspace."))
    .build_checked()?;
let plan = PermissionRequestBuilder::capture_studio().plan_against_manifest(&privacy)?;
tracing::info!(summary = plan.to_text(), "permission setup plan");

if permissions.has_blocking_denial() {
    if let Some(summary) = permissions.blocking_denial_summary() {
        eprintln!("permissions blocked: {summary}");
    }
    for denial in permissions.blocking_denials() {
        // Use denial.key to route settings guidance or choose a fallback.
    }
}
```

The snapshot reports the current OS status before any prompt is launched.
`capture_studio()` includes accessibility, microphone, camera, and screen-capture
preflight so desktop-app capture/recording tools do not forget to check
desktop capture support separately. Microphone and camera prompts can attach
callbacks with `.microphone_with_callback(...)` and `.camera_with_callback(...)`;
screen capture reports support/permission preflight status here, while platforms
may still show their own picker or OS prompt when sources are queried. Use
`permissions.to_text()` for one stable setup/audit line before enabling capture
features. Pair grouped runtime checks with `AppPrivacyManifestBuilder` and
`PermissionRequestBuilder::plan_against_manifest(...)` so generated apps catch
missing camera, microphone, or screen-capture rationale before presenting OS
prompts. `PermissionPreflightPlan` reports `missing_manifest_permissions()`,
`manifest_complete()`, `requires_manifest_update()`, and `to_text()` for setup
screens and agent audits; accessibility is reported separately because it is an
OS setup flow rather than a privacy-manifest declaration. The raw
single-permission methods remain available for just-in-time prompts.

Power management should also be builder-shaped for media, presentation, capture,
and background-task apps:

```rust
let handoff = cx.power_theme_idle_handoff_checked(
    PowerThemeIdleHandoffBuilder::new()
        .power_save_blocker_builder(
            PowerSaveBlockerBuilder::prevent_display_sleep().reason("video playback"),
        )?
        .power_source_query(SystemPowerSourceQueryBuilder::new().require_known_source())
        .native_theme(cx.native_theme_snapshot())
        .idle_policy_builder(SystemIdlePolicyBuilder::minutes(5).require_known_idle_time())?
        .hosted_power_bridge("player"),
)?;
tracing::info!(summary = handoff.to_text(), "power/theme/idle handoff");

let plan = cx.power_save_blocker_checked(
    PowerSaveBlockerBuilder::prevent_display_sleep()
        .reason("video playback"),
)?;
assert!(plan.prevents_display_sleep());
tracing::info!(summary = plan.to_text(), "power-save blocker plan");

let blocker = cx.start_power_save_blocker_checked(
    PowerSaveBlockerBuilder::prevent_display_sleep()
        .reason("video playback"),
)?;

// Later, when playback or capture ends:
if let Some(blocker) = blocker {
    cx.stop_power_save_blocker_checked(
        PowerSaveBlockerStopBuilder::handle(&blocker).reason("video stopped"),
    )?;
}
```

`PowerThemeIdleHandoffBuilder` validates sleep-prevention plans, stop requests,
power monitor descriptors, power-source queries, native theme snapshots, idle
policies, and explicit hosted power bridge scope before generated apps or
agents mutate system behavior. `to_text()` reports request kinds and next action
without logging blocker reasons, exact idle durations, battery percentages,
power event payloads, theme tokens, hosted IDs, or generated UI values.

The lower-level `start_power_save_blocker(PowerSaveBlockerKind::...)` and
`start_power_save_blocker_with(...)` remain available, and
`PowerSaveBlockerHandle::stop(cx)` remains the concise path when the handle is
still owned locally. The checked start/stop paths validate generated reasons,
expose a side-effect-free `PowerSaveBlockerPlan` with `to_text()` /
`has_reason()` for audit logs that avoid reason text, reject zero stop IDs, and
keep the platform ID, kind, and reason together so generated apps are less
likely to leak a blocker after playback ends.

Adaptive power and accessibility preferences should be monitored through one
runtime snapshot:

```rust
let monitor = cx.watch_system_power_checked(
    SystemPowerMonitorBuilder::new()
        .on_power_mode_changed(|snapshot, _cx| {
            if snapshot.should_reduce_work() {
                // Lower polling, effects, or render quality.
            }
        })
        .on_suspend(|_snapshot, _cx| {
            // Save state.
        })
        .on_resume(|_snapshot, _cx| {
            // Refresh stale data.
        }),
)?;

if monitor.initially_should_reduce_work() {
    // Start in battery/accessibility friendly mode.
}
```

The raw `power_mode()`, `reduce_motion()`, `system_idle_time()`, and
`on_system_power_event(...)` APIs remain available for custom routers. Use
`watch_system_power(...)` for snapshot-only monitors without callbacks. Use
`SystemPowerEvent::to_text()` and `SystemPowerSnapshot::to_text()` for stable
logs that report power mode, reduce-motion, idle telemetry availability, and
reduce-work decisions without exposing exact idle durations.

For desktop-app battery/external-power decisions, capture the source
explicitly:

```rust
let source = cx.system_power_source_snapshot_checked(
    SystemPowerSourceQueryBuilder::new()
        .require_known_source(),
)?;

if source.is_on_battery() || source.should_reduce_work() {
    // Lower polling, effects, sync frequency, or render quality.
}
```

`system_power_source_snapshot()` returns `SystemPowerSource::Unknown` and no
battery percentage on platforms that do not expose this telemetry yet. The
checked query lets generated code require a known source or battery percentage
before it makes product-critical decisions. `SystemPowerSourceQueryBuilder`,
`SystemPowerSource`, and `SystemPowerSourceSnapshot` expose `to_text()` helpers
for battery/external-power audit lines without logging exact battery
percentages.

For Desktop `nativeTheme`-style UI choices, use one native theme snapshot:

```rust
let theme = cx.native_theme_snapshot();
let panel_background = theme.choose(dark_panel, light_panel);

if theme.should_reduce_effects() {
    // Disable decorative blur, motion, or expensive effects.
}
if theme.should_reduce_background_work() {
    // Lower polling, sync, or preview generation.
}
```

`NativeThemeSnapshot` combines the current window appearance, reduce-motion
preference, and power mode, with helpers for dark/light/vibrant appearances and
structured `adaptations()` plus `should_avoid_animation()`,
`should_avoid_blur_or_vibrancy()`, `should_reduce_background_work()`, and
`should_reduce_effects()` decisions for generated UI. Use `theme.to_text()` and
`NativeThemeAdaptation::to_text()` for logs, diagnostics, and AI-agent traces
without recording app-specific palettes, colors, or copy.

For desktop-app "run this when the user has been idle" workflows, use
`SystemIdlePolicyBuilder` instead of repeating duration comparisons:

```rust
let idle = cx.system_idle_evaluation_checked(
    SystemIdlePolicyBuilder::minutes(5)
        .require_known_idle_time(),
)?;

if idle.is_idle() {
    // Run indexing, sync compaction, or expensive preview generation.
}
```

The checked policy rejects zero thresholds and contradictory unknown-idle
behavior. Platforms that cannot report idle time evaluate to `Unknown` by
default; opt into `.treat_unknown_as_idle()` only for work that is safe when idle
telemetry is unavailable. Use `SystemIdlePolicyBuilder::to_text()`,
`SystemIdlePolicy::to_text()`, and `SystemIdleEvaluation::to_text()` when
generated code needs an idle-gate summary without exact activity durations.

Desktop-easy media should be URL in, render instruction out. Use a checked
playback plan when generated code should not decide whether a source belongs in
native video or a browser-backed `<video>` island:

```rust
let render = VideoPlaybackPlanBuilder::url(video_url)
    .content_type(content_type_header)
    .webview_options(WebViewVideoOptions::default().controls(true));
tracing::info!(summary = render.to_text(), "video playback plan builder");
let render = render.build_checked()?.render_instruction();
tracing::info!(summary = render.to_text(), "video render instruction");

match render {
    VideoPlaybackRenderInstruction::Native { controller } => {
        controller.load_metadata()?;
        controller.play()?;
    }
    VideoPlaybackRenderInstruction::WebViewFallback {
        page_url,
        element_id,
        ..
    } => {
        return webview(element_id, page_url).size_full().into_any_element();
    }
}
```

This is the practical bridge for Desktop's forgiving video element: validate
the URL/file/bytes/reader source, account for `Content-Type`, configure browser
fallback controls, and produce either a ready `VideoController` or the WebView
page/id pair needed for HLS, DASH, extensionless CDN URLs, and other browser
media paths.

Hardware media keys and OS media controls should route through the same media
controllers instead of forcing each app to hand-roll a match statement:

```rust
let video = VideoController::url(video_url);

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

Use `playlist.to_text()` and `binding.to_text()` before installation when
builders or agents need a content-safe summary of source counts, repeat state,
controller routing, playlist presence, and callback wiring without logging media
URLs, file paths, source keys, or callback internals.

The builder maps play, pause, play/pause, and stop to either `AudioHandle` or
`VideoController`; next/previous can use `VideoPlaylist` for simple
source-replacement queues, while `on_next_track(...)` and
`on_previous_track(...)` remain available for database-backed queues, analytics,
or custom preload logic. Raw `on_media_key_event(...)` is still available for
custom OS-control routing.

User attention should be just as explicit for background tasks, downloads,
calls, and failed long-running jobs:

```rust
let attention = UserAttentionBuilder::informational()
    .reason("download complete")
    .build_plan_checked()?;
tracing::info!(summary = attention.to_text(), "user attention");
let request = cx.request_user_attention_plan_checked(attention)?;
tracing::info!(summary = request.to_text(), "active attention");

// Cancel when the app becomes active or the condition is resolved.
request.cancel(cx);

cx.cancel_user_attention_checked(UserAttentionCancelBuilder::app_activated())?;
```

`UserAttentionBuilder::critical()` maps to continuous or urgent platform
attention where the OS supports it. `build_plan_checked()` returns a
`UserAttentionPlan` with attention kind, reason, and summary helpers before
dispatch; `UserAttentionRequest` exposes the same style of summary for the
active signal. The checked request and cancel paths reject empty reasons; the raw `request_user_attention(...)`,
`request_user_attention_with(...)`, and `cancel_user_attention()` methods remain
available for custom lifecycle code.

Network status should be a first-class runtime signal for sync, presence,
upload queues, and offline-first apps:

```rust
let monitor = cx.watch_network_status_checked(
    NetworkStatusMonitorBuilder::new()
        .on_offline(|cx| {
            // Pause sync and surface offline state.
        })
        .on_online(|cx| {
            // Resume queued work.
        }),
)?;

if !monitor.initially_online() {
    // Start in offline mode.
}
```

The raw `network_status()` and `on_network_status_change(...)` methods remain
available, and `watch_network_status(...)` remains useful for snapshot-only
monitors without callbacks.
available when an app needs its own router.

Screen, camera, microphone, and system-audio capture should start from the
app-wired manager when builders need Desktop `media capture`-style workflows:

```rust
let manager = cx.capture_manager();
let sources = manager.sources(
    CaptureSourceQueryBuilder::screens_and_windows()
        .name_contains("Display")
        .limit(4),
)?;
tracing::info!(summary = sources.to_text(), "capture sources");

let mut pipeline = manager.start_pipeline_checked(
    CaptureConfigSetBuilder::screen_with_microphone()
        .video_frame_rate(30.0)
        .video_resolution(1920, 1080),
    std::sync::Arc::new(|frame| {
        // Encode, preview, stream, or analyze captured frames.
    }),
)?;

let handoff = cx.capture_handoff_checked(
    CaptureHandoffBuilder::screen_share_with_microphone().auto_start(true),
)?;
tracing::info!(summary = handoff.to_text(), "capture handoff");
if handoff.next_action() == CaptureHandoffNextAction::PreflightPermissions {
    tracing::info!("request capture_studio permissions before source picking");
}
```

The helper registers platform-default backends and copies the app permission
broker/process ID into the capture manager, so screen/camera/microphone capture
still goes through the same capability model as the rest of the app. Use
`CaptureConfigBuilder::{screen, window, camera, microphone, system_audio}()`
for common capture kinds, `.device_name_contains(...)` for a stable user-facing
preference, or `.device_id(...)` after presenting `manager.devices(kind)` in a
custom picker. Use `CaptureConfigSetBuilder::screen_with_microphone()`,
`camera_with_microphone()`, or `screen_with_system_audio()` when an app needs a
coordinated screen share, camera call, or screen-recording setup without wiring
each source by hand. Inspect `CaptureSourceQueryBuilder::to_text()`,
`CaptureSourceCatalog::to_text()`, `CaptureDeviceInfo::to_text()`,
`CaptureConfigBuilder::to_text()`, `CaptureConfig::to_text()`, and
`CaptureConfigSetBuilder::to_text()` in generated flows; summaries expose source
kinds, counts, availability, and option presence without logging device IDs,
device/window names, name filters, or exact resolution values. Use
`manager.pipeline_checked(...)` to resolve configs and own sessions before
starting, or `manager.start_pipeline_checked(...)` for the common branch-free
recorder/call/share path. `CaptureConfig::new(...)`,
`create_session(...)`, and `create_session_with(...)` remain available for
lower-level integrations.
Use `cx.capture_handoff_checked(CaptureHandoffBuilder::...)` for generated
meeting, recorder, and screen-share flows that need a single checked setup
packet. `CaptureHandoffNextAction` separates permission preflight, native source
picker UI, config resolution, and pipeline startup; `CaptureConsentKind` tells
setup UI which microphone, camera, or screen-capture consent surfaces are
involved without logging source IDs, device/window names, filters, or exact
resolution values.
Use `CaptureSourceQueryBuilder` for the Desktop `capture source catalog`
part of the flow before a capture session starts. It can query screens, windows,
or both, include unavailable sources for diagnostics, filter by display/window
name, and limit results for picker UI. The resulting `CaptureSourceCatalog`
keeps source metadata separate from capture constraints; choose a source for UI
or agent policy, then pass the selected ID into `CaptureConfigBuilder`.

Open/save dialogs now have explicit workflow handoffs plus builders over the
existing platform prompt methods:

```rust
let handoff = cx.file_dialog_handoff_checked(
    FileDialogHandoffBuilder::new()
        .open(
            OpenDialogBuilder::files()
                .image_files()
                .filter("Markdown", ["md", "markdown"])
                .prompt("Open"),
        )
        .save(
            SaveDialogBuilder::new(std::env::current_dir()?)
                .suggested_name("document")
                .text(),
        )
        .hosted_file_picker("workspace-picker")
        .roadmap_work("native save-as accessory views"),
)?;
tracing::info!(summary = handoff.to_text(), "file dialog handoff");

let open_plan = cx.open_dialog_checked(
    OpenDialogBuilder::files()
        .image_files()
        .filter("Markdown", ["md", "markdown"])
        .prompt("Open"),
)?;
assert_eq!(open_plan.filter_extension_count(), 9);
tracing::info!(summary = open_plan.to_text(), "open dialog");

let paths = cx
    .show_open_dialog(
        OpenDialogBuilder::files()
            .image_files()
            .filter("Markdown", ["md", "markdown"])
            .prompt("Open"),
    )
    .await??;

let save_plan = cx.save_dialog_checked(
    SaveDialogBuilder::new(std::env::current_dir()?)
        .suggested_name("document")
        .text(),
)?;
tracing::info!(summary = save_plan.to_text(), "save dialog");

let path = cx
    .show_save_dialog(
        SaveDialogBuilder::new(std::env::current_dir()?)
            .suggested_name("document")
            .text(),
    )
    .await??;
```

Open dialogs support desktop-app named extension filters through
`FileDialogFilter` presets such as `.image_files()`, `.audio_files()`,
`.video_files()`, `.pdf_files()`, `.text_files()`, or custom
`.filter("Documents", ["pdf", "docx"])` calls. The builder validates filter
names, extensions, and generated prompt labels before reaching platform code.
Use `FileDialogFilter::to_text()`, `extension_count()`, and
`OpenDialogPlan::filter_extension_count()` when agents need path-safe filter
coverage summaries before opening the native picker.
Save dialogs support default extension helpers with `.default_extension("pdf")`,
`.pdf()`, `.text()`, and `.json()`, appending the extension only when the
suggested name does not already include one. The builder rejects empty
directories, empty or padded suggested names, path separators in suggested
names, and malformed default extensions.
Use `open_dialog_checked(...)` and `save_dialog_checked(...)` when generated
apps, plugin systems, or AI agents need to inspect selection mode, filters,
suggested filenames, default-extension behavior, and required user-selected
filesystem capabilities before showing native UI. Prefer
`file_dialog_handoff_checked(...)` and `FileDialogNextAction` when the generated
workflow may route between native open, native save, raw path prompts, hosted
picker fallback, or tracked roadmap work. Use `handoff.to_text()` and
`plan.to_text()` for path-safe logs and agent summaries. Raw
`PathPromptOptions` remains available for lower-level prompt routing and now
exposes `to_text()` plus filter count helpers so agents do not need to log
prompt labels, filter names, suggested filenames, hosted picker IDs, paths, or
selected values.

Message dialogs now have a builder path for Desktop `native message dialog`,
browser-like alert/confirm flows that belong to the native app, confirmations,
and errors:

```rust
let rx = cx.show_message_dialog(
    MessageDialogBuilder::destructive_confirm("Delete Draft?", "This cannot be undone", "Delete")
        .detail("The draft will be removed from this device.")
)?;

if rx.await? == 1 {
    delete_draft()?;
}

let dialog_plan = cx.message_dialog_checked(
    MessageDialogBuilder::save_discard_cancel(
        "Save changes?",
        "This document has unsaved changes.",
    ),
)?;
tracing::info!(summary = dialog_plan.to_text(), "message dialog");
```

`MessageDialogBuilder::confirm(...)` sets Cancel as the escape/cancel action
and OK as the default action. `destructive_confirm(...)` keeps Cancel as the
default/cancel action while returning the destructive button at index `1`.
Custom button layouts can still set `.default_button(index)` and
`.cancel_button(index)` before calling `show_message_dialog(...)`.
`message_dialog_checked(...)` returns a `MessageDialogPlan` so generated apps,
plugins, and AI agents can inspect button order, default/cancel labels,
`button_index(...)`, and returned indexes before native UI is shown. Use
`dialog_plan.to_text()` for content-safe logs and agent summaries.
`MessageDialogBuilder::info(...)`, `.warning(...)`, `.error(...)`,
`.confirm(...)`, `.destructive_confirm(...)`, and
`.save_discard_cancel(...)` cover the common Desktop prompt recipes before an
app reaches for browser `alert`, `confirm`, `prompt`, `beforeunload`, or form
validation in a WebView island. `show_about_dialog_checked(...)` covers about
windows without reimplementing them in hosted HTML. `MessageDialogBuilder::to_text()`
and `DialogOptions::to_text()` provide the same label-safe summary for custom
lower-level dialog dispatch.

Session restore should persist both window state and app-specific workspace
state instead of forcing every app to invent a JSON file alongside window
geometry:

```rust
let store = SessionStore::new("my-app")?;

store.save_snapshot(
    &SessionSnapshotBuilder::new()
        .window_state("main", main_window.window_state())
        .app_data(serde_json::json!({
            "workspace": workspace_id,
            "sidebar": "files",
        }))?
        .build(),
)?;

let displays = cx.displays().iter().map(|display| display.id()).collect::<Vec<_>>();
let primary = cx.primary_display().map(|display| display.id());
let restored_windows = store.restore_window_states(&displays, primary)?;
let snapshot = store.load_snapshot()?;
```

Use `SessionSnapshotBuilder` when restoring desktop-app workspaces, tabs,
sidebar state, recent project ids, or panel layout metadata alongside window
bounds. `save_window_states(...)` and `load_window_states(...)` remain available
for geometry-only apps.
Inspect `SessionSnapshotBuilder::to_text()` before saving generated session
state and `SessionSnapshot::to_text()` after loading it so agents can report
window counts, display-bound/fullscreen counts, app-data presence, app-data JSON
shape, and coarse bounds state without logging window ids, workspace ids, file
paths, tab names, tokens, arbitrary JSON payloads, display ids, or exact bounds.
Use `restore_window_states_with_summary(...)` when monitor changes matter; its
`SessionRestoreResult::to_text()` reports restored window count, disconnected
display relocation count, available display count, primary-display fallback
presence, and coarse bounds state without leaking app-specific window ids.

Native menus now have template-style builders over the existing `Menu` and
`MenuItem` tree, plus a workflow handoff for generated app-menu/context-menu
routing:

```rust
let handoff = cx.menu_command_handoff_checked(
    MenuCommandHandoffBuilder::new()
        .menu_bar(
            MenuBarBuilder::new()
                .menu(MenuBuilder::new("File").action("Open...", menu_action::Open))
                .menu(MenuBuilder::standard_edit(
                    "Edit",
                    menu_action::Undo,
                    menu_action::Redo,
                    menu_action::Cut,
                    menu_action::Copy,
                    menu_action::Paste,
                    menu_action::SelectAll,
                )),
        )
        .context_menu(NativeContextMenuBuilder::new().action("Open", "open"))
        .edit_command_snapshot()
        .hosted_context_menu("editor-surface")
        .roadmap_work("native role menu parity"),
)?;
tracing::info!(summary = handoff.to_text(), "menu command handoff");

cx.set_menus_checked(
    MenuBarBuilder::new()
        .menu(
            MenuBuilder::new("File")
                .action("Open...", menu_action::Open)
                .separator()
                .action("Quit", menu_action::Quit),
        )
        .menu(MenuBuilder::new("Edit").action("Copy", menu_action::Copy)),
)?;

let menu_plan = cx.menu_bar_checked(
    MenuBarBuilder::new()
        .menu(MenuBuilder::new("File").action("Open...", menu_action::Open))
        .menu(MenuBuilder::standard_edit(
            "Edit",
            menu_action::Undo,
            menu_action::Redo,
            menu_action::Cut,
            menu_action::Copy,
            menu_action::Paste,
            menu_action::SelectAll,
        )),
)?;
tracing::info!(summary = menu_plan.to_text(), "menu bar");
```

The checked path rejects empty labels, accidentally padded labels, empty menus,
and duplicate top-level menu names before installing native menus.
`menu_bar_checked(...)` returns a `MenuBarPlan` so generated apps, plugin
systems, and AI agents can inspect top-level menu names, item/action counts,
native Edit role usage, and system-menu usage before mutating the live menu bar.
Prefer `menu_command_handoff_checked(...)` and `MenuCommandNextAction` when a
generated workflow may choose between installing a menu bar, showing a context
menu, snapshotting edit commands, delegating to hosted context-menu semantics,
or tracking missing menu parity. Use `MenuCommandHandoff::to_text()`,
`MenuBuilder::to_text()`, `MenuBarBuilder::to_text()`, and
`MenuBarPlan::to_text()` for content-safe audit lines that do not log menu
labels, action IDs, hosted surface IDs, roadmap text, or edit labels.

For desktop-app Edit role enablement, snapshot the active focused edit state:

```rust
let edit = cx.edit_command_state_snapshot_checked()?;
let undo_enabled = edit.can_undo();
let undo_label = edit.undo_label().unwrap_or("Undo");
```

This keeps native Edit menus, toolbar buttons, and command palettes in sync with
the focused element without each caller separately probing `has_undo`,
`has_redo`, and labels. The unchecked snapshot falls back to disabled Undo/Redo
when there is no active window.

For desktop-app dashboards, admin panels, file managers, logs, and
spreadsheet-like workspaces, use native `kael_ui::Table`, `DataTable`, and
`DataGrid` before reaching for a WebView table. `ColumnDef::to_text()`,
`DataTableState::to_text()`, `DataTable::to_text()`, and `RowAction::to_text()`
plus `GridColumnDef::to_text()`, `DataGridState::to_text()`, and
`DataGrid::to_text()` expose column counts, row counts, virtual vs in-memory
backing, cached virtual rows, page size, sort state, selection count, editable
columns, active edit buffers, search presence/length, edit handlers, row
actions, load-more/fetch-page wiring, sticky headers, and context menu presence
without logging column ids, headers, row values, row-action labels, edit text,
search queries, dimensions, pointer coordinates, or callback internals. This
lets generated apps prove they are using a native high-performance data surface
instead of defaulting to browser tables for every grid.

For desktop-app dialogs, sheets, custom menus, context menus, command
palettes, omniboxes, and plugin action pickers, use the native
`kael_ui::Dialog`, `Sheet`, `BottomSheet`, `Menu`, `ContextMenu`, `MenuBar`,
`CommandPalette`, and `CommandPaletteState` instead of DOM overlays.
`Dialog::to_text()`, `DialogHeader::to_text()`, `DialogPosition::to_text()`,
`Sheet::to_text()`, `BottomSheet::to_text()`, `MenuItem::to_text()`,
`Menu::to_text()`, `ContextMenu::to_text()`, `MenuBar::to_text()`,
`Command::to_text()`, and `CommandPaletteState::to_text()` expose
size/purpose/dismissal policy, header/content/footer presence, item totals,
nesting, disabled state, separator/submenu/action counts, shortcut coverage,
filtered result counts, query presence/length, selection state, recent count,
category coverage, and executable-handler coverage without logging
command/menu ids, labels, titles, descriptions, categories, shortcut strings,
pointer coordinates, dimensions, child contents, the active query, or callback
internals. That gives generated apps a safe audit surface for overlay, palette,
and custom-menu readiness while keeping user/project command text out of logs.
When command metadata is managed outside `kael_ui`, use
`PaletteCommandId::to_text()`, `CommandDescriptor::to_text()`, and
`CommandPalette::to_text()` for desktop-app command-palette catalogs. For
plugin-provided commands, menus, panels, and extension managers, use
`PluginManifest::to_text()`, `Contributions::to_text()`,
`ContributedCommand::to_text()`, `ContributedMenuItem::to_text()`,
`ContributedPanel::to_text()`, `ExtensionInfo::to_text()`,
`ExtensionHost::to_text()`, `ExtensionManifest::to_text()`,
`ExtensionDiagnostics::to_text()`, and `ExtensionRegistry::to_text()` so agents
can inspect contribution counts, active/loaded state, execution model, high-risk
capability count, health, and error counts without logging extension IDs, names,
entry paths, command IDs, labels, keybindings, author text, activation events,
or error messages.

For native document, canvas, note, editor, and design-tool state that lives
outside a hosted web editor, use Kael's `UndoRedoManager` instead of depending
on DOM editing history:

```rust
history.begin_transaction_checked("move selected layers")?;
history.push(move_layer_change);
history.push(update_selection_change);
history.end_transaction_checked()?;

tracing::info!(summary = history.to_text(), "undo redo");
```

This gives desktop-app Undo/Redo affordances to native app surfaces:
grouped operations, bounded retained history, source-targeted undo/redo, and
safe state inspection for menus, command palettes, autosave prompts, and AI
agents. `history.to_text()` reports undo count, redo count, total retained
history depth, max-depth pressure, open transaction state, and open transaction
change count without logging generated descriptions or document content. Use
`undo_count()`, `redo_count()`, `transaction_change_count()`, and
`is_at_max_depth()` when generated UI needs to explain or adapt history state.

desktop-app command palettes, menus, plugin contributions, and agent action
lists should register stable command IDs through the app-level checked path:

```rust
cx.register_command_checked("editor.save", "Save", || {
    save_current_document();
})?;
```

Checked command registration validates generated IDs and names, rejects
duplicates, and leaves the existing registry untouched on error. Raw
`register_command(...)` remains available when intentionally replacing a command.

desktop-app app accelerators should use the checked keybinding builder when
shortcut strings or context predicates come from generated config, plugins, or
AI agents:

```rust
let keymap_plan = cx.key_bindings_checked(
    KeyBindingSetBuilder::new()
        .binding("secondary-k", command::OpenPalette)
        .binding_with_context(
            "secondary-shift-f",
            command::FormatDocument,
            Some("Editor && mode == normal"),
        ),
)?;

cx.bind_keys_checked(
    KeyBindingSetBuilder::new()
        .binding("secondary-k", command::OpenPalette)
        .binding_with_context(
            "secondary-shift-f",
            command::FormatDocument,
            Some("Editor && mode == normal"),
        ),
)?;

cx.clear_key_bindings_checked(KeyBindingClearBuilder::extension_unloaded("vim-mode"))?;
```

The builder parses accelerator strings and key contexts before installing
anything, rejects duplicate bindings, and leaves the existing keymap untouched
on error. Inspect builders with `to_text()`, `context_count()`,
`has_contexts()`, `platform_binding_count()`, and `has_platform_bindings()`
before parsing or installing generated keymaps. `key_bindings_checked(...)`
returns a `KeyBindingSetPlan` with normalized keystrokes, action names, context
counts, and `to_text()` so generated preferences, command palettes, and plugin
keymaps can preview local shortcuts before mutating live input handling.
Checked keymap clears reject invalid generated reasons before removing
live shortcuts. Raw `bind_keys(...)`, `clear_key_bindings()`, and
`KeyBinding::new(...)` are still available for code that already validates its
shortcut table.

App globals are the native process-wide service/singleton layer for plugin
state, app services, theme bridges, and generated runtime configuration:

```rust
let removed = cx.remove_global_checked::<MyPluginState>(
    GlobalRemovalBuilder::extension_unloaded("vim-mode"),
)?;
```

Checked global removal validates the generated cleanup reason and returns
`Ok(None)` when the state is already absent, unless `.require_present()` is set.
Raw `remove_global::<T>()` remains available when missing state should panic.

Native workspace lifecycle has a checked close path for editor, IDE, and
project-oriented apps:

```rust
let closed = cx.close_workspace_checked(
    WorkspaceCloseBuilder::session_teardown("window closed"),
)?;
```

The checked close path validates generated reasons, reports whether a workspace
was actually closed, and supports `.require_open()` when missing workspace state
should be treated as an error. Raw `open_workspace()` and `close_workspace()`
remain available for hand-managed lifecycle code.

Deep links now have a grouped route builder for app startup:

```rust
Application::new()
    .deep_links_checked({
        let routes = DeepLinkRouterBuilder::new()
            .route("myapp", |url, cx| {
                println!("app link: {url}");
            })
            .route("oauth", |url, cx| {
                println!("oauth callback: {url}");
            });
        tracing::info!(summary = routes.to_text(), "deep-link routes");
        routes
    })?
    .run(|cx| {
        let schemes = UrlSchemeRegistrationBuilder::new()
            .scheme("myapp")
            .scheme("oauth");
        let setup = DeepLinkRouterBuilder::new()
            .route("myapp", |_, _| {})
            .route("oauth", |_, _| {})
            .setup_plan(&schemes)?;
        tracing::info!(summary = setup.to_text(), "deep-link setup");
        tracing::info!(summary = schemes.to_text(), "URL scheme registration");
        let tasks = cx.register_url_schemes(schemes).expect("valid URL schemes");

        for task in tasks {
            task.detach_and_log_err(cx);
        }

        // launch app
    });
```

Use checked grouped routes when handlers are generated from configuration; they
validate scheme syntax and reject duplicate route schemes. Use
`UrlSchemeRegistrationBuilder` when registering multiple custom schemes; it
validates scheme syntax and deduplicates repeated entries before calling the
platform registration API. Use `routes.to_text()` and `schemes.to_text()` for
startup, plugin, and AI-agent summaries before handlers or OS scheme
registration tasks are installed. Use `routes.setup_plan(&schemes)` or
`routes.setup_plan_with_default_handler(&schemes, &defaults)` when an app also
has a default-handler setup flow, so generated apps catch routes without
registration, default-handler schemes without runtime handlers, and registered
schemes with no handler. `DeepLinkSetupPlan::to_text()` reports counts and gap
status without logging scheme names. Use `OpenRequest::to_text()` inside
`.on_open_request(...)` or `.on_open_requests(...)` for content-safe launch
traces that classify file/deep-link/external/unknown handoffs without logging
raw URLs, local paths, or scheme names. For plugin or agent-installed runtime
handlers, use `cx.register_deep_link_handler_checked(handler)` so invalid
schemes fail before the handler registry changes; use
`cx.dispatch_deep_link_checked(url)` when manual dispatch should reject external
URLs or malformed strings before handlers run.

Before the launch/open event mutates native routes, shell URLs, document intake,
or hosted browser history, classify it with an open-request route plan:

```rust
let plan = OpenRequestRoutePlanBuilder::new()
    .request("myapp://settings/profile")
    .registered_deep_link_scheme("myapp")
    .native_route("settings/profile")
    .hosted_navigation_bridge("docs")
    .allow_external_urls()
    .build_checked()?;

tracing::info!(summary = plan.to_text(), "open request route plan");

match plan.next_action() {
    OpenRequestRouteNextAction::OpenFilePaths => {}
    OpenRequestRouteNextAction::DispatchDeepLink => {}
    OpenRequestRouteNextAction::OpenExternalUrl => {}
    OpenRequestRouteNextAction::PushNativeRoute => {}
    OpenRequestRouteNextAction::UseHostedNavigationBridge => {}
    OpenRequestRouteNextAction::ReviewUnknownRequest => {}
}
```

`OpenRequestRoutePlanBuilder` is the native-first replacement for assuming every
startup/open payload is browser `location` state. It validates registered
deep-link schemes, native route ids, hosted navigation bridge ids, external URL
policy, and file routing before generated code dispatches side effects. Its
summary reports counts and routing shape without logging raw URLs, file paths,
scheme names, route ids, or hosted bridge ids.

Custom app protocols cover the other Desktop pattern: serving app-owned
resources such as `app://assets/logo.svg`, internal previews, or generated
documents without leaking raw filesystem paths into UI code.

```rust
let app = Application::new();
app.custom_protocols_checked(
    CustomProtocolRouterBuilder::new()
        .route("app", |request, cx| {
            CustomProtocolResponse::text(format!("path: {}", request.path()))
        }),
)?;
app.run(|cx| {
    if let Some(response) = cx
        .handle_custom_protocol_url("app://assets/readme.txt")
        .expect("valid custom protocol URL")
    {
        println!("served {} bytes", response.body.len());
    }
});
```

The checked router rejects duplicate routes and standard-scheme shadowing
(`http`, `https`, `file`, `data`, `javascript`, etc.). Protocol requests expose
typed `scheme`, `host`, `path`, and `query` fields, and responses validate
status, MIME type, and headers before they are returned. Agents can log
`request.to_text()`, `response.to_text()`, and `router.to_text()` to describe
custom protocol work without exposing raw URLs, schemes, hosts, paths, query
strings, headers, MIME values, or body bytes.

For the common Desktop `protocol.handle("app", ...)` pattern that serves
packaged files, use a checked file resolver instead of manually joining URL
paths:

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

The resolver maps `app://assets/...` URLs to files below one root, returns `404`
for missing files or host mismatches, infers common MIME types, and rejects
plain or percent-encoded `..` traversal before reading. Existing files are
canonicalized against the resolver root, so symlink escapes are rejected too.
`CustomProtocolFileResolverBuilder::to_text()` and resolver summaries report
only booleans and counts, so build logs do not reveal root paths, accepted
hosts, index filenames, cache header values, or served file contents.

Single-instance startup now has a named launch result instead of repeating the
same `match` in every app:

```rust
match SingleInstanceBuilder::new("com.example.app").launch()? {
    SingleInstanceLaunch::Primary(instance) => {
        instance.on_activate(Box::new(|| {
            // Focus or reopen the main window.
        }));
    }
    SingleInstanceLaunch::Duplicate { notified, .. } => {
        debug_assert!(notified);
        return Ok(());
    }
}
```

Use the raw `SingleInstance::acquire(...)` and `send_activate_to_existing(...)`
helpers when an app needs custom duplicate-process forwarding.

Shell helpers should use explicit verbs instead of making builders remember
which low-level platform method maps to which OS behavior:

```rust
cx.open_external_url("https://example.com/docs")?;
cx.open_path(project_dir)?;
cx.show_item_in_folder(report_path)?;
cx.open_shell_target(ShellTarget::reveal_path(report_path))?;

cx.open_shell_targets(
    ShellTargetsBuilder::new()
        .url("https://example.com/docs/export")
        .reveal_path(report_path)
        .require_existing_paths(),
)?;

let shell_plan = cx.shell_targets_checked(
    ShellTargetsBuilder::new()
        .url("https://example.com/docs/export")
        .reveal_path(report_path)
        .require_existing_paths(),
)?;
tracing::info!(summary = shell_plan.to_text(), "shell targets");

let trash = cx.trash_request_checked(TrashRequest::builder(report_path).canonicalize_path())?;
tracing::info!(summary = trash.to_text(), "trash request");

let trashed = cx.trash_item_checked(TrashRequest::builder(report_path).canonicalize_path())?;
tracing::info!(summary = trashed.to_text(), "trash request");
```

`open_external_url(...)` uses the lower-risk URL capability, while
`open_path(...)`, `show_item_in_folder(...)`, and path/reveal batch targets
require `ShellExecute`. `ShellTargetsBuilder` keeps export/open/reveal workflows
ordered and validated without hand-written loops. It rejects empty or padded
URLs, unsupported shell URL schemes, missing HTTP(S) hosts, empty paths, and NUL
characters. `shell_targets_checked(...)` returns a `ShellTargetsPlan` so apps
and agents can preview the ordered targets and required URL/shell capabilities
before any browser, file, or reveal side effect occurs. Use builder or plan
`to_text()` for content-safe traces that classify URL/path/reveal targets without
logging URLs or local paths. Use `.canonicalize_paths()` when generated open/reveal
targets should be normalized before dispatch. Custom application schemes belong in
`DeepLinkRouterBuilder` / `UrlSchemeRegistrationBuilder`, not accidental shell
execution.
For Desktop `shell.trashItem(...)` parity, `TrashRequestBuilder` creates a
checked move-to-trash descriptor that rejects empty paths, NUL bytes, filesystem
roots, relative paths unless opted in, and missing targets by default. It is the
capability-checked preview for the native trash/recycle backend rather than a
permanent delete operation. Inspect `display_name()`, `parent_path()`,
`requires_shell_execute()`, and content-safe `to_text()` before dispatch when
generated UI, logs, or tests need to explain the side effect without logging the
target path or filename. Use `trash_item_checked(...)` after confirmation to
validate, capability-check, and dispatch the request to the platform
trash/recycle hook. macOS, Windows, and Linux currently provide native
trash/recycle dispatch; unsupported platform backends return an explicit error
until their native hook is implemented.

App storage should use checked path roles instead of hard-coded platform
directory guesses:

```rust
let paths = cx.app_paths_checked(
    AppPathBuilder::new("com.example.app")
        .all_common()
        .create_dirs(),
)?;
tracing::info!(summary = paths.to_text(), "app paths");

let settings = paths.config_dir().unwrap().join("settings.json");
let cache_dir = paths.cache_dir().unwrap();
let log_dir = paths.logs_dir().unwrap();
let downloads = paths.downloads_dir().unwrap();
```

This covers the practical `app path lookup` surface Desktop apps rely on for
user data, config, cache, logs, temp files, and downloads. `AppPathBuilder`
validates the app id, rejects duplicate roles, scopes app-owned paths by id, and
can create missing directories before migrations, logging, background downloads,
or plugin storage start. Use `AppPathBuilder::to_text()` before resolution and
`AppPathSet::to_text()` after resolution to report requested/resolved role
coverage without logging app ids or absolute paths.

For the storage that Desktop apps often leave inside Chromium localStorage,
IndexedDB, or ad hoc profile folders, declare a native app storage plan:

```rust
let storage = cx.app_storage_plan_checked(
    AppStoragePlanBuilder::new("com.example.app")
        .settings_json("settings", "settings.json")
        .sqlite_database("main-db", "state/app.sqlite")
        .blob_cache("previews", "previews")
        .entry(AppStorageEntryBuilder::key_value_store("tokens", "tokens").sensitive()),
)?;
tracing::info!(summary = storage.to_text(), "app storage plan");

storage.ensure_directories_checked()?;
```

`AppStoragePlanBuilder` is not a database engine; it is the checked contract
for where durable settings, SQLite state, key-value data, rebuildable blobs,
logs, and temporary workspaces belong. It resolves the required app path roles,
rejects duplicate ids, unsafe relative paths, parent-directory escapes,
absolute paths, invalid custom kinds, `Downloads` as a storage base, and invalid
quota values. Each entry exposes durability, optional byte budget, sensitivity,
absolute path, `required_directory()`, and `read_capability()` /
`write_capability()` values for worker or plugin permission wiring. The plan
also exposes `required_directories()` for preflight UI and
`ensure_directories_checked()` so generated apps can prepare settings, SQLite,
cache, log, and temp directories before storage engines open. That gives
builders and agents a native storage map instead of assuming a browser profile
exists. Use `AppStoragePlanBuilder::to_text()`, `AppStoragePlan::to_text()`,
and `AppStorageEntry::to_text()` for generated traces; they summarize storage
classes, durability, role usage, sensitivity, quota presence, and directory
counts without logging entry ids, relative paths, absolute paths, custom kind
strings, app ids, or quota sizes.

For AI-generated setup flows, route all storage/session variants through one
checked handoff before opening storage or a hosted browser profile:

```rust
let handoff = cx.app_storage_session_handoff_checked(
    AppStorageSessionHandoffBuilder::storage_plan(
        AppStoragePlanBuilder::new("com.example.app")
            .settings_json("settings", "settings.json")
            .sqlite_database("main-db", "state/app.sqlite")
            .blob_cache("previews", "previews"),
    ),
)?;

tracing::info!(summary = handoff.to_text(), "app storage session handoff");

match handoff.next_action() {
    AppStorageSessionNextAction::ResolveAppPaths => {}
    AppStorageSessionNextAction::PrepareStoragePlan => {}
    AppStorageSessionNextAction::RunStorageMigration => {}
    AppStorageSessionNextAction::RunStorageCleanup => {}
    AppStorageSessionNextAction::UseHostedProfileStorage => {}
}
```

Use `cx.app_storage_session_handoff_checked(...)` with
`AppStorageSessionHandoffBuilder::app_paths(...)`, `.storage_plan(...)`,
`.migration(...)`, `.cleanup(...)`, or `.hosted_profile_storage(...)` to
distinguish native app paths, storage maps, migration jobs, cleanup jobs, and
persistent WebView profile storage. Inspect `is_paths()`,
`is_storage_plan()`, `is_migration()`, `is_cleanup()`,
`is_hosted_profile_storage()`, typed builder accessors, and `to_text()` for
agent routing without logging app ids, paths, entry ids, profile ids, cookies,
tokens, or stored values.

For Electron-style browser profile state, add a browser-profile storage bridge
before deciding whether the data belongs in native storage, secure credentials,
or a hosted WebView profile:

```rust
let bridge = BrowserProfileStorageBridgePlanBuilder::new("com.example.app")
    .native_storage_plan(
        AppStoragePlanBuilder::new("com.example.app")
            .settings_json("settings", "settings.json")
            .sqlite_database("workspace-index", "state/index.sqlite"),
    )
    .cleanup_plan(StorageCleanupPlanBuilder::new("com.example.app").cache(
        "http-cache",
        "webview-http-cache",
    ))
    .hosted_profile_id("auth-profile")
    .local_storage_to_native("settings")
    .indexed_db_to_native("workspace-index")
    .cookies_to_secure_credentials("refresh-cookie")
    .auth_session_hosted("oauth-session")
    .http_cache_cleanup("cache-reset")
    .build_checked()?;

tracing::info!(summary = bridge.to_text(), "browser profile storage bridge");

match bridge.next_action() {
    BrowserProfileStorageNextAction::PrepareNativeStorage => {}
    BrowserProfileStorageNextAction::PrepareSecureCredentials => {}
    BrowserProfileStorageNextAction::UseHostedProfileStorage => {}
    BrowserProfileStorageNextAction::ClearHostedStorage => {}
    BrowserProfileStorageNextAction::UseBrowserStorageIsland => {}
    BrowserProfileStorageNextAction::TrackRoadmapWork => {}
}
```

This is the native-first replacement for treating Chromium's profile directory
as the app database. `BrowserProfileStorageBridgePlanBuilder` classifies
localStorage, sessionStorage, IndexedDB, cookies, CacheStorage, HTTP cache,
service workers, auth sessions, drafts, and custom browser-profile state, then
requires the matching native storage plan, hosted profile id, cleanup plan, or
sensitive credential marking before generated code continues. Its summaries
avoid app ids, profile ids, item ids, origins, keys, cookie names, paths, and
values.

Launch context should be explicit and safe for startup routing:

```rust
let launch = cx.launch_context_checked(
    LaunchContextBuilder::new()
        .environment_keys(["APP_CHANNEL", "KAEL_PROFILE"])
        .require_executable()
        .require_current_dir(),
)?;

let args = launch.args();
let channel = launch.env("APP_CHANNEL");
```

For generated startup flows, wrap the full route before opening windows or
forwarding duplicate launches:

```rust
let startup_handoff = cx.launch_environment_handoff_checked(
    LaunchEnvironmentHandoffBuilder::new()
        .launch_context(
            LaunchContextBuilder::new()
                .environment_keys(["APP_CHANNEL", "KAEL_PROFILE"])
                .require_executable()
                .require_current_dir(),
        )
        .argument_policy(
            LaunchArgumentPolicyBuilder::new()
                .allow_file_paths()
                .url_scheme("kael")
                .flag("--safe-mode"),
        )
        .environment_allowlist(
            LaunchEnvironmentAllowlistBuilder::new()
                .keys(["APP_CHANNEL", "KAEL_PROFILE"]),
        )
        .hosted_startup_state("browser-startup-state")
        .roadmap_work("cross-platform startup-source normalization"),
)?;

match startup_handoff.next_action() {
    LaunchEnvironmentNextAction::CaptureLaunchContext => {}
    LaunchEnvironmentNextAction::ValidateArgumentPolicy => {}
    LaunchEnvironmentNextAction::ValidateEnvironmentPolicy => {}
    LaunchEnvironmentNextAction::RouteDuplicateLaunch => {}
    LaunchEnvironmentNextAction::RecordStartupDiagnostics => {}
    LaunchEnvironmentNextAction::UseHostedStartupState => {}
    LaunchEnvironmentNextAction::TrackRoadmapWork => {}
}
```

`LaunchEnvironmentHandoff::to_text()` reports request kinds and next action
without logging launch arguments, URLs, paths, environment keys or values,
duplicate payloads, hosted state IDs, or roadmap text.

This gives generated apps an native startup argument / process-environment
equivalent without exposing the entire environment by default. Arguments are
captured as UTF-8-lossy strings, environment variables require an explicit
allowlist, duplicate or malformed keys fail early, and apps can require
executable/current-directory resolution when startup routing depends on them.

For Desktop `utility process`, `helper process`, and plugin-host workflows,
validate the whole helper/plugin contract before touching a platform supervisor:

```rust
let handoff = cx.helper_plugin_handoff_checked(
    HelperPluginHandoffBuilder::new()
        .launch_builder(HelperProcessLaunch::plugin_host(
            ProcessId(90),
            "extension-host",
            helper_path,
        ))
        .plugin_manifest(manifest)
        .plugin_permissions(permission_manifest, granted_permissions)
        .ipc_schema(IpcSchema::new(2, 1, vec!["plugin.ping".into()]))
        .crash_policy(CrashPolicy::default()),
)?;

tracing::info!(summary = handoff.to_text(), "helper plugin handoff");
```

Inspect `HelperPluginNextAction` to route plugin contract setup, broker/context
installation, supervisor policy, and final spawn as separate steps.
`cx.helper_plugin_handoff_checked(...)` with `HelperPluginHandoffBuilder`
rejects invalid launches, missing required plugin
permission grants, malformed or duplicate IPC message types, invalid crash
policy, and oversized generated handoff batches. Use handoff summaries without
logging plugin ids, helper names, paths, argv, env keys or values, capability
labels, IPC message names, crash ids, or raw errors.

For single helper launches, describe a checked helper launch before spawning:

```rust
let launch = HelperProcessLaunch::ffmpeg_transcoder(
    ProcessId(42),
    cx.auxiliary_executable_checked(
        AuxiliaryExecutableBuilder::new("transcoder").require_existing_file(),
    )?
    .into_path(),
)
.arg("--input")
.arg(input_path.display().to_string())
.env("RUST_LOG", "info")
.inherit_environment_keys(["PATH"])
.build_checked()?;

tracing::info!(summary = launch.to_text(), "helper process launch");
let (info, options) = launch.into_spawn_parts();
supervisor.spawn_with_options(info, options)?;
```

`HelperProcessLaunchBuilder` is not a shell-string API. Resolve helpers with
`AuxiliaryExecutableBuilder` first so generated code cannot smuggle a path-like
name into bundle lookup; checked lookup rejects empty, padded, path-like,
control-character, and overlong names, and can require an existing file. The
launch builder then validates process class, name, executable, args, explicit
env vars, inherited env allowlists, working directory, declared capability
labels, and restart/heartbeat policy. Use `ProcessClass::Utility` for app-owned
native tools that are not UI, media, extension, or long-running worker hosts.
Use `HelperProcessLaunch::ffmpeg_transcoder(...)`,
`HelperProcessLaunch::language_server(...)`, and
`HelperProcessLaunch::plugin_host(...)` as presets for common Desktop escape
hatches. They return normal `HelperProcessLaunchBuilder` values with process
class, restart, heartbeat, and capability defaults already set; callers can
still add args, environment, working directories, and stricter executable checks
before `build_checked()`. Inspect `HelperProcessProfile::key()`,
`HelperProcessLaunchBuilder::to_text()`, and `HelperProcessLaunch::to_text()`
for profile, class, arg/env/inherited-env/capability counts, working-directory
presence, restart policy, and heartbeat presence without logging helper names,
paths, args, env keys/values, or capability labels.
This gives builders an Desktop-like escape hatch for FFmpeg wrappers, language
servers, importers, exporters, and
model tools while preserving Kael's native process and permission boundaries.

For terminal panes in IDE-like apps, declare a checked terminal session before
opening a platform PTY:

```rust
let terminal = TerminalSessionRequest::builder(ProcessId(50), "Project Shell", "/bin/zsh")
    .arg("-l")
    .working_dir(project_root)
    .inherit_environment_keys(["PATH", "HOME", "LANG"])
    .size(120, 32)
    .scrollback_lines(20_000)
    .login_shell()
    .build_checked()?;

tracing::info!(summary = terminal.to_text(), "terminal session");
```

This covers the Desktop terminal/dev-tool expectation without smuggling shell
strings through WebView code. `TerminalSessionRequestBuilder` validates shell
paths, argv, explicit env, inherited env keys, working directory, dimensions,
scrollback, and login-shell intent; summaries expose only counts and shape, not
session names, shell paths, command text, project paths, env keys/values, or
scrollback contents. The actual PTY reader/writer remains the native backend
that consumes this descriptor.

Locale snapshots cover Desktop `locale snapshot` and preferred-language style
startup choices without using browser APIs:

```rust
let locale = cx.locale_snapshot_checked(
    LocaleSnapshotBuilder::new()
        .preferred_languages(["fr-FR", "en-US"])
)?;

let language = locale.language();
let rtl = locale.is_rtl();
```

The builder normalizes explicit candidates and system signals (`LC_ALL`,
`LC_MESSAGES`, `LANG`, `LANGUAGE`) into BCP-47-style tags, strips encoding and
modifier suffixes, infers region and text direction, and falls back to `en-US`
when the OS exposes only `C`/`POSIX` or no locale data.

Browser text fields also make spelling policy feel automatic in Desktop. For
native Kael editors and forms, create a checked text-checking descriptor before
calling an OS or bundled dictionary backend:

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

`TextCheckingRequestBuilder` validates text, locale, enabled features, custom
dictionary words, duplicates, and suggestion limits. Gate richer integrations
with `PlatformFeature::SpellChecking`; when it is partial or unavailable, keep
typing usable and omit underline/suggestion UI rather than routing through a
hidden browser field.

Desktop apps also inherit browser and OS keyboard behavior for shortcut labels,
hotkey settings, and command palettes. Kael exposes the active native layout as
a checked snapshot:

```rust
let layout = cx.keyboard_layout_snapshot_checked(
    KeyboardLayoutSnapshotBuilder::new().require_known_layout(),
)?;

let shortcut_region = layout.name();
let uses_equivalents = layout.has_key_equivalents();
```

`cx.keyboard_layout_snapshot()` is best-effort and tolerates `unknown` layouts
for tests, headless agents, and unsupported desktops. Use
`.require_known_layout()` when a preferences UI must display concrete
layout-aware shortcut labels. The checked path rejects malformed layout ids and
names before they reach generated menus or settings screens.

Runtime diagnostics should expose current native process cost without requiring
an embedded browser process model:

```rust
let metrics = cx.current_process_metrics();

tracing::info!(
    pid = metrics.process_id(),
    windows = metrics.window_count(),
    rss = ?metrics.resident_set_bytes(),
    uptime_ms = metrics.uptime().as_millis(),
    "desktop resource snapshot"
);
```

This gives builders and agents an native starting
point for resource audits: process id, uptime, open Kael window count,
executable/current-directory paths, and best-effort memory values. Memory is
reported as optional because each OS exposes low-cost process data differently;
agents should check `metrics.memory().is_supported()` before making hard budget
assertions.

For the "lighter than Desktop" promise, use checked resource budgets instead
of informal log inspection:

```rust
let budget = cx.evaluate_resource_budget_checked(
    AppResourceBudgetBuilder::new()
        .max_resident_set_bytes(256 * 1024 * 1024)
        .max_windows(4)
        .require_memory_metrics()
        .warn_when_power_constrained(),
)?;

if !budget.is_within_budget() {
    tracing::warn!(summary = budget.summary(), "resource budget exceeded");
    if budget.has_memory_pressure() {
        shed_caches();
    }
    if budget.has_window_pressure() {
        defer_extra_windows();
    }
}
```

This gives generated apps and agents a structured runtime gate over current
process metrics plus `runtime_snapshot()`: memory thresholds, window-count
limits, optional uptime limits, required memory-metric availability, and
power/accessibility pressure warnings. Inspect `issue_count()`, `issue_kinds()`,
`has_issue(kind)`, `has_memory_pressure()`, `has_window_pressure()`, and
`has_power_pressure()` to route mitigation without parsing summary text. It does
not replace benchmark evidence, but it gives each app a cheap guardrail before
expensive work, release checks, or AI-driven changes.

Support diagnostics bundle these native pieces into a privacy-aware support
report for "copy diagnostics", issue templates, and automated bug reports:

For generated debugger panels and AI-agent evidence collection, start with a
checked observability handoff that spans trace sessions, runtime snapshots,
resource budgets, support diagnostics, and explicit WebView console/DevTools
bridges:

```rust
let handoff = DeveloperObservabilityHandoffBuilder::new()
    .trace_session(
        TraceSessionBuilder::new("agent-audit")
            .runtime()
            .network()
            .ipc()
            .max_events(512),
    )
    .runtime_snapshot(AppRuntimeSnapshotQueryBuilder::new().require_not_quitting())
    .resource_budget(AppResourceBudgetBuilder::new().max_windows(4))
    .support_diagnostics(SupportDiagnosticsBuilder::new())
    .hosted_console_bridge("webview-console")
    .build_checked()?;

tracing::info!(summary = handoff.to_text(), "developer observability handoff");
```

Inspect `DeveloperObservabilityNextAction` before collecting evidence or
opening debug surfaces. The handoff keeps native trace/runtime/resource/support
evidence separate from hosted-page inspection and rejects unbounded traces, empty
resource budgets, unsafe support diagnostics, contradictory runtime queries, and
malformed hosted bridge ids.

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

By default the report includes OS info, locale, current-process metrics,
executable path, current directory, and no argv or environment values. Apps must
opt into `.include_launch_args()` and `.environment_keys([...])`; app paths are
side-effect free and reject `.create_dirs()` so diagnostics cannot mutate the
user's filesystem.

App identity should also be a typed object, not scattered strings in menus,
support pages, and diagnostics:

```rust
let metadata = AppMetadataBuilder::new("Kael Studio")
    .version(env!("CARGO_PKG_VERSION"))
    .build(option_env!("GIT_SHA").unwrap_or("dev"))
    .identifier("com.example.kael-studio")
    .website_url("https://example.com")
    .support_url("https://example.com/support")
    .license("Apache-2.0");

let metadata_summary = metadata.clone().build_checked()?.summary();
tracing::info!(summary = metadata_summary.to_text(), "app metadata");
cx.show_about_dialog_checked(metadata)?;
```

This covers the practical Desktop app-name/version/About-panel workflow for
generated native apps. `AppMetadataBuilder` validates display names,
version/build labels, identifiers, HTTP(S) support links, copyright, license,
and credits. `AppMetadata::about_dialog()` lets apps route the same validated
metadata through custom menu actions or native message dialogs. `summary()`
reports recommended version, identifier, and support URL coverage for About
dialogs, diagnostics, support screens, and agents.

When identity work spans packaging, URL schemes, file associations, default
handlers, icons, window grouping, and badges, prefer a checked handoff instead
of letting agents emit those pieces independently:

```rust
let manifest = AppPackageManifestBuilder::new(
    AppMetadataBuilder::new("Kael Studio")
        .identifier("com.example.kael-studio")
        .version(env!("CARGO_PKG_VERSION")),
)
.url_schemes(UrlSchemeRegistrationBuilder::new().scheme("kael"))
.file_associations(
    FileAssociationSetBuilder::new().association(
        FileAssociationBuilder::new("Kael Project")
            .extension("kaelproj")
            .mime_type("application/x-kael-project")
            .editor(),
    ),
)
.icons(AppIconSetBuilder::new().icon(AppIconAssetBuilder::app("assets/app.icns")));

let checked_manifest = manifest.clone().build_checked()?;

let handoff = cx.app_identity_metadata_handoff_checked(
    AppIdentityMetadataHandoffBuilder::new()
        .metadata(
            AppMetadataBuilder::new("Kael Studio")
                .identifier("com.example.kael-studio")
                .version(env!("CARGO_PKG_VERSION")),
        )
        .package_manifest(manifest)
        .package_readiness(AppPackageReadinessBuilder::new(checked_manifest))
        .default_handler(
            DefaultHandlerPlanBuilder::new("com.example.kael-studio").scheme("kael"),
        )
        .window_app_id(WindowAppIdBuilder::new("com.example.kael-studio"))
        .dock_badge(DockBadgeBuilder::count(3))
        .roadmap_work("localized about/legal templates"),
)?;

match handoff.next_action() {
    AppIdentityMetadataNextAction::BuildPackageManifest => {}
    AppIdentityMetadataNextAction::EvaluatePackageReadiness => {}
    AppIdentityMetadataNextAction::PrepareDefaultHandlerRegistration => {}
    AppIdentityMetadataNextAction::ApplyWindowGrouping => {}
    AppIdentityMetadataNextAction::UpdateDockBadge => {}
    AppIdentityMetadataNextAction::SurfaceMetadata => {}
    AppIdentityMetadataNextAction::TrackRoadmapWork => {}
}
```

`AppIdentityMetadataHandoff::to_text()` reports request counts, coverage, kinds,
and the next action without logging app names, identifiers, URLs, schemes,
extensions, MIME types, icon paths, badge labels, or roadmap text. That gives
builders one native-first coordination point for Electron-style app identity
without pretending platform registration, store listings, or localized legal
templates are already automatic.

Update UI should have a checked state model even before an app wires a native
installer or custom feed backend:

```rust
let update = cx.app_update_state_checked(
    AppUpdateStateBuilder::new(env!("CARGO_PKG_VERSION"))
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

let menu_label = update.menu_label();
let action = update.recommended_action();
let summary = update.summary();
tracing::info!(summary = summary.to_text(), "update state");

let decision = cx.app_update_offer_checked(
    AppUpdateOfferPolicyBuilder::stable().cohort_key(machine_install_id),
    AppUpdateReleaseBuilder::new("1.3.0")
        .channel(AppUpdateChannel::Stable)
        .download_url("https://example.com/downloads/kael-studio-1.3.zip")
        .signed()
        .rollout_percentage(25),
)?;
```

This is the honest Desktop `updater` bridge layer today: Kael validates the
state that menus, notifications, settings rows, and agents consume, but it does
not claim to provide a cross-platform installer backend yet. Available,
downloading, downloaded, and ready-to-install phases require release metadata;
download progress is valid only while downloading; failed states require a
sanitized error message; URLs must be HTTP(S). `summary()` gives generated UI,
tests, and agents one inspected value for phase, recommended action, menu label,
release version, progress, and error state.
Use `AppUpdateOfferPolicyBuilder` for the release-eligibility part that would
otherwise become custom updater glue. It checks channel match, rollout
percentage against an explicit bucket or stable cohort key, whether a download
URL is required, and whether a release must be signed before the UI offers it.
Decisions are `Offer`, `Defer`, or `Block`, so agents can distinguish "not for
this channel/cohort yet" from "do not install this release." The `.signed()`
flag is an assertion from the feed/package verifier, not a replacement for
signature verification.

Recent documents now have a builder path over the existing dock/jump-list
integration:

```rust
let recent_documents = RecentDocumentsBuilder::new()
    .require_existing_files()
    .canonicalize()
    .document(report_path)
    .document(notes_path);
tracing::info!(summary = recent_documents.to_text(), "recent documents");
let recent_plan = cx.recent_documents_checked(recent_documents.clone())?;
tracing::info!(summary = recent_plan.to_text(), "recent documents");
cx.add_recent_documents(recent_documents).expect("recent document paths");

let clear_recents = RecentDocumentsClearBuilder::new("User cleared recent files");
tracing::info!(summary = clear_recents.to_text(), "clear recent documents");
cx.clear_recent_documents_checked(clear_recents)?;
```

The lower-level `add_recent_document(path)` remains available for one-off
updates, and raw `clear_recent_documents()` remains available for already
validated integrations. The checked builders keep startup, file-open, privacy,
and reset flows easier for generated apps to compose: additions can require and
canonicalize real files, and `cx.recent_documents_checked(...)` returns a
`RecentDocumentsPlan` with `documents()`, `document_count()`, and content-safe
`to_text()` before OS state changes. Builder `to_text()` previews configured
counts before path resolution. Clearing requires a validated reason before
persistent OS recent-document state is removed; use clear-request `to_text()` /
`has_reason()`
without logging the reason text. Omit `.require_existing_files()` /
`.canonicalize()` when you want the permissive raw platform behavior.

File watching has checked options for desktop-app project folders, config
files, themes, generated assets, and logs:

```rust
let watch_options = FileWatchOptionsBuilder::new().max_depth(3);
tracing::info!(summary = watch_options.to_text(), "file watch options");

let watch_options = cx.file_watch_options_checked(watch_options)?;
let watch_set = cx.file_watch_set_checked(
    FileWatchSetBuilder::new()
        .paths([project_dir, config_file, log_dir])
        .options(watch_options.clone()),
)?;
tracing::info!(summary = watch_set.to_text(), "file watch set");

watcher.watch_set(watch_set)?;
watcher.watch_with_options(
    project_dir,
    watch_options,
)?;
```

Use `.recursive()` for all descendants, `.max_depth(depth)` for bounded project
watchers, and `.non_recursive()` for single files or direct children. The
checked path rejects zero-depth watches, raw depth limits without recursion,
empty watch sets, empty paths, missing paths, and duplicate canonical roots
before a platform watcher is registered. Prefer
`cx.file_watch_options_checked(...)` and `cx.file_watch_set_checked(...)` for
AI-generated project, config, theme, asset, and log watcher setup. Use
`FileWatchSetBuilder::to_text()` / `FileWatchSet::to_text()` before and after
grouped registration, and `FileWatchEvent::to_text()` inside callbacks, so
generated apps can log watcher counts, recursive/depth-limit settings, event
kind, removal state, and error presence without exposing project paths or
platform error messages. Raw `FileWatchOptions { ... }` and
`watch(path, recursive)` remain available for low-level integrations.

App lifecycle policy now has one checked startup path for desktop-app
`window-all-closed`, background app, and bounded cleanup behavior:

```rust
let lifecycle = cx.configure_lifecycle_policy_checked(
    AppLifecyclePolicyBuilder::new()
        .keep_alive_without_windows()
        .quit_cleanup_timeout(Duration::from_millis(500))
        .reason("tray sync stays active"),
)?;
tracing::info!(summary = lifecycle.to_text(), "app lifecycle policy");
```

Use `.quit_when_all_windows_close()` for normal document or utility apps and
`.keep_alive_without_windows()` for tray, menubar, sync, or agent apps. The
builder applies the platform keep-alive state and the timeout used for
`on_app_quit(...)` cleanup futures together, rejecting zero or longer-than-30s
cleanup windows and invalid diagnostic reasons before lifecycle policy becomes
ambiguous. Raw `set_keep_alive_without_windows(...)`, `on_app_quit(...)`,
`on_app_restart(...)`, and `on_window_closed(...)` remain available for custom
integrations. Use `policy.to_text()` or the builder summary in generated traces
to report behavior, keep-alive state, cleanup timeout, and reason presence
without logging diagnostic reason text.

App activation and terminal commands also have a checked route for Desktop
`app.focus(...)`, `app.hide()`, `app.quit()`, and relaunch-style flows:

```rust
let activate = AppLifecycleCommand::activate_with_options(true)
    .reason("show existing project window");
let activate_plan = cx.lifecycle_command_plan_checked(activate.clone())?;
tracing::info!(summary = activate_plan.to_text(), "lifecycle command plan");
if activate_plan.is_ready() {
    cx.perform_lifecycle_command_checked(activate)?;
}

let quit = AppLifecycleCommand::quit("user selected Quit");
let quit_plan = cx.lifecycle_command_plan_checked(quit.clone())?;
tracing::info!(summary = quit_plan.to_text(), "lifecycle command plan");
cx.perform_lifecycle_command_checked(quit)?;
```

`AppLifecycleCommand::quit(reason)` and `.restart(reason)` require explicit
validated reasons before dispatch, while focus/hide commands may attach optional
diagnostic reasons. Use `lifecycle_command_plan_checked(...)` to evaluate the
command against current runtime state before mutating app lifecycle: the plan
reports command kind, terminal-ness, reason presence, window count,
background-runtime state, shutdown-in-progress state, restart-path presence, and
readiness. This is useful for generated app menus, tray apps, updater prompts,
and agents that need to avoid dispatching lifecycle work after shutdown has
already begun. Use `command.to_text()` and `plan.to_text()` without logging the
reason itself. Raw `activate(...)`, `hide()`, `hide_other_apps()`,
`unhide_other_apps()`, `quit()`, and `restart()` remain available for already
validated integrations.

For generated startup, activation, duplicate-launch, login-item, and
recent-document flows, use one checked handoff before opening windows or
mutating OS state:

```rust
let handoff = cx.app_lifecycle_startup_handoff_checked(
    AppLifecycleStartupHandoffBuilder::command(
        AppLifecycleCommand::activate_with_options(true).reason("show existing project window"),
    ),
)?;

tracing::info!(summary = handoff.to_text(), "app lifecycle startup handoff");

match handoff.next_action() {
    AppLifecycleStartupNextAction::ConfigureLifecyclePolicy => {}
    AppLifecycleStartupNextAction::PlanLifecycleCommand => {}
    AppLifecycleStartupNextAction::RouteDuplicateLaunch => {}
    AppLifecycleStartupNextAction::ConfigureAutoLaunch => {}
    AppLifecycleStartupNextAction::AddRecentDocuments => {}
    AppLifecycleStartupNextAction::ClearRecentDocuments => {}
}
```

Use `cx.app_lifecycle_startup_handoff_checked(...)` with
`AppLifecycleStartupHandoffBuilder::policy(...)`, `.command(...)`,
`.duplicate_launch(...)`, `.auto_launch(...)`, `.recent_documents(...)`, or
`.clear_recent_documents(...)` to classify app readiness, focus, quit/restart,
second-instance routing, login-item preferences, and recent-document changes in
one redacted object. Inspect `is_policy()`, `is_command()`,
`is_duplicate_launch()`, `is_auto_launch()`, `is_recent_documents()`,
`is_clear_recent_documents()`, typed request accessors, and `to_text()` without
logging app ids, launch args, environment values, executable paths, current
directories, document paths, duplicate payloads, or reason text.

For Desktop `app.isReady()`-adjacent startup checks and agent audits, read a
checked runtime snapshot instead of probing unrelated platform APIs:

```rust
let runtime = cx.runtime_snapshot_checked(
    AppRuntimeSnapshotQueryBuilder::new()
        .require_not_quitting()
        .require_network_online()
        .allow_background_runtime(),
)?;

if runtime.is_background_runtime() {
    tracing::info!("tray or agent runtime is active");
}

if runtime.power().should_reduce_work() {
    defer_nonessential_indexing();
}
```

`AppRuntimeSnapshot` includes the capability process id, uptime, window count,
keep-alive policy, quit-cleanup timeout, quitting flag, network status, system
power snapshot, and native theme snapshot. `AppRuntimeSnapshotQueryBuilder` can
require not-quitting state, a foreground window, a background/tray runtime, or
online network status before generated startup work begins. Pair it with
`CapabilityReport::current()`: capability reports say what the desktop can do;
runtime snapshots say what this app process is doing now.

Display queries now have a checked Desktop `screen`-style path for palettes,
launchers, inspectors, capture tools, and generated window placement:

```rust
let cursor_display = cx
    .query_displays_checked(DisplayQueryBuilder::cursor().fallback_to_primary())?
    .first()
    .cloned();

let all_displays = cx.query_displays_checked(DisplayQueryBuilder::all())?;
let topology = all_displays.topology_summary();
tracing::info!(summary = topology.to_text(), "display topology");
```

`DisplaySnapshot` copies display id, optional stable UUID, bounds, default window
bounds, scale factor, refresh rate, primary-display state, and cursor containment
into a plain value. Use `scale_factor()` for Desktop `screen` /
`deviceScaleFactor` parity: crisp canvas/media backing stores, screenshot/capture
sizing, and HiDPI-aware generated layouts. Use `topology_summary()` for
single-vs-multiple-display layout, primary presence, cursor matching, virtual
bounds, max scale, max refresh rate, high-DPI displays, and high-refresh display
decisions without ad hoc monitor search code. Queries can target all displays,
the primary display, the cursor-containing display, or a specific display id,
with explicit empty-result or primary-fallback behavior.

Window progress has a checked path over the existing taskbar/dock progress
hook:

```rust
window.set_progress_bar_checked(WindowProgressBuilder::normal_percent(55))?;
window.set_progress_bar_checked(WindowProgressBuilder::indeterminate())?;
window.set_progress_bar_checked(WindowProgressBuilder::none())?;
```

Use `WindowProgressBuilder::normal(...)`, `error(...)`, and `paused(...)` for
fractional progress, or the `*_percent(...)` helpers for transfer progress,
export jobs, installers, or sync state. The checked path rejects NaN, infinity,
and values outside `0.0..=1.0`; the lower-level `window.set_progress_bar(...)`
and raw `ProgressBarState` remain available for already-validated
platform-specific state. Use `kind()`, `is_determinate()`, `is_clear()`, and
`to_text()` on builders or raw states when generated jobs need content-safe
progress traces without logging exact fractions.

Native window opacity has a checked path for Desktop `window management.setOpacity(...)`
style translucent palettes, HUDs, inspectors, media controllers, and overlays:

```rust
window.set_opacity_checked(WindowOpacityBuilder::fraction(0.86))?;
window.set_opacity_checked(WindowOpacityBuilder::opaque())?;
```

The checked builder rejects NaN, infinity, and fractions outside `0.0..=1.0`
before platform APIs run. macOS, Windows, and X11 apply native window opacity;
Wayland keeps the same API surface but treats the request as a compositor-limited
no-op until a backend-specific opacity protocol is available. Use
`is_opaque()`, `is_translucent()`, and `to_text()` on builders or checked values
for content-safe traces that avoid logging exact fractions. Raw
`window.set_opacity(...)` remains available for platform-owned flows.

Runtime window content resizing has a checked path for Desktop
`window management.setContentSize(...)` / `setSize(...)` style generated layouts:

```rust
window.resize_checked(WindowContentSizeBuilder::new(size(px(960.0), px(640.0))))?;
window.resize_checked(WindowContentSizeBuilder::dimensions(px(420.0), px(300.0)))?;
window.set_rem_size_checked(WindowRemSizeBuilder::new(px(18.0)))?;
```

The checked builder rejects non-finite, zero, negative, and excessively large
content dimensions before platform resize APIs run. Raw `window.resize(...)`
remains available when the app already owns sizing constraints or platform-
specific geometry policy. Use `is_landscape()`, `is_portrait()`, `is_square()`,
and `to_text()` on builders or checked requests for content-safe resize traces
that avoid logging exact window dimensions.
Use `WindowRemSizeBuilder` for desktop-app app zoom/accessibility density
controls that scale native `rem`-based UI; it rejects non-finite, tiny, and
excessively large base sizes before the whole window layout changes. Use
`size_class()` and `to_text()` for density traces that avoid logging exact scale
values. Raw `window.set_rem_size(...)` remains available for hand-validated
integrations.
Use `request_autoscroll_checked(WindowAutoscrollRequestBuilder::new(bounds))`
from prepaint code for generated drag, selection, editor, canvas, and design
tool surfaces; checked requests reject non-finite coordinates, negative sizes,
and excessively large bounds before scroll containers react. Use `is_empty()`
and `to_text()` for autoscroll traces that avoid logging coordinates or region
sizes. Raw `window.request_autoscroll(...)` remains available for hand-validated
element code.
Use `set_window_cursor_style_checked(WindowCursorStyleCommand::new(style, reason))`
for whole-window cursor overrides in generated canvas, drawing, drag, and resize
surfaces; checked commands require a valid diagnostic reason before overriding
element cursor styles. Use `has_reason()` and `to_text()` for cursor traces that
confirm intent without logging the reason text. Raw
`window.set_window_cursor_style(...)` remains available for hand-validated
element code.

Runtime always-on-top behavior has a checked path for Desktop
`window management.setAlwaysOnTop(...)` style mini-players, call windows, inspectors,
and tool palettes:

```rust
window.set_z_order_policy_checked(
    WindowZOrderPolicyBuilder::always_on_top("Keep call controls visible"),
)?;
window.set_z_order_policy_checked(WindowZOrderPolicyBuilder::normal())?;
```

Enabling always-on-top requires a validated reason, which helps generated
overlay and utility windows avoid trapping users behind accidental z-order
changes. macOS, Windows, and X11 apply native topmost/above state; Wayland keeps
the same API surface as a no-op unless the window was created through a
compositor-specific overlay path. Raw `window.set_always_on_top(...)` remains
available for custom window managers.

For desktop-app `window management` state checks before a menu, shortcut, media,
or agent command acts, capture one runtime snapshot:

```rust
let snapshot = window.runtime_snapshot_checked(
    WindowRuntimeSnapshotQueryBuilder::new()
        .require_visible()
        .require_display(),
)?;

if snapshot.is_visible() && !snapshot.is_fullscreen() {
    window.perform_window_interaction_checked(WindowInteractionCommand::enter_fullscreen())?;
}
```

`WindowRuntimeSnapshot` gathers bounds, persistable `WindowBounds`, viewport
size, display id, scale factor, appearance, active/hovered/visible state,
fullscreen, maximized, power mode, and reduce-motion state. The checked query
lets generated chrome require visibility, focus, or a known display before
issuing commands.

Window visibility, close, focus, minimize, zoom/maximize, fullscreen, and click-through overlay behavior
now have a checked command path over Desktop `window management.show()`,
`.hide()`, `.close()`, `.focus()`, `.minimize()`, and
maximize/zoom plus `setFullScreen(...)` / `setIgnoreMouseEvents(...)`:

```rust
window.perform_window_interaction_checked(WindowInteractionCommand::show())?;
window.perform_window_interaction_checked(WindowInteractionCommand::activate())?;
window.perform_window_interaction_checked(WindowInteractionCommand::zoom_window())?;
window.perform_window_interaction_checked(
    WindowInteractionCommand::close("User confirmed project window close"),
)?;
window.perform_window_interaction_checked(
    WindowInteractionCommand::enter_fullscreen().reason("Preview fullscreen"),
)?;
window.perform_window_interaction_checked(WindowInteractionCommand::exit_fullscreen())?;
window.perform_window_interaction_checked(WindowInteractionCommand::toggle_fullscreen())?;
window.perform_window_interaction_checked(
    WindowInteractionCommand::mouse_passthrough("HUD overlay should not block clicks"),
)?;
window.perform_window_interaction_checked(WindowInteractionCommand::receive_mouse_events())?;
```

The checked path rejects invalid diagnostic text and requires a reason before
requesting close or enabling mouse pass-through. Close requests go through the
platform lifecycle and existing `on_window_should_close(...)` hooks, while the
mouse pass-through guard keeps generated overlay windows from accidentally
becoming unclickable. Use the fullscreen interaction commands for ordinary menu,
shortcut, preview, and media controls; use `WindowPresentationPolicyBuilder`
when fullscreen carries presentation or kiosk intent. Raw `show_window()`,
`hide_window()`, `close_window()`, `activate_window()`, `minimize_window()`,
`is_window_visible()`, `toggle_fullscreen()`, and `set_mouse_passthrough(...)`
remain available for custom window managers.

For mostly-static native UIs, preference screens, dashboards, and document
surfaces without live external video/WebView surfaces, use a checked render
policy to avoid redundant full-frame GPU work:

```rust
window.set_render_policy_checked(
    WindowRenderPolicyBuilder::frame_skip("Static settings panel"),
)?;
window.set_render_policy_checked(WindowRenderPolicyBuilder::no_frame_skip())?;
```

This exposes a native performance lever toward the "lighter than Desktop"
goal without making agents guess when it is appropriate. Enabling frame skipping
requires a validated reason; raw `set_frame_skip_enabled(...)` remains
available for renderer-owned policy.

For high-performance native UIs that keep many documents, icons, glyphs,
thumbnails, or sprites warm, use a checked atlas budget to keep renderer memory
bounded:

```rust
window.set_atlas_byte_budget_checked(
    WindowAtlasBudgetBuilder::bytes(128 * 1024 * 1024)
        .reason("Large editor view churns text and symbol atlases"),
)?;
window.set_atlas_byte_budget_checked(WindowAtlasBudgetBuilder::clear())?;
```

This gives builders an Desktop-alternative memory lever that is native to the
renderer rather than a browser process setting. The checked builder rejects
zero-byte caps, excessively large caps, and invalid diagnostic text; raw
`set_atlas_byte_budget(...)` remains available for platform-owned memory policy.

For frameless/custom-titlebar windows, use checked chrome commands around the
native compositor hooks:

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
window.set_client_inset_checked(WindowClientInsetBuilder::new(px(8.0)))?;
```

This is the native counterpart to Desktop frameless-window drag regions and
system-menu affordances. The checked command rejects invalid diagnostic text and
non-finite menu positions; use `key()`, `has_reason()`, and `to_text()` for
custom chrome traces that avoid logging menu coordinates or reason text; raw
`request_decorations(...)`,
`show_window_menu(...)`, `start_window_move()`, and `start_window_resize(...)`
remain available for already-owned custom chrome.
Use `set_client_inset_checked(WindowClientInsetBuilder::new(inset))` for
client-side decoration resize borders; checked insets reject NaN, infinite,
negative, and excessively large values before compositor state changes. Use
`is_zero()` and `to_text()` for inset traces that avoid logging exact pixel
values.

Native system-UI commands now have a checked path for editor and custom-titlebar
affordances that Desktop apps commonly wire through DOM buttons or menu items:

```rust
window.perform_window_system_ui_command_checked(
    WindowSystemUiCommand::show_character_palette().reason("Editor emoji and symbol picker"),
)?;
window.perform_window_system_ui_command_checked(WindowSystemUiCommand::titlebar_double_click())?;
window.perform_window_system_ui_command_checked(WindowSystemUiCommand::zoom_window())?;
```

Use this for emoji/symbol pickers, generated custom titlebars that should honor
platform double-click behavior, and zoom/maximize controls. The checked command
validates optional diagnostics; raw `show_character_palette()`,
`titlebar_double_click()`, and `zoom_window()` remain available for custom native
window integrations.

Dock/taskbar badges now have a checked builder path for counts and short status
labels:

```rust
let badge = DockBadgeBuilder::count(7);
tracing::info!(summary = badge.to_text(), "dock badge");
cx.set_dock_badge_checked(badge)?;
let status_badge = DockBadgeBuilder::label("sync");
tracing::info!(summary = status_badge.to_text(), "dock badge");
cx.set_dock_badge_checked(status_badge)?;
cx.set_dock_badge_checked(DockBadgeBuilder::clear())?;
```

Use this for unread counts, sync/export status, and generated app chrome where
badge text may come from dynamic state. The checked path rejects empty labels,
padded labels, control characters, and labels longer than 16 characters before
platform badge rendering. Use `to_text()`, `has_label()`, `is_clear()`, and
`is_count()` for content-safe badge traces that avoid logging badge text or
count values. Raw `cx.set_dock_badge(Some(label))` and
`cx.set_dock_badge(None)` remain available for already-validated platform state.

Dock/taskbar menu installation and dispatch also have checked paths:

```rust
cx.set_dock_menu_checked(
    DockMenuBuilder::new()
        .action("Show Window", menu_action::ShowWindow)
        .separator()
        .action("Quit", menu_action::Quit),
)?;
cx.perform_dock_menu_action_checked(DockMenuActionBuilder::new(0))?;
```

Use `DockMenuBuilder` to reject empty generated menus, separator-only menus,
invalid labels, and empty submenu trees before native installation. Use
`DockMenuActionBuilder` when generated Windows jump-list glue or tests need to
dispatch an installed dock/taskbar action by index; the checked path rejects
missing menus, out-of-range indexes, and unsupported platforms before the raw OS
message path is used.

Windows jump lists have a builder path for task actions and recent workspace
groups:

```rust
let jump_list = JumpListBuilder::new()
    .action("Open Project", menu_action::Open)
    .workspace_path(project_dir)
    .workspace([project_dir, workspace_file]);
tracing::info!(summary = jump_list.to_text(), "jump list");
let jump_list_plan = cx.jump_list_checked(jump_list)?;
tracing::info!(summary = jump_list_plan.to_text(), "jump list");
cx.update_jump_list_plan_checked(jump_list_plan);
```

Use `JumpListBuilder` for desktop-app taskbar launchers, recent projects,
and multi-folder workspaces. `cx.jump_list_checked(...)` returns a `JumpListPlan` with
task, workspace, workspace-path, canonicalization, and existence-policy summary
helpers before OS state changes. Builder and plan `to_text()` summaries avoid
logging action labels or workspace paths. The checked path rejects empty jump lists,
non-action task menu items, padded/empty action labels, empty workspace entries,
and empty paths; `.require_existing_paths().canonicalize()` is available when a
launcher should only expose real projects. The lower-level
`cx.update_jump_list(menus, entries)` remains available for custom Windows
integrations.

For Desktop `app.requestSingleInstanceLock()` startup behavior, acquire the
lock before opening windows and log the checked launch outcome:

```rust
let launch = SingleInstanceBuilder::new("com.example.myapp").launch()?;
tracing::info!(summary = launch.to_text(), "single-instance launch");

match launch {
    SingleInstanceLaunch::Primary(instance) => {
        instance.on_activate(Box::new(|| { /* focus the existing window */ }));
    }
    SingleInstanceLaunch::Duplicate { .. } => return Ok(()),
}
```

`SingleInstanceBuilder` validates app IDs before platform lock names are
created. `SingleInstanceLaunch::to_text()` gives startup logs, telemetry, and
agents one stable summary of whether this process became primary or forwarded
activation to an existing process.

Launch-at-login now follows the same pattern:

```rust
let auto_launch = AutoLaunchBuilder::enable("com.example.myapp");
let plan = cx.auto_launch_plan_checked(auto_launch.clone())?;
tracing::info!(summary = plan.to_text(), "auto launch plan");
let state = cx.configure_auto_launch(auto_launch)?;

println!("auto launch enabled: {}", state.enabled());
println!("{}", state.to_text());
```

Use `AutoLaunchBuilder::disable(app_id)` for preferences screens. Builder
validation rejects empty app IDs and whitespace/control characters before
platform registration. Prefer `cx.auto_launch_plan_checked(...)` to expose the
requested state before platform mutation. `AutoLaunchPlan::to_text()` and
`AutoLaunchStatus::to_text()` summarize launch-at-login state without printing
app IDs. The raw `set_auto_launch(...)` and
`is_auto_launch_enabled(...)` methods remain available for direct platform
integrations.

Restart paths also have a checked builder for updater, migration, and helper
install flows:

```rust
let config = AutoUpdaterConfigBuilder::new("https://releases.example.com/feed.json")
    .check_interval(Duration::from_secs(86_400))
    .stable_only()
    .build_checked()?;

let updater = AutoUpdater::new_checked(config, current_version, http_client)?;

cx.set_restart_path_checked(
    RestartPathBuilder::current_exe()?
        .require_existing_file()
        .canonicalize(),
)?;
cx.restart();
```

Use `AutoUpdaterConfigBuilder` when generated apps configure update feeds; it
rejects empty or padded feed URLs, invalid URL syntax, non-HTTP(S) schemes,
missing hosts, and zero check intervals before network work begins. Raw
`AutoUpdaterConfig { ... }` and `AutoUpdater::new(...)` remain available for
already-validated updater integrations.

When generated tooling emits update entries, use `UpdateInfoBuilder`:

```rust
let update = UpdateInfoBuilder::new(version, package_url)
    .sha256(package_sha256)
    .size_bytes(package_size)
    .signature(ed25519_signature_base64)
    .build_signed_checked()?;
```

`build_checked()` validates download URL and optional integrity metadata;
`build_signed_checked()` requires signature, SHA-256, and package size before an
entry is treated as signed-update metadata. Raw `UpdateInfo { ... }` remains
available for already-validated feed parsers.

Use `RestartPathBuilder::new(path).require_existing_file().canonicalize()` when
the relaunch target should be a real binary. `.allow_missing()` preserves the raw
platform behavior for custom launchers, and raw `set_restart_path(path)` remains
available for already-validated integrations.

Biometric prompts validate deliberate user-facing reason text, reject accidental
leading/trailing whitespace, and report whether a platform prompt was actually
shown:

```rust
let request = cx.authenticate_biometric_with(
    BiometricAuthBuilder::new("Unlock your vault"),
    |success| {
        if success {
            // Proceed with the sensitive action.
        }
    },
)?;
tracing::info!(summary = request.to_text(), "biometric authentication");

if !request.prompted() {
    // Fall back to password or PIN.
}
```

The raw `biometric_status()` and `authenticate_biometric(...)` methods remain
available for app-specific flows. Use `request.available()`, `request.kind()`,
and `request.to_text()` when generated fallback UI or agents need one stable
summary of biometric availability, prompt dispatch, and reason text.

Global hotkeys now have a builder path so apps can parse shortcut strings once
and keep human-readable names beside their numeric IDs:

```rust
let hotkey_builder = GlobalHotkeyBuilder::new()
    .parse_named_hotkey(1, "Command Palette", "cmd-shift-k")?
    .parse_named_hotkey(2, "Toggle Capture", "cmd-alt-c")?;
tracing::info!(summary = hotkey_builder.to_text(), "global hotkey builder");
let hotkey_plan = cx.global_hotkeys_checked(hotkey_builder)?;
tracing::info!(summary = hotkey_plan.to_text(), "global hotkeys");

cx.register_global_hotkeys_checked(
    GlobalHotkeyBuilder::new()
        .parse_named_hotkey(1, "Command Palette", "cmd-shift-k")?
        .parse_named_hotkey(2, "Toggle Capture", "cmd-alt-c")?,
)?;

let cleanup = GlobalHotkeyUnregistration::new().id(1).id(2);
tracing::info!(summary = cleanup.to_text(), "global hotkey cleanup");
cx.unregister_global_hotkeys_checked(cleanup)?;
```

The ID callbacks remain the cross-platform event contract, including Wayland's
portal-backed async binding flow. The checked path rejects empty sets, duplicate
IDs, duplicate keystrokes, and invalid generated names before platform
registration begins. Use `GlobalHotkeyBuilder::to_text()`,
per-hotkey `to_text()`, `hotkey_plan.to_text()`, and cleanup `to_text()`
summaries before binding or unbinding system-wide shortcuts without logging
hotkey names or shortcut text.
`global_hotkeys_checked(...)` returns a `GlobalHotkeySet` with ids, names,
parsed keystrokes, and named-count helpers so generated preferences and plugins
can preview system-wide shortcuts before mutating platform registrations. Checked
unregistration gives preferences screens, plugins, and window-scoped shortcut
owners a matching cleanup path that rejects empty or duplicate ID requests before
raw platform calls run.

Launchers, capture tools, automation panels, and agents can inspect the active
desktop window through a checked query instead of reaching for Desktop native
modules or ad-hoc OS scripts:

```rust
let query = FocusedWindowQuery::builder()
    .external_only()
    .require_title()
    .require_pid()
    .app_name_contains("code");
tracing::info!(summary = query.to_text(), "focused window query");

if let Some(info) = cx.focused_window_info_checked(query)? {
    tracing::info!(summary = info.to_text(), "focused window");
    // Use info.app_name, info.window_title, info.bundle_id, or info.pid intentionally.
}
```

`FocusedWindowQuery` rejects contradictory process filters, empty or padded app
or bundle filters, control characters, zero PIDs, and exact-plus-contains
app-name filters before platform state is read. Use query `to_text()` /
`has_filter()` / `has_process_scope()` and result `to_text()` / `has_title()` /
`has_bundle_id()` / `has_pid()` for content-safe traces that avoid logging the
active app name, title, bundle ID, or process ID.

Window creation now has a builder path over the raw `WindowOptions` struct, so
agents can express window-management intent without remembering every field:

```rust
cx.open_window_checked(
    WindowIntentBuilder::utility()
        .title("Inspector")
        .windowed(Bounds::centered(None, size(px(900.0), px(640.0)), cx))
        .min_size(size(px(520.0), px(360.0))),
    |_window, cx| cx.new(|_| InspectorView::new()),
)?;
```

Use `WindowIntentBuilder::{main,palette,utility,modal,popup,overlay}()` first
for generated windows. It composes coherent window kinds, titlebar/background
defaults, resize/minimize/move flags, parent requirements, and placement into
checked raw `WindowOptions`, and `open_window_checked(...)` performs validation
inside the open call. It rejects invalid bounds/minimum sizes, padded or
control-character titles, path-like app IDs, modal intents without parents,
resizable popups, minimizable palettes, and kind/preset mismatches before any
window is inserted. Inspect generated intents with `to_text()`,
`options_summary()`, `kind()`, `has_bounds()`, `has_parent()`,
`has_transparent_titlebar()`, `starts_hidden()`, and `starts_unfocused()`
before creating native windows; `WindowOptions::to_text()` and the same
inspection helpers summarize checked raw options without exposing titles, app
IDs, bounds, tab identifiers, or parent handles. `WindowOptionsBuilder` remains
the lower-level escape hatch and preserves the full native option surface:
bounds, titlebar, focus/show behavior, window kind, move/resize/minimize flags,
display, native background appearance, app id, minimum size, decorations, tab
groups, mouse-passthrough overlays, and parent windows.

For generated custom titlebars and frameless window-management chrome, inspect
the available native controls and titlebar plan before wiring hit regions:

```rust
let controls = window.window_controls();
tracing::info!(summary = controls.to_text(), "window controls");

let options = WindowOptionsBuilder::new()
    .title("Inspector")
    .transparent_titlebar(true)
    .traffic_light_position(point(px(12.0), px(12.0)));
tracing::info!(summary = options.to_text(), "window options");
```

`WindowControls::to_text()` reports supported fullscreen, maximize, minimize,
window-menu, and zoom affordances. `TitlebarOptions::to_text()` reports title,
transparent-titlebar, and traffic-light-position presence without logging the
title or coordinates. Use `ResizeEdge::to_text()` for generated resize handles
and `WindowControlArea::to_text()` / `is_drag_region()` / `is_button()` for
custom close/min/max/drag hit regions before dispatching
`WindowChromeCommand`.

For Desktop fullscreen and kiosk flows, prefer a checked presentation policy
over ad hoc fullscreen toggles:

```rust
window.set_presentation_policy_checked(
    WindowPresentationPolicyBuilder::kiosk("Point of sale checkout"),
)?;
```

Use `WindowPresentationPolicyBuilder::fullscreen(reason)` for presentations,
media playback, onboarding, dashboards, and controlled display surfaces where
the user should keep normal exit behavior. Use
`WindowPresentationPolicyBuilder::kiosk(reason)` for POS and locked-down
workflows that want fullscreen, hidden chrome, and restricted user exit intent.
`clear_presentation_policy_checked()` returns to normal windowed behavior. The
checked path validates reasons, applies platform fullscreen state today, and
records kiosk intent for platform backends that can enforce stronger controls.

After opening a window, prefer
`window.set_app_id_checked(WindowAppIdBuilder::new(app_id))?` for generated
platform grouping IDs and
`window.set_tabbing_identifier_checked(WindowTabbingIdentifierBuilder::new(id))?`
for app-owned macOS tab groups. Use
`WindowTabbingIdentifierBuilder::clear()` to clear tab grouping. The checked
paths reject empty, padded, whitespace-containing, or control-character
identifiers before platform APIs see them; raw `set_app_id(...)` and
`set_tabbing_identifier(...)` remain available for already-validated platform
state. Inspect `WindowAppIdBuilder::to_text()` and
`WindowTabbingIdentifierBuilder::to_text()` when generated chrome, logs, or
agents need byte counts and clear/set state without logging identifiers.

For native document tabs, use checked tab commands around macOS tab affordances:

```rust
window.perform_window_tab_command_checked(
    WindowTabCommand::merge_all_windows().reason("Collect project windows"),
)?;
window.perform_window_tab_command_checked(WindowTabCommand::move_tab_to_new_window())?;
window.perform_window_tab_command_checked(WindowTabCommand::toggle_tab_overview())?;
```

This covers desktop-app document/workspace apps that need polished native
window management without routing everything through WebView state. The checked
command validates optional diagnostics; raw `merge_all_windows()`,
`move_tab_to_new_window()`, and `toggle_window_tab_overview()` remain available
for custom native integrations.

Document/editor windows should use a checked document state so generated apps do
not forget to keep the user-facing title and unsaved-changes marker together:

```rust
window.set_document_state_checked(
    WindowDocumentStateBuilder::document(project_path.join("Report.md"))
        .require_existing_path()
        .unsaved_changes(),
)?;
```

This is the native-window analogue of Desktop document chrome such as
`setDocumentEdited(...)`: it validates explicit titles, derives a title from the
document path, optionally requires/canonicalizes existing paths, and applies the
platform edited marker. Raw `set_window_title(...)` and
`set_window_edited(...)` remain available for already-validated custom flows.

Privacy-sensitive windows should record checked content-protection intent before
platform backends or capture flows decide whether a window can be shared:

```rust
window.set_content_protection_checked(
    WindowContentProtectionBuilder::exclude_from_capture("Protect checkout secrets"),
)?;
```

This is the native-window path for Desktop `setContentProtection(true)` use
cases such as auth, checkout, wallets, private documents, unreleased designs,
and confidential diagnostics. Use
`WindowContentProtectionBuilder::obscure_when_captured(...)` when blanking or
blurring captured output is acceptable, and `clear_content_protection_checked()`
when the private flow ends. The checked policy validates a reason and records
whether app-owned window capture should skip the window.

For popovers, tray panels, inspectors, and utility windows, resolve placement
before opening the window:

```rust
let placement = cx.resolve_tray_panel_placement_checked(
    TrayPanelPlacementBuilder::new(size(px(420.0), px(320.0)))
        .fallback_bottom_right(px(16.0)),
)?;

cx.open_window(
    WindowOptionsBuilder::new()
        .title("Downloads")
        .placement(&placement),
    |_window, cx| cx.new(|_| DownloadsView::new()),
)?;
```

The tray resolver centers above `tray_icon_bounds()` when the platform reports
them, otherwise it uses an explicit fallback such as bottom-right, top-right, or
center. Use `.require_tray_icon_bounds()` when an anchored panel should fail
instead of falling back. For non-tray popovers, palettes, inspectors, and utility
windows, use `resolve_window_placement(WindowPlacementBuilder::new(...))`. Raw
`displays()`, `primary_display()`, and `compute_window_bounds(...)` remain
available for advanced layout code. `WindowOptionsBuilder::placement(&placement)`
copies both the resolved bounds and display id.

Custom UI accessibility now has semantic recipes for the common controls agents
build by hand:

```rust
let handoff = cx.accessibility_automation_handoff_checked(
    AccessibilityAutomationHandoffBuilder::new()
        .tree(tree.clone())
        .attributes(AccessibilityAttributes::button("Save"))
        .action_request(AccessibilityActionRequest::new(
            save_button_id,
            AccessibilityAction::Click,
        ))
        .announcement("Saved")
        .focus_target(tree, save_button_id)
        .hosted_dom_automation("hosted-preview"),
)?;

tracing::info!(summary = handoff.to_text(), "accessibility automation handoff");
```

Inspect `AccessibilityAutomationNextAction` before exporting trees, routing
actions, announcing status, moving focus, or delegating to hosted DOM/selector
automation. The checked app helper and handoff reject malformed trees, invalid
custom attributes, hidden focus targets, bad hosted-surface ids, and oversized
generated batches.

```rust
let attrs = AccessibilityAttributes::switch("Enable sync", enabled)
    .disabled(is_busy);
attrs.validate()?;
let report = attrs.audit_report();
tracing::info!(summary = report.to_text(), "accessibility audit");
if !report.is_ready() {
    anyhow::bail!(report.summary());
}

div()
    .track_focus(&focus)
    .tab_stop(true)
    .accessibility(attrs);
```

Recipes cover buttons, links, checkboxes, switches, radio buttons, sliders,
progress bars, and text inputs. The lower-level
`AccessibilityAttributes::new(AccessibilityRole::...)` path remains available
for custom roles and unusual states.
Use `AccessibilityAttributes::audit_report()` for non-throwing component
reviews, and `AccessibilityTree::audit_report()` before platform export when an
app or agent needs to catch all structural issues at once. Use
`AccessibilityRole::to_text()`, `AccessibilityState::to_text()`,
`AccessibilityAction::to_text()`, `AccessibilityActionPayload::to_text()`,
`AccessibilityActionRequest::to_text()`, `AccessibilityValue::to_text()`,
`AccessibilityRect::to_text()`, `AccessibilityNode::to_text()`,
`AccessibilityTree::to_text()`, `AccessibilityAuditIssue::to_text()`,
`AccessibilityAttributes::to_text()`, and `report.to_text()` when generated
tests, traces, or AI agents need a DOM-like structural view of native controls
without logging labels, values, placeholders, descriptions, exact geometry,
payload text, or audit messages. The tree audit reports missing children, parent
mismatches, multiple focused nodes, hidden focused nodes, missing interactive
names/actions, conflicting states, unknown roles, and invalid range values.

For native app structure that developers often reach for the DOM to express,
prefer semantic primitives and navigator summaries before falling back to a
WebView. `MenuEntry::to_text()`, `Link::to_text()`, and `TreeItem::to_text()`
report labels by presence and byte length, activation/disabled state, expansion
state, callback wiring, and child counts without logging labels, URLs, child
contents, or callback internals. `Route::to_text()`,
`RouteChangeEvent::to_text()`, `Transition::to_text()`, and
`Navigator::to_text()` summarize route-id byte lengths, memento presence, stack
depth, current-route presence, and transition state without logging route IDs or
stored mementos. Use these for generated menus, sidebars, route stacks, tree
views, and inspector navigation where Desktop apps might otherwise depend on
browser history and DOM accessibility nodes.

For route-heavy native apps, create a checked navigation handoff before tabs,
breadcrumbs, command-palette routes, session restore, deep-link entry, or
hosted-history fallback mutate state:

```rust
let handoff = cx.navigation_handoff_checked(
    NavigationHandoffBuilder::new()
        .route(NavigationRouteDescriptorBuilder::new("home").restorable_state())
        .push_route("settings/profile")
        .restore_session(2)
        .deep_link("myapp")
        .hosted_history_bridge("docs"),
)?;
```

`cx.navigation_handoff_checked(NavigationHandoffBuilder::...)` validates route
ids, stack commands, restore depth, deep-link schemes, and hosted-history bridge
scope. `NavigationHandoff::to_text()` reports request kinds and the next action
without logging route ids, URLs, titles, tab labels, breadcrumbs, mementos, or
history entries. Keep
`WebViewOptions::navigation_state_bridge`, location/title/favicon bridges, and
`WebViewController::go_back` / `go_forward` for hosted pages whose browser
history is the app behavior, not for app-owned native route stacks.

For live updates and custom focus handoffs, prefer checked window APIs:

```rust
window.announce_accessibility_checked(
    AccessibilityAnnouncementBuilder::new("Download complete"),
)?;
window.focus_accessibility_node_checked(
    AccessibilityFocusBuilder::new(primary_action_id),
)?;
```

Checked announcements reject empty, padded, control-character, and overly long
live-region text. Checked accessibility focus rejects missing and hidden nodes
before changing the current tree, which gives agents a concrete failure instead
of silently leaving assistive technology out of sync.

## Capability documentation gates

Every feature that is sold as "native desktop capability" should have a gate:

| Gate | Evidence required |
| --- | --- |
| API exists | Public docs and examples compile |
| Cross-platform | `video_capability_report()` and platform docs say Full on macOS, Windows, and Linux |
| Graceful fallback | Partial/Unsupported paths are documented |
| Performance | Benchmark against a comparable Desktop sample |
| AI-agent ready | `llms.txt` includes the current correct API and an example |
| Production ready | Tests or examples cover failure states, not only happy paths |

Until a gate is green, docs should say "available", "partial", or "roadmap",
not "matches Desktop".

For performance evidence, compare a Kael result set against an Desktop sample
with `BaselineComparisonReport` and matching sample contracts:

```rust
let report = BaselineComparisonReport::generate_with_sample_pairs(
    &baseline_results,
    kael_harness.results(),
    &[sample_pair],
    Some("trace.json".into()),
);

println!("{}", report.summary());
```

Use the same `BenchmarkScenario` and metric names on both sides, build each
`BenchmarkSamplePair` from `BenchmarkSampleApp::builder(...)`, and inspect
`BenchmarkScenario::workload_spec()` before publishing a claim. Reports now keep
`MissingResult` issues when one side lacks a counterpart scenario result and
`DuplicateResult` issues when one side supplies multiple results for the same
scenario. They also keep `EnvironmentMismatch` issues when OS, CPU, memory, or
GPU conditions differ, and `MissingSample` issues when matching Desktop/Kael
sample descriptors are absent, so claims stay blocked until the measured apps
and measurement environments are comparable. See
[Benchmarking Kael Against Desktop](benchmarking.md) for the evidence workflow.

Apps should also gate their own hard requirements before they build a window or
start background work:

```rust
let report = CapabilityReport::current();
let readiness = CapabilityCheck::new()
    .require(PlatformFeature::WebView)
    .require_available(PlatformFeature::Notifications)
    .prefer_available(PlatformFeature::GlobalHotkeys)
    .require(PlatformFeature::PrecisionPointerInput)
    .prefer_available(PlatformFeature::GestureInput)
    .prefer_available(PlatformFeature::TouchInput)
    .prefer_available(PlatformFeature::PenInput)
    .evaluate(&report);

if let Some(summary) = readiness.required_failure_summary() {
    anyhow::bail!("unsupported desktop: {summary}");
}

if let Some(summary) = report.fallback_risk_summary() {
    tracing::info!("native fallbacks or setup paths needed: {summary}");
}

tracing::info!(
    summary = report.coverage_summary().to_text(),
    "desktop capability coverage"
);
```

Use `require(...)` for full-support-only requirements, `require_available(...)`
when `Partial` or `RequiresInit` is an acceptable setup/fallback path, and
`prefer_available(...)` for Desktop-like conveniences that should produce UI
fallbacks rather than block launch. Input-heavy apps should check
`PrecisionPointerInput`, `GestureInput`, `TouchInput`, and `PenInput` instead
of assuming one pointer-event shape is present on every native backend.
Before a game, whiteboard, music tool, creative editor, or kiosk binds handlers,
prefer an advanced input handoff so the app chooses an explicit native route for
pointer, gesture, touch, stylus, hosted browser islands, and still-roadmap
device classes:

```rust
let input = cx.advanced_input_handoff_checked(
    AdvancedInputHandoffBuilder::new()
        .capability_report(CapabilityReport::current())
        .pointer_policy(PointerInputPolicyBuilder::drag_surface("canvas").hover())
        .gesture_policy(GestureInputPolicyBuilder::canvas("canvas"))
        .touch_surface("canvas", 2)
        .stylus_surface("inking", true, true)
        .roadmap_gamepad("native controller input")
        .roadmap_midi("MIDI control surface"),
)?;

match input.next_action() {
    AdvancedInputNextAction::CheckInputCapabilities => collect_capabilities(),
    AdvancedInputNextAction::ConfigurePointerPolicy => bind_pointer_handlers(),
    AdvancedInputNextAction::ConfigureGesturePolicy => bind_gesture_handlers(),
    AdvancedInputNextAction::PrepareTouchSurface => prepare_touch_surface(),
    AdvancedInputNextAction::PrepareStylusSurface => prepare_stylus_surface(),
    AdvancedInputNextAction::UseHostedInputIsland => scope_browser_input_island(),
    AdvancedInputNextAction::TrackAdvancedInputRoadmap => record_product_caveat(),
}

tracing::info!(summary = input.to_text(), "advanced input handoff");
```

`AdvancedInputHandoffBuilder` validates bounded request counts, app-owned
surface ids, touch-point counts, pointer and gesture policies, hosted input
islands, and roadmap reasons. Its summary reports only request kinds and route
counts, so agents can reason about input coverage without logging surface ids,
shortcuts, labels, coordinates, raw event payloads, device names, or roadmap
text. Use hosted input islands only when browser DOM input semantics are the
dependency; gamepad, MIDI, and raw-input support should remain visible roadmap
work until native backends exist.
For broad capability audits, use `full_features()`, `available_features()`,
`partial_features()`, `requires_init_features()`, `unsupported_features()`,
`fallback_risk_features()`, `coverage_summary()`, and
`fallback_risk_summary()` to list the native surface that is ready, guarded, or
missing before choosing a WebView fallback, native implementation, or product
caveat.

## Priority roadmap

P0: truthful positioning and capability matrix.

P1: app-type capability recipes across the full desktop surface, not just hosted web
content. Each recipe should start from a real app category, list the Desktop
APIs developers expect, map the native Kael path, name the WebView fallback only
where the web platform is the dependency, and mark any remaining roadmap work.
The first set should cover media players, project/file explorers, dashboards,
developer tools, chat/collaboration, creative/canvas tools, commerce/payment
apps, and plugin-heavy apps.

P2: Desktop-easy media: URL in, player out, custom controls optional.

P3: WebView-island recipes for auth, maps, docs, payments, rich editors, and
advanced media.

P4: custom render targets and shaders as the top-level visual escape hatch.
Until that lands, route visuals through the current ladder: styled elements and
`kael_ui`, `canvas(...)` / `PathBuilder`, gradients, SVG, Lottie,
`backdrop_blur(...)` / `effect_layer(...)`, `HeadlessRenderer` for evidence,
and WebView islands for browser-only WebGL/WebGPU content. Use
`graphics_capability_report()` in builder tools and agents before promising
desktop-app graphics readiness:

```rust
let handoff = cx.graphics_canvas_handoff_checked(
    GraphicsCanvasHandoffBuilder::new()
        .capability_report(graphics_capability_report())
        .interactive_native_canvas("timeline")
        .svg_assets(3)
        .lottie_animations(1)
        .effect_layers(2)
        .headless_render_evidence(1)
        .browser_graphics_island("webgl-preview")
        .roadmap_custom_shader("native shader plugin"),
)?;
tracing::info!(summary = handoff.to_text(), "graphics/canvas handoff");

let graphics = graphics_capability_report();
tracing::info!(summary = graphics.to_text(), "graphics capabilities");

if graphics.has_roadmap_gaps() {
    // Public render targets/custom shaders are not a shipped native parity surface yet.
}
```

The report exposes stable `GraphicsCapabilityStatus` labels and counts so public
render targets/custom shaders stay marked `Roadmap`, browser-only
WebGL/WebGPU/canvas work stays marked as a WebView fallback, and native styled
elements/canvas/paths/gradients/SVG/Lottie can be described as ready without
over-selling the whole graphics stack.
`GraphicsCanvasHandoffBuilder` is the native-first route planner for generated
visual apps: it validates native canvas surface ids, optional draw-command
counts, SVG/Lottie/effect/headless artifact counts, browser graphics island
scope, and render-target or shader roadmap reasons before app generation. Use
`GraphicsCanvasNextAction` to build native canvas/SVG/Lottie/effect/headless
evidence first, reserve explicit WebView islands for browser-owned
WebGL/WebGPU/canvas engines, and keep public render-target or custom-shader
needs as roadmap work. `GraphicsCanvasHandoff::to_text()` reports only request
kinds, native/browser/roadmap booleans, and next action; it does not log surface
ids, asset names, shader code, generated coordinates, colors, image bytes, or
WebView ids.

For generated charts, timelines, waveform views, canvas editors, game HUDs, and
custom controls, prefer the immediate-mode native canvas and inspect the queued
draw pass before it flushes:

```rust
canvas(size(px(320.0), px(180.0)), |draw, _window, _cx| {
    draw.fill_rect(Bounds::new(point(px(0.0), px(0.0)), draw.size()), rgb(0x111111));
    tracing::info!(summary = draw.to_text(), "canvas draw");
})
```

`DrawContext::to_text()` reports command, path, quad, filled-quad,
stroked-quad, text, image, saved-state, and size counts without logging text,
image data, colors, or coordinates. Use `command_count()`, `path_count()`,
`quad_count()`, `filled_quad_count()`, `stroked_quad_count()`, `text_count()`,
`image_count()`, `state_stack_depth()`, and `is_empty()` in agent-generated
graphics tests and diagnostics.
For expensive native previews, overlays, filtered panels, and compositor-style
surfaces, inspect `Cached::to_text()`, `Deferred::to_text()`, and
`EffectLayer::to_text()` before rendering. These summaries report child
presence, explicit cache-key presence, draw priority/class, effect combination,
blur class, and shadow presence without logging cache ids, child contents, blur
radii, shadow offsets, shadow colors, or geometry. Use `cached(child)` for
tracked subtree reuse, `deferred(child)` for after-ancestor paint ordering, and
`effect_layer(child)` for native content blur/drop-shadow work before reaching
for a WebView just to get compositor effects.

For desktop-app modal, popover, fullscreen, and in-window overlay flows, use
`LayerStack` plus `LayerOptions::{modal,centered,fullscreen,anchored}`. Inspect
`LayerAnchor::to_text()`, `LayerOptions::to_text()`, and
`LayerStack::to_text()` before generated overlay flows; the summaries expose
placement, backdrop capture, priority class, dismissal policy, and active layer
counts without logging child views, colors, coordinates, margins, or layer
content. This gives builders a native path for common DOM overlay patterns
instead of routing every custom modal or popover through WebView.

For native asset-backed visuals, use `ImageSource::to_text()`,
`ImageStyle::to_text()`, `Img::to_text()`, `Svg::to_text()`,
`Transformation::to_text()`, `SurfaceSource::to_text()`, and
`Surface::to_text()` before generated image/icon/surface rendering. These
helpers report source class, resource byte length, object-fit key,
loading/fallback hooks, cache binding, SVG path presence, coarse transform kind,
and platform surface class without logging URLs, file paths, embedded asset
names, SVG paths, raw bytes, image IDs, pixel dimensions, transform values, or
pixel contents.
Use `RetainAllImageCacheProvider::to_text()`,
`LruImageCacheProvider::to_text()`, `ImageCacheElement::to_text()`,
`RetainAllImageCache::to_text()`, `LruImageCache::to_text()`, and
`ImageCacheItem::to_text()` around asset-heavy native galleries, maps, feeds,
and media browsers. These summaries expose retain-all vs LRU policy, entry
counts, loading/loaded/error counts, capacity, capacity class, at-capacity
state, and scoped child count without logging resource identifiers, element ids,
image ids, image bytes, error details, or asset names.

For CSS-transition-style generated motion, inspect the native timeline before
attaching it:

```rust
let animation = Animation::new(Duration::from_millis(240))
    .easing(Easing::EaseOutCubic)
    .repeat(Repeat::Count(2));

tracing::info!(summary = animation.to_text(), "animation");
```

`Animation::to_text()`, `AnimationSequence::to_text()`,
`Keyframes::to_text()`, `StyledKeyframe::to_text()`,
`KeyframeTrack::to_text()`, `MediaKeyframe::to_text()`, `Repeat::to_text()`,
and `Easing::to_text()` expose duration, delay, curve class, repeat class,
sequence/keyframe/property/interpolation counts, and finite-value checks without
logging transform distances, opacity values, media automation values, exact
keyframe times, custom easing callbacks, or cubic-bezier control points.

For animated vector assets, prefer native Lottie before reaching for a browser
island:

```rust
let motion = lottie("animations/status.json")
    .autoplay()
    .ping_pong()
    .prefetch_frames(8);

tracing::info!(summary = motion.to_text(), "lottie element");
```

`LottieSource::to_text()`, `LottieAnimation::to_text()`,
`LottiePlayer::to_text()`, and `lottie(...).to_text()` expose source class,
decoded frame/fps/size metadata, playback state, loop mode, object-fit mode,
prefetch counts, and loading/fallback configuration without logging URLs,
paths, embedded resource names, raw bytes, or replacement text.

P5: deeper headless component helpers: focus traps, composite keyboard
interaction, richer a11y action routing, and more prop-builder recipes.
`FocusTrapController` now gives custom modals/popovers/palettes reusable
Tab/Shift-Tab/Escape behavior over Kael's tab-group traversal.
`AccessibilityActionRequest` and `AccessibilityActionRouter` now give
assistive-technology actions a normalized app-routing contract. macOS/Linux
adapter drains preserve those normalized actions against the current tree, and
`Window::on_accessibility_action` / `Window::drain_accessibility_actions` expose
them to app code after each platform tree update. Windows now feeds standard UIA
focus, invoke, toggle, expand/collapse, value, and range-value pattern calls
into that same route; exact edits arrive as `AccessibilityAction::SetValue`
requests with `AccessibilityActionPayload::Value(...)` or
`AccessibilityActionPayload::NumericValue(...)`.
Common custom-control accessibility recipes are now available, but complex
widgets still need more headless guidance.

P6: benchmark suite comparing Kael and Desktop sample apps on memory, CPU,
startup, video playback, and idle behavior. The benchmark harness and
`BaselineComparisonReport` now provide the reporting path, and
`BenchmarkSamplePair` / `BenchmarkSampleApp` validate comparable sample
contracts; the remaining work is shipping those sample apps and publishing
measured baselines.

Kael can become a credible native desktop capability by being more honest and more
deliberate than Desktop: native by default, web-compatible when needed, and
clear about which rung of the builder ladder solves each problem.

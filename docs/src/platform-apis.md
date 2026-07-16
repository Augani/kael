# Platform APIs

Kael provides native platform integration for desktop apps without requiring an
Desktop runtime. Coverage is intentionally broad, but support varies by OS and
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

if let Some(summary) = report.fallback_risk_summary() {
    tracing::info!("native feature fallbacks/setup needed: {summary}");
}

let coverage = report.coverage_summary();
tracing::info!(summary = coverage.to_text(), "desktop capability coverage");
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
one pointer-event shape exists on every native backend. Use
`cx.pointer_input_policy_checked(PointerInputPolicyBuilder::drag_surface("canvas"))?`
and `cx.gesture_input_policy_checked(GestureInputPolicyBuilder::canvas("canvas"))?`
to validate hover, drag, wheel, pan, swipe, and zoom intent before UI code binds
surface-specific handlers.
For games, creative tools, whiteboards, kiosks, and music apps, prefer one
checked input handoff before falling back to a hosted browser input island:

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
```

`AdvancedInputHandoffBuilder` validates pointer, gesture, touch, stylus, hosted
island, and gamepad/MIDI/raw-input roadmap requests. Inspect
`AdvancedInputHandoff::to_text()` without logging surface ids, shortcut text,
labels, coordinates, raw event payloads, device ids, gamepad/MIDI names, or
roadmap text.
Use `full_features()`, `available_features()`, `partial_features()`,
`requires_init_features()`, `unsupported_features()`,
`fallback_risk_features()`, `coverage_summary()`, and
`fallback_risk_summary()` when an agent or settings page needs a broad
Desktop capability audit instead of one feature probe at a time.
For app-shape planning, start with
`DesktopAppCategory::MediaPlayerEditor.replacement_plan(&CapabilityReport::current())`
or another `DesktopAppCategory` before choosing implementation details. The
combined plan includes the app recipe, desktop surface audit, implementation
briefs, route counts, readiness, and feature gaps without logging user content,
paths, URLs, project names, or media metadata. Check
`DesktopCapabilityPlan::readiness()`, `can_generate()`, and
`needs_briefing()` before code generation so blocked feature gaps, intentional
WebView islands, and roadmap gaps are explicit. Use `action_items()` and
`blocking_action_items()` for the ordered checklist agents should handle before
marking readiness; blocking action items include safe `PlatformFeature` ids in
`feature_gaps` and support levels in `feature_gap_details`. Use
`PlatformFeatureGap::remediation()` or action helpers such as
`needs_initialization_or_permission()`, `needs_policy_change()`, and
`needs_fallback_or_roadmap()` to choose the next step; use
`brief()` and `remediation_summary()` for count-level briefing across the whole
plan.
When the app shape is already known, call `DesktopAppCategory::primitives()`,
`requirements()`, `capability_intake()`, `builder_handoff(&report)`,
`primitive_bridge_audit(&report)`, `capability_matrix(&report)`, or
`generation_blueprint(&report)` to select the usual Desktop requirement and
primitive families for that category and produce the ordered app-wide or
primitive-level handoff without manually listing window management, media, file,
capture, hardware, plugin, or diagnostics primitives.
`DesktopAppRequirement::from_primitive(primitive)` maps a primitive family back
to the builder-facing requirement used by custom intakes. Category contracts
also expose `capability_intake()`, `intake_contract()`, and `builder_handoff()`
so preset categories and custom intakes share the same requirement packets,
next-batch handoff, native/WebView/roadmap counts, strategic bridge track
(`design-freedom`, `canvas-graphics`, `media-audio`, `native-desktop-apis`,
`production-readiness`, `performance-memory`, or `agent-developer-experience`),
bridge priority, and evidence-loop shape. Use
`capability_matrix(&report)` when the agent needs
the compact answer to "what else besides WebView is missing?":
`ready_native_rows()` lists primitives that can start native work now,
`non_webview_gap_rows()` lists native/platform blockers, `webview_rows()` lists
justified browser-shaped islands, and `roadmap_rows()` lists caveats that must
be briefed. Use `backlog_items()`, `current_backlog_item()`,
`blocking_backlog_items()`, or `backlog_for_kind(kind)` when the matrix needs to
become an execution queue ordered as native capability blockers, ready native
primitive work, acceptance evidence, browser-island isolation, and roadmap
caveats. Use
`backlog_tickets()`, `current_backlog_ticket()`, `blocking_backlog_tickets()`,
or `tickets_for_kind(kind)` when the execution queue needs to carry the
concrete starter recipe, scaffold hint, acceptance criteria, and
missing-by-default evidence checklist for generation. Use `ticket.ready_review()`
before verification and `ticket.review_evidence(checklist)` after verification
to decide whether the ticket can close or which backlog kind should be handled
next. Use `matrix.ready_backlog_review()` before a run and
`matrix.review_backlog_evidence(checklists)` after a run when the agent needs
aggregate closable/open/blocking counts and the next backlog kind across every
ticket. Use `review.current_ticket()`, `review.current_evidence_checklist()`,
`review.open_tickets()`, and `review.closable_tickets()` to continue from the
first open ticket or report completed tickets without rescanning the matrix. Use
`review.current_generation_pass()` when the next agent step should be a single
focused pass with helpers for blocker resolution, native generation, evidence
collection, WebView isolation, and roadmap briefing. Use
`generation_brief(&report)` as the category-level pre-generation summary when
an agent needs one content-safe object that combines replacement readiness with
the primitive blueprint.
Use `generation_manifest(&report)` after the brief when the agent needs the
machine-readable implementation slices: blockers with safe feature details,
native starter APIs, setup checks, explicit WebView islands, and roadmap items
grouped by Desktop primitive and desktop surface area. Call
`manifest.generation_queue()` to get the ordered implementation loop and
`ready_entries()` for the native entries that can be generated immediately when
no blocker phase is first. Use `queue.ready_native_starters()` and
`queue.ready_setup_checks()` to hand agents the concrete native APIs and setup
checks for the current pass, and use `queue.queued_webview_islands()` plus
`queue.queued_roadmap_items()` to keep browser fallbacks and missing platform
work explicit instead of treating WebView as the default answer. Use
`entry.scaffold_hint()` or `queue.ready_scaffold_hints()` to place generated
modules, state owners, view components, command hooks, and verification hooks. Use
`entry.acceptance_criteria()` or `queue.ready_acceptance_criteria()` to verify
smoke, parity, and resource criteria before marking readiness. Use
`entry.acceptance_evidence_checklist()` or
`queue.ready_acceptance_evidence_checklists()` to track pass/fail/missing
evidence; inspect `expected_evidence_artifact()` on each item and
`expected_evidence_artifacts()` on each checklist so smoke checks get runtime or
checked-API proof, behavior checks get Desktop-behavior comparison proof, and
resource checks get snapshot, budget, or benchmark proof before status is set to
passed. Use `queue.ready_acceptance_evidence_report()` as the aggregate
readiness evidence for the current native work. Use
`queue.parity_claim_decision(&evidence)` or `queue.ready_parity_claim_decision()`
as the final allow/block decision before claiming readiness.
Use `generation_contract(&report)` when the agent needs one category handoff
object containing the brief, manifest, queue, capability matrix,
native-first plan, ready evidence report, initial ready-work claim decision, and
native-first claim decision. Call `contract.next_step()` or
`contract.next_action()` to choose whether to resolve blockers, generate ready
native work, fix failed evidence, collect remaining evidence, brief caveats, or
mark readiness. After verification, call `contract.with_ready_evidence(evidence)`
or `contract.with_ready_evidence_checklists(checklists)` to refresh the claim
decision and next-step recommendation without rebuilding the handoff. For
category-level native-first worker loops, call
`contract.current_phase_assignment()`, `contract.current_execution_kit()`, or
`contract.current_execution_batch()`; after workers return proof, call
`contract.continue_with_execution_batch_evidence(&batch, checklists)` to get a
refreshed category contract plus the next native-first batch.
Use `DesktopCapabilityPortfolioAudit::all(&report)` for broad framework
audits across every standard desktop-app category; it reports ready,
blocked, claimable, briefing-needed, next-action, feature-gap, and evidence
counts without turning non-WebView gaps into a generic WebView fallback. Use
`portfolio.prioritized_entries()` for the ordered category queue and
`portfolio.recommended_focus()` when the agent needs one category/action to
handle next. Use `portfolio.recommended_handoff()` when the agent needs the
focused contract plus the manifest entries, scaffold hints, acceptance criteria,
evidence report, and native-first execution batch relevant to that action. Use
`portfolio.recommended_execution_batch()` or
`portfolio.recommended_phase_assignment()` when a scheduler only needs the
recommended batch payload. After verification, call
`handoff.review_evidence(evidence)` or
`handoff.review_evidence_checklists(checklists)` to refresh the claim decision
and next action for the focused scope, or
`handoff.continue_with_execution_batch_evidence(&batch, checklists)` to refresh
the focused category contract and next native-first batch.
When the question starts from a familiar Desktop primitive instead of an app
category, use `DesktopPrimitive::MediaPlaybackSurface.bridge(&report)` or
`DesktopPrimitiveBridgeAudit::all(&report)` to map native app
chrome/components, window management, screen/display topology, media elements, menus/tray/dock/taskbar,
embedded views/panes, files/dialogs, message dialogs/prompts, app identity/metadata, clipboard/editing, input/IME/shortcuts, media capture/devices,
notifications/shell, printing/protocols/paths, network/realtime/downloads,
app storage/sessions, app lifecycle/single-instance, launch environment/config, IPC/command messaging, security/permissions policy, background tasks/workers, navigation/history/routing, power/theme/idle, canvas/WebGL, hardware device APIs, child processes/plugins,
packaging/updaters, accessibility/automation trees, performance diagnostics, crash reporting/diagnostics, and developer
tools/observability to the
owning Kael surface. The primitive bridge
reports route, checked feature count, feature gaps, native primitive count,
WebView conditions, roadmap items, and remediation counts without logging URLs,
paths, device identifiers, plugin metadata, or document contents. Call
`DesktopCapabilityIntake::from_requirements([...])` when the app brief is
custom rather than one preset category. Requirements such as
`MediaPlayback`, `FilesAndDocuments`, `HardwareDevices`,
`PluginsAndProcesses`, `AccessibilityAutomation`, and
`PerformanceDiagnostics` deduplicate into Desktop primitive families, then the
intake exposes `generation_brief(&report)`, `primitive_bridge_audit(&report)`,
`capability_matrix(&report)`, `native_first_plan(&report)`, and
`generation_contract(&report)` for custom app planning. The brief is the safe
first response for a bespoke app: it reports requirement, primitive, blocker,
native-ticket, evidence, deferred-WebView, roadmap, current-primitive, and
parity-status counts without logging app content. The contract is the
single-object worker handoff with the brief, matrix, native-first plan, backlog
review, claim, `current_generation_pass()`, `next_step()`, `next_action()`, and
`with_evidence_checklists(...)` after verification. When workers return
`DesktopCapabilityGenerationPassOutcome`, call
`contract.with_pass_outcomes(outcomes)` to refresh review, claim, next action,
and the count-level brief directly; use `current_pass_brief()` and
`current_evidence_checklist()` for compact worker assignment payloads.
`DesktopCapabilityIntakeNextStep` is the custom-brief dispatch summary for UI
or worker queues: it reports action, current primitive, blocker/native/evidence
ticket counts, deferred WebView and roadmap counts, evidence totals, and
briefing status. Use `contract.current_work_order()` when the worker queue needs
one serializable handoff containing that next-step summary plus the focused
pass; `work_order.assignment()` returns the smaller count-only scheduler
payload, including whether the current primitive should resolve native blockers,
generate native work, collect evidence, isolate a browser dependency, or brief a
roadmap caveat. Use `work_order.execution_kit()` when the worker needs concrete
built-in starter APIs, setup checks, scaffold placement, acceptance criteria,
and the evidence checklist for that native-first pass. For broader custom apps,
call `contract.open_execution_kits()` to see unresolved review-state work, and
`contract.planned_execution_kits()` to keep the whole planned capability map
visible. The filtered `blocker_execution_kits()`, `native_execution_kits()`,
`webview_execution_kits()`, and `roadmap_execution_kits()` helpers preserve
non-current surfaces such as files, capture, devices, packaging, accessibility,
and diagnostics beside media work. Use `contract.requirement_coverage()` when
the queue needs one row per requested app requirement, including shared
primitive mappings such as app chrome and window management, local focus,
planned/open kits, blockers, native work, evidence, WebView islands, roadmap
caveats, bridge track, bridge priority, starter/setup/criteria/evidence counts,
and native/platform booleans.
`contract.requirement_coverage_for(requirement)` returns the same row for a
single requested surface. Use `contract.requirement_work_packets()` when a
surface owner needs the coverage row plus concrete execution kits for every
requested requirement, or `contract.requirement_work_packet_for(requirement)`
for one surface-specific assignment. Each packet can complete matching evidence
checklists and return pass outcomes, which lets agents hand media, files,
devices, packaging, accessibility, diagnostics, and other areas to separate
workers without losing WebView islands or roadmap caveats. Use
`contract.builder_handoff()` as the app-wide serializable front door when the
builder needs the intake brief, execution coverage, recommended next batch, and
all requirement work packets in one object. The handoff exposes
`next_requirement_packets()`, `next_requirement_packet_count()`,
`has_next_requirement_packets()`,
`requirement_packets_for_bridge_track(track)`,
`requirement_packets_for_bridge_priority(priority)`,
`critical_bridge_packets()`, `bridge_track_summaries()`,
`active_bridge_track_summaries()`, `bridge_priority_summaries()`,
`active_bridge_priority_summaries()`, `bridge_track_workstreams()`,
`active_bridge_track_workstreams()`, `bridge_track_workstream(track)`,
`critical_bridge_workstreams()`, single track/priority summaries,
bridge track/priority packet counts, executable packet/evidence bundles, and
`requirement_packets_for_focus(focus)` so schedulers can assign the next
surface owners without reimplementing focus filters. Use
`contract.continue_with_bridge_track_workstream_evidence(&workstream,
checklists)` when a graphics, media, performance, production, native desktop
API, or developer-experience worker returns evidence and needs the refreshed
contract plus next app-wide builder handoff. Use
`contract.run_bridge_track_workstream_loop([(track, checklists)], max_steps)`
when a coordinator receives multiple returned strategic tracks and needs one
final handoff/report across the non-WebView bridge pass. Use
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
Use
`next_scaffold_hints()`, `next_native_starters()`,
`next_quick_start_steps()`, `next_setup_checks()`,
`next_acceptance_criteria()`, and `next_evidence_checklists()` when a codegen
agent needs the selected next-batch ingredients without walking every kit
manually. Native app chrome quick starts include
`AppChromeSurfaceHandoffBuilder`, `AppChromeSurfaceNextAction`,
`kael_ui::init`, `install_theme`, `ThemeTokens`, styled `div()` layout,
`Navigator`, `Route`, `tabs`, `splitter`, `Dialog`, `Sheet`, `BottomSheet`,
`Menu`, `ContextMenu`, `CommandPalette`, `DataTable`, `DataGrid`, `Editor`,
`Markdown`, and `AccessibilityAttributes`, so Desktop DOM/CSS product chrome,
dashboards, tool panes, navigation, overlays, tables, editors, command
surfaces, accessibility audits, and hosted UI fallbacks start from a checked
native workflow before legacy web UI or third-party web component libraries
become explicit WebView islands. Embedded hosted-view quick starts include
`EmbeddedHostedViewHandoffBuilder`, `EmbeddedHostedViewNextAction`,
`EmbeddedHostedPaneProfile`, native pane composition, cached/deferred panes,
floating panes, explicit hosted pane profiles, pane-scoped controllers, and
support preflight, so OAuth, maps, payments, docs, browser graphics, and
legacy web widgets are isolated as owned panes instead of becoming the default
app runtime. Layout/styling/animation quick starts include
`LayoutStylingHandoffBuilder`, `LayoutStylingNextAction`,
`CssTokenMigrationBuilder`, `AnimationTimelineBuilder`,
`ResponsiveLayoutPlanBuilder`, styled `div()` containers, `ThemeTokens`,
`Transition`, `Navigator`, `Route`, `cached`, `deferred`, `effect_layer`,
`LayerStack`, native image, SVG, Lottie, canvas surfaces, `UniformList`,
`RecyclingList`, and content-safe render summaries, so CSS layout, media-query
breakpoints, design-token migration, keyframe/timeline motion, effect, overlay,
list, and animation work has a checked native path before imported CSS
frameworks or exact browser animation engines become explicit WebView islands.
Visual capture quick starts include
`AppWindowCaptureRequestBuilder`, `AppWindowCaptureRequest`,
`AppWindowCaptureTarget`, `AppWindowCaptureFormat`,
`app_window_capture_request_checked`, `HeadlessRenderer`, cached/effect
snapshots, `WebViewController::capture_dom_image`,
`WebViewController::capture_media_frame`,
`WebViewController::element_snapshot`, and `SupportDiagnosticsBuilder`, so
desktop-app `visual capture`, screenshots, thumbnails, media frames, and
visual-regression evidence use bounded native descriptors first while
cross-origin full-page Chromium/offscreen capture remains an explicit browser
island or roadmap caveat. Find/page-search/zoom quick starts include
`CommandPalette`, `SearchField`, `Editor`, `Markdown`, `rich_text`,
`Navigator`, `Route`, `ScrollBar`, `TextInputImeState`,
`cx.focus_traversal_plan_checked(FocusTraversalPlanBuilder::form("search"))?`,
`EditCommandStateSnapshot`,
`TextCheckingRequestBuilder`, `FindZoomHandoffBuilder`, `FindZoomHandoff`,
`FindZoomNextAction`, `FindZoomRequest`, `DocumentZoomMode`,
`WebViewController::find_text`,
`WebViewController::find_text_result`, `WebViewFindOptions`,
`WebViewFindResult`, `WebViewFindEvent`, `WebViewStopFindAction`,
`WebViewOptions::find_result_bridge`, `WebViewOptions::zoom_hotkeys`, and
`WebViewController::set_zoom_factor`, so app-owned document search,
scroll-to-match, and zoom buckets stay native while iframe/shadow-DOM browser
matching and exact page zoom remain explicit hosted-page islands. Media quick
starts include direct URL playback moves such as
`VideoElementHandoffBuilder::url(url)`,
`VideoUrlPlaybackHandoff::url(url)?`, `MediaSourceBuilder`,
`VideoPlaybackPlanBuilder`, `VideoElementHandoff`,
`VideoElementHandoffNextAction`, `VideoPlaybackRequirementPlan`,
`video_capability_report()`, `VideoPlaybackControlsBuilder`,
`TextTrackBuilder`, `VideoPlaylist`, `MediaKeyBindingBuilder`,
`WebViewVideoOptions`, `WebViewVideoCommandBuilder`, and
`kael_ui::VideoPlayer::url(...)`, so ordinary URL/file/bytes/reader media starts
native while HLS/DASH/DRM/browser-only SDK needs become explicit WebView media
islands with checked commands. Files/dialogs quick starts include
`OpenDialogBuilder::files()`, `OpenDialogBuilder::directory()`,
`SaveDialogBuilder::new(dir).suggested_name(name)`, `FileIntakePlanBuilder`,
`FileExportDragIntentBuilder`, and `RecentDocumentsBuilder`, so agents can
cover desktop-app open/save dialogs, drops, file promises, and recent
documents without inventing a WebView file picker. Filesystem/workspace quick
starts include `FileIntakePlanBuilder`, `AppPathBuilder`,
`FileWatchOptionsBuilder`, `FileWatchSetBuilder`, `FileWatcher`,
`FileWatchEvent`, `RecentDocumentsBuilder`, `StorageMigrationPlanBuilder`,
`StorageCleanupPlanBuilder`, `AppStorageSessionHandoffBuilder`,
`AppStorageSessionHandoff`, `AppStorageSessionNextAction`, and
`ShellTargetsBuilder`, so desktop-app
Node `fs`, `path`, `fs.watch`, chokidar, workspace tree refresh, project
caches, recent projects, and reveal/open/trash actions start native while
browser File System Access API handles, OPFS, IndexedDB virtual filesystems,
and hosted cloud drive/file-manager widgets stay explicit browser islands.
Safe-storage quick starts include `CredentialBuilder`,
`CredentialServiceBuilder`, `CredentialWriteRequest`, `StoredCredential`,
`cx.write_secure_credential(...)`, `cx.read_secure_credential_checked(...)`,
and `cx.delete_secure_credential_checked(...)`, so Desktop `secure storage`,
encrypted token-file, API-key, refresh-secret, logout, and account-switch flows
start from OS keychain primitives instead of JSON settings or WebView storage.
App lifecycle quick starts include `AppLifecycleStartupHandoffBuilder`,
`AppLifecycleStartupHandoff`, `AppLifecycleStartupNextAction`,
`AppLifecyclePolicyBuilder`, `AppLifecycleCommand`,
`DuplicateLaunchHandoff`, `AutoLaunchBuilder`, and
`RecentDocumentsBuilder`, so Desktop `app.whenReady`, `window-all-closed`,
second-instance activation, login items, quit/relaunch, and recent-document
updates start from checked native routing before windows or OS state mutate.
Native image/icon quick
starts include `AppIconSetBuilder`, `AppIconAssetBuilder`, `AppIconPurpose`,
`AppIconFormat`, `AppIconCoverageSummary`, `AppPackageManifestBuilder`,
`FileIconRequestBuilder`, `FileIconSize`, `TrayIconBuilder`, `ImageSource`,
`img`, `RenderImage`, `ClipboardItem::builder`, `DrawContext::draw_image`,
`PrintContext::draw_image`, and `Window::drop_image`, so Desktop
`native image`, app/tray/document icons, `file icon request`, clipboard images,
canvas images, print images, and generated image payloads start from native
checked descriptors while browser image decoding, CSS filters, canvas pixel
extraction, cross-origin images, and web-only image editors stay explicit
WebView islands. Clipboard/editing quick starts include `ClipboardItem::builder`,
`ClipboardReadRequestBuilder`, `ClipboardEditingHandoffBuilder`,
`ClipboardEditingHandoff`, `ClipboardEditingNextAction`,
`MenuBuilder::standard_edit`, `cx.edit_command_state_snapshot_checked()`, and
`ClipboardClearBuilder`, so native Edit menus, paste flows, rich clipboard
payloads, and command-palette enablement do not depend on browser selection APIs
unless the editor itself is a hosted island. Notifications/shell quick
starts include `NotificationBuilder`,
`show_desktop_notification_with_action_router`, `ShellTargetsBuilder`,
`DeepLinkRouterBuilder`, `DeepLinkSetupPlan`, and `UserAttentionBuilder`, so
desktop-app `shell.openExternal`, `openPath`, `showItemInFolder`,
notifications, notification actions, deep links, and taskbar/dock attention
stay native by default. Capture/permissions quick starts include
`PermissionRequestBuilder::capture_studio()`, `AppPrivacyManifestBuilder`,
`CaptureSourceQueryBuilder::screens_and_windows()`, `CaptureConfigBuilder`,
`CaptureConfigSetBuilder`, `CaptureHandoffBuilder`, `CaptureHandoff`,
`CaptureHandoffNextAction`, `CaptureManager`, and `CapturePipeline`, so
desktop-app `capture source catalog`, camera/microphone checks, source pickers,
screen share, calls, and recordings start from native consent and capability
preflight.
Printing/protocol/path quick starts include `PrintJob`, `PrintRequest`,
`DocumentExportRequest`, `DocumentExportFormat`,
`DocumentExportDestination`, `DocumentOutputHandoffBuilder`,
`DocumentOutputHandoff`, `DocumentOutputNextAction`,
`CustomProtocolRouterBuilder`, `CustomProtocolFileResolverBuilder`,
`UrlSchemeRegistrationBuilder`, `DefaultHandlerPlanBuilder`,
`FileAssociationSetBuilder`, and `AppPathBuilder`, so Desktop `hosted document print`,
`hosted PDF export`, `hosted save-page export`, `protocol.handle`,
`default protocol registration`, document-default claims, and `app path lookup`
flows start from native checked descriptors while browser-owned print preview,
exact Chromium PDF pagination, and save-page resource serialization stay
explicit WebView islands or roadmap caveats.
Power/theme/idle quick starts include `PowerSaveBlockerBuilder`,
`PowerSaveBlockerStopBuilder`, `SystemPowerMonitorBuilder`,
`SystemPowerSourceQueryBuilder`, `NativeThemeSnapshot`,
`NativeThemeAdaptation`, `SystemIdlePolicyBuilder`, and
`SystemIdleEvaluation`, so Desktop `power-save blocker`, `power monitor`,
`nativeTheme`, and idle-gated background work stay native while browser page
visibility or wake-lock semantics remain explicit WebView islands.
Graphics/canvas quick starts include `graphics_capability_report()`,
`canvas(size, draw)`, `DrawContext`, `PathBuilder`, `ImageSource`, `svg()`,
`Lottie`, `effect_layer(...)`, and `HeadlessRenderer`, so canvas/SVG/design-tool
surfaces start native while browser-only WebGL, WebGPU, public render-target,
or custom-shader needs remain explicit WebView or roadmap items. Hardware quick
starts include `DeviceAccessRequest::{usb,hid,serial,bluetooth}`,
`DeviceAccessRequestBuilder`, `cx.device_access_request_checked(...)`,
`PermissionBroker`, matching `Capability::*` grants, and
`request.privacy_permission()` for manifests, so WebUSB/WebHID/Web Serial/Web
Bluetooth-style apps start from native descriptors and brokered consent rather
than hidden browser pages. Plugins/processes quick starts include
`PluginManifest`, `PluginPermissionManifest`, `HelperProcessLaunch`,
`HelperProcessLaunch::plugin_host(...)`, `ProcessSpawnOptionsBuilder`,
`PermissionBrokerInstallBuilder`, `ProcessContextBuilder`, `IpcSchema`, typed
worker/extension messages, and `CrashPolicy`, so Desktop `helper process`,
`utility process`, and plugin-host needs start from validated helpers, brokered
capabilities, typed IPC, and restart policy before any browser-hosted plugin UI
is considered. Packaging/update quick starts include
`AppPackageManifestBuilder`, `AppPackageReadinessBuilder`,
`AppDistributionPlanBuilder`, `AppSigningPlanBuilder`,
`AutoUpdaterConfigBuilder`, `UpdateInfoBuilder::build_signed_checked()`,
`AppUpdateOfferPolicyBuilder`, `DownloadExecutionPlan`, and
`RestartPathBuilder`, so Desktop-builder and `updater` flows start from
typed manifest, readiness, signing, feed, download, and relaunch contracts before
release portals or platform-specific backends enter the path. Accessibility
quick starts include `AccessibilityAttributes` recipes,
`AccessibilityTree::audit_report()`, `AccessibilityActionRouter`,
`AccessibilityAnnouncementBuilder`, `AccessibilityFocusBuilder`, semantic
`MenuEntry`/`Link`/`TreeItem`/`Navigator` summaries, and content-safe
`to_text()` methods, so DOM-like role/action/focus/audit needs start native
unless a legacy web UI must keep browser accessibility semantics. window management
quick starts include `WindowManagementHandoffBuilder`,
`WindowManagementNextAction`, `WindowIntentBuilder`, `WindowPlacementBuilder`,
`WindowControls`, `WindowChromeCommand`, `WindowPresentationPolicyBuilder`,
`WindowRuntimeSnapshotQueryBuilder`, `WindowInteractionCommand`,
`WindowZOrderPolicyBuilder`, `WindowOpacityBuilder`,
`WindowContentProtectionBuilder`, `SessionStore`, and
`SessionSnapshotBuilder`, so main, palette, utility, modal, popup, overlay,
custom-chrome, fullscreen, kiosk, always-on-top, translucent, protected, and
restored windows start from native checked APIs before browser-owned popup
islands enter the path. Menu/tray/dock/taskbar quick starts include
`MenuBarBuilder`, `MenuBarPlan`, `MenuBuilder::standard_edit`,
`TrayAppBuilder`, `TrayIconBuilder`, `TrayMenuBuilder`, `TrayTooltipBuilder`,
`NativeContextMenuBuilder`, `DockBadgeBuilder`, `DockMenuBuilder`,
`DockMenuActionBuilder`, `JumpListBuilder`, `JumpListPlan`,
`GlobalHotkeyBuilder`, `GlobalHotkeyUnregistration`, and
`WindowProgressBuilder`, so app menus, Edit roles, tray apps, context menus,
dock/taskbar badges and menus, Windows taskbar tasks, global shortcuts, and
progress indicators start from native checked command surfaces before hosted
context-menu semantics enter the path.
Network/realtime/download quick starts include `NetworkPolicyBuilder`,
`AppNetworkRequestBuilder`, `Capability::Network`, `PermissionBroker`,
`AppRealtimeConnection`, `AppRealtimeConnectionSet`,
`AppRealtimeReconnectPolicy`, `DownloadRequest`, `DownloadBatch`,
`DownloadExecutionPlan`, `DownloadHandoffBuilder`, `DownloadHandoff`,
`DownloadHandoffNextAction`, `NetworkStatusMonitorBuilder`, and
`AppHttpClientInstallBuilder`, so Desktop `fetch`, WebSocket, EventSource, and
Chromium-download replacements start from native checked descriptors while
OAuth, payments, maps, hosted docs, browser cookies, and browser-owned downloads
remain explicit WebView islands. Performance
diagnostics quick starts include `PerformanceEvidenceHandoffBuilder`,
`PerformanceEvidenceNextAction`, `AppRuntimeSnapshotQueryBuilder`,
`current_process_metrics()`, `AppResourceBudgetBuilder`, `BenchmarkHarness`,
`BenchmarkSampleApp`, `BenchmarkSamplePair`, and
`BaselineComparisonReport::generate_with_sample_pairs(...)`, so memory, CPU,
startup, idle, and "lighter than hosted runtime" claims require a checked
process-metrics/resource-budget/benchmark-evidence handoff before readiness is
stated. After workers return evidence for that handoff's next batch, call
`contract.continue_with_builder_handoff_evidence(&handoff, checklists)` to get a
refreshed contract plus the next app-wide handoff without dropping to lower
level batch plumbing. Use `contract.run_builder_handoff_loop(evidence_batches,
max_steps)` when an agent has multiple app-wide evidence submissions and needs a
bounded result with builder continuations, stop reason, final handoff, and a
compact final report. `contract.execution_coverage()` gives the count-level
front door across those kits: planned/open kit counts, blocker, native,
evidence, WebView, roadmap, feature-gap, starter, setup, criteria, and evidence
totals, plus native-generation and parity booleans. Call
`coverage.recommendation()` when a scheduler needs the routeable focus bucket
(`native-blockers`, `native-work`, `failed-evidence`, `evidence`,
`browser-islands`, `roadmap-caveats`, or `parity-claim`) before choosing a
filtered kit list. Use `contract.recommended_execution_kits()` or
`contract.execution_kits_for_recommendation(&recommendation)` to resolve that
route into the exact executable kits. Use
`contract.recommended_execution_batch()` when a queue wants the recommendation,
selected kits, and selected-kit counts as one serializable handoff. A batch can
call `batch.complete_with_evidence_checklists(checklists)` to return pass
outcomes for `contract.with_pass_outcomes(...)`, or callers can use
`contract.continue_with_execution_batch_evidence(&batch, checklists)` to get the
refreshed contract and next recommended batch in one handoff. The continuation
exposes `next_focus()`, `has_next_batch()`, and `should_continue()` so agents can
iterate without inferring terminal state from raw counts. Use
`continuation.remaining_work()` when the loop needs count-level reasons for
continuing: next focus, next kit count, planned/open kits, blocker/native/
evidence/WebView/roadmap counts, failed/missing evidence, and readiness state. Use
`contract.run_execution_evidence_loop(evidence_batches, max_steps)` when a
scheduler has multiple evidence batches and needs a bounded result with step
count, explicit stop reason, step-limit status, final readiness state, and final
remaining work. Call `loop_result.final_report()` for a compact final report
with stop reason, final action/focus, continue/parity flags, next-kit count, and
failed/missing evidence counts, or `loop_result.final_handoff()` when the
scheduler needs that report plus the next recommended batch as one serializable
handoff. The report exposes
`targets_native_work()`, `targets_browser_island()`,
`targets_roadmap_caveat()`, and `targets_parity_claim()` so schedulers can route
the final state without matching focus strings.
`DesktopCapabilityIntakeWorkOrder::to_text()` stays count-only. Workers can
call `work_order.complete_with_evidence(checklist)` to return an
`DesktopCapabilityGenerationPassOutcome`, then refresh the contract with
`contract.with_pass_outcomes(...)`.
`capability_matrix()` on the primitive audit when agents need count-level rows
with `can_start_native_work()`, `has_non_webview_gap()`, `feature_gaps()`,
native starter counts, setup check counts, WebView condition counts, roadmap
counts, and acceptance criterion counts before deciding whether a gap is native,
WebView, or roadmap. Matrix backlog items expose
`DesktopCapabilityBacklogKind::{ResolveNativeCapabilityGap, BuildReadyNativePrimitive, CollectAcceptanceEvidence, IsolateBrowserDependency, StateRoadmapCaveat}`
with count-safe `to_text()` summaries so agents can act without logging user
content. Matrix backlog tickets expose the same ordered item plus
`starter_recipe`, `scaffold_hint`, `acceptance_criteria`, and
`evidence_checklist`, so agents can generate or verify work without guessing
where the code belongs or which behavior checks are required. Ticket reviews
expose `next_kind`, `can_close_ticket`, failure counts, and missing counts so
the implementation loop remains explicit after evidence is collected. Matrix
backlog reviews roll those ticket reviews up into `closable_count()`,
`open_count()`, `blocking_count()`, `next_kind()`, and evidence totals without
logging criterion text; they also expose `current_ticket()` and
`current_evidence_checklist()` as the next concrete handoff.
`matrix.native_first_plan()` groups that same queue into native/platform
blockers, native primitive generation, acceptance evidence, deferred WebView
islands, and roadmap caveats. Use its `phases()`, `current_phase()`,
`current_phase_assignment()`, `current_execution_kit()`,
`current_execution_batch()`, `current_ticket()`, `current_generation_pass()`,
`ready_review()`, and `should_defer_webview()` helpers when an agent or UI must
prove it is prioritizing native work before browser compatibility islands.
Phase objects carry `assignment()`, `execution_kits()`, `execution_batch()`,
and `evidence_checklists()` so a whole phase can be assigned with count-only
scheduler data, concrete starter payloads, and proof work without flattening
WebView tickets ahead of native work. After workers return proof, call
`native_first.continue_with_execution_batch_evidence(&batch, checklists)` for a
refreshed review, claim, and next native-first batch. Use
`matrix.ready_parity_claim_decision()` / `native_first.ready_readiness_decision()`
before verification or `matrix.parity_claim_decision(checklists)` /
`native_first.readiness_decision(checklists)` after verification to get a
matrix-level claim status that blocks on native generation gaps, failed
evidence, missing evidence, deferred WebView islands, and roadmap caveats before
stating native desktop capability readiness.
`DesktopCapabilityGenerationPass` wraps that current review into a small
command surface with `resolves_blocker()`, `can_generate_native()`,
`should_collect_evidence()`, `should_isolate_browser_dependency()`, and
`should_state_roadmap_caveat()`. It also exposes pass-scoped
`feature_gaps()`, `native_starters()`, `setup_checks()`, `webview_islands()`,
`roadmap_items()`, `scaffold_hint()`, `acceptance_criteria()`, and
`expected_evidence_checklist()` so agents can execute the next pass without
walking nested ticket fields; after the run, call
`pass.review_evidence(checklist)` to re-enter the ticket review loop, or
`pass.complete_with_evidence(checklist)` when the worker should return an
outcome with the reviewed ticket and evidence checklist for matrix refresh. Use
`pass.brief()` for a count-level worker assignment summary that reports the
pass kind, primitive, area, blocker/evidence/generation flags, and required
starter/evidence buckets without logging paths, criteria text, or app content.
Call
`generation_blueprint()` on the primitive audit when agents need an ordered
handoff: resolve feature gaps, build native surfaces, isolate WebView islands,
then state roadmap items. For any native generation step, call
`step.starter_recipe()` or `DesktopPrimitive::starter_recipe()` to get concrete
API-family starters, setup checks, WebView island starters, and roadmap reminders
before writing app code.
For broader audits, use
`DesktopSurfaceAuditPlan::for_category(DesktopAppCategory::PluginHeavy, &CapabilityReport::current())`
or `DesktopSurfaceAuditPlan::all(&CapabilityReport::current())` to evaluate
the complete `DesktopSurfaceArea::all()` inventory: app chrome, embedded view
composition, layout/styling/animation, windows, screen/display topology, media,
audio graph/recording, files, message dialogs, filesystem/workspace access,
image/icon assets, app identity, drag/drop, clipboard/editing,
menus/tray/dock/taskbar, input/IME/shortcuts, localization/text, forms,
notifications/shell, printing/protocols/paths, app storage/sessions, secure
storage/credentials, lifecycle/single-instance, launch environment/config,
IPC/command messaging, security/permissions policy, background tasks/workers,
navigation/history/routing, find/zoom document tools, network/realtime
downloads, capture/permissions, power/theme/idle, graphics/canvas, visual
capture/snapshots, hardware devices, plugins/processes, packaging/updates,
accessibility/automation, performance diagnostics, crash reporting diagnostics,
developer tools/observability, WebView compatibility, and low-level GPU escape
hatches in one pass.
Call `audit.action_items()`, `audit.blocking_action_items()`,
`audit.work_queue()`, `audit.recommended_action_item()`, and
`audit.recommended_handoff()` when broad audits need serializable worker
assignments. `DesktopSurfaceAuditWorkQueue` groups prioritized handoffs into
blockers, native/platform work, explicit WebView islands, roadmap items, and
evidence totals so schedulers can route non-WebView native work directly.
Use `work_queue.current_assignment()` for the count-level worker focus and
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
`DesktopSurfaceAuditHandoff`
includes the action, surface area, evaluated area plan, implementation brief,
feature-gap details, native starter list, justified WebView conditions, and
roadmap items. It also exposes `acceptance_criteria()`,
`evidence_checklists()`, `evidence_report()`, `review_evidence(...)`, and
`review_evidence_checklists(...)`; `DesktopSurfaceAuditEvidenceReview` returns
the next broad-surface action after blockers, failed proof, missing proof,
WebView/roadmap caveats, or successful evidence. `to_text()` remains count-only
for safe logs. Use `audit.continue_with_handoff_evidence(...)` or
`audit.continue_with_handoff_evidence_checklists(...)` to produce a
`DesktopSurfaceAuditContinuation` with the next action, next handoff, and
continue/stop booleans for broad worker loops. Use
`audit.run_handoff_evidence_loop(evidence_batches, max_steps)` for a bounded
multi-step loop; `DesktopSurfaceAuditLoop` and
`DesktopSurfaceAuditLoopReport` expose the stop reason, final action, final
handoff presence, target helpers, and evidence counts. Call
`loop_result.readiness_decision()` before claiming broad desktop readiness; it
returns the final `DesktopParityClaimStatus` plus blocker, briefing, evidence,
and continue/stop booleans for the loop result. Call
`loop_result.final_loop_handoff()` when schedulers need the final report paired
with the next broad-surface handoff in one serializable object.
`loop_result.final_dossier()` is the builder-facing summary for dashboards,
agent messages, and release-readiness notes: it includes the readiness status,
final action, next area/action, target flags, audited area count, feature-gap
count, and evidence counts without app payloads.
Call `DesktopSurfaceAreaPlan::implementation_brief()` for any flagged area when
an agent needs concrete native primitives, justified WebView conditions, and
roadmap items before generating code. These area-level briefs now carry concrete
native starters such as `WindowChromeCommand`, `VideoUrlPlaybackHandoff`,
`TrayAppBuilder`, `HelperProcessLaunch::plugin_host`, and
`BaselineComparisonReport`, so broad audits remain actionable without relying on
generic placeholders.

For DOM-like native automation and accessibility checks, use Kael's typed
accessibility model instead of scraping rendered pixels. `AccessibilityRole`,
`AccessibilityState`, `AccessibilityAction`, `AccessibilityValue`,
`AccessibilityAttributes`, `AccessibilityNode`, and `AccessibilityTree` all
provide content-safe summaries through `to_text()` helpers. These report role,
state, action, value kind, label/value byte lengths, focus/actionability, hidden
state, child counts, and audit readiness without logging labels, field values,
placeholders, descriptions, action payload text, exact geometry, or audit
messages.

---

## File Dialogs

Use `FileDialogHandoffBuilder` when generated apps, plugins, or AI agents need
one checked descriptor for Desktop `showOpenDialog`/`showSaveDialog` parity,
raw path prompts, hosted picker islands, or roadmap dialog work before showing
file UI:

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
match handoff.next_action() {
    FileDialogNextAction::ShowOpenDialog => { /* show native open UI */ }
    FileDialogNextAction::ShowSaveDialog => { /* show native save UI */ }
    FileDialogNextAction::PromptForPath => { /* use raw PathPromptOptions */ }
    FileDialogNextAction::UseHostedFilePicker => { /* route to hosted picker */ }
    FileDialogNextAction::TrackDialogRoadmap => { /* track missing parity */ }
}
```

`FileDialogHandoff::to_text()` reports request counts and routing state without
logging prompt labels, filter names, suggested filenames, hosted surface IDs,
roadmap text, paths, or selected values.

Native open/save file pickers:

```rust
// Open file dialog
let open_plan = cx.open_dialog_checked(
    OpenDialogBuilder::files()
        .image_files()
        .filter("Markdown", ["md", "markdown"])
        .prompt("Open"),
)?;
assert_eq!(open_plan.filter_names(), vec!["Images", "Markdown"]);
assert_eq!(open_plan.filter_extension_count(), 9);
tracing::info!(summary = open_plan.to_text(), "open dialog");

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
let save_plan = cx.save_dialog_checked(
    SaveDialogBuilder::new(std::env::current_dir()?)
        .suggested_name("document")
        .text(),
)?;
assert_eq!(save_plan.suggested_name(), Some("document.txt"));
tracing::info!(summary = save_plan.to_text(), "save dialog");

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
Use `FileDialogFilter::to_text()`, `extension_count()`, and
`OpenDialogPlan::filter_extension_count()` when generated workflows need
path-safe filter summaries before showing native UI.
Use `SaveDialogBuilder::default_extension(...)`, `.pdf()`, `.text()`, or
`.json()` when a suggested save name should get a default extension only if the
user has not already supplied one.
Open prompts reject empty, padded, control-character, and overlong generated
labels. Save dialogs reject empty directories, empty or padded suggested names,
path separators in suggested names, and malformed default extensions.
Use `open_dialog_checked(...)` and `save_dialog_checked(...)` when generated
workflows, plugins, or AI agents need to preview selection mode, prompt text,
filter names, suggested save names, default-extension behavior, and required
user-selected filesystem capability before showing the native picker. Prefer
`file_dialog_handoff_checked(...)` when the workflow may choose between native
open, native save, raw path prompts, hosted picker fallback, and roadmap work.
Use `plan.to_text()` for path-safe logs and agent summaries before showing
native file UI.
The lower-level `prompt_for_paths(PathPromptOptions { ... })` and
`prompt_for_new_path(...)` calls remain available when you already have raw
options; use `PathPromptOptions::to_text()` when those raw options need the
same path-safe mode, prompt, filter, and extension-count summary.

For apps that reopen documents, projects, or export locations later, convert
user-approved paths into checked file access bookmarks:

```rust
let bookmark = cx.file_access_bookmark_checked(
    FileAccessBookmark::builder("project.main", project_dir)
        .scope(PathScope::UserSelected)
        .read_write()
        .require_existing_path()
        .canonicalize_path()
        .ttl_seconds(60 * 60 * 24),
)?;

let mut tokens = AccessTokenStore::new();
let token = bookmark.issue_token(&mut tokens, now_unix_seconds)?;
```

`cx.file_access_bookmark_checked(FileAccessBookmark::builder(...))` validates
stable bookmark IDs, path text, optional existence/canonicalization, read/write
mode, and token TTL. Use
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
tracing::info!(summary = drop.to_text(), "file drop intent");

for path in drop.paths() {
    open_media(path)?;
}
```

At the drop-zone boundary, summarize normalized external payloads before
touching files, text, or URLs:

```rust
let filter = FileDropFilter::media().max_files(4);

drop_zone
    .can_drop_external(filter.clone())
    .on_external_drop(move |data, _window, _cx| {
        tracing::info!(summary = filter.to_text(), "file drop filter");
        tracing::info!(summary = data.to_text(), "external drop");
    });
```

`ExternalDropData::to_text()`, `ExternalPaths::to_text()`,
`FileDropFilter::to_text()`, and `FileDropMatch::to_text()` report path, text,
URL, accepted, rejected, extension-filter, and max-file counts without logging
local paths, filenames, dragged text, URLs, or extension labels.

For Desktop `DragEvent` / `DataTransfer` parity, treat `ExternalDropData` as
the normalized native payload shape for files, text, and URLs. Use
`FileDropFilter` for hover feedback, `DataTransferDropIntakePlanBuilder`
before app state changes, `FileDropIntentBuilder` after the drop,
`FileExportDragIntentBuilder` for drag-out exports, `ClipboardItem::builder`
when app-owned text/html/image payloads need a transfer-compatible descriptor,
and `DragDropTransferHandoffBuilder` when generated builders or AI agents need
one checked decision point before accepting drops, starting export drags,
configuring internal drag routing, or delegating to hosted browser content. Use
WebView drag/drop handlers only when hosted content owns browser `DataTransfer`
semantics.

```rust
let intake = DataTransferDropIntakePlanBuilder::new(drop_data)
    .file_filter(FileDropFilter::media().max_files(4))
    .max_urls(2)
    .max_text_bytes(16 * 1024)
    .allow_missing_paths()
    .build_checked()?;

tracing::info!(summary = intake.to_text(), "data transfer drop intake");
match intake.next_action() {
    DataTransferDropIntakeNextAction::RoutePaths => {
        let files = intake.file_intake().unwrap();
        tracing::info!(summary = files.to_text(), "drop file intake");
    }
    DataTransferDropIntakeNextAction::RouteUrls => {}
    DataTransferDropIntakeNextAction::RouteText => {}
    DataTransferDropIntakeNextAction::ReviewMixedPayload => {}
    DataTransferDropIntakeNextAction::ReviewUnknownPaths => {}
    DataTransferDropIntakeNextAction::UseHostedDataTransfer => {}
    DataTransferDropIntakeNextAction::RejectDrop => {}
}

let handoff = DragDropTransferHandoffBuilder::media_drop(dropped_paths)
    .build_checked()?;
tracing::info!(summary = handoff.to_text(), "drag/drop handoff");
match handoff.next_action() {
    DragDropTransferNextAction::AcceptIncomingDrop => {
        let drop = handoff.incoming_drop_builder().unwrap().clone().build_checked()?;
        tracing::info!(summary = drop.to_text(), "file drop intent");
    }
    DragDropTransferNextAction::StartFileExportDrag => {
        // Start the native file-export drag from the checked export builder.
    }
    DragDropTransferNextAction::ConfigureInternalDrag => {
        // Enable internal drag handles/drop targets with pointer policy.
    }
    DragDropTransferNextAction::UseHostedDataTransfer => {
        // Let the WebView island own DOM DataTransfer semantics.
    }
}
```

`FileDropIntentBuilder` gives drops a semantic purpose such as open document,
import files, import folder, media source, project workspace, or a custom app
purpose. It validates non-empty paths, optional existence, file-vs-directory
policy, extension allowlists, max path count, canonicalization, and duplicate
paths before work starts. The lower-level drop-zone filter remains useful for
hover feedback; this intent builder is the app-owned gate after the user drops.
Use `FileDropIntentBuilder::to_text()` and `FileDropIntent::to_text()` for
path-safe agent traces that report purpose, path count, path-kind policy,
extension count, max-path policy, and canonicalization/existence checks.
Use `DragDropTransferHandoff::to_text()` and
`DragDropTransferNextAction::{AcceptIncomingDrop,StartFileExportDrag,ConfigureInternalDrag,UseHostedDataTransfer}`
to keep drag/drop routes explicit without logging dropped paths, file names,
text, URLs, MIME strings, generated byte contents, internal route ids, WebView
ids, coordinates, selectors, or drag payload contents.

For outbound drags, generated exports, and desktop-app file promises, build
a checked export descriptor before starting a platform drag session:

```rust
let export = cx.file_export_drag_checked(
    FileExportDragIntentBuilder::generated_files("Drag generated image.")
        .virtual_file_with_mime("preview.png", "image/png", image_bytes)
        .max_virtual_file_bytes(32 * 1024 * 1024),
)?;
assert_eq!(export.display_names(), vec!["preview.png"]);

if CapabilityReport::current().is_available(PlatformFeature::FileExportDrag) {
    // Hand export.items() to the native drag-source backend.
}
```

`FileExportDragIntentBuilder` supports existing file paths and virtual files
with generated bytes. It validates user-facing purpose text, item count, safe
file names, optional MIME types, non-empty virtual bytes, virtual file size
limits, optional existence for existing paths, and deduplicates repeated path
items. Existing-path exports declare a `Capability::FilesystemRead` requirement
with `PathScope::UserSelected` and `file_export_drag_checked(...)` preflights it
before a drag session begins; generated virtual files require no filesystem
capability. Inspect `item_count()`, `display_names()`, `existing_path_count()`,
and `virtual_file_count()` for export previews. This gives designers, media tools, and AI artifact generators an
app-owned native export path without forcing a WebView download.

After a dialog, drop, recent-document restore, or file deep link, use one
workspace-open handoff when the app may need project roots, documents, media,
and watcher setup together:

```rust
let handoff = cx.workspace_open_handoff_checked(
    WorkspaceOpenHandoffBuilder::paths(paths)
        .canonicalize_paths()
        .watch_depth(2),
)?;

tracing::info!(summary = handoff.to_text(), "workspace open handoff");

if let Some(watch_set) = handoff.watch_set() {
    watcher.watch_set(watch_set.clone())?;
}

match handoff.next_action() {
    WorkspaceOpenNextAction::OpenWorkspace => open_workspace(handoff.intake())?,
    WorkspaceOpenNextAction::OpenDocuments => open_documents(handoff.intake().document_paths())?,
    WorkspaceOpenNextAction::OpenMedia => open_media(handoff.intake().media_paths())?,
    WorkspaceOpenNextAction::ReviewArchives => review_archives(handoff.intake().archive_paths())?,
    WorkspaceOpenNextAction::ReviewUnknown => show_unknown_file_review(handoff.intake())?,
}
```

`WorkspaceOpenHandoffBuilder` wraps `FileIntakePlanBuilder` and
`FileWatchSetBuilder` for the common file/project app path. It classifies mixed
opened paths, derives workspace/project watcher roots from directories and
project-file parents, supports bounded watcher depth, and returns a
`WorkspaceOpenNextAction`. Inspect `workspace_entry_count()`,
`watch_root_count()`, `has_watch_set()`, `needs_unknown_review()`,
`can_open_known_entries()`, and `to_text()` for path-safe handoffs before
opening editors, media players, project trees, or archive review UI.

When an app only needs classification, use file intake directly before routing
paths to document, media, data, project, or archive handlers:

```rust
let intake = cx.file_intake_plan_checked(
    FileIntakePlanBuilder::new()
        .paths(paths)
        .canonicalize_paths()
    .reject_unknown(),
)?;

tracing::info!(summary = intake.to_text(), "file intake");

for video in intake.paths_of_kind(FileIntakeKind::Video) {
    open_video(video)?;
}

if intake.has_documents() {
    queue_editor_tabs(intake.document_paths())?;
}
```

`FileIntakePlanBuilder` validates non-empty paths, optional existence, max path
count, canonicalization, deduplication, and optional rejection of unknown file
kinds. Entries expose normalized extensions and coarse `FileIntakeKind` values:
directory, project, image, audio, video, PDF, text, data, archive, or unknown.
Use `entry_count()`, `kind_count(kind)`, `media_paths()`, `document_paths()`,
`project_paths()`, `archive_paths()`, and `unknown_paths()` to branch accepted
files into native players, editors, importers, or project loaders without
repeating extension scans. Use `to_text()` for path-safe logs and agent
summaries before routing local files.

For app-owned storage, project metadata, indexes, caches, and generated exports,
validate scoped file operations before dispatching real filesystem work:

```rust
let operations = cx.file_operation_handoff_checked(
    FileOperationHandoffBuilder::new()
        .read(AppPathRole::Config, "settings.json")
        .operation(
            FileOperationRequestBuilder::write(AppPathRole::Data, "projects/index.json")
                .create_parent_dirs()
                .overwrite()
                .max_bytes(64 * 1024),
        )
        .copy(
            AppPathRole::Data,
            "projects/index.json",
            AppPathRole::Cache,
            "snapshots/index.json",
        )
        .move_path(
            AppPathRole::Cache,
            "snapshots/index.json",
            AppPathRole::Cache,
            "snapshots/latest.json",
        )
        .delete(AppPathRole::Temp, "exports/draft.tmp")
        .hosted_file_manager("cloud-drive")
        .roadmap_work("bulk operation undo"),
)?;

match operations.next_action() {
    FileOperationNextAction::ReadScopedFile => {}
    FileOperationNextAction::WriteScopedFile => {}
    FileOperationNextAction::CopyScopedPath => {}
    FileOperationNextAction::MoveScopedPath => {}
    FileOperationNextAction::DeleteScopedPath => {}
    FileOperationNextAction::UseHostedFileManager => {}
    FileOperationNextAction::TrackFilesystemRoadmap => {}
}
```

`FileOperationHandoffBuilder` validates app-scoped roles, relative paths,
copy/move targets, overwrite and create-parent policy, byte budgets, hosted file
manager ids, and roadmap notes before any read, write, copy, move, or delete
side effect. It deliberately targets app-owned paths under `AppPathRole` roots;
user-selected external files should still enter through dialogs, drops, recent
documents, shell/trash requests, or explicit file-export flows. Use
`FileOperationHandoff::to_text()` and `FileOperationRequest::to_text()` for
agent-safe summaries without logging paths, filenames, file contents, hosted ids,
or roadmap text.

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
tracing::info!(summary = associations.to_text(), "file associations");

if associations.accepts_extension("md") {
    enable_markdown_open_flow();
}
```

`FileAssociationSetBuilder` is validation-only metadata for bundlers,
installers, docs, and agents. It normalizes extensions, validates MIME types,
and rejects duplicate extension or MIME claims across associations. Runtime file
opens still arrive through open requests, dialogs, recent documents, drops, or
platform-specific installer registration. Use association builder/set `to_text()`
for content-safe setup traces that avoid logging association names, extensions,
MIME types, or descriptions.

If a document app has an explicit runtime allowlist, compare runtime intake with
package/default-handler metadata before release:

```rust
let setup = FileDropIntentBuilder::open_document()
    .extensions(["kaelproj", "md"])
    .setup_plan_with_default_handler(&associations, &defaults);

tracing::info!(summary = setup.to_text(), "file handling setup");
```

`FileHandlingSetupPlan` reports extensions accepted by runtime file/drop intake
but missing from checked file associations or default-handler claims. Use exact
missing-extension getters in tests and setup screens; use `to_text()` for
content-safe generated summaries that expose counts and readiness without
logging extension labels or MIME types.

For Desktop `file icon request` style file explorers, recent-document rows,
upload pickers, and project launchers, build a checked native file icon request
before invoking a platform icon backend:

```rust
let image_assets = cx.image_icon_asset_handoff_checked(
    ImageIconAssetHandoffBuilder::new()
        .app_icons(
            AppIconSetBuilder::new()
                .icon(AppIconAssetBuilder::app("assets/app.icns"))
                .icon(AppIconAssetBuilder::tray("assets/tray.svg").template())
                .icon(AppIconAssetBuilder::document("assets/document.png").size_px(128)),
        )
        .file_icon(FileIconRequestBuilder::new("Draft.kaelproj").small())
        .tray_icon(TrayIconBuilder::png(include_bytes!("tray.png").to_vec()))
        .ui_image()
        .render_image()
        .clipboard_image()
        .canvas_image()
        .print_image()
        .drop_image()
        .hosted_image_island("web-image-editor")
        .roadmap_work("native resize encode pipeline"),
)?;

match image_assets.next_action() {
    ImageIconAssetNextAction::PrepareAppIconAssets => prepare_icons(),
    ImageIconAssetNextAction::RequestFileIcon => request_file_icon(),
    ImageIconAssetNextAction::ApplyTrayIcon => apply_tray_icon(),
    ImageIconAssetNextAction::RouteNativeImagePayload => route_native_image(),
    ImageIconAssetNextAction::UseHostedImageIsland => route_hosted_image_surface(),
    ImageIconAssetNextAction::TrackImageRoadmap => record_gap(),
}

let icon = cx.file_icon_request_checked(
    FileIconRequestBuilder::new(project_path)
        .large()
        .require_existing_path(),
)?;
tracing::info!(summary = icon.to_text(), "file icon request");

let planned = cx.file_icon_request_checked(
    FileIconRequestBuilder::new("Draft.kaelproj")
        .small(),
)?;
tracing::info!(summary = planned.to_text(), "file icon request");
```

`FileIconRequestBuilder` validates non-empty/NUL-free paths, optional existence
requirements, optional canonicalization, small/normal/large/custom icon sizes,
and generic extension fallback for planned or missing paths. It does not render
the icon by itself; it is the typed handoff to platform icon extraction. Use
`request.to_text()` for path-safe logs and agent summaries before platform icon
lookup.
Use `ImageIconAssetHandoffBuilder` when generated branding, file explorers,
tray/status UI, native image rendering, clipboard, canvas, print, drag/drop, or
hosted browser image islands need one checked route before side effects. Inspect
`ImageIconAssetNextAction` and `ImageIconAssetHandoff::to_text()` without
logging icon paths, asset names, file paths, raw bytes, image ids, dimensions,
colors, URLs, document names, clipboard payloads, generated image contents,
hosted surface ids, or roadmap text.

When setup code needs Desktop `default protocol registration` or
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

tracing::info!(summary = defaults.to_text(), "default handler plan");
```

`DefaultHandlerPlanBuilder` validates the app identifier, app name, URL schemes,
file associations, duplicate claims, and requested scope. It does not mutate OS
defaults by itself; use it as the typed handoff to installer code, first-run
setup, or platform-specific default-app registration. Use `to_text()` when
generated setup screens, release scripts, or agents need one deterministic
content-safe summary of claimed scheme/document counts, scope, and confirmation
policy without logging app IDs, app names, scheme names, extensions, or MIME
types.

When the app needs Electron-style identity coverage across About metadata,
packaging, URL/file handlers, window grouping, badges, and installer readiness,
wrap those native requests in one checked handoff before generating release or
setup code:

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

tracing::info!(summary = handoff.to_text(), "app identity handoff");
```

`AppIdentityMetadataHandoffBuilder` validates the same concrete builders used by
About dialogs, package manifests, URL schemes, file associations, default
handlers, icons, window grouping, and dock/taskbar badges. Its summary is
content-safe: it reports request kinds, coverage booleans, and the next action
without logging app names, app IDs, versions, URLs, schemes, extensions, badge
labels, icon paths, or roadmap text.

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
let icon_coverage = manifest.icons().coverage_summary();
tracing::info!(summary = icon_coverage.to_text(), "package icon coverage");
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
`AppIconSet::coverage_summary()` reports app, tray, document, and installer
coverage so generated package manifests can fail or warn before platform icon
conversion starts.
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
`AppDistributionPlanBuilder` covers the Desktop-builder target-list side of
the workflow. It validates an absolute output directory, known artifact formats
for macOS (`dmg`, `mac-zip`), Windows (`msi`, `nsis`), and Linux (`appimage`,
`deb`, `rpm`, `tar-gz`), duplicate format/channel pairs, and portable release
channel labels. The result derives predictable artifact paths from the checked
manifest, but still leaves the actual bundling/signing/notarization work to the
platform-specific packaging tool.

For generated release work, prefer one checked handoff before packaging,
signing, updater UI, crash diagnostics, or restart setup diverge:

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

tracing::info!(summary = release_handoff.to_text(), "release handoff");
```

`PackagingUpdateHandoffBuilder` validates package readiness, distribution
targets, signing plans, update release/policy state, crash reporter setup, and
restart paths in one release descriptor. When both distribution and signing are
present, the handoff checks that signing declarations cover every planned
platform. Its summary reports request kinds and next action without logging app
names, app IDs, versions, artifact paths, signing identities, release URLs,
crash endpoints, restart paths, or roadmap text.

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
tracing::info!(summary = usb.to_text(), "device access request");

let bluetooth = cx.device_access_request_checked(
    DeviceAccessRequest::bluetooth("Pair with the heart-rate strap.")
        .service_uuid("180D")
        .allow_background(),
)?;
tracing::info!(summary = bluetooth.to_text(), "device access request");

let plan = CapabilityReport::current().device_access_plan(&bluetooth);
tracing::info!(summary = plan.to_text(), "device access plan");

let handoff = cx.hardware_device_handoff_checked(
    HardwareDeviceHandoffBuilder::new()
        .device_access(bluetooth.clone())
        .policy_for_request(&bluetooth)
        .hosted_vendor_config("device-setup")
        .native_backend_work(DeviceAccessKind::Bluetooth, "native scanner"),
)?;
tracing::info!(summary = handoff.to_text(), "hardware device handoff");
```

`DeviceAccessRequestBuilder` validates user-facing purpose text, timeouts,
USB/HID vendor/product filters, serial port hints, Bluetooth service UUIDs, and
rejects filters that belong to a different device family. Gate execution with
`PlatformFeature::UsbDevices`, `HidDevices`, `SerialPorts`, or
`BluetoothDevices`, request the matching capability (`Capability::UsbDevice`,
`HidDevice`, `SerialPort`, or `Bluetooth`) through the permission broker, and
include `request.privacy_permission()` in packaging metadata. Inspect
`DeviceAccessRequestBuilder::to_text()`, `DeviceAccessRequest::to_text()`, and
the `has_vendor_id()`, `has_product_id()`, `has_service_uuid()`, and
`has_port_name_hint()` helpers before prompting or opening a backend; summaries
report device family and filter shape without logging purpose text, vendor or
product IDs, service UUIDs, port hints, or exact timeout values. Current
platform reports expose these as checked descriptors first; use
`CapabilityReport::device_access_plan(&request)` or
`device_access_plan_checked(builder)` to turn support levels into a concrete
next action. `DeviceAccessExecutionPlan::next_action()` returns
`OpenNativePath`, `RequestPermissionOrMetadata`, `UseGuardedNativeDescriptor`,
`ChangePolicyOrConfiguration`, or `BuildNativeBackend`; use
`requires_permission_or_metadata()`, `requires_guarded_native_descriptor()`,
`requires_policy_change()`, and `requires_native_backend_work()` to keep
permission setup, guarded native descriptors, policy fixes, and per-OS
discovery/IO backend work separate from browser-island decisions.
`HardwareDeviceHandoffBuilder` groups checked device descriptors, broker
capabilities, packaging privacy declarations, hosted vendor setup, and native
backend work into one inspected route. Use `HardwareDeviceNextAction` before
generated hardware apps prompt users, open privileged IO, embed setup pages, or
claim WebUSB/WebHID/Web Serial/Web Bluetooth parity. Its summary avoids logging
purpose text, vendor/product ids, service UUIDs, port hints, hosted surface ids,
backend reason text, or exact timeout values.

For generated security work, wrap the intended broker, process identity,
network policy, runtime OS permission preflight, and hosted-page permission
bridge in one checked handoff before mutating app state:

```rust
let handoff = cx.security_permission_handoff_checked(
    SecurityPermissionHandoffBuilder::new()
        .permission_broker_install(
            ProcessId(42),
            PermissionBrokerInstallBuilder::new()
                .grant(Capability::ShellExecute)
                .grant(Capability::Network {
                    hosts: vec!["api.example.com".into()],
                })
                .deny_ungranted(),
        )
        .network_policy(NetworkPolicyBuilder::new().allow_host("api.example.com"))
        .hosted_webview_permission("media"),
)?;

tracing::info!(summary = handoff.to_text(), "security handoff");
match handoff.next_action() {
    SecurityPermissionNextAction::InstallPermissionBroker => {}
    SecurityPermissionNextAction::ConfigureProcessContext => {}
    SecurityPermissionNextAction::BuildNetworkPolicy => {}
    SecurityPermissionNextAction::CheckOrGrantCapabilities => {}
    SecurityPermissionNextAction::PreflightRuntimePermissions => {}
    SecurityPermissionNextAction::UseHostedPermissionBridge => {}
}
```

`cx.security_permission_handoff_checked(...)` with `SecurityPermissionHandoffBuilder`
rejects empty handoffs, zero process ids,
invalid capability host lists, malformed network policies, padded hosted
permission keys, and oversized generated request batches. Use the
`installs_permission_broker()`, `configures_process_context()`,
`builds_network_policy()`, `preflights_runtime_permissions()`, and
`uses_hosted_permission_bridge()` helpers so agents can route native security
setup separately from explicit browser-island permission decisions without
logging capability labels, hosts, paths, permission reasons, process names, or
prompt details.

Install app capability policy through the checked broker builder before
generated code opens URLs, reads the clipboard, runs shell handoffs, shows
notifications, or starts workers/plugins:

```rust
let policy = cx.configure_permission_broker_checked(
    PermissionBrokerInstallBuilder::new()
        .grant(Capability::ShellExecute)
        .grant(Capability::Network {
            hosts: vec!["api.example.com".into()],
        })
        .deny_ungranted(),
)?;
tracing::info!(summary = policy.to_text(), "permission broker installed");

assert!(policy.grants(&Capability::ShellExecute));
```

`PermissionBrokerInstallBuilder` registers the current process class, applies a
`ThreatModel`, validates direct grants, and installs the broker atomically.
Network capability grants reject empty hosts, URL-shaped strings, paths,
duplicates, and oversized generated host lists. The install report exposes the
process id, process class, threat-model defaults, direct/current grants, and
fixed prompt policy so settings screens, audits, and agents can explain why a
desktop action is allowed or blocked. Use `policy.to_text()` for one stable
startup audit line. Raw `set_permission_broker(...)` remains available when an
app already owns broker construction.

When generated helpers, plugin hosts, or tests need to switch the app capability
context, use a checked process-context request:

```rust
let context = cx.set_current_process_id_checked(
    ProcessContextBuilder::worker(ProcessId(42)),
)?;
assert_eq!(context.process_class(), ProcessClass::Worker);
tracing::info!(summary = context.to_text(), "process context switched");
```

`ProcessContextBuilder::existing(id)` only switches to process ids already
registered with the permission broker. `worker(...)`, `utility(...)`,
`media(...)`, `extension(...)`, and `register(id, class)` register the process
class first, then switch the current capability identity. The switch report
exposes the previous process id, active process id/class, whether registration
occurred, and the capabilities now visible to native actions. Raw
`set_current_process_id(...)` remains available for apps that already maintain
their process registry. Use `context.to_text()` when startup logs or agents need
one line describing the active process identity and grant count.

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

Native message boxes and confirmations replace hosted runtime `native message dialog`
for app-owned info, warning, error, confirm, destructive, unsaved-change, and
about flows:

```rust
let prompts = cx.message_dialog_handoff_checked(
    MessageDialogHandoffBuilder::new()
        .message(MessageDialogBuilder::info("Export Complete", "The export finished."))
        .confirm(MessageDialogBuilder::confirm("Close Window?", "Close this project window?"))
        .destructive_confirm(MessageDialogBuilder::destructive_confirm(
            "Delete Draft?",
            "This cannot be undone.",
            "Delete",
        ))
        .unsaved_changes(MessageDialogBuilder::save_discard_cancel(
            "Save changes?",
            "This document has unsaved changes.",
        ))
        .about(AppMetadataBuilder::new("Kael Studio").version("1.2.3"))
        .hosted_browser_dialog("checkout-beforeunload")
        .roadmap_work("localized button roles"),
)?;

match prompts.next_action() {
    MessageDialogNextAction::ShowNativeMessage => show_message(),
    MessageDialogNextAction::ShowNativeConfirm => show_confirm(),
    MessageDialogNextAction::ShowDestructiveConfirm => show_destructive_confirm(),
    MessageDialogNextAction::ShowUnsavedChangesPrompt => show_unsaved_prompt(),
    MessageDialogNextAction::ShowAboutDialog => show_about(),
    MessageDialogNextAction::UseHostedBrowserDialog => route_hosted_dialog(),
    MessageDialogNextAction::TrackDialogRoadmap => record_gap(),
}

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

let dialog_plan = cx.message_dialog_checked(
    MessageDialogBuilder::save_discard_cancel(
        "Save changes?",
        "This document has unsaved changes.",
    ),
)?;
assert_eq!(dialog_plan.button_index("Don't Save"), Some(1));
assert_eq!(dialog_plan.default_button_label().map(|label| label.as_ref()), Some("Save"));
tracing::info!(summary = dialog_plan.to_text(), "message dialog");

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
button copy. `MessageDialogHandoffBuilder` validates native message,
confirmation, destructive, unsaved-change, About, hosted-browser-dialog, and
roadmap prompt requests before side effects. Inspect
`MessageDialogHandoff::to_text()` without logging dialog titles, messages,
detail text, button labels, app names, versions, document names, paths, URLs,
hosted ids, prompt inputs, returned business payloads, or roadmap text. Use
`message_dialog_checked(...)` when generated workflows,
plugins, or AI agents need to preview button order, default/cancel labels, and
returned indexes before showing native UI; `MessageDialogPlan::button_index`,
`default_button_label`, `cancel_button_label`, and `to_text` make generated
side effects index-safe and content-safe. Use `dialog_plan.to_text()` for
content-safe logs and agent summaries. `MessageDialogBuilder::info`,
`warning`, `error`, `confirm`, `destructive_confirm`, and
`save_discard_cancel` cover the common Desktop prompt recipes, while
`show_about_dialog_checked(...)` handles about dialogs. Keep browser
`alert`, `confirm`, `prompt`, `beforeunload`, and hosted form-validation
semantics inside WebView islands when a hosted page owns the interaction.
`MessageDialogBuilder::to_text()` and `DialogOptions::to_text()` provide the
same label-safe count/default/cancel summary before lower-level dispatch. Use lower-level
`show_dialog(DialogOptions { ... })` when you already have raw platform dialog
options. Use `.default_button(index)` and
`.cancel_button(index)` when you need to preserve desktop-app default or
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

assert_eq!(menu_plan.top_level_names(), vec!["File", "Edit"]);
tracing::info!(summary = menu_plan.to_text(), "menu bar");
```

`MenuBuilder::standard_edit(...)` provides the common desktop-app Edit menu
shape with native OS role mappings for Undo, Redo, Cut, Copy, Paste, and Select
All. Checked menu builders reject empty, padded, control-character, and overly
long labels, plus empty menus and duplicate top-level menu names before native
installation. Use `menu_bar_checked(...)` when generated apps, plugin
contributions, or AI agents need to preview top-level menu names, item counts,
action counts, native role usage, and system-menu usage before mutating the
native menu bar. Use `MenuBuilder::to_text()`, `MenuBarBuilder::to_text()`, and
`MenuBarPlan::to_text()` for content-safe menu summaries before installing
native chrome.

When updating menu item enablement, command palette rows, or toolbar buttons,
read one active-window edit snapshot instead of probing undo and redo
separately:

```rust
let edit = cx.edit_command_state_snapshot_checked()?;

if edit.can_undo() {
    let label = edit.undo_label().unwrap_or("Undo");
}
```

`EditCommandStateSnapshot` validates generated Undo/Redo labels before they are
shown in native menus or app chrome. `cx.edit_command_state_snapshot()` returns
a safe disabled snapshot when no active window can answer the focused edit
state. Use `edit.to_text()` for menu, command palette, and agent diagnostics;
it reports undo/redo availability plus label presence without logging the
generated Undo/Redo labels themselves.

Raw `set_menus(...)`, `Vec<Menu>` values, and `MenuItem::action(...)` remain
available for code that already validates or constructs menu trees manually.

For document, canvas, note, editor, and design-tool state that is not owned by a
hosted web editor, keep native app history in `UndoRedoManager` and group
multi-step operations with checked transactions:

```rust
history.begin_transaction_checked("move selected layers")?;
history.push(move_layer_change);
history.push(update_selection_change);
history.end_transaction_checked()?;

tracing::info!(summary = history.to_text(), "undo redo");
```

`UndoRedoManager::to_text()` reports undo count, redo count, total retained
history depth, max-depth pressure, open transaction state, and the number of
changes inside the open transaction without logging operation descriptions.
Use `undo_count()`, `redo_count()`, `transaction_change_count()`, and
`is_at_max_depth()` to drive menus, command palettes, autosave prompts, and
agent-visible diagnostics without depending on DOM editing state.

## App Commands

Commands are stable app-level actions for command palettes, menus, toolbar
buttons, plugin contributions, and agent-visible action lists:

```rust
cx.register_command_checked("editor.save", "Save", || {
    save_current_document();
})?;
let handoff = cx.command_ipc_handoff_checked(
    CommandIpcHandoffBuilder::register_command("editor.save", "Save"),
)?;
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

Checked registration validates generated command IDs and names, rejects duplicate
IDs, and leaves the existing command registry untouched on error. Raw
`register_command(...)` and lower-level `CommandRegistry::register_action(...)`
remain available when intentional replacement is desired.
For command-palette catalogs that are separate from execution, use
`CommandDescriptor::to_text()` and `CommandPalette::to_text()` before exposing
menus, toolbars, plugin commands, or agent action lists. These summaries report
command/category/keybinding/icon counts and byte lengths without logging command
IDs, labels, categories, shortcut strings, or icon names.
Use `cx.command_ipc_handoff_checked(CommandIpcHandoffBuilder::register_command(...))`,
`.palette_descriptor(...)`, `.ipc_request(...)`, `.ipc_response(...)`,
`.ipc_progress(...)`, `.ipc_cancel(...)`, `.extension_rpc(...)`, or
`.hosted_bridge(...)` before generated commands, plugin contributions, helper
IPC, and WebView bridge traffic are dispatched. Inspect `CommandIpcNextAction`,
`is_register_command()`, `is_palette_descriptor()`, `is_ipc_request()`,
`is_ipc_response()`, `is_ipc_progress()`, `is_ipc_cancel()`,
`is_extension_rpc()`, `is_hosted_bridge()`, typed accessors, and `to_text()` for
redacted routing without logging command ids, labels, categories, shortcuts,
icon names, correlation ids, payloads, bridge message kinds, or error text.

## App Keybindings

Application-local accelerators use the same action system as menus, command
palettes, and element handlers:

```rust
let shortcuts = cx.shortcut_input_handoff_checked(
    ShortcutInputHandoffBuilder::new()
        .app_keybindings(
            KeyBindingSetBuilder::new()
                .binding("secondary-k", command::OpenPalette)
                .binding_with_context(
                    "secondary-shift-f",
                    command::FormatDocument,
                    Some("Editor && mode == normal"),
                ),
        )
        .global_hotkeys(
            GlobalHotkeyBuilder::new()
                .parse_named_hotkey(1, "Command Palette", "cmd-shift-k")?
                .parse_named_hotkey(2, "Toggle Capture", "cmd-alt-c")?,
        )
        .global_hotkey_cleanup(GlobalHotkeyUnregistration::new().id(1).id(2))
        .keybinding_cleanup(KeyBindingClearBuilder::mode_changed("command mode"))
        .keyboard_layout(KeyboardLayoutSnapshotBuilder::new())
        .hosted_shortcut_island("rich-editor-beforeinput")
        .roadmap_work("gamepad shortcut capture"),
)?;

match shortcuts.next_action() {
    ShortcutInputNextAction::InstallAppKeybindings => install_keymap(),
    ShortcutInputNextAction::RegisterGlobalHotkeys => register_hotkeys(),
    ShortcutInputNextAction::UnregisterGlobalHotkeys => unregister_hotkeys(),
    ShortcutInputNextAction::ClearAppKeybindings => clear_keymap(),
    ShortcutInputNextAction::SnapshotKeyboardLayout => snapshot_layout(),
    ShortcutInputNextAction::UseHostedShortcutIsland => route_hosted_keyboard(),
    ShortcutInputNextAction::TrackShortcutRoadmap => record_gap(),
}

let keymap_plan = cx.key_bindings_checked(
    KeyBindingSetBuilder::new()
        .binding("secondary-k", command::OpenPalette)
        .binding_with_context(
            "secondary-shift-f",
            command::FormatDocument,
            Some("Editor && mode == normal"),
        ),
)?;
assert_eq!(keymap_plan.normalized_keystrokes(), vec!["cmd-k", "cmd-shift-f"]);

cx.bind_keys_checked(
    KeyBindingSetBuilder::new()
        .binding("secondary-k", command::OpenPalette)
        .binding_with_context(
            "secondary-shift-f",
            command::FormatDocument,
            Some("Editor && mode == normal"),
        ),
)?;

cx.clear_key_bindings_checked(
    KeyBindingClearBuilder::mode_changed("command mode"),
)?;
```

`KeyBindingSetBuilder` validates shortcut text, context predicates, duplicate
bindings, empty sets, and oversized generated keymaps before the app keymap is
mutated. Inspect builders with `to_text()`, `context_count()`,
`has_contexts()`, `platform_binding_count()`, and `has_platform_bindings()` for
content-safe shortcut audits before parsing or installing generated keymaps. Use
`key_bindings_checked(...)` when generated preferences, command palettes,
plugin keymaps, or AI agents need to preview normalized shortcuts, action names,
context usage, and `KeyBindingSetPlan::to_text()` before mutating live input handling. Use
`.platform_binding(...)` when loaded user keymaps should be mapped
through the current platform keyboard layout. `KeyBindingClearBuilder` validates
cleanup reasons before app-wide shortcut resets, such as plugin unloads or mode
switches. Raw `bind_keys(...)`, `clear_key_bindings()`, and
`KeyBinding::new(...)` remain available when the caller already owns parsing and
validation.
Use `ShortcutInputHandoffBuilder` when generated preferences, command palettes,
plugins, or AI agents need one checked shortcut workflow across app-local
accelerators, system-wide hotkeys, hotkey cleanup, keymap cleanup,
keyboard-layout snapshots, hosted keyboard islands, and roadmap work. Inspect
`ShortcutInputNextAction` and `ShortcutInputHandoff::to_text()` without logging
shortcut text, hotkey names, action names, context predicates, layout names,
hosted surface ids, cleanup reasons, roadmap text, typed text, selected text, or
composition payloads.

---

## App Globals

App globals store process-wide services, plugin state, theme bridges, and runtime
singletons:

```rust
cx.set_global(MyPluginState::new());

let removed = cx.remove_global_checked::<MyPluginState>(
    GlobalRemovalBuilder::extension_unloaded("vim-mode"),
)?;
```

Use `GlobalRemovalBuilder::mode_changed(...)` for app-mode or workspace-profile
resets, and `.require_present()` when absence should be treated as a failed
invariant. The checked path validates generated cleanup reasons and returns
`Ok(None)` for idempotent missing-state cleanup by default. Raw
`remove_global::<T>()` remains available when missing state should panic.

---

## Workspaces

Workspace apps can keep dockable panels and layout state in Kael's native
workspace model:

```rust
let workspace = cx.open_workspace();

let closed = cx.close_workspace_checked(
    WorkspaceCloseBuilder::switching_workspace("project-b"),
)?;
```

Use `WorkspaceCloseBuilder::session_teardown(...)` when closing because a
window, project, or restored session ended. The checked path validates generated
close reasons and returns whether a workspace was actually closed; add
`.require_open()` when closing without an active workspace should be an error.
Raw `open_workspace()` and `close_workspace()` remain available for callers that
already own the lifecycle invariant.

---

## Context Menus

Use `MenuCommandHandoffBuilder` when generated apps, plugins, or AI agents need
one checked descriptor for app menu bars, native context menus, edit-command
state, hosted context menus, or menu roadmap work before mutating native menus:

```rust
let handoff = cx.menu_command_handoff_checked(
    MenuCommandHandoffBuilder::new()
        .menu_bar(
            MenuBarBuilder::new()
                .menu(MenuBuilder::new("File").action("Open", menu_action::Open))
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
```

Inspect `MenuCommandNextAction` before installing a menu bar, showing a context
menu, snapshotting edit commands, routing to a hosted menu, or tracking missing
parity. `MenuCommandHandoff::to_text()` reports request counts and routing
state without logging menu labels, action IDs, hosted surface IDs, roadmap text,
or edit labels.

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
menus share the same native item model. Inspect `NativeContextMenuBuilder`
`item_count()`, `action_count()`, `toggle_count()`, `checked_toggle_count()`,
`submenu_count()`, `separator_count()`, and `to_text()` before showing generated
context menus so agents can report menu shape without logging labels or action
IDs. Use `show_context_menu(...)` when you already validated a raw item tree
yourself.

---

## System Tray

Tray icon with menu and click handling:

```rust
// Builder-friendly path
cx.set_tray_icon_checked(TrayIconBuilder::png(include_bytes!("icon.png").to_vec()))?;

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

let tray_plan = cx.tray_app_checked(
    TrayAppBuilder::new()
        .action("Show Window", "show")
        .toggle("Pause Sync", false, "pause-sync")
        .status_tooltip("My App - Running")
        .panel()
        .keep_alive_without_windows(true),
)?;
tracing::info!(summary = tray_plan.to_text(), "tray app");
assert_eq!(tray_plan.action_ids(), vec!["show", "pause-sync"]);

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
invalid tray surface. Use `tray_app_checked(...)` when generated background apps,
plugin contributions, or AI agents need to preview action IDs, item/toggle
counts, tooltip presence, panel behavior, and lifetime policy before changing OS
UI. Use tray app `to_text()` for a single content-safe summary that avoids
logging labels, action IDs, or tooltip text. Use `set_tray_menu_checked(...)`,
`set_tray_icon_checked(...)`, `set_tray_tooltip_checked(...)`, and
`set_tray_panel_mode(...)` when those pieces are owned by separate parts of the
app.
Inspect `TrayMenuBuilder::to_text()` or `TrayMenuItem::items_to_text(...)`
before installing generated tray menus. The summaries expose total item,
action, toggle, checked-toggle, submenu, separator, and max-depth counts without
logging menu labels or action IDs. Use the matching count helpers when tray
status UI, tests, or agents need a stable shape check before platform install.

For generated desktop shell work, prefer one checked handoff before tray,
window placement, taskbar/dock progress, badges, and attention requests drift
apart:

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
user attention, and roadmap shell work together. Its summary reports request
kinds and the next action without logging labels, action IDs, tooltip text,
progress scopes, badge labels, attention reasons, geometry, or roadmap text.

Use the lower-level checked builders directly when a subsystem owns one shell
piece independently.

Use `TrayIconBuilder::png(...)`, `ico(...)`, or `bytes(...)` for generated tray
assets and `clear()` when no icon should be shown. The checked path rejects
empty, too-small, oversized, and unknown-format byte buffers before platform tray
APIs receive them. PNG, ICO, GIF, JPEG, and WebP signatures are accepted; raw
`set_tray_icon(...)` remains available for already-validated or
platform-specific icon handling. Use `TrayIconBuilder::to_text()` for generated
logs and AI-agent summaries without recording encoded icon bytes.

Use `TrayTooltipBuilder::status(...)` or `text(...)` for short background-app
state and `clear()` when no tooltip should be shown. The checked path rejects
empty tooltips, padded text, control characters, and text longer than 256
characters before platform UI receives it. Use tooltip `to_text()` /
`has_tooltip()` / `is_clear()` for content-safe traces that avoid logging
tooltip text. Raw `set_tray_tooltip(...)` remains
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

// Read checked text from generated paste commands or app-owned import actions.
let read = ClipboardReadRequestBuilder::text("Paste into editor");
tracing::info!(summary = read.to_text(), "clipboard read");
if let Some(text) = cx.read_clipboard_text_checked(read)? {
    println!("Got: {}", text);
}
```

Use `ClipboardItem::builder()` when you need metadata, images, or multi-entry
payloads:

```rust
// desktop-app rich HTML with plain-text fallback.
cx.write_clipboard_html(
    "Quarterly report",
    "<strong>Quarterly report</strong>",
)?;

let item = ClipboardItem::builder()
    .try_text_with_json_metadata("formatted text", json!({"source": "my_app"}))?
    .image_ref(&image);
tracing::info!(summary = item.to_text(), "clipboard write");
cx.write_clipboard_item_checked(item)?;

let handoff = ClipboardEditingHandoffBuilder::read_any("Inspect paste payload")
    .build_checked()?;
tracing::info!(summary = handoff.to_text(), "clipboard editing handoff");
match handoff.next_action() {
    ClipboardEditingNextAction::WriteClipboard => {
        // Call cx.write_clipboard_item_checked(...) with the validated builder.
    }
    ClipboardEditingNextAction::ReadClipboard => {
        // Call cx.read_clipboard_item_checked(...) or read_clipboard_text_checked(...).
    }
    ClipboardEditingNextAction::ClearClipboard => {
        // Call cx.clear_clipboard_checked(...).
    }
    ClipboardEditingNextAction::SnapshotEditCommands => {
        // Call cx.edit_command_state_snapshot_checked().
    }
}

// Clear sensitive clipboard content after a timeout or explicit user action.
let clear_clipboard = ClipboardClearBuilder::new("Copied token expired");
tracing::info!(summary = clear_clipboard.to_text(), "clipboard clear");
cx.clear_clipboard_checked(clear_clipboard)?;

// Read rich clipboard contents.
let read = ClipboardReadRequestBuilder::any("Inspect paste payload");
tracing::info!(summary = read.to_text(), "clipboard read");
if let Some(item) = cx.read_clipboard_item_checked(read)? {
    tracing::info!(summary = item.to_text(), "clipboard item");

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
characters before platform clipboard code receives the payload and require the
current process to hold `Capability::ClipboardWrite`. Use `item.to_text()` for
content-safe clipboard logs and agent summaries before reading or writing text,
HTML, metadata, or images. Runtime `Image` values expose `format()`,
`byte_len()`, `has_bytes()`, and `to_text()` so generated clipboard/native-image
flows can inspect format and byte presence without logging raw image bytes. The
builder also exposes `entry_count()`,
`text_count()`, `image_count()`, `metadata_count()`, `text_len_bytes()`,
`has_text()`, `has_html()`, and `has_image()` before platform mutation. Use
`ClipboardReadRequestBuilder::text(reason)`, `.html(reason)`, `.image(reason)`,
or `.any(reason)` with `read_clipboard_text_checked(...)` or
`read_clipboard_item_checked(...)` for Desktop `clipboard.readText()` /
`clipboard.readHTML()` / `clipboard.readImage()` style reads from generated
paste/import flows; checked reads reject invalid reasons and missing expected
formats before checking `Capability::ClipboardRead`. Use
`ClipboardReadRequestBuilder::to_text()` for logs and agent summaries without
recording the reason or clipboard contents. Use
`clear_clipboard_checked(ClipboardClearBuilder::new(reason))` for Desktop
`clipboard.clear()` style privacy/reset flows; checked clears reject invalid
reasons before removing user-visible clipboard contents. Use
`ClipboardClearBuilder::to_text()` for logs and agent summaries without
recording the reason text. Use `ClipboardEditingHandoffBuilder` when generated
builders or AI agents need one checked descriptor for clipboard writes,
clipboard reads, clipboard clears, or active edit-command snapshots before
touching user-visible state. Inspect `ClipboardEditingNextAction` and
`handoff.to_text()` without logging clipboard text, HTML, metadata, image bytes,
clear/read reasons, command labels, selectors, or URLs. The lower-level
`write_clipboard_text(...)`, `write_clipboard_item(...)`,
`write_to_clipboard(...)`, and `clear_clipboard()` methods remain available for already-validated custom
integrations.

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

let sheet = ShareSheet::builder()
    .subject("Export bundle")
    .text("Assets are ready")
    .file(export_path)
    .build_checked()?;
tracing::info!(summary = sheet.to_text(), "share sheet");
tracing::debug!(summary = sheet.platform_support().to_text(), "share support");
cx.show_share_sheet_checked(sheet).await?;
```

Use `ShareItem::{text,url,file,files,image}` or
`ShareSheet::{text,url,file,files}` for one-line payloads, and
`ShareSheet::builder()` / `ShareSheetBuilder::new()` for export bundles. The
checked path validates at least one non-empty payload, URL schemes, image MIME
types and bytes, and file existence before invoking the platform backend.
`cx.share_support()` reports the current backend destinations, while
`cx.show_share_sheet_checked(sheet).await?` accepts a fully built `ShareSheet`.
Use `ShareSheetBuilder::to_text()` before building and `sheet.to_text()` after
building when logs, export flows, or agents need stable item, text, URL, file,
image, exclusion, and subject counts. `ShareItem::to_text()`,
`ShareImage::to_text()`, `PlatformShareSupport::to_text()`, and
`ShareResult::to_text()` cover per-payload, destination-support, and completion
traces without logging text bodies, URLs, file paths, image MIME strings,
suggested names, subjects, or platform activity identifiers.

---

## Secure Credentials

Store login tokens, refresh tokens, service credentials, and Desktop
`secure storage` replacements in the platform keychain / credential manager:

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

tracing::info!(summary = handoff.to_text(), "secure credential handoff");

let credential = CredentialBuilder::new("https://api.example.com")
    .username("ada")
    .password(refresh_token);
tracing::info!(
    summary = credential.clone().build()?.to_text(),
    "credential write"
);

let write = cx.write_secure_credential(credential)?;

write.await?;

if let Some(credential) = cx
    .read_secure_credential_checked(CredentialServiceBuilder::new("https://api.example.com"))?
    .await?
{
    tracing::info!(summary = credential.to_text(), "credential read");
    println!("credential for {}", credential.username());
}

let service = CredentialServiceBuilder::new("https://api.example.com");
tracing::info!(summary = service.to_text(), "credential delete");
cx.delete_secure_credential_checked(
    service,
)?.await?;
```

Use these builders before reaching for encrypted token files, JSON settings, or
WebView localStorage. Prefer `SecureCredentialHandoffBuilder` when generated
code needs to coordinate keychain support, permission broker setup, writes,
reads, deletes, redacted diagnostics, hosted auth fallback, or roadmap work in
one checked object. The handoff summary reports request kinds and next action
without logging service keys, usernames, secret bytes, token values, hosted
profile IDs, or roadmap text. `CredentialBuilder` validates the service key,
username, and secret before calling the OS keychain API. Use
`CredentialServiceBuilder` for read/delete
paths so generated service values are checked too. Service and username values
may not be empty, accidentally padded with whitespace, overly long, or contain
control characters, and secrets may not be empty. Use `to_text()` on checked
write requests, stored credentials, and service builders for secret-safe audit
logs; summaries include byte counts, not secret contents. The lower-level
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
flows, `screen_capture()` when a capture tool only needs a screen-capture
preflight, and `media_devices()` when you only need microphone and camera.
Screen capture reports `Granted` when the current build/platform exposes capture
support and `Denied` when it does not; platforms may still show their own picker
or OS prompt when sources are queried. Inspect `requested_permissions()`,
`granted_permissions()`, `pending_permissions()`, `granted_summary()`,
`blocking_denial_summary()`, and `to_text()` to drive setup screens, fallbacks,
logs, and settings links without parsing OS-specific strings.
Before requesting grouped permissions in generated apps, pair the runtime
preflight with checked privacy metadata via `plan_against_manifest(...)`.
`PermissionPreflightPlan` reports `requested_count()`,
`manifest_backed_count()`, `missing_manifest_permissions()`,
`missing_manifest_count()`, `manifest_complete()`, `requires_manifest_update()`,
and `to_text()` so setup screens and agents can catch missing package rationale
before an OS prompt appears. Accessibility is reported separately because it is
an OS setup flow rather than a privacy-manifest declaration.

Use the lower-level `accessibility_status()`, `microphone_status()`,
`camera_status()`, and individual request methods when a feature needs to ask
for exactly one permission at the moment of use.

---

## Accessibility Semantics

For generated accessibility and automation work, validate the whole handoff
before exporting a platform tree, routing an action, announcing status, moving
focus, or delegating to a hosted DOM island:

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
match handoff.next_action() {
    AccessibilityAutomationNextAction::AuditAccessibilityTree => {}
    AccessibilityAutomationNextAction::ValidateAttributes => {}
    AccessibilityAutomationNextAction::RouteActionRequest => {}
    AccessibilityAutomationNextAction::AnnounceStatus => {}
    AccessibilityAutomationNextAction::FocusAccessibilityNode => {}
    AccessibilityAutomationNextAction::UseHostedDomAutomation => {}
}
```

`cx.accessibility_automation_handoff_checked(...)` and the underlying
`AccessibilityAutomationHandoffBuilder` reject malformed trees, custom
interactive attributes without names/actions, zero action-request ids, empty or
padded announcements, hidden/missing focus targets, malformed hosted surface
ids, and oversized generated batches. Use the handoff booleans to keep native
accessibility and automation evidence separate from explicit WebView
DOM/selector automation without logging labels, values, placeholders,
descriptions, payload text, audit messages, hosted selectors, URLs, or geometry.

Custom UI should declare semantic roles, labels, values, states, and actions
before it is exposed to the platform accessibility backend:

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
progress bars, and text inputs. Use `AccessibilityAttributes::validate()` for a
fail-fast component check, and `audit_report()` when generated UI or AI agents
need every issue at once. Use `report.to_text()` for a content-safe issue,
error, warning, and readiness summary before exposing generated custom controls.
Full `AccessibilityTree::audit_report()` catches tree-level problems such as
missing children, parent mismatches, multiple focused nodes, hidden focused
nodes, missing interactive names/actions, conflicting states, unknown roles, and
invalid range values before emitting a platform tree.

Use checked live announcements and focus changes for custom controls, async
workflows, and generated test harnesses:

```rust
window.announce_accessibility_checked(
    AccessibilityAnnouncementBuilder::new("Upload complete"),
)?;
window.focus_accessibility_node_checked(
    AccessibilityFocusBuilder::new(save_button_id),
)?;
```

The checked announcement path rejects empty, padded, control-character, and
overly long live-region text. Checked focus rejects missing or hidden nodes
before mutating the current accessibility tree. Raw
`announce_accessibility(...)` and `focus_accessibility_node(...)` remain
available when an app already owns validation.

---

## Global Hotkeys

System-wide keyboard shortcuts (work even when app is unfocused):

```rust
let hotkey_builder = GlobalHotkeyBuilder::new()
    .parse_named_hotkey(1, "Command Palette", "cmd-shift-k")?
    .parse_named_hotkey(2, "Toggle Capture", "cmd-alt-c")?;
tracing::info!(summary = hotkey_builder.to_text(), "global hotkey builder");
let hotkey_plan = cx.global_hotkeys_checked(hotkey_builder)?;
assert_eq!(hotkey_plan.ids(), vec![1, 2]);
tracing::info!(summary = hotkey_plan.to_text(), "global hotkeys");

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

let cleanup = GlobalHotkeyUnregistration::new().id(1).id(2);
tracing::info!(summary = cleanup.to_text(), "global hotkey cleanup");
cx.unregister_global_hotkeys_checked(cleanup)?;
```

The lower-level ID API remains available when you already parsed the keystroke:

```rust
cx.register_global_hotkey(1, &Keystroke::parse("cmd-shift-k")?)?;
cx.unregister_global_hotkey(1);
```

Use `register_global_hotkeys(...)` for permissive raw sets. The checked builder
path rejects empty sets, duplicate IDs, duplicate keystrokes, and invalid
generated names before platform registration begins. Use
`GlobalHotkeyBuilder::to_text()` before validation/registration and
`global_hotkeys_checked(...)` when generated preferences, plugins, or AI agents
need to preview ids, name counts, and parsed keystrokes before binding
system-wide input; use per-hotkey `to_text()` and `hotkey_plan.to_text()` for
registration summaries without logging hotkey names or shortcut text. Use
`GlobalHotkeyUnregistration::new().id(id)` or
`.hotkey_set(&registered_hotkeys)` when disabling shortcuts, unloading plugins,
or tearing down window-scoped commands; use `request.to_text()` for cleanup
summaries, and checked unregistration rejects empty
cleanup requests and duplicate IDs before platform calls run.

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
let query = FocusedWindowQuery::builder()
        .external_only()
        .require_title()
        .require_pid()
    .app_name_contains("code");
tracing::info!(summary = query.to_text(), "focused window query");

if let Some(info) = cx.focused_window_info_checked(query)? {
    tracing::info!(summary = info.to_text(), "focused window");
    println!(
        "focused app={} pid={:?} title={}",
        info.app_name, info.pid, info.window_title
    );
}
```

`FocusedWindowQuery` rejects contradictory generated filters, empty or padded
app names, control characters, zero PIDs, and exact-plus-contains app-name
filters before platform state is read. Use `to_text()` / `has_filter()` /
`has_process_scope()` on queries and `to_text()` / `has_title()` /
`has_bundle_id()` / `has_pid()` on results for content-safe traces that avoid
logging active app names, window titles, bundle IDs, or process IDs. Use
`.external_only()` when the feature should act on another app,
`.current_process_only()` for app-owned windows, `.bundle_id(...)` for macOS app
targeting, `.pid(...)` for exact process matching, and `.require_title()` when
an empty title should be treated as no match.

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

let notification = NotificationBuilder::new("Build Complete", "All tests passed");
tracing::info!(summary = notification.to_text(), "notification");
cx.show_desktop_notification(notification)?;

let update_notification =
    NotificationBuilder::new("Update Available", "Version 2.0 is ready to install")
        .critical()
        .tag("update-available")
        .group("updates")
        .timeout_secs(30)
        .open_and_dismiss_actions("Install Now", "Remind Later");
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
            defer_update();
        }
    },
)?;

cx.show_desktop_notification_with_action_router(
    NotificationBuilder::new("Sync Failed", "Could not reach the server")
        .retry_action("Retry")
        .settings_action("Settings")
        .dismiss_action("Later"),
    |action| {
        if action.is_retry() {
            retry_sync();
        } else if action.is_settings() {
            open_sync_settings();
        }
    },
)?;
```

`NotificationBuilder` rejects empty, padded, control-character, overly long, and
duplicate action data before callback routing becomes ambiguous. It also tracks
delivery intent metadata with `.low_priority()`, `.critical()`, `.silent(...)`,
`.deliver_silently()`, `.tag(...)`, `.group(...)`, `.timeout_ms(...)`, and
`.timeout_secs(...)`; these hints let builders express desktop-app urgency,
silent delivery, replacement, grouping, and expiration intent even when a
particular platform backend only honors part of the metadata today. Inspect
`urgency_level()`, `is_silent()`, `has_tag()`, `has_group()`, `has_timeout()`,
and `timeout_millis()` when app policy needs exact metadata. Use
`.open_and_dismiss_actions(...)`, `.retry_action(...)`, and
`.settings_action(...)` for common native notification flows; use
`.action(id, label)` when the app owns stable custom action IDs. Prefer
`show_desktop_notification_with_action_router(...)` for generated action
callbacks; it maps platform IDs into `NotificationActionEvent::Known(...)` or
`Unknown(...)` and exposes helpers such as `is_open()`, `is_dismiss()`,
`is_retry()`, and `is_settings()`. Use `notification.to_text()` and
`action.to_text()` for content-safe logs and agent summaries before dispatch or
inside callbacks; summaries report urgency, metadata presence, counts, and
booleans without logging title, body, tags, groups, action labels, raw action
IDs, unknown platform action IDs, or timeout values. Use
`action_ids()` only when the app intentionally needs to inspect exact IDs.
When platform support is known, use `NotificationFeatureSupport` and
`notification.delivery_plan(...)` before dispatch to identify metadata that will
degrade. `NotificationFeatureSupport::basic()` represents title/body-only
delivery, `.actions()` adds action buttons, and `.rich()` marks all builder
metadata as supported. `NotificationDeliveryPlan::missing_features()`,
`missing_feature_count()`, `is_fully_supported()`, `requires_fallback()`, and
`to_text()` let generated apps decide whether to drop actions, route to an
in-app inbox, or show a simpler notification before platform variance surprises
the callback path.
When notification actions also need shell follow-up or user attention, build a
single handoff before dispatch:

```rust
let handoff = cx.notification_flow_handoff_checked(
    NotificationFlowHandoffBuilder::rich(
        NotificationBuilder::new("Export Ready", "Open or reveal the report")
            .open_and_dismiss_actions("Open", "Later")
            .tag("export-ready"),
    )
    .shell_targets(
        ShellTargetsBuilder::new()
            .url("https://example.com/help/export")
            .reveal_path(report_path)
            .require_existing_paths(),
    )
    .attention(UserAttentionBuilder::informational().reason("export finished")),
)?;

tracing::info!(summary = handoff.to_text(), "notification flow handoff");
match handoff.next_action() {
    NotificationFlowNextAction::DispatchPlain => dispatch_plain(handoff.delivery_plan()),
    NotificationFlowNextAction::DispatchWithActionRouter => dispatch_with_router(&handoff),
    NotificationFlowNextAction::RequestAttentionThenDispatch => request_attention_and_dispatch(&handoff),
    NotificationFlowNextAction::UseFallbackUi => show_in_app_inbox(handoff.delivery_plan()),
}
```

`NotificationFlowHandoffBuilder` validates notification metadata, backend
support, optional shell targets, and optional `UserAttentionBuilder` before any
platform side effect. `NotificationFlowHandoff` exposes `delivery_plan()`,
`shell_targets()`, `attention()`, `next_action()`, `requires_fallback()`,
`has_actions()`, and `to_text()` so generated code can decide between plain
dispatch, action-router dispatch, attention-then-dispatch, or in-app fallback
without logging titles, bodies, action labels, URLs, paths, tags, groups, or
attention reasons.

Inside the action router, use a checked follow-up plan before running app
commands or shell side effects:

```rust
let notification = NotificationBuilder::new("Export Ready", "Open or reveal the report")
    .open_and_dismiss_actions("Open", "Later");

cx.show_desktop_notification_with_action_router(notification.clone(), move |action| {
    let follow_up = NotificationActionFollowUpBuilder::new(notification.clone(), action)
        .app_command_for_action(NotificationAction::OPEN_ID, "exports.open-latest")
        .shell_targets(
            ShellTargetsBuilder::new()
                .url("https://example.com/help/export")
                .reveal_path(report_path.clone())
                .require_existing_paths(),
        )
        .fallback_unknown_actions();

    match follow_up.build_checked() {
        Ok(plan) => match plan.next_action() {
            NotificationActionFollowUpNextAction::RunAppCommand => run_export_command(),
            NotificationActionFollowUpNextAction::OpenShellTargets => open_export_targets(),
            NotificationActionFollowUpNextAction::RequestAttention => request_attention(),
            NotificationActionFollowUpNextAction::ShowFallbackUi => show_export_inbox(),
            NotificationActionFollowUpNextAction::IgnoreUnknownAction => {}
            NotificationActionFollowUpNextAction::AcknowledgeAction => {}
        },
        Err(error) => {
            tracing::warn!(?error, "invalid notification action follow-up");
        }
    }
})?;
```

`NotificationActionFollowUpBuilder` validates that known action ids were
declared on the notification, app-command mappings target declared actions,
shell targets are checked before use, attention requests are valid, and unknown
platform action ids have an explicit ignore or fallback policy. Its summaries do
not log notification copy, action labels, command ids, URLs, paths, or attention
reasons. Use `cx.notification_action_follow_up_checked(...)` when the callback
is routed through app state and should preflight shell capabilities before any
open/reveal side effect.
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

let shell_plan = cx.shell_targets_checked(
    ShellTargetsBuilder::new()
        .url("https://example.com/docs/export")
        .reveal_path(report_path)
        .require_existing_paths(),
)?;
tracing::info!(summary = shell_plan.to_text(), "shell targets");
assert!(shell_plan.requires_open_external_url());
assert!(shell_plan.requires_shell_execute());

// Validate a move-to-trash/recycle request before shell integration handles it.
let trash = cx.trash_request_checked(TrashRequest::builder(report_path).canonicalize_path())?;
tracing::info!(summary = trash.to_text(), "trash request");

// Dispatch the checked trash/recycle handoff after confirmation.
let trashed = cx.trash_item_checked(TrashRequest::builder(report_path).canonicalize_path())?;
tracing::info!(summary = trashed.to_text(), "trash request");
```

`open_external_url(...)` uses the `OpenExternalUrl` capability. `open_path(...)`
and `show_item_in_folder(...)` use the higher-risk `ShellExecute` capability.
`ShellTarget::validate()` and `ShellTargetsBuilder` reject empty or padded URLs,
unsupported shell URL schemes, missing HTTP(S) hosts, empty paths, and NUL
characters before opening each target in order. `shell_targets_checked(...)`
returns a `ShellTargetsPlan` that exposes ordered targets, path-canonicalization
state, and the URL/shell capabilities needed before dispatch, which lets export
screens, logs, and AI agents preview side effects before handing work to the OS.
Use builder or plan `to_text()` for content-safe traces that classify URL, path,
and reveal targets without logging URLs or local paths. Use
`.require_existing_paths()` for export/reveal workflows and
`.canonicalize_paths()` when generated paths should be normalized first. Shell
URL targets intentionally allow `http`, `https`, and `mailto`; custom app
schemes should use the deep-link registration APIs. The lower-level
`open_url(...)`, `open_with_system(...)`, and `reveal_path(...)` calls remain
available for platform integrations that already manage capability boundaries.
For Desktop `shell.trashItem(...)` style flows, `TrashRequestBuilder` validates
empty paths, NUL bytes, filesystem roots, relative paths unless explicitly
allowed, and missing targets by default. The checked request does not permanently
delete anything; it previews the typed handoff for a platform trash/recycle
backend. Use `display_name()`, `parent_path()`, `requires_shell_execute()`, and
content-safe `to_text()` to drive confirmations, logs, tests, and agents before
the backend receives the request without logging the target path or filename.
Call `trash_item_checked(...)` after confirmation to validate, capability-check,
and dispatch the request to the platform trash/recycle hook. macOS, Windows,
and Linux currently provide native trash/recycle dispatch; unsupported platform
backends return an explicit error until their native hook is implemented.

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
tracing::info!(summary = paths.to_text(), "app paths");

let settings_path = paths.config_dir().unwrap().join("settings.json");
let cache_dir = paths.cache_dir().unwrap();
let log_dir = paths.logs_dir().unwrap();
let download_dir = paths.downloads_dir().unwrap();
```

`AppPathBuilder` validates the app id, rejects duplicate roles, and resolves
common Desktop `app path lookup` equivalents: `Data`, `Config`, `Cache`,
`Logs`, `Temp`, and `Downloads`. App-owned roles are scoped by the app id;
`Downloads` returns the user's downloads directory. Use `.create_dirs()` when a
startup path should exist before migrations, logs, databases, downloads, or
background workers begin. Inspect `AppPathBuilder::to_text()`,
`AppPathSet::to_text()`, `role_count()`, `app_scoped_role_count()`,
`user_global_role_count()`, and `has_role(...)` for generated setup logs without
printing app ids or absolute paths.

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
tracing::info!(summary = storage.to_text(), "app storage plan");

let db_path = storage.entry("main-db").unwrap().absolute_path();

storage.ensure_directories_checked()?;
```

`AppStoragePlanBuilder` resolves the needed app path roles and checks every
entry before migrations, settings loads, caches, or background workers start.
Entries declare a kind (`SettingsJson`, `SqliteDatabase`, `KeyValueStore`,
`BlobCache`, `LogFile`, `TempWorkspace`, or custom), durability (`Durable`,
`Rebuildable`, or `Temporary`), relative path, optional max byte budget, and
sensitivity for diagnostics. Paths must stay relative to app-owned roles and
cannot target `Downloads`; duplicate ids, unsafe names, parent-directory
escapes, absolute paths, and invalid quotas fail early. Use entry
`required_directory()` or plan-level `required_directories()` for preflight UI
and `ensure_directories_checked()` before opening storage. Use entry
`read_capability()` / `write_capability()` when wiring worker or plugin
permissions. Inspect `AppStoragePlanBuilder::to_text()`,
`AppStoragePlan::to_text()`, `AppStorageEntry::to_text()`, `entry_count()`,
`durability_count(...)`, `sensitive_count()`, `quota_count()`, and
`required_directories()` before opening files or creating directories. Summaries
report storage classes, durability, roles, sensitivity, quota presence, and
counts without logging app ids, entry ids, relative paths, absolute paths,
custom storage kind strings, or quota sizes.

When generated code or an agent has to decide whether it is resolving app
paths, preparing a storage map, running migrations, cleaning storage, or
opening a hosted WebView profile boundary, wrap the request in a checked
handoff first:

```rust
let handoff = cx.app_storage_session_handoff_checked(
    AppStorageSessionHandoffBuilder::storage_plan(
        AppStoragePlanBuilder::new("com.example.app")
            .settings_json("settings", "settings.json")
            .sqlite_database("main-db", "state/app.sqlite")
            .blob_cache("thumbnails", "thumbnails"),
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
`.migration(...)`, `.cleanup(...)`, or `.hosted_profile_storage(...)` before
opening settings, SQLite, caches, temp workspaces, cleanup jobs, or persistent
WebView profiles. Inspect `is_paths()`, `is_storage_plan()`, `is_migration()`,
`is_cleanup()`, `is_hosted_profile_storage()`, typed builder accessors, and
`to_text()` for agent routing without logging app ids, paths, storage entry
ids, profile ids, cookies, tokens, or stored values.

When replacing Electron/Chromium profile usage, classify browser-owned state
before choosing a storage route:

```rust
let bridge = cx.browser_profile_storage_bridge_checked(
    BrowserProfileStorageBridgePlanBuilder::new("com.example.app")
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
        .http_cache_cleanup("cache-reset"),
)?;

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

`BrowserProfileStorageBridgePlanBuilder` handles the fuzzy Electron profile
area explicitly: localStorage, sessionStorage, IndexedDB, cookies, CacheStorage,
HTTP cache, service workers, auth sessions, drafts, and custom browser-profile
state must be routed to native storage, secure credentials, hosted WebView
profile storage, cleanup, a browser island, or roadmap work. Native destinations
require an `AppStoragePlanBuilder`, hosted/browser destinations require a hosted
profile id, cleanup destinations require a cleanup plan, and secure credential
destinations must be marked sensitive. Summaries report counts and next action
without logging app ids, profile ids, item ids, origins, keys, cookie names,
paths, or values.

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

let startup_handoff = cx.launch_environment_handoff_checked(
    LaunchEnvironmentHandoffBuilder::new()
        .launch_context(
            LaunchContextBuilder::new()
                .environment_keys(["KAEL_PROFILE", "APP_CHANNEL"])
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
                .keys(["KAEL_PROFILE", "APP_CHANNEL"]),
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

let arg_policy = LaunchArgumentPolicyBuilder::new()
    .allow_file_paths()
    .url_scheme("kael")
    .flag("--safe-mode")
    .build_checked()?;
let args = arg_policy.classify_snapshot(&launch)?;

let env_policy = LaunchEnvironmentAllowlistBuilder::new()
    .keys(["KAEL_PROFILE", "APP_CHANNEL"])
    .build_checked()?;
let env = env_policy.summarize_snapshot(&launch)?;

if launch.is_development_mode() {
    tracing::info!("running a development build");
}

let duplicate_payload = DuplicateLaunchPayload::from_launch(
    &launch,
    &arg_policy,
    &env_policy,
    StartupMode::Development,
)?;
let duplicate = DuplicateLaunchHandoff::new(duplicate_payload);
let diagnostics = StartupDiagnosticBuilder::new(StartupMode::Development)
    .argument_report(args)
    .environment_snapshot(env)
    .duplicate_launch(duplicate)
    .build_checked()?;
tracing::info!(summary = diagnostics.to_text(), "startup diagnostics");
```

`LaunchContextBuilder` captures command-line arguments by default and captures
environment variables only from an explicit allowlist. It validates environment
keys, rejects duplicate keys, and can require the executable path or current
directory when startup routing depends on them. Use `cx.launch_context()` for a
best-effort snapshot with args and no environment values. Prefer
`LaunchEnvironmentHandoffBuilder` when generated startup code needs one checked
route across launch context capture, argument policies, environment allowlists,
duplicate-launch routing, startup diagnostics, hosted startup state, or roadmap
work. Its summary reports request kinds and next action without logging launch
arguments, URLs, paths, environment keys or values, duplicate payloads, hosted
state IDs, or roadmap text. Use
`LaunchArgumentPolicyBuilder` before routing startup payloads so file paths,
deep-link schemes, flags, and opaque tokens are explicit. Use
`LaunchEnvironmentAllowlistBuilder` to summarize environment state without
logging values. `DuplicateLaunchPayload`, `DuplicateLaunchHandoff`, and
`StartupDiagnosticBuilder` provide redacted startup handoffs for
single-instance apps, support bundles, and agents.

---

## Helper Processes

For generated helper/plugin work, validate the full launch contract before
touching a platform supervisor:

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
match handoff.next_action() {
    HelperPluginNextAction::ConfigurePluginContracts => {}
    HelperPluginNextAction::InstallBrokerAndContext => {}
    HelperPluginNextAction::ConfigureSupervisorPolicy => {}
    HelperPluginNextAction::SpawnNativeHelper => {}
}
```

`cx.helper_plugin_handoff_checked(...)` with `HelperPluginHandoffBuilder` covers helper launch descriptors, plugin
manifests, plugin permission manifests, IPC schemas, and crash/restart policy in
one checked descriptor. It rejects invalid launches, missing required plugin
permission grants, malformed or duplicate IPC message types, invalid crash
policy, and oversized generated handoff batches. Use `helper_plans()`,
`has_plugin_contracts()`, `requires_broker_and_context()`, and
`has_supervisor_policy()` so agents can separate plugin contract setup,
least-privilege process context, supervision, and final spawn without logging
plugin ids, helper names, paths, argv, env keys or values, capability labels,
IPC message names, crash ids, or raw errors.

Describe app-owned native helper processes without dropping to shell strings:

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

`HelperProcessLaunchBuilder` covers Desktop `utility process` and
`helper process`-style app helpers while keeping launch validation outside the
renderer layer. Pair it with `AuxiliaryExecutableBuilder` so helper names and
resolved paths are validated before launch. The auxiliary lookup rejects empty,
padded, path-like, control-character, and overlong names, and
`.require_existing_file()` fails before a supervisor is asked to spawn a missing
binary. The launch builder then validates the process class, name, executable
path, arguments, explicit environment variables, inherited environment
allowlist, working directory, declared capability labels, and
restart/heartbeat policy. `ProcessClass::Utility` is the neutral bucket for
app-owned tools that are not UI, media, worker, or extension hosts.
Environment inheritance is off by
default; opt into `.inherit_environment_keys(...)` when the helper needs a
small parent-env allowlist.
Use `HelperProcessLaunch::ffmpeg_transcoder(...)`,
`HelperProcessLaunch::language_server(...)`, and
`HelperProcessLaunch::plugin_host(...)` as presets for FFmpeg wrappers,
language servers, and plugin hosts. They return normal
`HelperProcessLaunchBuilder` values with process class, restart, heartbeat, and
capability defaults already set; inspect `HelperProcessProfile::key()`,
`HelperProcessLaunchBuilder::to_text()`, and `HelperProcessLaunch::to_text()`
for profile/class/arg/env/inherited-env/capability counts, working-dir
presence, restart policy, and heartbeat presence without logging helper names,
paths, args, env keys or values, or capability labels.

For IDE-like shells and developer tools, model terminal panes with a checked
terminal session descriptor before handing the request to a PTY backend:

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
let process = terminal.process_info_builder().build_checked()?;
```

`TerminalSessionRequestBuilder` is not a shell-string API. It validates the
shell executable path, argv items, explicit environment variables, inherited
environment allowlist, optional working directory, terminal dimensions,
scrollback budget, and login-shell intent before a backend opens a PTY. Inspect
`TerminalSessionRequestBuilder::to_text()`, `TerminalSessionRequest::to_text()`,
`arg_count()`, `env_count()`, `inherited_env_count()`, `has_working_dir()`,
`has_scrollback_limit()`, `size()`, and `is_login_shell()` without logging shell
paths, session names, command text, project paths, environment keys or values,
or scrollback contents. The descriptor gives Desktop-terminal apps a native
launch contract; platform PTY IO is still the backend layer that consumes it.

---

## Native IPC

Worker and extension transports use typed messages instead of raw string
channels. Use `IpcMessage::to_text()`, `WorkerRequest::to_text()`,
`WorkerResponse::to_text()`, `WorkerProgress::to_text()`,
`WorkerError::to_text()`, `BootstrapMessage::to_text()`,
`frame_summary(frame).to_text()`, and extension RPC message `to_text()` helpers
when generated helpers, plugin hosts, tests, or AI agents need desktop-app
IPC traces. The summaries report message kind, correlation id, response/error
shape, JSON payload class and item counts, frame completeness, and bootstrap or
extension message shape without logging payloads, command ids, settings keys,
panel ids, capability labels, payload strings, or error messages.

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

For generated debugging and agent evidence collection, wrap native trace,
runtime snapshot, resource-budget, support diagnostics, and hosted WebView
console/DevTools decisions in one checked handoff:

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
match handoff.next_action() {
    DeveloperObservabilityNextAction::StartTraceSession => {}
    DeveloperObservabilityNextAction::CaptureRuntimeEvidence => {}
    DeveloperObservabilityNextAction::EvaluateResourceBudget => {}
    DeveloperObservabilityNextAction::ExportSupportDiagnostics => {}
    DeveloperObservabilityNextAction::UseHostedDevtoolsBridge => {}
}
```

`DeveloperObservabilityHandoffBuilder` rejects empty handoffs, unbounded trace
sessions, empty resource budgets, unsafe support diagnostics, contradictory
runtime queries, and malformed hosted bridge ids. Use the `starts_trace_session`,
`captures_runtime_evidence`, `evaluates_resource_budget`,
`exports_support_diagnostics`, and `uses_hosted_devtools_bridge` helpers to keep
native diagnostics separate from explicit WebView-island inspection without
logging payloads, URLs, headers, selectors, document text, paths, argv,
environment values, tokens, cookies, or raw traces.

Use `TraceSessionBuilder` when a debug panel, benchmark, support flow, or agent
needs a bounded runtime trace descriptor before collection begins:

```rust
let trace = cx.trace_session_checked(
    TraceSessionBuilder::new("startup-profiler")
        .startup()
        .runtime()
        .network()
        .ipc()
        .worker()
        .max_events(10_000)
        .max_duration(std::time::Duration::from_secs(60))
        .payload_shapes(),
)?;

tracing::info!(summary = trace.to_safe_text(), "trace session ready");
```

Trace sessions are descriptors, not an unbounded recorder. Checked sessions
require an app-owned scope, at least one category, a positive event budget, and a
duration of no more than one day. Runtime snapshots and process metrics are
enabled by default; `.support_diagnostics()` opts into the privacy-aware support
report path, and `.payload_shapes()` records only event shape metadata rather
than raw payload bodies. Use `to_safe_text()` for shared agent traces because it
omits the scope and buckets event budgets.

---

## Localization and Text System

Read a native locale snapshot for formatting, catalog selection, onboarding, and
support diagnostics. This is the native replacement lane for Desktop
`locale snapshot`, preferred-language startup decisions, browser text direction,
and browser spellcheck policy:

```rust
let text_system = cx.localization_text_handoff_checked(
    LocalizationTextHandoffBuilder::new()
        .locale_snapshot(
            LocaleSnapshotBuilder::new()
                .locale("de_DE.UTF-8")
                .preferred_languages(["de-DE", "en-US"]),
        )
        .text_checking(
            TextCheckingRequestBuilder::new(editor_text)
                .locale("de_DE")
                .check_grammar()
                .autocorrect(),
        )
        .capability_report(CapabilityReport::current())
        .hosted_text_island("rich-editor-intl")
        .roadmap_work("native Intl date formatting"),
)?;

match text_system.next_action() {
    LocalizationTextNextAction::BuildLocaleSnapshot => {
        // Capture locale state before choosing catalogs or layout direction.
    }
    LocalizationTextNextAction::PrepareTextChecking => {
        // Prepare spellcheck, grammar, or autocorrect policy.
    }
    LocalizationTextNextAction::CheckCapabilityReport => {
        // Gate native IME/spellchecking behavior before enabling rich UI.
    }
    LocalizationTextNextAction::UseHostedTextIsland => {
        // Scope rich-editor or browser Intl behavior to one hosted surface.
    }
    LocalizationTextNextAction::TrackRoadmapWork => {
        // Keep native gaps explicit instead of silently claiming parity.
    }
}
```

`LocalizationTextHandoffBuilder` validates locale snapshots, text-checking
requests, capability reports, hosted text islands, and roadmap work before
localization or editor side effects. Inspect `LocalizationTextHandoff::to_text()`
for generated diagnostics without logging user text, exact locale environment
values, custom dictionary words, suggestions, grammar messages, hosted ids, or
roadmap details.

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

For Desktop forms, prefer native controls for app-owned forms:
`text_input`, `input`, `checkbox`, `radio_group`, `slider`, `select`,
`date_picker`, `time_picker`, `color_picker`, `OtpInput`, `FileUpload`,
`TagInput`, and `Rating`. Keep required-field, range, choice, file-intake,
submit/reset, and dirty-state validation in app state; pair text fields with
`NativeFormSchemaBuilder`, `FormValidationHandoffBuilder`,
`FormFieldDescriptorBuilder`, `TextInputImeState`,
`cx.focus_traversal_plan_checked(FocusTraversalPlanBuilder::overlay("scope"))?`,
`EditCommandStateSnapshot`, and `TextCheckingRequestBuilder` when the form needs
keyboard traversal, edit menus, spelling, grammar, or autocorrect. Inspect
generated summaries with `to_text()` helpers before agents act, and avoid
logging field values, labels, placeholders, selected text, validation messages,
file names, file paths, URLs, credentials, autofill contents, selectors, or form
payloads.

```rust
let schema = cx.native_form_schema_checked(
    NativeFormSchemaBuilder::new("signup")
        .field(
            FormFieldDescriptorBuilder::new("email", FormFieldKind::Email)
                .label("Email address")
                .required(),
        )
        .field(
            FormFieldDescriptorBuilder::new("plan", FormFieldKind::Select)
                .label("Plan")
                .option_count(3),
        )
        .step("account", ["email"])
        .step("billing", ["plan"])
        .dirty_state_tracking()
        .disable_submit_until_valid()
        .autofill_enabled(false),
)?;

let handoff = cx.form_validation_handoff_checked(
    schema
        .validation_handoff_builder()
        .text_checking(TextCheckingRequestBuilder::new(editor_text).check_grammar())
        .submit("signup")
        .reset("signup"),
)?;

assert_eq!(handoff.next_action(), FormValidationNextAction::ValidateNativeFields);
```

`NativeFormSchemaBuilder` validates field ids, duplicate fields, wizard steps,
unknown step references, dirty-state policy, submit gating, and autofill intent.
`FormValidationHandoffBuilder` then validates field ids, labels,
required/disabled state, ranges, patterns, select/radio options, text-checking
policy, submit and reset ids, autofill policy, and hosted-form bridge scope
before rendering or mutating form state.

Use WebView form bridges only for hosted checkout, auth, account, document, or
vendor forms that must keep browser constraint validation, file-input behavior,
autofill, or password-manager semantics. Scope these islands with
`WebViewOptions::form_bridge`, `WebViewFormEvent`, `WebViewFileInputEvent`,
`WebViewOptions::general_autofill_disabled`, and the controller helpers
`set_form_value`, `submit_form`, and `reset_form`.

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

Shortcut editors, command palettes, and hotkey settings can snapshot the active
keyboard layout without reaching into the raw platform mapper:

```rust
let layout = cx.keyboard_layout_snapshot_checked(
    KeyboardLayoutSnapshotBuilder::new().require_known_layout(),
)?;

let label = layout.name();
let needs_equivalents = layout.has_key_equivalents();
```

`cx.keyboard_layout_snapshot()` returns a best-effort snapshot and allows
`unknown` layouts for headless/background runtimes. The checked builder rejects
empty, padded, control-character, or overlong layout ids/names, and
`.require_known_layout()` fails when the OS exposes only a fallback layout.

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
    if budget.has_memory_pressure() {
        shed_caches();
    }
    if budget.has_power_pressure() {
        reduce_background_work();
    }
}
```

`AppResourceBudgetBuilder` validates positive thresholds and requires at least
one configured check. `AppResourceBudgetEvaluation` includes the sampled process
metrics, runtime snapshot, structured issues, `is_within_budget()`,
`issue_count()`, `issue_kinds()`, `has_issue(kind)`, `has_memory_pressure()`,
`has_window_pressure()`, `has_power_pressure()`, `missing_required_metrics()`,
and a compact `summary()`. Memory metrics remain best-effort across OSes; use
`.require_memory_metrics()` when a test or release gate must fail if the
platform cannot provide memory data.

When a builder or AI agent wants to claim that a Kael app is lighter, faster to
start, lower-memory, or more resource efficient than a hosted runtime, wrap the
evidence in one checked handoff before publishing the claim:

```rust
let evidence = cx.performance_evidence_handoff_checked(
    PerformanceEvidenceHandoffBuilder::new()
        .process_metrics()
        .resource_budget(
            AppResourceBudgetBuilder::new()
                .max_resident_set_bytes(256 * 1024 * 1024)
                .max_windows(4)
                .require_memory_metrics(),
        )
        .benchmark_scenario(BenchmarkScenario::Chat)
        .benchmark_sample_pair(sample_pair)
        .baseline_comparison(report)
        .trace_session(TraceSessionBuilder::new("perf-audit").runtime())
        .support_diagnostics(SupportDiagnosticsBuilder::new())
        .hosted_profiler_island("webview-profiler")
        .roadmap_work("native memory timeline"),
)?;

match evidence.next_action() {
    PerformanceEvidenceNextAction::CaptureProcessMetrics => capture_metrics(),
    PerformanceEvidenceNextAction::EvaluateResourceBudget => evaluate_budget(),
    PerformanceEvidenceNextAction::CollectBenchmarkSamples => collect_samples(),
    PerformanceEvidenceNextAction::CompareBenchmarkEvidence => compare_results(),
    PerformanceEvidenceNextAction::StartTraceSession => start_trace(),
    PerformanceEvidenceNextAction::ExportSupportDiagnostics => export_diagnostics(),
    PerformanceEvidenceNextAction::UseHostedProfilerIsland => profile_hosted_content(),
    PerformanceEvidenceNextAction::TrackPerformanceRoadmap => record_gap(),
}
```

`PerformanceEvidenceHandoffBuilder` validates resource budgets, benchmark sample
pairs, clean baseline comparison reports, trace-session bounds, support
diagnostics, hosted profiler island ids, and roadmap reasons. Its `to_text()`
summary reports request counts and route kinds only, so agents can audit
performance readiness without logging executable paths, current directories,
commands, sample names, trace files, raw benchmark values, environment values,
or roadmap text. Hosted profiler islands should stay scoped to browser-owned
content; native app performance claims need native metrics plus comparable
Baseline/Kael evidence.

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

let metadata_summary = metadata.clone().build_checked()?.summary();
tracing::info!(summary = metadata_summary.to_text(), "app metadata");
cx.show_about_dialog_checked(metadata)?;
```

`AppMetadataBuilder` validates user-facing names, version/build labels,
bundle-style identifiers, HTTP(S) website/support URLs, copyright, license, and
credits before they reach native chrome. `build_checked()` returns
`AppMetadata`, which exposes accessors plus `display_title()` and
`about_dialog()` when an app wants to route the metadata through its own menu or
custom dialog flow. Use `summary()` to check recommended version, identifier,
and support URL coverage for About dialogs, support screens, diagnostics, and
agents before showing chrome.

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
let summary = update.summary();
tracing::info!(summary = summary.to_text(), "update state");

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
Use `summary()` when generated UI or agents need one value containing phase,
recommended action, menu label, release version, progress, and error status.
`AppUpdateOfferPolicyBuilder` adds the app-facing release gate that Desktop
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
        tracing::info!(summary = request.to_text(), "open request");
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
    .deep_links_checked({
        let routes = DeepLinkRouterBuilder::new()
            .route("myapp", |url, cx| {
                // Handle myapp://path/to/resource
            })
            .route("oauth", |url, cx| {
                // Handle oauth://callback?code=...
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
    });
```

`DeepLinkRouterBuilder` validates grouped route schemes and rejects duplicates;
use `routes.to_text()` for startup/plugin/agent summaries before installing
handlers.
`UrlSchemeRegistrationBuilder` validates scheme syntax, deduplicates repeated
schemes, and keeps startup code readable; use `schemes.to_text()` before
runtime OS scheme registration. Use `routes.setup_plan(&schemes)` or
`routes.setup_plan_with_default_handler(&schemes, &defaults)` before OS
registration to catch runtime routes missing URL-scheme registration, default
handler claims missing runtime routes, or registered schemes with no handler.
`DeepLinkSetupPlan::to_text()` reports counts and gap status without logging
scheme names; inspect `missing_registration_schemes()`,
`missing_default_handler_schemes()`, and `unhandled_registration_schemes()` only
when setup UI intentionally needs exact scheme names. Use `.on_open_requests(...)` or
`.on_open_request(...)` when the app needs to distinguish app-owned deep links,
external URLs, and `file://` document opens without custom parsing. The
`OpenRequest::to_text()` summary reports the classified kind and presence flags
without logging raw launch strings, local paths, or URL schemes; use
`request.scheme()`, `request.raw()`, or `request.file_path()` only when exact
routing data is intentionally needed. The runtime
`register_deep_link_handler_checked(...)` path validates dynamic
plugin/agent-installed handler schemes before mutating the handler registry; use
`dispatch_deep_link_checked(...)` when tests, plugins, or agents dispatch URLs
manually and should reject non-deep-link input before handlers run. The
lower-level `register_url_scheme("scheme")`, `.on_open_urls(...)`,
`.on_open_url(...)`, `.on_deep_link("scheme", callback)`, and `.deep_links(...)`
methods remain available for direct route management.

When a launch/open event must choose between document intake, native routes,
external URLs, or hosted browser history, build an open-request route plan first:

```rust
let plan = cx.open_request_route_plan_checked(
    OpenRequestRoutePlanBuilder::new()
        .request("myapp://settings/profile")
        .registered_deep_link_scheme("myapp")
        .native_route("settings/profile")
        .hosted_navigation_bridge("docs")
        .allow_external_urls(),
)?;

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

`OpenRequestRoutePlanBuilder` validates generated registered deep-link schemes,
native route ids, hosted navigation bridge ids, external-URL policy, and file
routing before mutating a native stack, shell URL, document workspace, or WebView
history. Its summary reports counts and route shape without logging raw URLs,
file paths, scheme names, route ids, or hosted bridge ids.

For app-owned navigation, describe the native route stack before wiring tabs,
breadcrumbs, command palette routes, session restore, or deep-link entry points:

```rust
let handoff = cx.navigation_handoff_checked(
    NavigationHandoffBuilder::new()
        .route(
            NavigationRouteDescriptorBuilder::new("home")
                .restorable_state()
                .require_activation(),
        )
        .push_route("settings/profile")
        .restore_session(2)
        .deep_link("myapp"),
)?;

assert_eq!(
    handoff.next_action(),
    NavigationHandoffNextAction::ValidateNativeRoutes
);
```

`cx.navigation_handoff_checked(NavigationHandoffBuilder::...)` validates route
ids, native stack commands, restore depth, deep-link schemes, and hosted-history
bridge scope before navigation side effects. Inspect `handoff.to_text()` for
request counts and next action without logging route ids, URLs, tab labels,
titles, breadcrumbs, mementos, or history entries. Use `Navigator`, `Route`,
`RouteChangeEvent`, and `Transition` for app-owned route stacks; keep
`WebViewOptions::navigation_state_bridge`,
location/title/favicon bridges, and `WebViewController::go_back` /
`go_forward` scoped to browser-owned hosted pages.

For app-owned document search, result navigation, and zoom, build a checked
find/zoom handoff before wiring command palettes, find bars, scroll-to-match,
selection state, or document zoom controls:

```rust
let handoff = cx.find_zoom_handoff_checked(
    FindZoomHandoffBuilder::new()
        .search_with_options("needle", false, true, true)
        .result_summary(12, Some(3))
        .next_result()
        .custom_scale(1.25, true)
        .hosted_find_zoom_bridge("docs"),
)?;

assert_eq!(handoff.next_action(), FindZoomNextAction::SearchNativeDocument);
```

`FindZoomHandoffBuilder` validates query shape, result counts, current match
indexes, result-navigation commands, clear-selection requests, document zoom
modes, custom scale bounds, persistence intent, and hosted find/zoom bridge
scope before document side effects. Inspect `handoff.to_text()` for request
kinds and next action without logging query text, selected text, matched
snippets, document contents, selectors, URLs, route ids, exact zoom factors,
coordinates, or viewport geometry. Keep `WebViewController::find_text`,
`find_text_result`, `stop_finding`, `set_zoom_factor`,
`WebViewOptions::find_result_bridge`, and `zoom_hotkeys` scoped to hosted pages
that own browser text matching, iframe/shadow-DOM search, browser selection
highlighting, or exact page zoom.

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
types, and headers before they are handed back to the app. Use
`request.to_text()`, `response.to_text()`, and `router.to_text()` for
content-safe diagnostics; summaries include booleans and counts without logging
raw URLs, schemes, hosts, paths, query strings, headers, MIME values, or
response bodies.

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
root, so symlink escapes are rejected as well. Use
`resolver.to_text()` or the builder summary before registration when agents need
to report configuration state without exposing filesystem roots, accepted hosts,
index filenames, cache header values, or served bytes.

---

## Multi-Window

Open multiple windows with independent views:

```rust
cx.open_window_checked(
    WindowIntentBuilder::main()
        .title("Kael Studio")
        .windowed(bounds)
        .min_size(size(px(720.0), px(480.0))),
    |_window, cx| cx.new(|_| MainView::new()),
).unwrap();

cx.open_window_checked(
    WindowIntentBuilder::palette()
        .title("Settings")
        .windowed(bounds)
        .min_size(size(px(480.0), px(320.0))),
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

Prefer `open_window_checked(WindowIntentBuilder::...)` for generated
window-management intent: `main`,
`palette`, `utility`, `modal(parent)`, `popup`, and `overlay` presets compose
coherent window kinds, resize/minimize/move flags, titlebar/background defaults,
and parent requirements before opening a window. It validates finite positive
bounds/minimum sizes, titles, app IDs, tab identifiers, and intent-specific
invariants such as modal parent handles, non-minimizable palettes, non-resizable
popups, and overlay window kind inside the open call. Inspect generated window
intents with `to_text()`, `options_summary()`, `kind()`, `has_bounds()`,
`has_parent()`, `has_transparent_titlebar()`, `starts_hidden()`, and
`starts_unfocused()` before creating native windows. Use
`WindowIntentBuilder::build_checked()?` when a lower-level flow needs raw
`WindowOptions` first, then inspect the result with `WindowOptions::to_text()`
or helpers such as `bounds_mode()`, `has_title()`, `has_min_size()`,
`has_display_id()`, `has_app_id()`, `has_tabbing_identifier()`,
`uses_client_decorations()`, and `fixed_size()`. Drop to
`WindowOptionsBuilder` when an app needs the full native option surface
directly; it exposes the same content-safe inspection helpers before build.

Raw `WindowOptions { ... }` values remain available when constructing options
manually.

For generated Desktop shells that need a BrowserWindow-style bundle of intent,
placement, custom chrome, fullscreen/kiosk policy, focus queries, z-order,
opacity, capture protection, document state, and hosted popup/fullscreen
fallbacks, validate the whole plan first:

```rust
let windows = cx.window_management_handoff_checked(
    WindowManagementHandoffBuilder::new()
        .window_intent(
            WindowIntentBuilder::main()
                .title("Project Dashboard")
                .min_size(size(px(640.0), px(420.0))),
        )
        .window_placement(WindowPlacementBuilder::new(size(px(960.0), px(640.0))))
        .focused_window_query(FocusedWindowQueryBuilder::new().current_process_only())
        .chrome_command(WindowChromeCommand::request_decorations(WindowDecorations::Client))
        .client_inset(WindowClientInsetBuilder::new(px(8.0)))
        .presentation_policy(WindowPresentationPolicyBuilder::fullscreen("Present dashboard"))
        .interaction_command(WindowInteractionCommand::show().reason("restore window"))
        .z_order_policy(WindowZOrderPolicyBuilder::always_on_top("Keep inspector visible"))
        .opacity(WindowOpacityBuilder::fraction(0.92))
        .content_protection(
            WindowContentProtectionBuilder::exclude_from_capture("Protect checkout secrets"),
        )
        .document_state(WindowDocumentStateBuilder::new().title("Project Dashboard"))
        .hosted_window_island("oauth-popup")
        .roadmap_work("native child-window docking"),
)?;

match windows.next_action() {
    WindowManagementNextAction::BuildWindowIntent => build_intent(),
    WindowManagementNextAction::ResolveWindowPlacement => resolve_placement(),
    WindowManagementNextAction::QueryFocusedWindow => query_focus(),
    WindowManagementNextAction::ConfigureWindowChrome => configure_chrome(),
    WindowManagementNextAction::ApplyPresentationPolicy => apply_presentation(),
    WindowManagementNextAction::DispatchWindowInteraction => dispatch_interaction(),
    WindowManagementNextAction::ApplyWindowPolicy => apply_policy(),
    WindowManagementNextAction::ApplyDocumentState => apply_document_state(),
    WindowManagementNextAction::UseHostedWindowIsland => scope_hosted_popup(),
    WindowManagementNextAction::TrackWindowRoadmap => record_gap(),
}
```

`WindowManagementHandoffBuilder` delegates validation to the same checked
window builders used by runtime APIs, then summarizes only request counts and
route kinds. Use `WindowManagementHandoff::to_text()` without logging titles,
app ids, tab identifiers, bounds, display ids, parent handles, menu positions,
opacity values, document paths, hosted surface ids, reason text, or roadmap
text.

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
same state update. Inspect generated titles with `WindowTitleBuilder::to_text()`
and document chrome with `WindowDocumentStateBuilder::to_text()` or
`WindowDocumentState::to_text()` for summaries that avoid logging titles or
paths. Raw `set_window_title(...)` and `set_window_edited(...)` remain available
when an app owns the validation.

For translucent native palettes, inspectors, HUDs, media controls, and overlay
windows, set checked window opacity instead of passing arbitrary floats to the
platform backend:

```rust
window.set_opacity_checked(WindowOpacityBuilder::fraction(0.86))?;
window.set_opacity_checked(WindowOpacityBuilder::opaque())?;
```

`WindowOpacityBuilder` validates finite `0.0..=1.0` fractions before changing
the platform window. Inspect builders or checked opacity values with
`is_opaque()`, `is_translucent()`, and `to_text()` without logging exact
fractions. macOS, Windows, and X11 apply native opacity; Wayland keeps the API
stable and no-ops the request because the core protocol does not expose a
universal top-level opacity control. Raw `set_opacity(...)` remains available
for already-validated custom integrations.

For runtime layout changes, media-player modes, inspectors, and utility panels,
resize native window content through the checked runtime builder:

```rust
window.resize_checked(WindowContentSizeBuilder::new(size(px(960.0), px(640.0))))?;
window.resize_checked(WindowContentSizeBuilder::dimensions(px(420.0), px(300.0)))?;
window.set_rem_size_checked(WindowRemSizeBuilder::new(px(18.0)))?;
```

`WindowContentSizeBuilder` validates finite positive dimensions and rejects
absurdly large generated sizes before platform resize APIs run. Use
`is_landscape()`, `is_portrait()`, `is_square()`, and `to_text()` on builders or
checked requests for content-safe resize traces that avoid logging exact window
dimensions. Raw `resize(...)` remains available for custom geometry managers
that already own their constraints.
Use `WindowRemSizeBuilder` for generated zoom/accessibility density controls;
it rejects non-finite, tiny, and excessively large base `rem` sizes before
rescaling native layout. Use `size_class()` and `to_text()` for density traces
that avoid logging exact scale values. Raw `set_rem_size(...)` remains available
for hand-validated integrations.
Use `request_autoscroll_checked(WindowAutoscrollRequestBuilder::new(bounds))`
from prepaint code for generated drag, selection, editor, canvas, and design
tool surfaces; checked requests reject non-finite coordinates, negative sizes,
and excessively large bounds before scroll containers react. Use `is_empty()`
and `to_text()` for autoscroll traces that avoid logging coordinates or region
sizes. Raw `request_autoscroll(...)` remains available for hand-validated
element code.
Use `set_window_cursor_style_checked(WindowCursorStyleCommand::new(style, reason))`
for whole-window cursor overrides in generated canvas, drawing, drag, and resize
surfaces; checked commands require a valid diagnostic reason before overriding
element cursor styles. Use `has_reason()` and `to_text()` for cursor traces that
confirm intent without logging the reason text. Raw
`set_window_cursor_style(...)` remains available for hand-validated element code.

For runtime mini-players, call controls, inspectors, and palettes that need to
stay above normal app windows, use a checked z-order policy:

```rust
window.set_z_order_policy_checked(
    WindowZOrderPolicyBuilder::always_on_top("Keep call controls visible"),
)?;
window.set_z_order_policy_checked(WindowZOrderPolicyBuilder::normal())?;
```

`WindowZOrderPolicyBuilder` requires a validated reason before enabling
always-on-top behavior and rejects accidental diagnostic text such as empty,
padded, or control-character strings. macOS, Windows, and X11 apply native
topmost/above state; Wayland keeps the API stable and no-ops runtime changes
unless a compositor-specific overlay protocol is used. Raw
`set_always_on_top(...)` remains available for custom window managers.

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
    tracing::info!(summary = event.to_text(), "file change");
})?;

let builder = FileWatchSetBuilder::new()
    .paths([project_dir, config_file, log_dir])
    .max_depth(3);
tracing::info!(summary = builder.to_text(), "file watch set");

let watch_set = cx.file_watch_set_checked(builder)?;
watcher.watch_set(watch_set.clone())?;
tracing::info!(summary = watch_set.to_text(), "file watch set registered");

let options = cx.file_watch_options_checked(
    FileWatchOptionsBuilder::new()
        .non_recursive()
)?;
watcher.watch_with_options(
    single_file,
    options,
)?;
```

Use `FileWatchSetBuilder::new().paths([...]).recursive()` or
`.max_depth(depth)` when one feature needs to watch several project, config, log,
or generated-asset roots with shared options. The checked path rejects empty
sets, empty paths, missing paths, raw non-recursive depth limits, and zero-depth
watches before platform registration starts, then canonicalizes and deduplicates
the paths. Use `FileWatchOptionsBuilder::new().recursive()` for all descendants,
`.max_depth(depth)` for bounded project-folder watches, and `.non_recursive()`
for single files or direct children. Prefer
`cx.file_watch_options_checked(...)` and `cx.file_watch_set_checked(...)` before
platform registration so generated project, config, theme, asset, and log
watchers fail early. Inspect `FileWatchOptions::to_text()`,
`FileWatchOptionsBuilder::to_text()`, `FileWatchSetBuilder::to_text()`,
`FileWatchSet::to_text()`, `path_count()`, and `configured_path_count()` for
generated setup logs without printing watched paths. Inside callbacks, use
`FileWatchEvent::kind()`, `is_removal()`, `is_error()`, and `to_text()` for
content-safe routing and logs without leaking file paths or platform error text.
Raw `FileWatchOptions { ... }`,
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

let destination = cx.download_destination_plan_checked(
    DownloadDestinationPlanBuilder::new("https://cdn.myapp.com/exports/report.pdf")
        .suggested_file_name("report.pdf")
        .download_dir(dirs.download_dir())
        .network_policy(policy.clone())
        .sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .size_bytes(42_000)
        .create_parent_dirs(),
)?;

tracing::info!(summary = destination.to_text(), "download destination");

match destination.next_action() {
    DownloadDestinationNextAction::PromptForDestination => {
        // open the app's native Save As picker, then rebuild with .destination(path)
    }
    DownloadDestinationNextAction::ReviewOverwritePolicy => {
        // ask before overwriting, then rebuild with .overwrite_existing()
    }
    DownloadDestinationNextAction::BuildRequest => {}
}

let request = destination.build_request_checked()?;

tracing::info!(summary = request.to_text(), "download request");
```

`cx.download_destination_plan_checked(DownloadDestinationPlanBuilder::...)`
covers the Electron-style "Save As" gap before the transfer starts: URL
validation, suggested filename validation, download directory resolution,
explicit destination validation, parent-directory policy, network policy,
integrity metadata, and overwrite review. Its safe summary reports the shape of
the plan without logging the URL, destination path, suggested filename, or exact
size.

For flows that already have an explicit destination, build the request directly:

```rust
let policy = NetworkPolicyBuilder::new()
    .allow_host("cdn.myapp.com")
    .build_checked()?;

let download = DownloadRequest::builder(
    "https://cdn.myapp.com/exports/report.pdf",
    dirs.download_dir().join("report.pdf"),
)
.sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
.size_bytes(42_000)
.network_policy(policy)
.create_parent_dirs();
tracing::info!(summary = download.to_text(), "download request");
tracing::debug!(summary = download.to_safe_text(), "download request shape");
let request = cx.download_request_checked(download)?;

tracing::info!(summary = request.to_text(), "download request");
```

This is separate from WebView downloads. WebView download handlers preserve
browser behavior for hosted pages; `DownloadRequest` is the native descriptor to
queue, audit, and execute app-owned downloads consistently across workers,
plugins, and generated automation. Use `cx.download_request_checked(...)` before
queueing generated downloads so invalid URLs, missing parent directories, denied
network policies, and absent integrity metadata can be reported before the
builder is consumed. Use `to_safe_text()` when agent traces or telemetry should
avoid hosts, destination paths, and exact byte sizes.
For multi-file queues, use `cx.download_batch_checked(...)` so empty queues and
duplicate destinations fail before a worker starts:

```rust
let batch = DownloadBatch::builder()
    .request(request)
    .request_builder(
        DownloadRequest::builder(model_url, model_path)
            .sha256(model_sha256)
            .size_bytes(model_size)
            .create_parent_dirs(),
    )?;

tracing::info!(summary = batch.to_text(), "download batch");
let batch = cx.download_batch_checked(batch)?;
```

Inspect `DownloadBatch` with `request_count()`, `requests()`, `into_requests()`,
`sha256_count()`, `size_count()`, `create_parent_dirs_count()`,
`network_policy_count()`, `to_text()`, and `to_safe_text()` before handing a
native download queue to a background job, plugin host, or export manager.

Before a worker starts the queue, wrap the checked batch with
`cx.download_execution_plan_checked(...)` so generated download managers also
validate execution policy:

```rust
let plan = cx.download_execution_plan_checked(
    DownloadExecutionPlan::builder(batch)
        .max_parallel(2)
        .retry_attempts(3)
        .temporary_file_extension("partial"),
)?;

tracing::info!(summary = plan.to_text(), "download execution plan");
```

The execution plan rejects zero or excessive parallelism, more than ten retry
attempts, unsafe temporary-file extensions, and existing destinations unless
`.overwrite_existing()` is explicit. Use `.serial()`, `.no_retries()`,
`.without_temporary_files()`, or `.overwrite_existing()` when those behaviors
are intentional.

For builder- or agent-generated download surfaces, prefer a `DownloadHandoff`
when the app needs one object that says both "what will run" and "what is still
missing":

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

match handoff.next_action() {
    DownloadHandoffNextAction::ReviewOverwritePolicy => review_overwrite_policy(),
    DownloadHandoffNextAction::AddNetworkPolicy => collect_allowed_hosts(),
    DownloadHandoffNextAction::AddIntegrityMetadata => collect_hashes_and_sizes(),
    DownloadHandoffNextAction::QueueDownloads => queue_downloads(handoff.execution_plan()),
}
```

The handoff exposes `has_complete_network_policy()`,
`has_complete_integrity_metadata()`, `needs_overwrite_review()`,
`is_queue_ready()`, and `to_text()` so a generated UI can explain why a native
download queue is not ready without leaking URLs, paths, or exact byte sizes.

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
    tracing::info!(summary = ctx.to_text(), "print page");
})
    .orientation(PrintOrientation::Portrait)
    .margins(Edges::all(px(36.0)));

job.validate()?;
let request = PrintRequest::dialog(job);
tracing::info!(summary = request.to_text(), "print request");
window.print_checked(request, cx)?;

// WebView-hosted documents can use the same checked request surface.
let webview_request = PrintRequest::webview("invoice-preview");
tracing::info!(summary = webview_request.to_text(), "print request");
window.print_checked(webview_request, cx)?;

// Builder and AI-agent flows can inspect one handoff before dispatch.
let handoff = cx.document_output_handoff_checked(
    DocumentOutputHandoffBuilder::export_webview_pdf_file(
        "invoice-preview",
        "/tmp/invoice-preview.pdf",
    ),
)?;
assert_eq!(handoff.next_action(), DocumentOutputNextAction::ExportHostedPdf);
tracing::info!(summary = handoff.to_text(), "document output handoff");
```

Use `PrintJob::letter(...)`, `PrintJob::a4(...)`, `PrintPage::letter(...)`, and
`PrintPage::a4(...)` for common paper sizes instead of repeating point values.
`PrintJob::validate()` catches empty, padded, control-character, or overly long
titles, missing pages, invalid page sizes, mixed page sizes, negative margins,
and margins that leave no drawable content area before native print UI opens.
Print drawing helpers drop invalid generated commands with non-finite points,
empty rectangles, invalid stroke widths, invalid font sizes, or unsupported
control characters in text. Inside render callbacks, use
`PrintContext::command_count()`, `fill_count()`, `stroke_count()`,
`text_count()`, `image_count()`, `is_empty()`, and `to_text()` to verify page
composition without logging document text, labels, image bytes, or drawing
coordinates.
Use `PrintRequest::dialog(job)` for the normal native print UI,
`PrintRequest::silent(job)` only when the app intentionally owns direct printer
dispatch, and `PrintRequest::webview(id)` for Desktop
`hosted document print(...)` style hosted documents. `Window::print_checked(...)`
validates native jobs or WebView ids before dispatching. Use `request.to_text()`
for document-safe logs and agent summaries before showing print UI or sending a
silent print job.
Use `cx.document_output_handoff_checked(DocumentOutputHandoffBuilder::...)` when
a builder or agent needs one checked descriptor for native print, hosted print,
native PDF export, hosted PDF export, or save-page export before dispatch.
Inspect `handoff.next_action()` for
`PrintNative`, `PrintHostedDocument`, `ExportNativePdf`, `ExportHostedPdf`, or
`SaveHostedPage`, and use `handoff.to_text()` without logging document titles,
file paths, generated bytes, document text, WebView ids, selectors, or URLs.

---

## Power Management

Prevent sleep and detect power state:

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

// Prevent display sleep during video playback
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
if let Some(blocker) = &blocker {
    cx.stop_power_save_blocker_checked(
        PowerSaveBlockerStopBuilder::handle(blocker).reason("video stopped"),
    )?;
}

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

`PowerThemeIdleHandoffBuilder` validates sleep-prevention plans, stop requests,
power monitor descriptors, power-source queries, native theme snapshots, idle
policies, and hosted power bridge scope before generated apps or agents mutate
system behavior. Its summary reports request kinds and next action without
logging blocker reasons, exact idle durations, battery percentages, power event
payloads, theme tokens, hosted IDs, or generated UI values.

The raw `start_power_save_blocker(PowerSaveBlockerKind::...)` and
`stop_power_save_blocker(id)` methods remain available when you already store
platform IDs yourself. Prefer the checked builder paths for media playback,
presentations, capture tools, and long-running tasks because they validate
generated reasons, expose a side-effect-free `PowerSaveBlockerPlan` for logging
or fallback decisions, provide `to_text()` / `has_reason()` for one stable audit
line without logging reason text, reject invalid stop ids, and return a typed
handle with the blocker kind and optional reason.

Use `cx.system_power_snapshot()` when you only need a synchronous view of
`power_mode`, `reduce_motion`, and `system_idle_time`. Use
`watch_system_power(...)` when you only need the initial snapshot without
callbacks. The raw `cx.on_system_power_event(...)`, `cx.power_mode()`,
`cx.reduce_motion()`, and `cx.system_idle_time()` hooks remain available for
custom routers. Use `SystemPowerEvent::to_text()` and
`SystemPowerSnapshot::to_text()` for stable adaptive-work logs that report
mode, reduce-motion, idle-telemetry presence, and reduce-work decisions without
logging exact idle durations.

Use `cx.system_power_source_snapshot_checked(...)` when product behavior depends
on whether the device is on battery, charging, or external power:

```rust
let source = cx.system_power_source_snapshot_checked(
    SystemPowerSourceQueryBuilder::new()
        .require_known_source(),
)?;

if source.is_on_battery() || source.should_reduce_work() {
    /* lower polling, sync, animation, or render quality */
}
```

`system_power_source_snapshot()` is permissive and reports
`SystemPowerSource::Unknown` plus `None` battery percentage when a platform does
not expose battery telemetry. The checked query can require a known source or
battery percentage before generated code makes a critical adaptive decision.
Use `SystemPowerSourceQueryBuilder::to_text()`,
`SystemPowerSource::to_text()`, and `SystemPowerSourceSnapshot::to_text()` when
agents need source, known/unknown, battery/external, percentage-availability,
mode, and reduce-work summaries without logging exact battery percentages.

For desktop-app native theme decisions, use `cx.native_theme_snapshot()`:

```rust
let theme = cx.native_theme_snapshot();
let background = theme.choose(dark_background, light_background);

if theme.should_reduce_effects() {
    /* disable decorative blur, motion, or expensive effects */
}
if theme.should_reduce_background_work() {
    /* lower sync, polling, or preview generation */
}
```

`NativeThemeSnapshot` combines `window_appearance()`, `reduce_motion()`, and
`power_mode()` into one small value with `is_dark()`, `is_light()`,
`is_vibrant()`, `is_low_power()`, `adaptations()`,
`should_avoid_animation()`, `should_avoid_blur_or_vibrancy()`,
`should_reduce_background_work()`, and `should_reduce_effects()` helpers. Use
`theme.to_text()` or `NativeThemeAdaptation::to_text()` for agent/debug traces
that describe the decision without logging app-specific colors or copy. Raw
platform calls remain available when a feature needs a single signal.

For desktop-app idle gating, use a checked `SystemIdlePolicy` instead of
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
are safe to run when idle telemetry is unavailable. Use
`SystemIdlePolicyBuilder::to_text()`, `SystemIdlePolicy::to_text()`, and
`SystemIdleEvaluation::to_text()` for idle-gate audit lines that avoid exact
idle durations.

---

## Native Media

For URL/file/bytes video players, prefer checked media sources before wiring a
controller, native element, or `kael_ui::VideoPlayer`:

```rust
let source = MediaSourceBuilder::url("https://cdn.example.com/movie.mp4")
    .build_checked()?;

let video = VideoController::new(source)
    .volume(0.8)
    .playback_rate(1.0);

video.add_webvtt_text_track_checked("en", "English", Some("en"), captions)?;

video.select_text_track_checked("en")?;

video.load_metadata()?;
video.play()?;
let controls = VideoPlaybackControlsBuilder::new()
    .volume(0.7)
    .playback_rate(1.25)
    .fast_seek_secs(42.0);
tracing::info!(summary = controls.to_text(), "video controls");
video.apply_controls_checked(controls)?;
```

Use `MediaSourceBuilder::file(path).require_existing_file().canonicalize_file()`
for local files, `MediaSourceBuilder::bytes(bytes)` for memory-backed clips,
and `MediaSourceBuilder::reader(key, open)` for generated reader sources with a
stable cache key. The raw `MediaSource::url(...)`, `.file(...)`, `.bytes(...)`,
and `.reader(...)` constructors remain available for custom FFmpeg inputs.
Use `MediaSourceBuilder::to_text()` for content-safe logs and agent summaries
before building or replacing a source; it reports source kind and file checks
without exposing URLs, local paths, reader keys, or media bytes.
At the UI layer, inspect `VideoPlayer::to_text()`,
`VideoPlayerState::to_text()`, `VideoCaptionStyle::to_text()`,
`AudioPlayer::to_text()`, `AudioPlayerState::to_text()`, and
`Waveform::to_text()` before generated media apps customize controls,
captions, timelines, waveforms, or event handlers. These summaries report
source kind, route, player size, controls/captions/poster/source/title
presence, progress/volume buckets, handler counts, and waveform shape without
logging media URLs, file paths, titles, caption text, exact seek times,
volume/rate values, waveform amplitudes, or colors.
For Web Audio-style apps, treat simple playback, previews, timelines,
waveforms, microphone capture, and system-audio recording as native guarded
work: pair `AudioPlayer`, `Waveform`, `CaptureConfigBuilder::microphone`,
`CaptureConfigBuilder::system_audio`, `CaptureConfigSetBuilder`,
`CaptureManager`, `CapturePipeline`, `PermissionRequestBuilder`, and
`AppPrivacyManifestBuilder` before generating recorder or editor flows. Keep
arbitrary `AudioContext` node graphs, `AudioWorklet` processors, offline
rendering, and sample-accurate scheduling as explicit WebView islands or
roadmap work until native graph APIs land.

For generated audio workflows, prefer one checked handoff before playback,
recording, waveform UI, permissions, package metadata, and browser fallbacks
split apart:

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
graph roadmap work together. Summaries report request kinds and next action
without logging media URLs, file paths, reader keys, byte payloads, device
filters, permission reasons, waveform samples, capability notes, or roadmap
text.
Use `VideoController::set_source_checked(MediaSourceBuilder::...)` or the
checked convenience methods `set_url_checked(...)`, `set_file_checked(...)`,
`set_bytes_checked(...)`, and `set_reader_checked(...)` for generated runtime
source swaps so invalid URLs, empty bytes, empty reader keys, and optionally
missing files fail before controller state is reset. Raw replacement setters
remain available for hand-validated integrations.

Use `VideoPlaybackControlsBuilder` for generated player controls, keyboard
shortcuts, and AI-authored media widgets. The checked path batches volume,
muted, playback-rate, looping, and seek changes while rejecting empty updates,
NaN/infinite values, volume outside `0.0..=1.0`, playback rates outside
`0.0625..=16.0`, negative seek seconds, and extremely large seek positions
before mutating the controller. Raw `set_volume(...)`, `set_playback_rate(...)`,
and seek methods remain available when an app already owns validation.
Use `VideoPlaybackControlsBuilder::to_text()` before applying generated control
batches; it reports which control fields are configured without logging exact
seek positions, volume values, or rates.
Use `select_text_track_checked(id)` and `disable_text_track_checked()` for
generated caption/subtitle controls so malformed ids, unknown tracks, and
double-disable actions fail without silently changing the active track. The raw
`select_text_track(...)` and `disable_text_track()` methods remain available for
permissive app-owned flows.
Use `TextTrackBuilder`, `add_text_track_checked(...)`,
`add_srt_text_track_checked(...)`, and `add_webvtt_text_track_checked(...)` for
generated caption/subtitle setup so empty metadata, empty parsed cue sets,
invalid cue ranges, and duplicate track ids fail before the controller changes
state.

For WebView-routed browser video, use `WebViewVideoCommandBuilder` with
`VideoController::dispatch_webview_command_checked(...)` or
`webview_command_script_checked(...)` to drive custom chrome with checked
play/pause, seek, volume, mute, playback-rate, loop, text-track, fullscreen,
picture-in-picture, and snapshot commands. The raw `WebViewVideoCommand` path
remains available for hand-validated integrations. Use
`WebViewVideoCommandBuilder::to_text()` and `command_kind()` before dispatch
when generated controls need to log command category, seek/audio/presentation
class, or invalid-command audits without exposing seek positions, volume/rate
values, or text-track selectors.

For generated desktop-app video players, use
`cx.video_element_handoff_checked(VideoElementHandoffBuilder::...)` when the app
wants the closest replacement for `<video src="...">` plus JavaScript control
wiring: URL in, checked route out, optional initial controls, optional
playlist/media-key intent, and a next action that says whether to render native,
render the checked browser fallback, accept documented limits, or build backend
support. Use `VideoUrlPlaybackHandoff` for the smaller source-to-render
handoff. Drop to `VideoPlaybackPlanBuilder` when the generator needs to tune
content-type routing, fallback options, or requirement audits directly.

```rust
let handoff = cx.video_element_handoff_checked(
    VideoElementHandoffBuilder::url(video_url.clone())
        .initial_controls(
            VideoPlaybackControlsBuilder::new()
                .volume(0.6)
                .muted(false)
                .playback_rate(1.0),
        )
        .playlist(VideoPlaylist::new([MediaSource::url(video_url)])),
)?;

tracing::info!(summary = handoff.to_text(), "video element handoff");
let requirements = handoff.requirement_plan();
tracing::info!(summary = requirements.to_text(), "video element requirements");
let instruction = handoff.render_instruction();
tracing::info!(summary = instruction.to_text(), "video render instruction");
```

`VideoElementHandoff` exposes `controller_checked()`,
`media_key_binding_builder_checked()`, `initial_controls()`, `playlist()`,
`requirement_plan()`, `next_action()`, `is_native()`,
`uses_webview_fallback()`, `is_ready()`, and `to_text()`. Its summaries report
source kind, route, controls, playlist count, requirement counts, and next
action without logging URLs, file paths, MIME strings, seek positions, volume
values, or fallback page data URLs.

When the app needs the “developers can tweak the video element however they
want” surface that Electron gets from DOM JavaScript, add a checked
customization plan before rendering:

```rust
let customization = cx.video_element_customization_plan_checked(
    VideoElementCustomizationPlanBuilder::new(handoff.clone())
        .html_video_baseline()
        .timeline_scrubbing()
        .captions_ui()
        .fullscreen()
        .picture_in_picture()
        .playlist_media_keys(),
)?;

tracing::info!(
    summary = customization.to_text(),
    "video element customization"
);
```

`cx.video_element_customization_plan_checked(...)` validates custom
properties/events, app-owned controls, timeline scrubbing, captions UI,
fullscreen, picture-in-picture, hardware decode, source switching, and
playlist/media-key behavior. The resulting plan reports satisfied, limited, and missing
customization counts plus a next action: render the configured player, accept a
documented limit, use the checked WebView fallback for browser-only media
behavior, configure playlist/handlers, or build missing native backend support.
Use it before claiming Electron-style video-element customizability; its
summary avoids URLs, file paths, MIME strings, caption text, seek positions,
volume/rate values, fallback reasons, and generated data URLs.

When the generator needs to tune content-type routing or fallback options, build
the checked plan directly:

```rust
let tuned = VideoPlaybackPlanBuilder::url(video_url)
    .content_type(content_type_header)
    .webview_options(WebViewVideoOptions::default().controls(true));
tracing::info!(summary = tuned.to_text(), "video playback plan builder");
let tuned = tuned.build_checked()?;
tracing::info!(summary = tuned.to_text(), "video playback plan");

let instruction = tuned.render_instruction();
tracing::info!(summary = instruction.to_text(), "video render instruction");
match instruction {
    VideoPlaybackRenderInstruction::Native { controller } => {
        let video = controller;
        video.load_metadata()?;
        video.play()?;
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

`VideoPlaybackPlanBuilder` validates URLs/files/bytes/readers through
`MediaSourceBuilder`, validates optional MIME/content types, validates
`WebViewVideoOptions`, and rejects memory-backed sources when a WebView fallback
is requested because browsers need a URL/file source. Use
`render_instruction()` when generated code wants the Desktop-like path: a
validated source becomes either a ready `VideoController` for native rendering
or a WebView page URL and stable element id for browser media fallback.
Use `VideoPlaybackPlan::to_text()` to log the selected target, route, native
`canPlayType` confidence, content-type presence, and fallback track count
without exposing the source URL or file path.
Use `VideoPlaybackPlanBuilder::to_text()` before build when generated code needs
to audit source kind, content-type presence, WebView preference, fallback track
count, and start-position presence without logging media URLs, MIME strings, or
paths. Use `VideoPlaybackRenderInstruction::to_text()` after planning when
dispatch logs should distinguish native controller rendering from browser
fallback without logging generated data URLs or fallback reasons.
When a generated player requires specific affordances beyond the baseline URL
player promise, evaluate them explicitly:

```rust
let requirements = tuned.requirement_plan([
    VideoPlaybackRequirement::BasicPlayback,
    VideoPlaybackRequirement::TextTracks,
    VideoPlaybackRequirement::AdaptiveStreaming,
    VideoPlaybackRequirement::HardwareDecode,
]);
tracing::info!(summary = requirements.to_text(), "video requirements");
```

`VideoPlaybackRequirementPlan` reports which requested affordances are
satisfied, limited, or missing for the selected native/WebView route. Use it
before claiming desktop-app parity for video players that need adaptive
streaming, picture-in-picture, fast seek, playback-rate controls, hardware
decode, or native stream selection; exact requirement getters are available for
setup screens and tests, while `to_text()` reports only counts, target, and the
next action. Use `next_action()` when generated builders need a single handoff:
`RenderPlannedRoute` means the selected route can be rendered, `AcceptLimitedSupport`
means the app can ship with an explicit limitation, `UseWebViewFallback` means
the requested capability belongs in a browser media island for now, and
`BuildNativeBackend` means native backend work is still required. Use
`requires_webview_fallback()`, `webview_fallback_requirements()`,
`requires_native_backend_work()`, and `native_backend_work_requirements()` to
split the checklist into browser-island work and native media-runtime work
without parsing prose.
Use `video_capability_report()` before claiming native media parity with
Desktop. The report exposes `full_count()`, `partial_count()`,
`roadmap_count()`, `native_gap_count()`, `has_native_gaps()`,
`has_webview_fallback()`, and `to_text()` so agents can distinguish direct
native support from WebView fallback and roadmap work.

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
tracing::info!(summary = options.to_text(), "webview video options");

let page_url = webview_video_player_url(&source, &options)
    .expect("URL/file media can be wrapped for WebView fallback");
```

`WebViewVideoOptions::validate()` rejects empty/padded poster or track URLs,
unsupported URL schemes, invalid `controlslist` tokens, invalid text-track
metadata, and unsafe `object-fit` values before generated media UI reaches the
embedded browser.
Use `WebViewVideoOptions::to_text()` for content-safe fallback-page summaries;
it reports booleans and counts without logging poster URLs, track URLs, or
inline caption text.

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
playlist routing without a bound video controller. Use `playlist.to_text()` and
`binding.to_text()` for content-safe diagnostics that report source counts,
repeat state, controller routing, playlist presence, and callback wiring without
logging media URLs, file paths, source keys, or callback internals. Use raw
`install(...)` or the lower-level `on_media_key_event(...)` callback when you
need a custom event router.

---

## User Attention

Bounce the dock icon, flash the taskbar, or request equivalent desktop
attention for background work:

```rust
let attention = UserAttentionBuilder::informational()
    .reason("download complete")
    .build_plan_checked()?;
tracing::info!(summary = attention.to_text(), "user attention");
let request = cx.request_user_attention_plan_checked(attention)?;
tracing::info!(summary = request.to_text(), "active attention");

// Later, when the user opens the app or the condition is resolved:
request.cancel(cx);

cx.cancel_user_attention_checked(
    UserAttentionCancelBuilder::condition_resolved("download opened"),
)?;
```

Use `UserAttentionBuilder::critical()` for urgent conditions that should keep
requesting attention until cancelled. Use
`UserAttentionCancelBuilder::app_activated()` when activation alone clears the
signal. The checked request and cancel paths reject empty reasons; the raw
`request_user_attention(AttentionType::...)`, `request_user_attention_with(...)`,
and `cancel_user_attention()` methods remain available when you already manage
the attention lifecycle. Use `UserAttentionPlan` and `UserAttentionRequest`
helpers such as `is_informational()`, `is_critical()`, `has_reason()`, and
`to_text()` when generated UI, logs, tests, or agents need to explain the
platform attention signal before or after dispatch.

---

## Window Progress

For long-running operations, first build an operation-level progress plan:

```rust
let progress = cx.progress_indicator_checked(
    ProgressIndicatorBuilder::normal("export", 0.42)
        .window()
        .cancellable()
        .unit("files"),
)?;
tracing::info!(summary = progress.to_text(), "operation progress");
if let Some(window_progress) = progress.window_progress_builder() {
    window.set_progress_bar_checked(window_progress)?;
}
```

`ProgressIndicatorBuilder` validates hidden, indeterminate, normal, paused, and
error progress states before generated UI exposes inline progress, platform
window progress, cancellation affordances, or coarse unit labels. Use
`progress.to_text()` for content-safe agent logs; it reports progress kind,
determinate state, completion bucket, inline/window targets, cancellation, and
unit presence without logging operation scopes, exact fractions, or unit text.

Show download, export, install, or sync progress in the platform window
representation:

```rust
window.set_progress_bar_checked(WindowProgressBuilder::normal_percent(42))?;
window.set_progress_bar_checked(WindowProgressBuilder::indeterminate())?;

// Later, when work completes or the user cancels:
window.set_progress_bar_checked(WindowProgressBuilder::none())?;
```

Use `WindowProgressBuilder::normal(...)`, `error(...)`, and `paused(...)` for
checked fractional determinate states, or `normal_percent(...)`,
`error_percent(...)`, and `paused_percent(...)` when generated code has whole
percentages. The checked builder and `set_progress_bar_checked(...)` reject NaN,
infinity, and fractions outside `0.0..=1.0` before values reach dock/taskbar
APIs. Inspect builders or raw states with `kind()`, `is_determinate()`,
`is_clear()`, and `to_text()` for content-safe traces that avoid logging exact
progress fractions. The raw `set_progress_bar(...)` method and
`ProgressBarState` enum remain available when the caller has already validated
platform-specific state.

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

Before replacing the app-wide HTTP client used by updaters, downloads, workers,
or generated integrations, validate the client metadata:

```rust
let descriptor = cx.set_http_client_checked(
    AppHttpClientInstallBuilder::new(client)
        .require_user_agent()
        .disallow_proxy(),
)?;

println!("HTTP client: {}", descriptor.type_name());
```

`AppHttpClientInstallBuilder` does not send requests or replace
`NetworkPolicy`. It checks the reported client type name, optional user-agent,
and optional proxy URL before the client becomes app-wide state. Use
`.require_user_agent()` when generated app requests must identify themselves,
and `.disallow_proxy()` for offline, local-only, or privacy-sensitive builds.

For outbound requests from workers, extensions, sync clients, or generated HTTP
integrations, pair network status with a checked host policy:

```rust
let policy = NetworkPolicyBuilder::new()
    .allow_host("api.example.com")
    .allow_url("https://cdn.example.com/assets/app.js")?
    .build_checked()?;

let handoff = cx.network_realtime_handoff_checked(
    NetworkRealtimeHandoffBuilder::new()
        .request_builder(AppNetworkRequestBuilder::post("https://api.example.com/v1/sync"))?
        .realtime_connection_builder(
            AppRealtimeConnection::websocket("wss://events.example.com/socket")
                .protocol("app.v1")
                .reconnect_conservative(),
        )?
        .network_policy(policy.clone())
        .hosted_network_bridge("checkout"),
)?;

tracing::info!(summary = handoff.to_text(), "network/realtime handoff");

if policy.check_url("https://api.example.com/v1/sync")? {
    // Safe to hand this URL to the app HTTP client.
}

let request = AppNetworkRequestBuilder::post("https://api.example.com/v1/sync")
    .header("Content-Type", "application/json")
    .header("X-Client-Version", env!("CARGO_PKG_VERSION"))
    .body_size_bytes(512)
    .network_policy(policy.clone());

tracing::info!(summary = request.to_text(), "app network request");
tracing::debug!(summary = request.to_safe_text(), "app network request shape");
let request = request.build_checked()?;
```

`NetworkRealtimeHandoffBuilder` validates app-owned HTTP descriptors, realtime
connection descriptors or sets, network policy, and hosted network bridge scope
before generated workers or agents dispatch network side effects. Its summary
reports request kinds and next action without logging URLs, hosts, headers,
body contents, credentials, cookies, subprotocols, destination paths, byte
sizes, hashes, or reconnect timings.

`NetworkPolicyBuilder` validates host strings and URL-derived hosts, rejects
non-HTTP(S) URLs, duplicate hosts, and mixed allow/deny lists, and defaults to
`DenyAll` when no hosts are configured.
Use `AppNetworkRequestBuilder` when generated workers, plugin hosts, sync
clients, or export flows need checked request metadata before using the app HTTP
client. It validates HTTP(S) URLs, host policy, request methods, duplicate or
malformed headers, CR/LF header injection, optional body sizes, and body/method
shape. Use `AppNetworkRequestBuilder::validate()` and `to_text()` before
queueing generated network work so the app can show or log a credential-safe
plan before the builder is consumed. Use `to_safe_text()` when agent traces or
telemetry should avoid hosts and exact body sizes. It does not send the request;
it is the typed handoff to your transport.

## Background Jobs

Queue indexing, sync, export, and agent work through the app scheduler with
checked metadata:

```rust
let descriptor = JobDescriptor::new("export/video")
    .with_priority(JobPriority::High)
    .with_retry_policy(retry_policy);
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

Use `cx.schedule_job_checked(job)?` for default descriptor settings. Checked
scheduling rejects invalid job IDs, descriptor/job ID mismatches,
self-dependencies, duplicate or invalid dependency IDs, and invalid retry
policies before the queue is mutated. Use `JobPriority::key()`,
`RetryPolicy::to_text()`, `JobDescriptor::to_text()`, `JobStatus::to_text()`,
`JobProgress::to_text()`, and `JobInfo::to_text()` for generated scheduler logs,
status panels, and agents without logging job ids, dependency ids, progress
messages, exact percentages, retry attempts, or timing details. Raw
`schedule_job(...)` and lower-level `JobScheduler::schedule(...)` remain
available when the app owns validation.
Use `cx.background_work_handoff_checked(...)` with
`BackgroundWorkHandoffBuilder::job(...)`, `.descriptor(...)`, `.progress(...)`,
`.cancel(...)`, `.pause(...)`, `.resume(...)`, `.worker_pool(...)`, or
`.helper_process(...)` before generated work queues, status panels, progress
bridges, and agent loops mutate scheduler state.
Inspect `BackgroundWorkNextAction`, `is_job()`, `is_progress()`, `is_cancel()`,
`is_pause()`, `is_resume()`, `is_worker_pool()`, `is_helper_process()`, typed
accessors, and `to_text()` to route work without logging job ids, dependency
ids, progress messages, percentages, worker reasons, helper-process reasons, or
payloads.

For long-lived realtime transports, use `AppRealtimeConnection` as the checked
descriptor before opening a WebSocket or server-sent events stream:

```rust
let realtime = cx.realtime_connection_checked(
    AppRealtimeConnection::websocket("wss://events.example.com/socket")
        .protocol("kael.v1")
        .heartbeat_interval(std::time::Duration::from_secs(30))
        .max_message_bytes(64 * 1024)
        .reconnect_policy(AppRealtimeReconnectPolicy::conservative())
        .network_policy(policy),
)?;

tracing::info!(summary = realtime.to_text(), "app realtime connection");
tracing::debug!(summary = realtime.to_safe_text(), "app realtime shape");
```

`AppRealtimeConnection` validates WebSocket `ws`/`wss` URLs, EventSource
`http`/`https` URLs, duplicate or malformed headers, WebSocket subprotocol
tokens, heartbeat bounds, inbound message budgets, reconnect/backoff policy, and
attached network policy. Use
`cx.realtime_connection_checked(AppRealtimeConnection::...)` before opening
generated realtime work so the app can show or log a credential-safe plan before
the builder is consumed. Use
`AppRealtimeReconnectPolicy::conservative()` for ordinary chat/presence flows or
`.persistent()` for critical background sync; custom policies reject more than
100 attempts, sub-100ms initial delays, max delays below initial delays, and max
delays above one hour. Use `to_safe_text()` when agent traces or telemetry
should avoid hosts, heartbeat timing, reconnect timing, and exact message-size
budgets. It does not open the socket; it gives generated agents and native
workers a typed, auditable handoff to the app realtime transport.
For apps that open multiple channels together, group descriptors with
`cx.realtime_connection_set_checked(AppRealtimeConnectionSet::builder()...)`.
The checked app helper rejects empty sets and exact duplicate connection
descriptors, while preserving per-connection validation. Use
`connection_count()`, `websocket_count()`,
`server_sent_events_count()`, `protocol_count()`, `header_count()`,
`heartbeat_count()`, `max_message_count()`, `reconnect_policy_count()`,
`network_policy_count()`, `connections()`, `into_connections()`, `to_text()`,
and `to_safe_text()` before opening chat, presence, notification,
collaboration, or background-sync transports.

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
Inspect `SessionSnapshotBuilder::to_text()` before saving generated state and
`SessionSnapshot::to_text()` after loading it so traces expose window counts,
display-bound/fullscreen counts, app-data presence, app-data JSON shape, and
coarse bounds state without logging window IDs, workspace IDs, paths, tab names,
tokens, arbitrary JSON values, display IDs, or exact bounds.

Use `restore_window_states(...)` when reopening windows after monitor changes:

```rust
let displays = cx.displays().iter().map(|display| display.id()).collect::<Vec<_>>();
let primary = cx.primary_display().map(|display| display.id());
let restored = store.restore_window_states(&displays, primary)?;
```

Use `restore_window_states_with_summary(...)` when restore UX, diagnostics, or
agents need to know whether disconnected-display relocation happened. The
returned `SessionRestoreResult` exposes `window_states()`, `into_window_states()`,
`window_count()`, `relocated_window_count()`, `available_display_count()`,
`has_primary_display()`, `has_relocations()`, and content-safe `to_text()`.

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

For tray panels, prefer the tray-specific resolver so generated apps do not have
to hand-roll missing-bounds behavior:

```rust
let panel = cx.resolve_tray_panel_placement_checked(
    TrayPanelPlacementBuilder::new(size(px(360.), px(280.)))
        .fallback_bottom_right(px(16.)),
)?;

cx.open_window(
    WindowOptionsBuilder::new()
        .title("Sync")
        .placement(&panel)
        .floating(),
    |_window, cx| cx.new(|_| SyncPanel::new()),
)?;
```

`TrayPanelPlacementBuilder` centers above the tray icon when
`tray_icon_bounds()` is available, otherwise it uses an explicit fallback such
as bottom-right, top-right, or center. Use `.require_tray_icon_bounds()` when an
anchored panel should fail instead of falling back. The checked resolver rejects
invalid sizes and fallback margins before computing bounds.

For Desktop `screen`-style display queries, use `DisplayQueryBuilder`:

```rust
let display_setup = cx.display_topology_handoff_checked(
    DisplayTopologyHandoffBuilder::new()
        .display_query(DisplayQueryBuilder::all())
        .display_query(DisplayQueryBuilder::cursor().fallback_to_primary())
        .window_placement(WindowPlacementBuilder::new(size(px(420.), px(320.))).bottom_right(px(16.)))
        .runtime_snapshot(AppRuntimeSnapshotQueryBuilder::new().require_window())
        .focused_window(FocusedWindowQueryBuilder::new().current_process_only())
        .hosted_screen_island("browser-device-pixel-ratio")
        .roadmap_work("display change observer recipes"),
)?;

match display_setup.next_action() {
    DisplayTopologyNextAction::QueryNativeDisplays => query_displays(),
    DisplayTopologyNextAction::ResolveWindowPlacement => resolve_placement(),
    DisplayTopologyNextAction::ReviewRuntimeState => review_runtime_state(),
    DisplayTopologyNextAction::QueryFocusedWindow => query_focused_window(),
    DisplayTopologyNextAction::UseHostedScreenIsland => route_hosted_screen_island(),
    DisplayTopologyNextAction::TrackDisplayRoadmap => record_gap(),
}

let primary = cx
    .query_displays_checked(DisplayQueryBuilder::primary())?
    .first()
    .cloned();

let cursor_display = cx
    .query_displays_checked(DisplayQueryBuilder::cursor().fallback_to_primary())?
    .first()
    .cloned();

let displays = cx.query_displays_checked(DisplayQueryBuilder::all())?;
let topology = displays.topology_summary();
tracing::info!(summary = topology.to_text(), "display topology");
```

`DisplaySnapshot` exposes display id, optional stable UUID, bounds, default
window bounds, scale factor, refresh rate, whether it is primary, and whether it
contains the cursor. Use `scale_factor()` for Desktop `screen` /
`deviceScaleFactor` style HiDPI decisions, pixel-perfect capture sizing, and
canvas/media backing-store calculations. Use `topology_summary()` when builders
or agents need one audited value for single-vs-multiple-display layout, primary
presence, cursor matching, virtual bounds, max scale, max refresh rate, high-DPI
displays, and high-refresh displays. Checked queries can require a match, allow
empty results, or fall back to the primary display for cursor/id lookups.
`DisplayTopologyHandoffBuilder` validates display queries, monitor-aware window
placement, runtime state checks, focused-window queries, hosted screen islands,
and roadmap work before side effects. Inspect `DisplayTopologyHandoff::to_text()`
without logging display ids, UUIDs, labels, coordinates, bounds, scale factors,
refresh rates, window titles, hosted surface ids, roadmap text, or app content.

Enumerate raw monitors when you need platform handles or backend-specific data:

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
let handoff = cx.crash_reporting_handoff_checked(
    CrashReportingHandoffBuilder::new()
        .reporter(
            CrashReporterBuilder::new("com.example.my-app")
                .endpoint("https://crashes.example.com/reports")
                .http_client(http_client.clone()),
        )
        .pending_upload(
            CrashReporterBuilder::new("com.example.my-app")
                .endpoint("https://crashes.example.com/reports")
                .http_client(http_client.clone()),
            NetworkPolicyBuilder::new().allow_host("crashes.example.com"),
            consent_recorded,
        )
        .support_diagnostics(SupportDiagnosticsBuilder::new())
        .hosted_crash_dashboard("vendor-crash-console")
        .roadmap_work("native symbol upload"),
)?;

match handoff.next_action() {
    CrashReportingNextAction::ConfigureCrashReporter => {}
    CrashReportingNextAction::ReviewPendingUpload => {}
    CrashReportingNextAction::ExportSupportDiagnostics => {}
    CrashReportingNextAction::UseHostedCrashDashboard => {}
    CrashReportingNextAction::TrackCrashRoadmap => {}
}
```

`CrashReportingHandoffBuilder` validates reporter setup, pending upload consent,
endpoint network policy, support diagnostics, hosted crash dashboards, and
roadmap work before installing hooks, submitting reports, exporting diagnostics,
or opening hosted crash consoles. `CrashReportingHandoff::to_text()` summarizes
request kinds and next action without logging app ids, endpoints, report
directories, hostnames, panic text, backtraces, paths, hosted ids, or roadmap
details.

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
tracing::info!(summary = lifecycle.to_text(), "app lifecycle policy");

let auto_launch = AutoLaunchBuilder::enable("com.example.app");
let auto_launch_plan = cx.auto_launch_plan_checked(auto_launch.clone())?;
tracing::info!(summary = auto_launch_plan.to_text(), "auto launch plan");
let launch = cx.configure_auto_launch(auto_launch)?;
let enabled = launch.enabled();
tracing::info!(summary = launch.to_text(), "auto launch status");

// App ids are validated before platform registration.
assert_eq!(launch.app_id(), "com.example.app");

let activate = AppLifecycleCommand::activate_with_options(true)
    .reason("show existing project window");
let activate_plan = cx.lifecycle_command_plan_checked(activate.clone())?;
tracing::info!(summary = activate_plan.to_text(), "lifecycle command plan");
if activate_plan.is_ready() {
    cx.perform_lifecycle_command_checked(activate)?;
}

let restart = AppLifecycleCommand::restart("apply downloaded update");
tracing::info!(summary = restart.to_text(), "lifecycle command");
let restart_plan = cx.lifecycle_command_plan_checked(restart.clone())?;
tracing::info!(summary = restart_plan.to_text(), "lifecycle command plan");
cx.perform_lifecycle_command_checked(restart)?;

let badge = DockBadgeBuilder::count(3);
tracing::info!(summary = badge.to_text(), "dock badge");
cx.set_dock_badge_checked(badge)?;
let status_badge = DockBadgeBuilder::label("sync");
tracing::info!(summary = status_badge.to_text(), "dock badge");
cx.set_dock_badge_checked(status_badge)?;
cx.set_dock_badge_checked(DockBadgeBuilder::clear())?;
cx.set_dock_menu_checked(
    DockMenuBuilder::new()
        .action("Show Window", menu_action::ShowWindow)
        .separator()
        .action("Quit", menu_action::Quit),
)?;
cx.perform_dock_menu_action_checked(DockMenuActionBuilder::new(0))?;
window.set_progress_bar_checked(ProgressBarState::normal(0.7)?)?;
let recent_documents = RecentDocumentsBuilder::new()
    .require_existing_files()
    .canonicalize()
    .document("/path/to/report.pdf")
    .document("/path/to/notes.md");
tracing::info!(summary = recent_documents.to_text(), "recent documents");
let recent_plan = cx.recent_documents_checked(recent_documents.clone())?;
tracing::info!(summary = recent_plan.to_text(), "recent documents");
cx.add_recent_documents(recent_documents)
    .expect("recent document paths");
let clear_recents = RecentDocumentsClearBuilder::new("User cleared recent files");
tracing::info!(summary = clear_recents.to_text(), "clear recent documents");
cx.clear_recent_documents_checked(clear_recents)?;
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
let jump_list = JumpListBuilder::new()
    .action("Open Project", menu_action::Open)
    .workspace_path("/path/to/project")
    .workspace(["/path/to/project", "/path/to/workspace.code-workspace"]);
tracing::info!(summary = jump_list.to_text(), "jump list");
let jump_list_plan = cx.jump_list_checked(jump_list)?;
tracing::info!(summary = jump_list_plan.to_text(), "jump list");
cx.update_jump_list_plan_checked(jump_list_plan);
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
`.restart(reason)` for Desktop `app.focus(...)`, hide/show, quit, and relaunch
flows. `perform_lifecycle_command_checked(...)` validates optional diagnostics
and requires an explicit reason before quit or restart dispatches. Use
`lifecycle_command_plan_checked(...)` or
`AppLifecycleCommand::plan_against_with_restart_path(...)` before dispatch when
generated setup, menus, updater prompts, or agents need to know the current
window count, background-runtime state, shutdown-in-progress flag, restart-path
presence, and readiness without mutating app state. Use policy, command, and
plan `to_text()` summaries in generated startup/quit traces; they report
behavior, keep-alive state, cleanup timeout, command kind, terminal-ness,
activation options, window count, restart-path presence, and reason presence
without logging diagnostic reason text.
Use `cx.app_lifecycle_startup_handoff_checked(...)` with
`AppLifecycleStartupHandoffBuilder::policy(...)`, `.command(...)`,
`.duplicate_launch(...)`, `.auto_launch(...)`, `.recent_documents(...)`, or
`.clear_recent_documents(...)` when agents need one checked descriptor for
Electron-style startup and app-state flows before mutating windows, process
lifecycle, login items, or OS recent-document state. Inspect
`AppLifecycleStartupNextAction`, `is_policy()`, `is_command()`,
`is_duplicate_launch()`, `is_auto_launch()`, `is_recent_documents()`,
`is_clear_recent_documents()`, typed request accessors, and `to_text()` for
routing without logging app ids, launch args, environment values, executable
paths, current directories, document paths, duplicate payloads, or reason text.

Use `cx.runtime_snapshot_checked(...)` when startup gates, diagnostics, or AI
agents need a single read-only view of app readiness and lifecycle state:

```rust
let runtime = cx.runtime_snapshot_checked(
    AppRuntimeSnapshotQueryBuilder::new()
        .require_not_quitting()
        .require_network_online()
        .allow_background_runtime(),
)?;

if runtime.is_background_runtime() {
    tracing::info!("running without visible windows");
}

if runtime.power().should_reduce_work() {
    schedule_lightweight_sync();
}
```

`AppRuntimeSnapshot` includes the capability process id, uptime, window count,
keep-alive policy, quit-cleanup timeout, quitting flag, network status, system
power snapshot, and native theme snapshot. The checked query can require
not-quitting state, an open foreground window, a background/tray runtime, online
network status, or explicit background-runtime tolerance before generated work
starts. This is the app-runtime companion to `CapabilityReport::current()`: use
the capability report for platform support, and the runtime snapshot for current
process/app state.

For desktop-app `visual capture` workflows, build a checked app-window capture
request before invoking a platform snapshot backend:

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

`AppWindowCaptureRequestBuilder` targets the focused window, a specific app
window, or all visible app windows. It validates purpose text, requested PNG or
raw RGBA output, optional window chrome/cursor flags, max dimensions, max pixel
count, and the multi-window rule that cursor capture is ambiguous. Gate capture
backends with `CapabilityReport::current().is_available(PlatformFeature::AppWindowCapture)`.
Requests that allow occluded/minimized OS-level capture expose
`required_capability() == Some(Capability::ScreenCapture)`; visible app-owned
window render snapshots do not require that capability.
`VisualCaptureHandoffBuilder` validates app-window capture, native
headless/cached/effect evidence, scoped hosted element/DOM/media capture,
support diagnostics, and roadmap capture work before screenshots, thumbnails,
support bundles, or AI-agent visual evidence runs. Inspect
`VisualCaptureNextAction` to choose app-window capture, native evidence, hosted
surface capture, support diagnostics export, or roadmap work. Its summary avoids
logging pixels, file paths, URLs, selectors, document text, window titles,
bounds, coordinates, image bytes, exact dimensions, hosted ids, roadmap reason
text, or generated preview contents.

For desktop-app `window management` state checks before issuing commands, capture
one runtime window snapshot:

```rust
let snapshot = window.runtime_snapshot_checked(
    WindowRuntimeSnapshotQueryBuilder::new()
        .require_visible()
        .require_display(),
)?;

if !snapshot.is_fullscreen() && snapshot.animations_enabled() {
    window.perform_window_interaction_checked(WindowInteractionCommand::enter_fullscreen())?;
}
```

`WindowRuntimeSnapshot` includes bounds, persistable `WindowBounds`, viewport
size, display id, scale factor, appearance, active/hovered/visible state,
fullscreen, maximized, power mode, and reduce-motion state. Use the checked
query when generated chrome or agents require a visible, active, or
display-associated window before taking action; raw `window_state()`,
`bounds()`, `is_fullscreen()`, `is_maximized()`, and `is_window_visible()`
remain available for hand-validated flows.

For Desktop `window management.show()`, `.hide()`, `.close()`, `.focus()`,
`.minimize()`, native zoom/maximize, `setFullScreen(...)`, and
`setIgnoreMouseEvents(...)` style flows, use a checked window interaction
command:

```rust
window.perform_window_interaction_checked(WindowInteractionCommand::show())?;
window.perform_window_interaction_checked(WindowInteractionCommand::activate())?;
window.perform_window_interaction_checked(WindowInteractionCommand::zoom_window())?;
window.perform_window_interaction_checked(
    WindowInteractionCommand::close("User confirmed close"),
)?;
window.perform_window_interaction_checked(WindowInteractionCommand::enter_fullscreen())?;
window.perform_window_interaction_checked(WindowInteractionCommand::exit_fullscreen())?;
window.perform_window_interaction_checked(WindowInteractionCommand::toggle_fullscreen())?;
window.perform_window_interaction_checked(
    WindowInteractionCommand::mouse_passthrough("Heads-up overlay should not block clicks"),
)?;
window.perform_window_interaction_checked(WindowInteractionCommand::receive_mouse_events())?;
```

`WindowInteractionCommand` validates optional diagnostics and requires an
explicit reason before requesting close or enabling mouse pass-through. Close
requests run through the platform lifecycle and existing
`on_window_should_close(...)` hooks; click-through windows can be hard for users
to recover if generated accidentally. Use fullscreen interaction commands for
ordinary menu, shortcut, preview, and media controls; use
`WindowPresentationPolicyBuilder` when fullscreen carries presentation or kiosk
intent. Raw `show_window()`, `hide_window()`, `close_window()`,
`activate_window()`, `minimize_window()`, `is_window_visible()`,
`toggle_fullscreen()`, and `set_mouse_passthrough(...)` remain available for
already-validated custom integrations.

For editor surfaces, custom titlebars, and desktop-app native window
affordances, dispatch checked system-UI commands instead of calling raw platform
hooks directly:

```rust
window.perform_window_system_ui_command_checked(
    WindowSystemUiCommand::show_character_palette().reason("Editor emoji and symbol picker"),
)?;
window.perform_window_system_ui_command_checked(WindowSystemUiCommand::titlebar_double_click())?;
window.perform_window_system_ui_command_checked(WindowSystemUiCommand::zoom_window())?;
```

`WindowSystemUiCommand` validates optional diagnostics before opening the native
character palette, performing the platform titlebar double-click action, or
toggling platform window zoom/maximize behavior. Raw
`show_character_palette()`, `titlebar_double_click()`, and `zoom_window()`
remain available for already-validated platform-specific integrations.

For document editors and workspace apps that use native window tabs, dispatch
checked tab commands instead of calling platform tab hooks directly:

```rust
window.perform_window_tab_command_checked(
    WindowTabCommand::merge_all_windows().reason("Collect project windows"),
)?;
window.perform_window_tab_command_checked(WindowTabCommand::move_tab_to_new_window())?;
window.perform_window_tab_command_checked(WindowTabCommand::toggle_tab_overview())?;
```

`WindowTabCommand` validates optional diagnostics before merging compatible
windows into tabs, detaching the current tab into a new window, or toggling the
native tab overview. Raw `merge_all_windows()`, `move_tab_to_new_window()`, and
`toggle_window_tab_overview()` remain available for already-validated
platform-specific integrations.

For mostly-static native windows, apply a checked render policy before enabling
whole-frame damage skipping:

```rust
window.set_render_policy_checked(
    WindowRenderPolicyBuilder::frame_skip("Static settings panel"),
)?;
window.set_render_policy_checked(WindowRenderPolicyBuilder::no_frame_skip())?;
```

The checked policy requires a diagnostic reason when frame skipping is enabled,
which keeps generated apps from applying it blindly to live video, WebView, or
animation-heavy surfaces. Raw `set_frame_skip_enabled(...)` remains available
for renderer-owned policy.

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
window.set_client_inset_checked(WindowClientInsetBuilder::new(px(8.0)))?;
```

`WindowChromeCommand` validates optional diagnostics and rejects non-finite
window-menu positions before platform backends receive them. Use `key()`,
`has_reason()`, and `to_text()` for custom chrome traces that avoid logging menu
coordinates or reason text. Raw
`request_decorations(...)`, `show_window_menu(...)`, `start_window_move()`, and
`start_window_resize(...)` remain available for already-validated custom chrome.
Use `set_client_inset_checked(WindowClientInsetBuilder::new(inset))` for
client-side decoration resize borders; checked insets reject NaN, infinite,
negative, and excessively large values before compositor state changes. Use
`is_zero()` and `to_text()` for inset traces that avoid logging exact pixel
values.

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
text before the platform tries to render a dock badge or taskbar overlay. Use
`to_text()`, `has_label()`, `is_clear()`, and `is_count()` for content-safe badge
traces that avoid logging badge text or count values. Raw
`set_dock_badge(Some(label))` and `set_dock_badge(None)` remain available when
you already own validation.

Use `DockMenuBuilder` for app icon context menus so generated action labels,
submenus, and separators are validated before platform installation. The checked
path rejects empty menus, separator-only menus, empty submenu trees, padded
labels, control characters, and overly long labels. Raw `set_dock_menu(items)`
remains available for apps that already own menu validation.

Use `perform_dock_menu_action_checked(DockMenuActionBuilder::new(index))` when
test harnesses, Windows jump-list callbacks, or generated shell glue need to
dispatch an installed dock/taskbar action by index. The checked path rejects
dispatch before any menu actions are installed, rejects indexes outside the
platform's installed action list, and reports unsupported platforms instead of
silently dropping the event. Raw `perform_dock_menu_action(index)` remains
available for platform-specific integrations.

Use `JumpListBuilder` for Windows taskbar jump lists and desktop-app recent
workspace groups. `build_checked()` returns a `JumpListPlan` with task,
workspace, workspace-path, canonicalization, and existence-policy summary
helpers before OS state changes. Builder and plan `to_text()` summaries avoid
logging action labels or workspace paths. Task entries are validated as action menu
items, workspace entries must contain at least one non-empty path, and optional
`.require_existing_paths().canonicalize()` gives generated apps a safer path for
project launchers. Raw `update_jump_list(menus, entries)` remains available for
custom Windows integrations.

Enforce a single running instance — acquire a lock at startup and forward later launches to the existing process:

```rust
use kael::{SingleInstanceBuilder, SingleInstanceLaunch};

let launch = SingleInstanceBuilder::new("com.example.app").launch()?;
tracing::info!(summary = launch.to_text(), "single-instance launch");

match launch {
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
`launch.notified_existing()` for telemetry and branch-free startup plumbing, or
`launch.to_text()` for one stable startup log line. Use
`SingleInstance::acquire(...)` and `send_activate_to_existing(...)` directly when
you need lower-level lock/notification control.

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
tracing::info!(summary = request.to_text(), "biometric authentication");

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
generated reason strings. Inspect `request.available()`, `request.kind()`, and
`request.to_text()` when generated fallback UI, logs, or agents need a stable
summary. The raw `cx.authenticate_biometric(reason, callback)` hook remains
available for platform-specific flows.

## Graphics Capabilities

Use `graphics_capability_report()` before generated apps promise
desktop-app custom visuals, browser canvas, WebGL/WebGPU, or native shader
escape hatches:

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

if graphics.has_webview_fallbacks() {
    // Browser-only canvas/WebGL/WebGPU work should use an explicit WebView island.
}

if graphics.has_roadmap_gaps() {
    // Public render targets and custom shaders are not stable public APIs yet.
}
```

The report uses `GraphicsCapabilityStatus::{Full, Partial, WebView, Roadmap}`.
Native styled elements, immediate canvas, vector paths, gradients, SVG, and
Lottie are reported as full; clip shapes, effect layers, and headless rendering
are partial; browser graphics are an explicit WebView fallback; public render
targets and custom shaders remain roadmap. Use `to_text()` for stable
builder/agent summaries.
`GraphicsCanvasHandoffBuilder` validates native canvas surface ids, optional
draw-command counts, SVG/Lottie/effect/headless artifact counts, hosted
graphics island scope, and render-target or shader roadmap reasons before
generated visual apps choose a rendering route. Inspect
`GraphicsCanvasNextAction` to keep charts, timelines, waveforms, drawing tools,
HUDs, SVG/Lottie, and visual evidence on native APIs first; route
browser-owned WebGL/WebGPU/canvas engines to explicit WebView islands; keep
public render targets and custom shaders marked as roadmap. Its summary avoids
logging surface ids, asset names, shader code, generated coordinates, colors,
image bytes, or WebView ids.

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
tracing::info!(summary = sources.to_text(), "capture sources");

if let Some(source) = sources.first() {
    tracing::info!(summary = source.to_text(), "capturable source");
}

let mut pipeline = manager.start_pipeline_checked(
    CaptureConfigSetBuilder::screen_with_microphone()
        .video_frame_rate(30.0)
        .video_resolution(1920, 1080),
    std::sync::Arc::new(|frame| {
        // Handle CaptureFrame::Video or CaptureFrame::Audio.
    }),
)?;

let handoff = cx.capture_handoff_checked(
    CaptureHandoffBuilder::screen_share_with_microphone().auto_start(true),
)?;
tracing::info!(summary = handoff.to_text(), "capture handoff");
match handoff.next_action() {
    CaptureHandoffNextAction::PreflightPermissions => {
        // Request PermissionRequestBuilder::capture_studio() first.
    }
    CaptureHandoffNextAction::ShowSourcePicker => {
        // Present handoff.source_query() through native picker UI.
    }
    CaptureHandoffNextAction::ResolveCaptureConfigs => {
        let _configs = handoff.resolve_configs(&manager)?;
    }
    CaptureHandoffNextAction::StartCapturePipeline => {
        let _pipeline = handoff.start_pipeline_checked(&manager, std::sync::Arc::new(|_| {}))?;
    }
}
```

Use `CaptureConfigSetBuilder::screen_with_microphone()`,
`camera_with_microphone()`, or `screen_with_system_audio()` for common app
flows, then apply `.video_frame_rate(...)` and `.video_resolution(...)` to every
video source in the set. Use
`CaptureConfigBuilder::{screen, window, camera, microphone, system_audio}()`
for a single source, `.device_name_contains(...)` for remembered preferences, or
`.device_id(...)` after presenting `manager.devices(kind)` in a custom source
picker. Inspect `CaptureSourceQueryBuilder::to_text()`,
`CaptureSourceCatalog::to_text()`, `CaptureDeviceInfo::to_text()`,
`CaptureConfigBuilder::to_text()`, `CaptureConfig::to_text()`, and
`CaptureConfigSetBuilder::to_text()` for source kinds, counts, availability,
and option presence without logging device IDs, device/window names, name
filters, or exact resolution values. Use `manager.pipeline_checked(...)` when
the app wants to configure or inspect the pipeline before starting, and
`manager.start_pipeline_checked(...)` for the common "resolve, create sessions,
and start capture" path. The
lower-level `CaptureConfig::new(...)`, `create_session(...)`, and
`create_session_with(...)` APIs remain available when the app needs direct
control.
Use `cx.capture_handoff_checked(CaptureHandoffBuilder::...)` for builder and
AI-agent flows that need one checked capture setup descriptor before touching
devices. It separates permission preflight, source-picker UI, config
resolution, and pipeline startup through `CaptureHandoffNextAction`, and reports
required consent surfaces through `CaptureConsentKind` without logging source
IDs, device/window names, filters, or exact resolution values.
Use `CaptureSourceQueryBuilder` when you need a hosted runtime
`capture source catalog(...)`-style source catalog before constructing
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

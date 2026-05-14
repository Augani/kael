# Manual Verification Matrix

This guide is the human complement to the automated checks in
`.github/workflows/platform-readiness.yml`.

Run the automated sweep first:

```sh
cargo fmt --all --check
bash scripts/ci/verify-kael.sh default
```

Platform-specific follow-ups:

```sh
# Linux only
bash scripts/ci/verify-kael.sh linux-x11
bash scripts/ci/verify-kael.sh linux-wayland

# Windows only
pwsh -File scripts/ci/verify-kael.ps1
```

## Golden-Path Commands

Use these repo surfaces during manual QA:

```sh
cargo run -p kael --example platform_features
cargo run -p kael --example daemon_app
cargo run -p kael --example input
cargo run -p kael --example form_controls
cargo run -p kael --example perf_bench
cargo run -p kael --example capture_demo --features screen-capture
cargo run -p xtask -- dry-run
```

## Evidence Log Template

For every release candidate, create or attach an evidence note with this shape:

```md
Release candidate:
Commit:
Tester:
Platform:
OS version:
Display server, if Linux:
Assistive technology:
Hardware notes:

Automated commands:
- [ ] cargo fmt --all --check
- [ ] cargo check --workspace
- [ ] cargo test --workspace
- [ ] cargo check -p kael --examples
- [ ] cargo test -p kael --test worker_process
- [ ] cargo test -p kael --test extension_process
- [ ] cargo run -p xtask -- dry-run

Manual checks:
- [ ] Accessibility
- [ ] Screen capture
- [ ] Notifications
- [ ] Tray/background lifecycle
- [ ] Dialogs/clipboard
- [ ] Session restore
- [ ] Release artifact install/launch

Findings:
```

## Matrix

| Area | Reference Surface | Platforms | What to Verify |
| --- | --- | --- | --- |
| Session restore | `platform_features` | macOS, Windows, Linux X11, Linux Wayland | Resize/move the window, quit, relaunch, and confirm bounds restore. Disconnect or disable a display if available and confirm restore falls back to the primary display. |
| Crash reporting | Any app bootstrapped with `CrashReporter` | macOS, Windows, Linux X11, Linux Wayland | Trigger a controlled panic in a non-production build and confirm a JSON report lands in the platform data directory. Relaunch and verify pending-report submission or retention behavior matches configuration. |
| Trace export | `platform_features` | macOS, Windows, Linux X11, Linux Wayland | Interact with the demo, quit, and confirm the exported trace opens cleanly in Chrome Trace or Perfetto and contains frame, layout, and input events. |
| File watching | `platform_features` | macOS, Windows, Linux X11, Linux Wayland | Modify files inside the watched session directory and confirm change callbacks arrive on the foreground executor without UI stalls. |
| Notifications with actions | `platform_features` and `daemon_app` | macOS, Windows, Linux X11, Linux Wayland | Confirm the notification is shown, action buttons are rendered when supported, and the callback receives the expected action ID. |
| Tray and background lifecycle | `daemon_app` | macOS, Windows, Linux X11, Linux Wayland | Verify the tray icon appears, menu actions fire, keep-alive behavior is correct, and reopening the main surface does not require a process restart. |
| Dialogs and prompts | `window` example or app under validation | macOS, Windows, Linux X11, Linux Wayland | Confirm prompt buttons, cancellation, and modal focus behavior feel native and do not deadlock input. |
| Clipboard interoperability | App under validation | macOS, Windows, Linux X11, Linux Wayland | Copy plain text and rich payloads to and from native apps and confirm no data loss or format surprises. |
| Tier 1 text input | `input` | macOS, Windows, Linux X11, Linux Wayland | Verify single-line editing, multiline editing, selection, password masking, Enter submit on the primary field, and Cmd/Ctrl+Z / Cmd/Ctrl+Shift+Z while the field is focused. Confirm IME composition placement and caret movement on at least one non-Latin keyboard if available. |
| Tier 1 form controls | `form_controls` | macOS, Windows, Linux X11, Linux Wayland | Verify checkbox, toggle, slider, radio group, select, and date picker mouse and keyboard interactions. Confirm popup focus/escape behavior, slider drag undo coalescing, and focused Cmd/Ctrl+Z / Cmd/Ctrl+Shift+Z for each control. |
| Power, network, and OS integrations | `platform_features` | macOS, Windows, Linux X11, Linux Wayland | Verify power/network callbacks fire, OS info is populated, biometrics return a sensible status, and attention/badge APIs degrade gracefully where unsupported. |
| Performance baseline | `perf_bench` | macOS, Windows, Linux X11, Linux Wayland | Capture FPS and interaction smoothness before release, then compare against the previous tagged baseline. |
| Screen capture | App under validation with `screen-capture` enabled | macOS, Windows, Linux X11, Linux Wayland | On Wayland, verify portal source selection and stable frame delivery. On every platform, confirm capture does not regress UI responsiveness. |
| Accessibility | App under validation | macOS, Windows, Linux X11, Linux Wayland | Validate focus order, names, roles, and announcements with VoiceOver, Narrator/NVDA, and Orca. |
| Worker isolation | `cargo test -p kael --test worker_process` | macOS, Windows, Linux X11, Linux Wayland | Confirm worker bootstrap, health checks, typed request/response, and crash reporting pass on the target OS. |
| Extension isolation | `cargo test -p kael --test extension_process` | macOS, Windows, Linux X11, Linux Wayland | Confirm external-process extension activation, command RPC, contribution RPC, deactivation, and crash reporting pass on the target OS. |
| Release dry run | `cargo run -p xtask -- dry-run` | macOS, Windows, Linux X11, Linux Wayland | Confirm artifact planning, signing/notarization command planning where applicable, update metadata generation, and publish planning succeed. |

## Platform Notes

### macOS

- Validate notification permission prompts, dock badge behavior, and relaunch after a signed/notarized build.
- Run at least one pass with VoiceOver enabled.

### Windows

- Validate taskbar progress, Windows Hello availability, and notification actions from Action Center.
- Run at least one pass with Narrator or NVDA active.

### Linux

- Run the matrix twice when possible: once on X11 and once on Wayland.
- Confirm portal-backed behavior for dialogs, notifications, and screen capture on Wayland.
- Run at least one pass with Orca enabled.

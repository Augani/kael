# Platform Distribution Guide

This guide turns Kael's production modules into a repeatable release story for
macOS, Windows, and Linux.

## Shared Release Inputs

Before building installers or update feeds, lock down these shared inputs:

- A stable application identifier used by `SessionStore`, `CrashReporter`, auto-launch, URL schemes, and updater feeds.
- A semantic versioning policy that maps cleanly to `AutoUpdater`.
- A signing identity for every shipped binary.
- A release-notes source for update prompts and support triage.
- An explicit crash-reporting and telemetry opt-in policy.
- A canonical icon set: app icon, tray icon, installer artwork, and notification assets.

## macOS

Recommended shipping shape:

- Build a `.app` bundle.
- Sign every embedded binary with a Developer ID Application certificate.
- Notarize the final bundle or disk image.
- Staple the notarization ticket before publishing.
- Ship either a signed `.dmg` or a notarized `.zip` alongside a versioned update feed.

macOS release checklist:

- Confirm the bundle identifier matches launch-at-login, notification, and URL-scheme configuration.
- Verify crash reports and session data land under `~/Library/Application Support/<app-id>/`.
- Validate Touch ID, notification actions, dock badge, and relaunch/update behavior on a clean machine.
- Test both direct launch and first-run from the signed installer artifact.

## Windows

Recommended shipping shape:

- Build a signed installer package such as MSI or a signed bootstrapper such as NSIS.
- Timestamp signatures so installs remain valid after certificate rotation.
- Publish a versioned update artifact that `AutoUpdater` can download and hand off to the installer backend.
- Optionally publish a `winget` manifest for enterprise-friendly distribution.

Windows release checklist:

- Verify `%APPDATA%/<app-id>/` or `%LOCALAPPDATA%/<app-id>/` paths are writable and stable across upgrades.
- Validate taskbar progress, toast notifications with actions, launch-at-login, and Windows Hello behavior.
- Test install, upgrade, rollback, and uninstall on a clean VM.
- Confirm SmartScreen reputation, code signing, and restart-after-update flows behave as expected.

## Linux

Recommended shipping shape:

- Pick one primary distribution channel, preferably AppImage for direct downloads or Flatpak for desktop integration.
- Optionally produce `.deb` and `.rpm` packages for distro-specific deployment.
- Install a `.desktop` file, icons, and any required autostart entries.
- Publish checksums and detached signatures for downloaded artifacts.

Linux release checklist:

- Validate both X11 and Wayland where supported.
- Confirm notification, tray, clipboard, and dialog flows use the expected desktop integration path.
- Verify session and crash-report data land under `$XDG_DATA_HOME/<app-id>/` or `~/.local/share/<app-id>/`.
- If shipping screen capture, test portal-based flows on Wayland and compositor-specific behavior on X11.

## Release Artifacts

Treat these as the minimum publish set for a production release:

- Signed platform artifact for each supported OS.
- Update feed entry or manifest for that version.
- Checksums and release notes.
- Trace/perf comparison against the previous release baseline.
- Completed pass through `docs/guides/manual-verification-matrix.md`.

## Operational Recommendation

Keep packaging logic outside the core framework crate, but keep the release
contract stable:

- `AutoUpdater` should consume a predictable feed shape.
- `CrashReporter` and `SessionStore` should keep stable storage roots.
- Example apps and templates should use the same bootstrap pattern as real apps.

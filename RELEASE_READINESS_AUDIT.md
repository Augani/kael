# Kael Release Readiness Audit

This audit records the code and performance issues found before publishing Kael. Items in the first section were fixed in this pass. Items in the release blocker section should be implemented before a public framework release.

## Implemented in This Pass

- `kael`: styled shadows now render through the existing shadow atlas cache as premultiplied polychrome sprites instead of submitting live shadow primitives every frame. The test atlas now preserves texture kind, and a style test asserts the cached shadow path.
- `kael-macros`: stale generated test imports were corrected so macro integration tests compile under the current crate name.
- `media`: the build script now gates on `CARGO_CFG_TARGET_OS`, so cross-target builds from macOS do not incorrectly generate macOS bindings for non-macOS targets.
- `media`: CoreMedia wrappers now surface failure and null states instead of returning unchecked buffers and slices.
- `kael_icons`: unsupported platforms now compile through an explicit fallback bridge instead of failing module selection.
- `kael_cache`: disk cache namespaces are validated before path construction, preventing namespace traversal outside the cache root.
- `kael_i18n`: number formatting now rounds scaled absolute values before splitting integer and fractional parts, fixing carry cases like `1.999 -> 2.00`.
- `kael_net`: auth token and API request debug output now redacts secrets and request bodies.
- `kael_share`: image materialization now writes into unique private temporary directories and uses non-overwriting file creation.
- `kael_document`: version blobs are verified against their stored SHA-256 digest before reads return data.
- `kael_engines`: search now honors regex queries, case sensitivity, and whole-word matching; media timeline queries reject invalid or overflowing clip ranges.
- `kael_release`: update manifests now require HTTPS artifact URLs.
- `xtask`: dist validation now rejects placeholder app identity, icon paths, signing identities, updater feed URLs, and updater public keys.
- `xtask`: bundling now fails on missing required assets, errors on missing binaries outside dry-run mode, and fixes Linux AppRun path handling.
- `xtask`: update feed generation now propagates checksum errors and recognizes `.AppDir` Linux artifacts.
- `scripts/publish-all.sh`: publish ordering now includes the publishable foundation and service crates and no longer skips crates just because a crate name already exists on crates.io.
- `templates`: template crates are marked `publish = false`.

## Release Blockers

### `kael`

- Add a bounded eviction policy for shadow atlas entries. The current cache path removes per-frame raster work, but highly varied shadow sizes, colors, radii, and opacities can still grow atlas memory without a release-oriented budget.
- Consider nine-slice or separable blur strategies for large rounded shadows. Full raster tiles are simple and correct, but large soft shadows will remain expensive to allocate and upload.
- Revisit publish surface in `crates/kael/Cargo.toml`. Test fixture binaries and examples should be intentional before pushing the framework crate.
- Preserve compatibility aliases for any inherited GPUI or Zed environment variables if external users are likely to have existing workflows built around them.

### `kael_audio`

- Fix overlapping load races. A newer load request should cancel or supersede older work so stale completion cannot overwrite current playback state.
- Implement or remove the public `set_rate` API. A public no-op is a bad framework contract because callers cannot detect that rate changes are unsupported.
- Put explicit memory and duration limits around decoded audio buffers, and prefer streaming for large media.

### `kael_cache`

- Add a capacity policy for disk entries by namespace and globally. The traversal issue is fixed, but release users still need predictable storage limits and eviction behavior.
- Add stress tests around concurrent writers if this crate is intended to be shared across app subsystems.

### `kael_collections`, `kael_sum_tree`, `kael_refineable`, `kael_derive_refineable`, `kael_semantic_version`, `kael_util`, `kael_util_macros`, `kael_http_client`, `kael_perf`

- Keep these foundation crates minimal and API-stable before publishing. The audit did not find immediate correctness fixes, but each public item should be checked for accidental exposure, missing documentation on public APIs, and semver commitment.
- Remove stale per-crate lockfiles if any are present in member crates before publishing from the workspace.

### `kael_diagnostics`

- Decide whether diagnostics are internal framework plumbing or a stable public API. If public, add contract tests for diagnostic identity, severity ordering, serialization, and localization behavior.

### `kael_document`

- Add atomic write and recovery tests for version metadata and blob writes. Digest verification now catches corruption on read, but crash recovery needs explicit guarantees.
- Audit retention behavior so version stores cannot grow without user or application policy.

### `kael_engines`

- Add cancellation and budget controls around search/indexing workloads. Regex support now works, but pathological regexes or large indexes need bounded execution.
- Expand timeline tests around zero-duration clips, negative edits if introduced later, and high frame-number arithmetic.

### `kael_i18n`

- Validate locale data fallback behavior for unsupported locales and malformed tags.
- Add snapshot coverage for date, number, grouping, sign, and decimal behavior across representative locales before publishing localization APIs.

### `kael_icons`

- Define the behavior of the unsupported-platform fallback. It currently compiles cleanly; release docs and tests should make clear whether native icon lookup is unavailable or emulated.

### `kael_media` and `media`

- Avoid full PCM decode for large audio/video assets where streaming is possible. Full decode paths can spike memory in real editors and viewers.
- Add hard caps and error reporting for media dimensions, frame counts, sample rates, channel counts, and duration.
- Extend CoreMedia wrapper tests to cover null buffers, failed status values, and target-gated build behavior.

### `kael_net`

- Add timeout, retry, and cancellation policy tests around every transport. Secret redaction is fixed, but release network behavior needs predictable failure semantics.
- Ensure response body size limits exist wherever untrusted remote data is accepted.

### `kael_notifications`

- Replace one sleeping OS thread per delayed notification with a shared scheduler or async timer. The current shape can exhaust resources under repeated delayed notifications.
- Preserve cancellation payloads and delivery state so callers can inspect what was canceled or skipped.

### `kael_pdf`

- Bound rendered page caches by memory and page count. PDF rendering can allocate very large images, and release apps need predictable cache pressure.
- Move synchronous text extraction and expensive document operations off latency-sensitive UI paths.
- Add tests for encrypted, malformed, very large, and rotated PDFs.

### `kael_release`

- Add signed update manifests or detached artifact signatures with pinned public keys. HTTPS-only URLs are necessary but not sufficient for a secure updater.
- Validate update channel transitions, downgrade prevention, and minimum-version behavior with tests.

### `kael_share`

- Decide target support for non-Linux, non-macOS, non-Windows platforms. If FreeBSD or other Unix targets are in scope, add explicit fallback behavior and tests.
- Add cleanup policy for materialized temporary payloads after platform handoff.

### `kael_storage`

- Fix stale JSON snapshot races with atomic compare/write, file locks, or a single-writer model.
- Fix SQLite observer registration race windows so observers cannot miss changes between initial read and subscription.
- Add crash recovery and concurrent writer tests for both JSON and SQLite backends.

### `kael-macros`

- Add compile-fail tests for invalid derive inputs and missing required attributes.
- Keep generated code paths free of stale crate names and ensure public diagnostics point to user code spans.

### Templates

- Keep template crates unpublished, pinned to workspace paths, and compile-checked as part of release CI.
- Add a lightweight template smoke test that renders or starts each template enough to catch broken imports and feature drift.

### `xtask`, Packaging, and Release Automation

- Make bundle commands produce real distributable artifacts (`.dmg`, `.zip`, `.msi`, `.AppImage`, `.tar.gz`) before update feed or publish steps consume them.
- Make publish commands fail if required tools such as `gh` are missing or if artifact upload fails.
- Model notarization credentials explicitly and fail early when configuration is incomplete.
- Replace placeholder `kael.dist.toml` values with real app identity, signing team, certificate, updater feed URL, public key, and existing icon assets.
- Add a dry-run release CI job that runs validation, bundle assembly, feed generation, and publish planning without uploading.

## Validation Completed

- `cargo test -p kael_cache -p kael_i18n -p kael_net -p kael_share -p kael_document -p kael_engines -p kael_release --lib`
- `cargo test -p kael style::tests --lib`
- `cargo test -p kael-macros`
- `cargo check -p kael_media_sys --lib`
- `cargo check -p kael_icons --lib`
- `cargo check --workspace --all-targets`
- `cargo check -p kael --example crispness_showcase`
- `cargo test --workspace --lib --quiet`

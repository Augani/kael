# Release plan: Kael 0.2.0 — "the general-purpose release"

This is the step-by-step plan to publish the next version after the
re-centering work (VISION.md, the `kael_engines` split, the roadmap reframe)
lands on `main`. Execute it from a clean checkout of `main` after PR #6 merges.

## Why 0.2.0 (not 0.1.3)

The `kael_engines` split is a **breaking change** for that crate (15 modules
moved to the new `kael_media_engines`). Under the workspace's stated 0.x
policy (see CHANGELOG header), breaking changes ride minor bumps — so the
whole workspace moves to **0.2.0**. It also marks the direction change
visibly: 0.2.0 is the first release where the crate layout itself enforces
the general-purpose layering.

## What ships

- `VISION.md`, retitled roadmap, repositioned README.
- New crate **`kael_media_engines`** (first publish).
- `kael_engines` slimmed to domain-neutral modules (breaking).
- Design 0001 (render targets + custom shaders) — docs only, no API yet.
- Everything already in `[Unreleased]` in CHANGELOG.md (including `kael_ui`,
  which has not yet been published — 0.2.0 is its first crates.io release).

## Packaging artifacts

`cargo run -p xtask -- bundle --output dist --binary <release-binary>`
produces real, installable artifacts per host. The packaging logic lives in
`xtask/src/bundle.rs`; signing/notarization metadata is read from
`kael.dist.toml`.

| Host    | Artifacts                                   | Requires installed                          |
| ------- | ------------------------------------------- | ------------------------------------------- |
| macOS   | `.app` bundle, `.dmg` (codesigned + stapled when configured) | `hdiutil`, `codesign`, `xcrun notarytool`/`stapler` (Xcode CLT); an Apple Developer cert for signing |
| Windows | staged dir + `.wxs`, **`.msi`** (signed when configured) | **WiX v4** CLI (`wix`); `signtool` (Windows SDK) for signing |
| Linux   | `.AppDir`, **`.deb`**, **`.AppImage`**       | nothing for the `.deb`; `appimagetool` for the AppImage |

What is real vs. tool-gated:

- **macOS `.dmg`** — built unconditionally on a macOS host. Code-signing
  needs `signing.macos_certificate`; notarization needs `KAEL_NOTARY_PROFILE`
  to name a stored `notarytool` keychain profile. Without those it produces an
  unsigned image.
- **Windows `.msi`** — after emitting the `.wxs`, xtask locates the WiX v4
  CLI (the `WIX` env var, or `wix`/`wix.exe` on `PATH`) and runs
  `wix build <wxs> -o <name>.msi`. There is **no candle/light (WiX v3)
  fallback** by design. If WiX is not installed the step is skipped with a
  warning and only the `.wxs` source remains, so MSI builds must run on a
  **Windows runner with WiX v4**. The `.msi` is signed with `signtool` when
  `signing.windows_certificate` is set.
- **Linux `.deb`** — assembled **directly in Rust** (an `ar` archive of
  `debian-binary` + `control.tar.gz` + `data.tar.gz`), so it builds on **any
  host including macOS and CI** with no `dpkg-deb` or system tooling. The
  `control` file is generated from `kael.dist.toml` (`copyright` →
  `Maintainer`, `file_description` → `Description`, first `linux_categories`
  entry → `Section`).
- **Linux `.AppImage`** — built from the `.AppDir` via `appimagetool` when it
  is on `PATH`; skipped with a warning otherwise. Full AppImage assembly
  therefore needs a **Linux runner with `appimagetool`** (FUSE or
  `APPIMAGE_EXTRACT_AND_RUN=1`). Optional update-information can be embedded by
  setting `KAEL_APPIMAGE_UPDATE_INFO` (passed as `appimagetool -u`).

The `--dry-run` flag prints the planned outputs (including the would-be `.msi`,
`.deb`, and `.AppImage` paths) without invoking any external tool.

## Pre-flight checklist

1. **Branch state:** PR #6 merged to `main`; CI fully green on `main`
   (Verify macOS/Linux/Windows, both Linux backend checks, benchmarks,
   Template Compile Check, Dry-Run Release Validation, Rustfmt).
2. **Version bump (one commit):**
   - `Cargo.toml` (workspace): `version = "0.1.2"` → `"0.2.0"`.
   - Update internal dependency requirements that currently say
     `version = "0.1.x"` for workspace crates — they will not match 0.2.0.
     They live in ~16 manifests (40 in `crates/kael/Cargo.toml` alone; also
     `kael_ui`, `kael_audio`, `kael_document`, `http_client`, `util`,
     `util_macros`, `refineable`, `perf`, `media`, `kael_storage`,
     `kael_share`, the three templates, and `kael_media_engines`'s
     `kael_render_graph` req). Mechanical sweep:
     ```bash
     # internal deps are recognizable by their path = "../..." sibling keys;
     # verify with git diff that only kael-internal deps changed
     grep -rl 'version = "0.1' crates/*/Cargo.toml templates/*/Cargo.toml \
       | xargs sed -i 's/^version = "0\.1\.[0-9]*"/version = "0.2.0"/'
     ```
     then hand-review the diff: external crates pinned at 0.1.x (if any)
     must be reverted.
   - `cargo update --workspace` to refresh `Cargo.lock`.
3. **CHANGELOG:** rename `[Unreleased]` → `[0.2.0] - <date>`, add a fresh
   empty `[Unreleased]` above it.
4. **Local verification (all must pass):**
   ```bash
   cargo check --workspace
   bash scripts/ci/verify-kael.sh            # default mode
   bash scripts/ci/verify-kael.sh linux-x11
   bash scripts/ci/verify-kael.sh linux-wayland
   cargo +stable fmt --all -- --check
   ```
   (macOS/Windows verify modes run in CI on the bump PR.)
5. **Publish dry-run:**
   ```bash
   ./scripts/publish-all.sh --dry-run
   ```
   Known watch-items for the dry run:
   - **Four crates were missing from the publish list and are now in it** in
     dependency order: `kael_render_graph` (needed by `kael_media_engines`),
     `kael_gpu_budget` (needed by `kael`), `kael_secrets` (needed by
     `kael_net`), and the new `kael_media_engines`.
   - **First-time publishes:** `kael_render_graph`, `kael_gpu_budget`,
     `kael_secrets`, `kael_media_engines`, and `kael_ui` — confirm the crate
     names are available (or already owned) on crates.io before release day.
   - Any path-dependency-without-version errors the dry run surfaces must be
     fixed by adding `version` keys (the audit found only
     `kael-macros`' dev-dependency on `kael`, which cargo strips and is fine).
6. Land the bump as its own PR; wait for green CI; merge.

## Publish day

1. **Tag first** (so a botched publish can be re-run from a known commit):
   ```bash
   git tag v0.2.0 && git push origin v0.2.0
   ```
2. **Publish to crates.io in dependency order** (the script handles ordering
   and 429 rate-limit retries):
   ```bash
   ./scripts/publish-all.sh
   ```
   Order highlights: tier-0 leaves (`kael_render_graph` before
   `kael_engines`/`kael_media_engines`) → … → `kael-macros` → `kael` →
   `kael_ui` last.
3. **GitHub release:** create the `v0.2.0` release from the tag; paste the
   `[0.2.0]` CHANGELOG section as the body. Lead with the re-centering
   paragraph and the `kael_engines` migration note.
4. **Docs:** confirm the `docs.yml` workflow deployed the book to
   `augani.github.io/kael` from the tagged commit.

## Post-publish

1. **Smoke-test the published crates** from a scratch project outside the
   workspace:
   ```toml
   [dependencies]
   kael = "0.2"
   kael_ui = "0.2"
   kael_engines = "0.2"
   kael_media_engines = "0.2"
   ```
   Build the README counter example; confirm no path-dependency leakage.
2. **crates.io metadata check:** new descriptions render correctly
   (`kael_engines` now says "General-purpose workload engines…",
   `kael_media_engines` says "Optional media/NLE engines…").
3. **Announce.** This is the release that answers the community thread about
   GPUI forks; the announcement should say, in this order:
   - Kael is the general-purpose, community-driven GPUI fork — VISION.md.
   - Radial **and conic** gradients are already shipped in the core.
   - Custom shaders are committed as public API — link design 0001 and
     invite feedback, explicitly welcoming other GPUI forks to align on the
     contract.
   - The adabraka-gpui → Kael naming history (same project, renamed; not a
     break with Zed).
4. **Open the slice-1 tracking issue** for design 0001 (Metal reference:
   `RenderTarget` + `register_fragment_shader` + `run_pass` +
   `render_target_image`, golden-image tests, one non-media example).

## Rollback notes

- crates.io publishes are immutable: if a bad crate ships, `cargo yank` the
  affected version and publish a `.1` patch — do not delete the tag.
- The publish script is resumable: it treats "already exists" as success, so
  re-running after a partial failure continues from where it stopped.

# Release Process

Kael releases are made from one reviewed commit on `main`. Crate publication,
native installers, updater metadata, and the Git tag must all identify that
same commit and version. Never publish from a dirty worktree.

## Documentation-only changes

Documentation is not a framework release. Changes limited to `docs/**`,
Markdown files such as `README.md` or crate READMEs, `llms.txt`, or the docs
workflow do not require a workspace version bump, changelog release section,
platform rebuild, crate publication, or Git tag.

The Documentation workflow validates those changes on pull requests. A push to
`main` builds and deploys the guide to GitHub Pages. Platform Readiness runs one
small classifier for required-check compatibility, then skips the macOS, Linux,
Windows, and browser build jobs. The stable `Platform readiness` check passes
after classification, so documentation pull requests remain mergeable.

Keep unreleased documentation improvements on `main`. They become part of the
next crate release naturally when a later code change requires a new version.
Do not dispatch `Publish crates` for a documentation-only commit.

Changes to source, manifests, lockfiles, build scripts, fixtures, or platform
workflows are not documentation-only and still require the normal code gates.

## Prepare the release candidate

Update the workspace version, `kael.dist.toml`, scaffold dependency version,
and the dated changelog section together. Then run the local gates:

```sh
cargo fmt --all --check
bash scripts/ci/verify-cross-targets.sh
bash scripts/ci/audit-dependencies.sh
bash scripts/ci/verify-kael.sh default
bash scripts/publish-all.sh --preflight
bash scripts/ci/verify-docs.sh
```

On macOS, these commands require full Xcode rather than Command Line Tools for
the package archive's default Metal shader build. Select it either system-wide
with `xcode-select` or per shell with `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`.

The documentation gate builds every page, copies `llms.txt`, rejects orphaned
pages, broken local links, duplicate HTML IDs, stale LLM routes, oversized font
assets, and guides that stop including the compiled quick-start source. The
quick-start example is compiled on native and Wasm targets in Platform
Readiness.

The publication preflight selects all 34 crates in dependency order, checks
their license and package contents, builds the actual `.crate` archives as one
unpublished workspace set, compiles every extracted archive, and enforces the
crates.io 10 MiB compressed archive limit. It does not upload anything.

`verify-cross-targets.sh` uses Zig to link the portable Linux test graphs from
the development machine and separately checks the public Wasm graphs. It is a
fast compiler/linker preflight, not runtime evidence: it cannot execute Metal,
Direct3D, WebView2, WKWebView, WebKitGTK, or browser engines.

Commit and push the complete candidate before treating runtime evidence as
release evidence. That exact push starts the compact Platform Readiness gate:
Linux quality/package checks, Linux renderer/WebView runtime, the browser
matrix, macOS native/WKWebView/Metal/browser-hardware checks, and two Windows
native/WebView/MSI compatibility runners. The Metal browser job must report a
non-software WebGL adapter; merely omitting the forced SwiftShader query is not
sufficient.

After Platform Readiness is green, run `Publish crates` with `publish=false`.
This is a cheap attestation: it requires a successful Platform Readiness run
whose `head_sha` exactly matches the checked-out candidate and does not rerun
the same platform matrix.

```sh
gh workflow run release.yml --ref main -f publish=false
gh run list --workflow release.yml --branch main --limit 1
gh run watch <run-id> --exit-status
```

Do not substitute a Zig cross-compile for the hosted platform runtime jobs.

## Publish crates and tag the commit

The `crates-io` GitHub environment must contain `CARGO_REGISTRY_TOKEN`. After
the non-publishing workflow run is green, dispatch the same workflow with
`publish=true`:

```sh
gh workflow run release.yml --ref main -f publish=true
gh run list --workflow release.yml --branch main --limit 1
gh run watch <run-id> --exit-status
```

The workflow only publishes from `refs/heads/main`, requires the exact
confirmation generated from the workspace version, uploads crates in
dependency order, waits for each registry version to become visible, and can
resume a partial upload without overwriting an immutable crates.io version.

After all crates are visible, create the annotated version tag on the exact
published commit and push only that tag:

```sh
git tag -a v0.4.1 <published-commit-sha> -m "Kael 0.4.1"
git push origin v0.4.1
```

## macOS distribution order

`kael.dist.toml` may contain the public Developer ID identity and team ID. Keep
notary credentials out of the repository. Store them once with Apple's tool and
provide only the profile name through `KAEL_NOTARY_PROFILE`:

```sh
xcrun notarytool store-credentials <profile> \
  --apple-id <apple-id> --team-id <team-id>
```

The production order is deliberate: sign the hardened-runtime app, create the
DMG, timestamp-sign the DMG, notarize it, and staple the accepted ticket.

```sh
cargo build --release -p kael-cli --bin kael
cargo run -p xtask -- bundle kael.dist.toml \
  --output dist --binary target/release/kael
cargo run -p xtask -- sign kael.dist.toml --artifact dist/kael.dmg
KAEL_NOTARY_PROFILE=<profile> cargo run -p xtask -- \
  notarize kael.dist.toml --artifact dist/kael.dmg

codesign --verify --deep --strict --verbose=4 dist/Kael.app
codesign --verify --strict --verbose=4 dist/kael.dmg
xcrun stapler validate dist/kael.dmg
```

Bundling signs the app before it enters the disk image. The standalone signer
uses a trusted timestamp and omits app-only hardened-runtime flags for a DMG.
Production notarization fails closed when `KAEL_NOTARY_PROFILE` is missing.

## Windows and Linux installers

On Windows, install the pinned WiX v4 toolchain used by CI. Supply the PFX path
and password through `KAEL_WINDOWS_CERTIFICATE` and
`KAEL_WINDOWS_CERTIFICATE_PASSWORD`; do not commit the password. Bundling signs
the MSI with SHA-256 and a trusted timestamp. Run
`scripts/ci/verify-windows-msi.ps1` against the result to prove the MSI database,
payload hash, extracted executable, and Authenticode status.

Linux bundling produces the `.deb` and AppImage payloads. The standalone Linux
signer creates an armored detached GPG signature using the maintainer's selected
key. Verify installer behavior on the same supported distribution baseline used
by platform-readiness CI.

## Signed updater metadata

Generate the Ed25519 updater key pair once:

```sh
cargo run -p xtask -- generate-update-key
```

Store the private value as `KAEL_UPDATE_SIGNING_KEY`; never commit or print it
in CI logs. Put only the matching public key in `updater.public_key` in
`kael.dist.toml`. A production `xtask publish` requires real regular-file
artifacts, non-placeholder hashes and sizes, a selected update artifact that is
also uploaded, and a private key that matches the configured public key. Dry-run
feeds are deliberately unsigned and are not release artifacts.

## Browser artifacts

The browser target is published through the same crates and source tree, not as
a forked UI implementation. Before tagging, retain the generated-project parity
report, optimized Wasm size report, suite-scale report, and Chromium/Firefox/
WebKit matrix report from the exact SHA. Also retain the macOS hardware report,
its renderer/vendor identity, the compositor screenshots, and the raw retained
framebuffer PNGs. The latter keep visual evidence deterministic when an
automation compositor omits a restored WebGL plane. Browser security boundaries
such as permission prompts and cross-origin iframe restrictions remain
capability differences; they must be handled through Kael's typed capability
reports rather than platform-specific view code.

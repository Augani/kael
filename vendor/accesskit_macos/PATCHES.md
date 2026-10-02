# Kael-maintained macOS adapter

This directory contains the published `accesskit_macos` **0.26.3** source from
AccessKit commit [`c88605b96d04431f9c3c792464a0f2f253480e94`](https://github.com/AccessKit/accesskit/tree/c88605b96d04431f9c3c792464a0f2f253480e94/platforms/macos).
Kael ships it as the separate package **`kael_accesskit_macos` 0.4.1**, aliased
as `accesskit_macos` by the macOS target dependency. Upstream source headers and
dependency versions remain intact; AppKit dependencies are target gated so the
workspace is portable. The package metadata records Kael as the fork repository.
`UPSTREAM-SHA256.json` records the exact registry source hashes;
`outline-support.patch` is the complete source/manifest/test diff against that
version. This patch does not claim to be an upstream AccessKit release.

Upstream source uses MIT or Apache-2.0, at the user's option. Both licenses are
included unchanged. The Chromium-derived mapping also retains its original BSD
notice and `LICENSE.chromium`. Kael's additions carry the same licenses as the
upstream files. `README.md` and `CHANGELOG.md` are unchanged upstream copies.

## Why this patch is needed

The upstream macOS `accessibilityRows` implementation uses the generic
`items()` iterator, which stops at each tree item. A hierarchy of 25 projects
and 100,000 documents consequently exposes only the 25 project rows through
the native outline's row collection. It also omits the outline-row disclosure
properties and native expand/collapse actions.

Apple specifies [all outline rows](https://developer.apple.com/documentation/appkit/nsaccessibilityprotocol/accessibilityrows()),
[disclosed rows](https://developer.apple.com/documentation/appkit/nsaccessibilityprotocol/accessibilitydisclosedrows()),
[the disclosing parent row](https://developer.apple.com/documentation/appkit/nsaccessibilityprotocol/accessibilitydisclosedbyrow()),
[disclosure level](https://developer.apple.com/documentation/appkit/nsaccessibilityprotocol/accessibilitydisclosurelevel()),
and the [settable disclosure state](https://developer.apple.com/documentation/appkit/nsaccessibilityprotocol/setaccessibilitydisclosed(_:)).

The patch adds iterative preorder enumeration of disclosed tree items, retains
ordinary table/list behavior, and bridges those outline-row properties. Expansion
setters and `AXExpand`/`AXCollapse` actions dispatch only supported actions and
ignore unchanged states, disabled rows, or retained objects in collapsed/hidden
subtrees. All native mutation entrypoints resolve their current visible, enabled
node before dispatch, so stale handles cannot execute actions after collapse. Offscreen rows need no invented
geometry. The core immutable logical snapshot remains shared; native platform
objects are created only when the assistive client requests them.

Regression tests cover hierarchical preorder, transparent containers, hidden and
collapsed subtrees, disclosure depth, supported/disabled action dispatch, and
100,001 rows without visual bounds. Run on macOS with:

```sh
cargo test --locked -p kael_accesskit_macos --lib
cargo test --locked -p kael_accesskit_macos --test native_outline --test native_text_selection
```

The harness-free native test calls the actual Objective-C NSAccessibility
getters and setters on the process's main thread. Its 25 projects and 100,000
documents verify full rows, disclosure children/parent/level, offscreen
focus/press/scroll, collapsed-action rejection, re-expansion identity, and
released-context rejection. It creates no visible window. Non-macOS runs print
an explicit skip; only execution on macOS supplies native protocol evidence.
The production virtual-tree example separately exercises Window action routing.
The `native_text_selection` harness additionally verifies actual settable
selected-text ranges, complete multiline values, UTF-16 counts/ranges, Unicode
setter requests, reversed selection, and released-context rejection. Multiline
values are computed from retained runs on native request instead of storing a
full copy in each caret-only root update. Apple documents the
[selection setter](https://developer.apple.com/documentation/appkit/nsaccessibilityprotocol/setaccessibilityselectedtextrange(_:)).

## Distribution and release order

Kael's target-specific dependency has `path`, `version`, and `package` fields.
Git/path consumers receive this adapter automatically without a workspace patch.
Cargo rewrites the dependency to `kael_accesskit_macos = "0.4.1"` when packaging;
registry consumers receive the same adapter after that crate is published first.
`scripts/publish-all.sh` includes it before `kael`. No crate publication is part
of this change. Restore the upstream package only when an upstream release
supplies equivalent tested behavior, and preserve semantic identity during that
transition.

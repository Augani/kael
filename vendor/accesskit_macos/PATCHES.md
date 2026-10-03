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

## Native text follow-on

The fork additionally implements Apple's [visible-character getter](https://developer.apple.com/documentation/appkit/nsaccessibilityprotocol/accessibilityvisiblecharacterrange()),
[visible-range setter](https://developer.apple.com/documentation/appkit/nsaccessibilityprotocol/setaccessibilityvisiblecharacterrange(_:))
and [selected-text setter](https://developer.apple.com/documentation/appkit/nsaccessibilityprotocol/setaccessibilityselectedtext(_:)).
Mounted shaped run geometry supplies UTF-16 visible ranges, screen rectangles
and range-at-position results, including zero-advance line endings. Logical
unmounted runs retain their text without fabricated geometry. Detached views
return empty screen geometry; overflowing or non-atomic UTF-16 setter ranges
are rejected. String and attributed-string reads preserve exact valid UTF-16
substrings inside combining units and CRLF; they do not reuse the atomic setter
converter or silently round the requested range. Native reveal targets the original run with a TopEdge hint,
without modifying selection. AccessKit reveal is run-level, so the host resolves
its owning immutable document and run byte span; this does not claim arbitrary
character-level reveal precision.

`Adapter::new_with_text_handler` and `SubclassingAdapter::new_with_text_handler`
are opt-in additions. Existing constructors retain their behavior and do not
advertise the selected-text setter when atomic editing is unsupported. The
handler receives the directed original run identities and one Replace/Copy/
Cut/Paste operation. The native clipboard actions use the advertised reserved
custom-action capabilities. Read-only text permits Copy and selection, while
partial edits, Cut and Paste are rejected; disabled/hidden/retired objects do
not dispatch. Kael queues one bounded normalized request, rechecks the immutable
document at foreground execution, and releases queued work when the window closes.

The native text harness exercises the actual Objective-C setter/action methods,
real hidden-window screen conversion, Unicode glyph bounds/hit testing, visible
ranges, immutable reveal/edit/clipboard request routing, read-only/disabled/
focused-hidden guards, overflow, detached views and released contexts. It proves
adapter protocol behavior; the shared `editor_accessibility` fixture and real
assistive client are separately required to prove Editor presentation, shaped
geometry, model mutation, clipboard and undo on a live platform window.

## Distribution and release order

Kael's target-specific dependency has `path`, `version`, and `package` fields.
Git/path consumers receive this adapter automatically without a workspace patch.
Cargo rewrites the dependency to `kael_accesskit_macos = "0.4.1"` when packaging;
registry consumers receive the same adapter after that crate is published first.
`scripts/publish-all.sh` includes it before `kael`. No crate publication is part
of this change. Restore the upstream package only when an upstream release
supplies equivalent tested behavior, and preserve semantic identity during that
transition.

Cargo reserves `Cargo.toml.orig` when building an archive. Its exact upstream
contents are shipped as `UPSTREAM-Cargo.toml`; any original reserved-name copy
is excluded. Cargo generates its own normalized-manifest companion for the
named fork.

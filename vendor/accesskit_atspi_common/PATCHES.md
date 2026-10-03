# Kael AT-SPI translation fork

This is published `accesskit_atspi_common` 0.19.1 from AccessKit commit
[`c88605b96d04431f9c3c792464a0f2f253480e94`](https://github.com/AccessKit/accesskit/tree/c88605b96d04431f9c3c792464a0f2f253480e94/platforms/atspi-common),
distributed as **kael_accesskit_atspi_common 0.4.1**. It is a named maintained
fork, not an upstream AccessKit release. `UPSTREAM-SHA256.json` records original
registry file hashes. Original source notices, README, CHANGELOG and original
manifest remain intact. MIT, Apache-2.0 and the Chromium BSD license are included
unchanged; additions use the same licensing. `native-text-support.patch` records
the complete source/manifest changes against the published source.

## Changes

- Refresh the existing native cache object when its advertised interfaces change,
  including text runs arriving after provider activation or disappearing later.
  Addition/removal regressions require the same native identity and truthful
  Text/EditableText capability sets.

- Capability-derived Click, Expand and Collapse actions, with deterministic
  ordering, supported current-state dispatch and current enabled/visible guards.
- Expandable, Expanded and Collapsed states, using the existing native state
  event diff. Disabled controls no longer incorrectly report Enabled/Sensitive.
  Logical rows without geometry remain Visible but do not claim Showing.
- EditableText support with whole-value replacement and atomic insert/delete/
  copy/cut/paste requests. The optional `TextEditHandler` accepts one operation
  carrying immutable original run positions; it never changes selection first.
  Hosts must revalidate the document identity before deferred execution, marshal
  clipboard work to their UI thread and apply edits as one undo transaction.
- Byte-bounded replacement values (16 MiB). Insert positions and deletion ranges
  use Unicode scalar offsets; InsertText length uses UTF-8 bytes, and a partial
  scalar truncation is rejected. Exact substring reads preserve scalars inside
  shaped graphemes and CRLF, while non-atomic selection/edit requests are rejected
  instead of silently rounded. Read-only controls allow selection and copy.
- Unsupported opt-in operations produce an explicit UnsupportedOperation error.
  Disabled/hidden/stale retained objects reject mutation without dispatch.

GNOME specifies the [disclosure states](https://gnome.pages.gitlab.gnome.org/at-spi2-core/libatspi/enum.StateType.html),
[EditableText operations](https://gnome.pages.gitlab.gnome.org/at-spi2-core/libatspi/iface.EditableText.html)
and [InsertText byte-length semantics](https://gnome.pages.gitlab.gnome.org/at-spi2-core/libatspi/method.EditableText.insert_text.html).
The independent D-Bus XML/C source and void/bool transport distinctions are
recorded in the Unix fork patch record. Selection and document text continue to use retained AccessKit runs without a
duplicated whole-document root Value.

## Validation and distribution

`cargo test --locked -p kael_accesskit_atspi_common --all-features --lib` exercises
the original adapter regressions plus disclosure events, collapsed/disabled/hidden
and released guards, Unicode offsets/lengths, one-request edits, read-only copy
and legacy unsupported behavior. These are translation tests and do not replace
the actual Linux AT-SPI D-Bus client/runtime gate.

The Unix fork depends on this named package with path/version/package fields.
External Git/path consumers need no root patch. Registry release order is this
package, kael_accesskit_unix, then kael. Publishing is a separate release step;
no crate was published as part of this change.

Cargo reserves `Cargo.toml.orig` when building an archive. Its exact upstream
contents are shipped as `UPSTREAM-Cargo.toml`; any original reserved-name copy
is excluded. Cargo generates its own normalized-manifest companion for the
named fork.

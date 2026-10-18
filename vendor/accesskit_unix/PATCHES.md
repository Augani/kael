# Kael Unix accessibility adapter fork

This is published `accesskit_unix` 0.22.1 from AccessKit commit
[`c88605b96d04431f9c3c792464a0f2f253480e94`](https://github.com/AccessKit/accesskit/tree/c88605b96d04431f9c3c792464a0f2f253480e94/platforms/unix),
distributed as **kael_accesskit_unix 0.4.1**. Original source notices, README,
CHANGELOG and original manifest remain intact. `UPSTREAM-SHA256.json` records
original registry file hashes. Unchanged MIT, Apache-2.0 and Chromium BSD licenses
are included; additions use the same licensing. `native-text-support.patch`
records the complete changes against the published source.

The adapter directly aliases `kael_accesskit_atspi_common` rather than relying on
a consuming workspace patch. It registers and unregisters the actual
`org.a11y.atspi.EditableText` D-Bus interface, exposing all six standard methods:
SetTextContents, InsertText, DeleteText, CopyText, CutText and PasteText. Unsupported
operations return NotSupported; unsuccessful guarded mutations return false.
CopyText has a void D-Bus signature and returns NotSupported if not accepted.

The existing `Adapter::new` constructor remains compatible. The new optional
`Adapter::new_with_text_handler` requires a host `TextEditHandler` and transports
partial edits and clipboard operations as one immutable-origin request. It
does not queue a selection followed by an edit. Host handlers run on the adapter
worker and must queue bounded foreground work, check current document identity
again at dispatch and use the platform clipboard on its proper thread.

Both async-io and Tokio runtime modes remain available and mutually exclusive.
Root Accessible/Cache interfaces are installed before desktop publication.
Bounded registration batches yield to the D-Bus dispatcher during large logical
tree updates. `KAEL_ATSPI_TRACE` optionally records registration counts without
document contents; the external CI clients preserve these failure diagnostics.
Translation/unit checks on another Unix host are compile/logic evidence only.
Actual Linux AT-SPI D-Bus exploration, disclosure, Unicode selection/geometry,
editing, clipboard and stale/lifecycle rejection remain native CI gates.

Kael uses a direct target-specific path/version/package dependency. External
Git/path consumers receive these forks without root patch overrides. Registry
release order is kael_accesskit_atspi_common, kael_accesskit_unix, then kael, as
recorded in `scripts/publish-all.sh`. No publication occurred during this work.

## Wire format and independently verified offset contract

The AddAccessible cache signal has **one struct argument**, with body signature
`((so)(so)(so)iiassusau)`. RemoveAccessible similarly has one `(so)` argument.
Passing either struct directly to zbus flattens its fields; libatspi then rejects
the signal. The fork wraps the arguments in singleton tuples and tests the actual
zbus Message serializer against the [GNOME cache protocol](https://gnome.pages.gitlab.gnome.org/at-spi2-core/devel-docs/doc-org.a11y.atspi.Cache.html).

The independent [GNOME D-Bus XML](https://github.com/GNOME/at-spi2-core/blob/main/xml/EditableText.xml)
and [C implementation](https://github.com/GNOME/at-spi2-core/blob/main/atspi/atspi-editabletext.c)
specify InsertText's position as a character offset and its length as UTF-8 bytes;
the native client sends `(position, "日本🙂", 6)` and checks that only `日本` is
inserted. No negative-length sentinel is specified, so negative lengths are
rejected. A byte prefix ending inside a scalar is rejected before creating the
replacement. CopyText has no D-Bus output; libatspi's C/GI wrapper returns true
after the no-output call, while errors propagate through GError. The native
client therefore verifies actual system clipboard contents, not that return
value alone. Other EditableText operations return a boolean.

Text offsets/counts are Unicode scalar indices; native EOF is the complete
scalar length, including CRLF and the trailing line feed. Exact substring reads
preserve scalars inside shaped graphemes and CRLF. Selection/edit requests that
cannot be represented as atomic AccessKit text units are rejected rather than
silently rounded to a different position. Word navigation follows the host's
exported word boundaries; range reveal addresses the originating text run and
preserves selection. The application reports UTF-8 byte endpoints independently
of the client's scalar offsets to detect domain mixups.

The actual tree client compares D-Bus service/path identity across disclosure,
not a cached Python GObject address: libatspi's [RemoveAccessible handler](https://github.com/GNOME/at-spi2-core/blob/main/atspi/atspi-misc.c)
disposes that object and [removes it from the client cache](https://github.com/GNOME/at-spi2-core/blob/main/atspi/atspi-object.c).

Cargo reserves `Cargo.toml.orig` when building an archive. Its exact upstream
contents are shipped as `UPSTREAM-Cargo.toml`; any original reserved-name copy
is excluded. Cargo generates its own normalized-manifest companion for the
named fork.

# Kael-maintained Windows adapter

This is the published `accesskit_windows` **0.34.0** source from AccessKit commit
[`c88605b96d04431f9c3c792464a0f2f253480e94`](https://github.com/AccessKit/accesskit/tree/c88605b96d04431f9c3c792464a0f2f253480e94/platforms/windows).
Kael publishes the separate package **`kael_accesskit_windows` 0.4.1** with the
Rust library name `accesskit_windows`. Existing `Adapter` and
`SubclassingAdapter` constructors are retained. This is a maintained fork,
not an upstream AccessKit release.

`UPSTREAM-SHA256.json` records exact original registry source hashes.
`text-pattern-support.patch` contains source, manifest, and example changes
against that registry source. `README.md`, `CHANGELOG.md`, `Cargo.toml.orig`,
and upstream copyright headers are retained. MIT and Apache-2.0 licenses are
included; the Chromium-derived mapping retains its BSD notice and
`LICENSE.chromium` from the same AccessKit commit. Additions use the same
licenses. Native API dependencies and examples are target gated; the consumer
query model remains testable on other hosts.

## Native text behavior

Microsoft specifies [contiguous visible text ranges](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationcore/nf-uiautomationcore-itextprovider-getvisibleranges),
including a degenerate range when nothing is visible. The fork computes visible
spans from actual transformed run bounds, character positions, widths, text
direction, and ancestor clips. Missing geometry remains unknown visibility;
it does not make an offscreen document visible. Returned arrays are non-null.

[FindText](https://learn.microsoft.com/dotnet/api/system.windows.automation.provider.itextrangeprovider.findtext)
searches complete or subset ranges, including offscreen text, with forward-first
and backward-last behavior. Search streams the consumer's text runs with KMP:
linear comparisons and scratch proportional to the needle, without copying the
document or scanning it again for every match. Case-insensitive matching uses
Windows `CompareStringOrdinal`, without canonical normalization or multi-scalar
case expansion. Matches preserve the consumer's atomic units, including CRLF;
no result splits a surrogate or a grouped consumer unit. An empty search has no
match; malformed UTF-16 returns `E_INVALIDARG`.

UIA's no-match contract is **S_OK with an initialized null interface pointer**.
The generated windows-rs trait cannot express a nullable successful interface.
A private wrapper keeps the exact `ITextRangeProvider` IID and vtable layout,
replacing only the `FindText` entry with `Result<Option<ITextRangeProvider>>`
transport. It clears the native output on every result path, checks null output
pointers, and never creates a Rust interface containing a null pointer. Other
methods retain generated ABI implementations. Comparisons safely reject foreign
COM implementations and cross-document owners. Unsupported rich-text attribute
search returns `E_NOTIMPL`, rather than success with an uninitialized output.

[Bounding rectangles](https://learn.microsoft.com/dotnet/api/system.windows.automation.provider.itextrangeprovider.getboundingrectangles)
are clipped to the control and ancestor viewport. Unknown/offscreen/degenerate
geometry returns an allocated empty array. Read-only
[SetValue](https://learn.microsoft.com/dotnet/api/system.windows.automation.provider.ivalueprovider.setvalue)
returns `UIA_E_INVALIDOPERATION` before dispatch; malformed UTF-16 no longer
panics in the native boundary. Disabled selection/reveal returns
`UIA_E_ELEMENTNOTENABLED`. Weak ranges continue to reject retired run identities.

Retained multiline text inputs also expose `ValuePattern`, following Microsoft's
[current Value implementation guidance](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-implementingvalue).
The consumer deliberately omits a flattened multiline value; this fork constructs
the requested string from the retained text runs on demand, using the same source
as `TextPattern`. It adds no duplicate value to the root node or caret updates.
Read-only/disabled `SetValue` continues to reject before dispatch. The real native
Unicode Editor client exercises full value reads and the read-only mutation guard.

## Evidence

Portable model tests cover Unicode across runs, overlapping backward matches,
case matching, subset ranges, atomic CRLF/surrogate boundaries, disjoint horizontal
visibility, RTL geometry, missing geometry, and a 100,000-character linear-search
comparison bound. Native Windows tests additionally exercise the actual COM
nullable ABI (including a caller sentinel, null output, and malformed UTF-16),
non-null visible arrays, cross-owner ranges, and read-only rejection.

The Windows CI helper requires those native provider tests and drives a real
Kael Editor through an external UIA client. Its full document, visible/search,
idle reveal, selection/caret, screen-point, read-only, disabled, and stale-revision
checks are mandatory. Native runtime success requires the actual Windows job;
portable tests and cross compilation alone are not runtime proof.

`Cargo.toml.orig` remains in the checkout as pristine provenance and is excluded from the package because Cargo reserves that archive path. Its identical packaged copy is `UPSTREAM-Cargo.toml`.

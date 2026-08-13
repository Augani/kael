# Native Capability Bridge

Kael's primary application surface combines Rust, native windows, and a retained
GPU-rendered UI tree. A WebView is an explicit compatibility island for a
dependency that is genuinely web-shaped, such as an OAuth page, payment flow,
map, hosted document, or vendor widget.

## Choose the smallest layer

| Need | Start with |
| --- | --- |
| Runtime, rendering, input, text, layout, and windows | `kael` |
| A custom design system | `kael` primitives and `Styled` |
| Ready-made, brandable controls | `kael_ui` |
| Product services | the focused `kael_*` support crates |
| A browser-owned surface | Kael's optional `webview` feature |

`kael` never depends on `kael_ui`. Applications can build their entire visual
language on the primitive crate or use only the component families they want.

## Treat capability reports as runtime evidence

```rust
use kael::{CapabilityReport, PlatformFeature};

let report = CapabilityReport::current();

if report.is_supported(PlatformFeature::GlobalHotkeys) {
    // Enable the primary native workflow.
} else if report.is_available(PlatformFeature::GlobalHotkeys) {
    // Explain setup or platform limitations and retain a fallback.
} else {
    // Hide the workflow or use a deliberate alternative.
}
```

`Full` means Kael exposes a usable backend without a documented fallback.
`Partial` and `RequiresInit` require the caller to handle the note and setup.
`Unsupported` means a descriptor or OS API may exist, but Kael does not provide
the native operation. `Disabled` means the required Kael feature was not built.

Do not infer support from a builder type. Checked descriptors validate intent;
they do not substitute for an OS backend.

## Native-first decision rule

1. Use native primitives for app chrome, editors, navigation, data surfaces,
   commands, menus, files, background work, and long-lived product state.
2. Use a focused support crate for storage, secrets, documents, diagnostics,
   networking, notifications, sharing, media, and release services.
3. Query the capability report for platform-dependent workflows.
4. Use a WebView only for a scoped web dependency, with explicit navigation,
   permission, storage, and bridge policy.

## Important 0.3 boundaries

The following are not native batteries in Kael 0.3 and are reported as
unsupported: push-registration backends, native geolocation, USB/HID/serial/
Bluetooth discovery and I/O, outbound file-promise drag sources, app-window
snapshot backends, and native spellchecking. Applications may supply their own
integration without pretending Kael completed it.

Outbound sharing is feature-gated and platform-dependent. macOS has the
broadest destination support; Windows and Linux currently provide narrower
mail/clipboard handoffs. Registering an app as a share receiver is not yet
implemented. WebView support is reported as disabled when the `webview` feature
is absent and partial when enabled because it is a native composition island
with platform/runtime constraints rather than a GPU scene primitive.

## Optional agent planning metadata

The `agent-tools` feature exposes Kael's structured desktop-capability planning
metadata. It is off by default because those types describe and audit
implementation work; normal applications should not pay to compile them.

```toml
[dependencies]
kael = { version = "0.3", features = ["agent-tools"] }
```

Agents do not need this feature to build Kael applications. The public Rust API,
crate documentation, concise `llms.txt`, and Astryx source are the primary
references.

## Readiness rule

A capability is production-ready only when the native operation exists, errors
are actionable, platform variance is represented, and CI exercises the relevant
target. A descriptor, roadmap entry, or showcase rendering is not sufficient
evidence by itself.

# What remains

Kael is broad, but it is not finished. This page separates current gaps from
deliberate product boundaries.

`CapabilityReport::current()` is the live source of truth at runtime. A type or
request builder does not prove that every backend implements the operation.

## Framework work

These areas need more implementation or wider platform coverage:

| Area | Current gap |
|---|---|
| Spellcheck | No native or bundled spelling backend |
| Realtime network | Server sent event descriptors exist; live transport is not complete |
| File drag | No outbound promised file drag backend |
| Sharing | No share receiver; outgoing support varies by platform |
| Location and devices | Geolocation, USB, HID, serial, and Bluetooth backends are absent |
| Browser install features | Push, notification actions, and share targets need product service worker code |
| Input depth | Some native pen, touch, and gesture paths need broader coverage |
| Media | Portable spatial audio is stereo and distance based, not a full room or HRTF engine |
| Web packaging | The CLI packages source HTML and assets, but has no content hashing or service worker generator |
| Platform quality | Linux services and hardware coverage still need continued hardening |

The public API is pre 1.0. Stabilization, compatibility policy, and wider
hardware testing remain release work even where a feature is already usable.

## Deliberate boundaries

Some work belongs to applications or focused engines rather than Kael core:

* Exact Microsoft Office layout, spreadsheet calculation, and slide playback
* Full PDF authoring and layout parity with specialist products
* A complete 3D engine with custom shaders, compute, physics, and asset pipelines
* Browser access to native paths, subprocesses, keychains, global hotkeys, or detached OS windows
* Product specific collaboration protocols and document semantics

Kael supplies the application runtime, rendering, controls, data foundations,
portable files, document bytes, and extension points. A suite or game engine can
build its product model above those parts without hiding the remaining work.

## How to plan a portable feature

1. Build the shared view and state first.
2. Query `CapabilityReport` at the platform boundary.
3. Define a useful browser fallback before calling a native service.
4. Test desktop and browser as separate release targets.
5. Record unsupported behavior in the product, not only in build scripts.

See [One codebase](one-codebase.md) for the structure and
[Platform APIs](platform-apis.md) for the capability model.

# Linux WebView hosting

Kael's maintained Linux WebView stack uses one GTK4-owned window hierarchy on
X11, XWayland, and native Wayland. The archived GTK3 stack is not shipped;
WebViews never use a detached top-level overlay.

| Desktop path | Kael feature | Native stack | Contract |
| --- | --- | --- | --- |
| Native Wayland | `webview` | GTK4 + GSK + WebKitGTK 6.0 | Maintained production path |
| X11 or XWayland | `webview` | GTK4 + GSK + WebKitGTK 6.0 | Maintained production path |
| Deprecated feature spelling | `webview-legacy-gtk3` | Redirects to GTK4 + WebKitGTK 6.0 | Source-compatible alias only |
| Raw Wayland/X11 without the GTK4 host | `wayland` / `x11` | Kael-owned native surface | WebViews unavailable |
| Headless | any | no native browser surface | WebViews unavailable |

## Maintained GTK4 production host

Enable the portable feature when an application needs WebViews on any desktop:

```toml
[dependencies]
kael = { version = "0.4", features = ["webview"] }
```

Ubuntu/Debian builders need `libgtk-4-dev` and `libwebkitgtk-6.0-dev` in
addition to Kael's standard Linux packages. `webview-gtk4` and
`webview-wayland-gtk4` remain Linux-specific compatibility spellings. The
deprecated `webview-legacy-gtk3` name also selects this maintained graph; no
Kael feature pulls the archived GTK3/WebKitGTK 4.1 stack on Linux.

GTK owns `ApplicationWindow -> Fixed`, with a retained-scene `Picture` and each
WebKit view as siblings. The Kael scene is converted to cached GSK nodes; it is
not screen-scraped or copied into a detached overlay. This gives the window and
its WebViews one compositor surface, one scale/monitor lifecycle, one focus and
input hierarchy, and one minimize/workspace lifetime.

The host implements:

- retained Scene primitives, text/image atlases, paths, sprites, patterns,
  shadows and supported backdrop effects through GSK;
- declarative WebView bounds, clipping, visibility, opacity and focus;
- URL/header/HTML navigation, history, reload/stop, zoom, find, print,
  downloads, cookies, named/incognito profiles, user scripts, IPC and native
  permission policy;
- app-owned custom protocols, including main-document navigation and
  same-origin subresources with status, MIME type, headers, and bounded body;
- pointer, wheel, keyboard, touch, IME composition, focus traversal, window and
  WebView file drag/drop, rich bounded clipboard content, and AT-SPI updates;
- monitor/backing-scale changes, raw Wayland or X11 window/display handles,
  XDG print parenting, native pointer lock, move/resize/menu/decorations, mouse
  passthrough, and retained-scene PNG export.

WebViews remain rectangular native islands above the retained scene. Kael
hides a view when a non-translation transform would make its native rectangle
incorrect. Arbitrary rotation/perspective through WebKit content and drawing
retained content over the middle of a WebView are not claimed. That is why the
overall host support level remains `Partial` while many individual operations
report `Full`.

## Backend selection

Selection is deterministic:

1. `KAEL_HEADLESS` selects headless mode.
2. `KAEL_LINUX_BACKEND=x11|wayland|headless` wins when set.
3. `GDK_BACKEND` ordering is honored when the named backend has a valid display.
4. Otherwise Wayland is selected when `WAYLAND_DISPLAY` is available, followed
   by X11 when `DISPLAY` is available.
5. With neither display available, Kael uses its headless fallback.

If an application selects a raw X11 or Wayland backend without compiling the
GTK4 WebView host, the capability report marks every WebView operation
unavailable and controller commands return a deterministic error. Kael never
creates a visually adjacent top-level window and calls it embedded.

## Why the GTK-owned host is necessary

The raw Wayland backend owns a `wl_surface` through Kael's Wayland client
connection. Wayland cannot attach a GTK surface from another client as a
positioned child, and Wry's Wayland constructor requires a GTK container. A
second GTK top level would break compositor placement, clipping, focus, input
methods, accessibility, minimization, and workspace movement.

The native host therefore changes ownership of the complete window rather than
patching only WebView rendering. GTK/GDK supplies the Wayland or X11 handles,
and Kael's scene, event translation, services, and WebKit widgets all live
inside that hierarchy.

## Release acceptance

`scripts/ci/run-linux-webview-wayland-gtk4.sh` starts a real headless Weston
session with `DISPLAY` unset. It runs two bounded proofs:

1. a focused same-GDK-surface scene/WebKit composition test; and
2. the production `PlatformWindow` implementation with retained-scene PNG
   export, raw Wayland handles, app-protocol navigation and subresources,
   page-to-host IPC, host-to-page messaging, JavaScript result serialization,
   current URL state, and clean application shutdown.

`scripts/ci/run-linux-webview-xwayland.sh` selects the same maintained host
through GDK's X11 backend and proves retained GSK output, raw X11 handles,
native pointer-lock acquisition and release after a native menu interaction,
custom protocols, IPC, JavaScript, URL state, and event-driven idle operation.
The scripts fail unless every stage marker is present. Mesa software rendering
is used on CI machines without a physical render node, while still exercising
the compositor/client EGL path. Logs are retained as release artifacts. The
deprecated feature alias is checked separately to prove it cannot reintroduce
the archived stack or substitute a different host.

Run both native proofs locally with:

```bash
bash scripts/ci/run-linux-webview-wayland-gtk4.sh
bash scripts/ci/run-linux-webview-xwayland.sh
```

For operation-specific decisions, query
`WebViewCapabilityReport::current()` instead of inferring support from the OS
name. It distinguishes full engine operations from native-island limitations
and from a feature that was not compiled.

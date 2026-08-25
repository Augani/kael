# Browser and WebAssembly

Kael runs the same application model on desktop and in the browser. State,
layout, components, painting, virtualization, animation, and accessibility stay
in Rust. The browser backend renders the retained scene into WebGL2.

This is a real application target. It is not a DOM rewrite and it is not a
WebView around the desktop build.

## What works

The browser backend connects these inputs to Kael's normal event paths:

| Input or service | Browser path |
|---|---|
| Pointer and buttons | Pointer events and pointer capture |
| Trackpad and mouse wheel | Wheel events |
| Keyboard and shortcuts | Keyboard events |
| Text and IME | Input and composition events |
| Copy, cut, and paste | Clipboard events with bounded payloads |
| File intake | File picker and drag and drop bytes |
| Animation | `requestAnimationFrame` |
| Accessibility | A synchronized semantic DOM mirror |
| Embedded web content | Managed iframe layers |

Kael tracks the device pixel ratio, refresh rate, visibility, and WebGL context.
Moving a page between displays updates the backing scale. Hidden or idle windows
stop requesting frames. A restored WebGL context receives a complete retained
frame without resetting application state.

## Do I need JavaScript?

No JavaScript is needed for normal Kael UI, input, state, animation, or canvas
work. `kael web build` generates `app.js`. That file loads the Wasm module and
connects it to the browser.

Write JavaScript only when the product needs a browser feature that Kael does
not expose, such as:

* a service worker
* a third party DOM widget
* a browser vendor SDK
* an unwrapped Web API
* custom host page integration

Keep that code at the product boundary. The application itself can remain one
Rust codebase.

## Build and run

Install the pinned packaging tools once:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.122 --locked
npm install --global binaryen@132.0.0
```

Run the development build:

```sh
kael web serve --debug
```

Build the optimized site:

```sh
kael web build
```

The default output is:

```text
dist/web/
├── index.html
├── app.js
└── app_bg.wasm
```

Use `--package <name>` or `--bin <name>` in a Cargo workspace. Use
`--out-dir <path>` to change the output directory. Use `--html <file>` for a
source-owned host page and `--assets <directory>` for product web assets.
`kael web serve` also accepts `--port <number>` and `--no-open`.

See [Web build and deployment](web-deployment.md) for the complete host
contract.

## Keep dependencies portable

A portable project selects native and browser features by target:

```toml
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
kael = { version = "0.4", features = ["runtime_shaders"] }
kael_ui = "0.4"

[target.'cfg(target_arch = "wasm32")'.dependencies]
kael = { version = "0.4", default-features = false, features = ["browser"] }
kael_ui = { version = "0.4", default-features = false, features = ["browser"] }
```

Use `CapabilityReport::current()` before a workflow depends on an operating
system service. This keeps the main view and state shared while making the
fallback explicit.

## Files and documents

Use `App::prompt_for_files` or `App::show_open_files` to receive
`ExternalFile` values. They contain a safe name, optional MIME type, and bytes.
Desktop selections can also contain a native path. Browser selections never
invent one.

Use `App::save_file_bytes` for generated files. Desktop opens a Save dialog.
The browser starts a download. DOCX, XLSX, PPTX, PDF, and project bytes can use
the same parser and exporter on both targets.

Browser file intake is limited to 256 MiB per file and 512 MiB per selection or
drop. Failed files remain visible with an error.

## Browser boundaries

The browser cannot provide every desktop service. It does not provide native
paths, subprocesses, global hotkeys, status items, keychain access, detached OS
windows, auto update, system idle state, or unrestricted device access.

Notifications, microphone capture, and screen capture require browser
permission and an enabled Kael feature. Sharing is partial. Push, durable
notification actions, and share targets need product owned PWA or service worker
code.

Secondary Kael windows are surfaces inside the page. They cannot detach to
another display or become taskbar or dock windows.

See [What remains](remaining-work.md) for the current framework gaps.

## Release evidence

The maintained browser suite covers Chromium, Firefox, and WebKit. The current
release tested baseline is Chromium 151, Firefox 153, and WebKit 26.5. Those are
the versions pinned by the release matrix, not a claim that older browsers work.

The smoke app renders one million logical table rows with no more than 64 rows
mounted. It tests scroll input, direct jumps, IME, clipboard, retained damage,
component animation frames, WebGL loss and restoration, and framebuffer output.
Hardware and software rasterizer results are reported separately.

Read [Benchmarking evidence](benchmarking.md) for limits and reports,
[Browser workers](browser-workers.md) for bounded background work, and
[Browser audio](browser-audio.md) for the Web Audio path.

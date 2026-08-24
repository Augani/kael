# Testing

## Browser engine matrix

The release-level browser proof runs the same packaged WebAssembly artifacts in
Chromium, Firefox, and WebKit. Install the pinned Playwright dependency and
browser binaries, then run:

```sh
cargo install wasm-bindgen-cli --version 0.2.122 --locked
python3 -m venv target/browser-matrix-venv
target/browser-matrix-venv/bin/python -m pip install \
  -r scripts/browser-matrix-requirements.txt
target/browser-matrix-venv/bin/python -m playwright install \
  chromium firefox webkit
KAEL_PLAYWRIGHT_PYTHON=target/browser-matrix-venv/bin/python \
  bash scripts/verify-browser-matrix.sh
```

Use `KAEL_BROWSER_MATRIX_ENGINES=firefox` for one-engine diagnosis.
`KAEL_BROWSER_MATRIX_SKIP_BUILD=1` reuses existing packaged artifacts, while
`KAEL_BROWSER_MATRIX_SKIP_SUITE=1` and
`KAEL_BROWSER_MATRIX_SKIP_REALTIME=1` are diagnostic-only reductions.
`KAEL_BROWSER_MATRIX_SKIP_CAPTURE=1` omits only the injected canvas-backed
display-capture lifecycle fixture. Release CI uses none of these reductions.
Evidence is written to `target/browser-matrix`.

## Generated project parity

The maintained release gate invokes the actual CLI, leaves its generated
`src/main.rs` and `Cargo.toml` byte-for-byte unchanged, and checks the same
source against both target dependency sets:

```sh
KAEL_PLAYWRIGHT_PYTHON=target/browser-matrix-venv/bin/python \
  bash scripts/verify-generated-project-parity.sh
```

The verifier creates a temporary nested workspace under `target`, applies
`kael` and `kael_ui` through a parent workspace `[patch.crates-io]` table, seeds
resolution from the repository `Cargo.lock`, and deletes only that temporary
workspace on exit. This lets release CI prove the next unpublished Kael version
without rewriting the generated project or selecting an unrelated fresh set of
transitive versions. It checks the native target, fetches the locked wasm graph,
then packages the unchanged binary offline through `kael web build`. The gate
requires the pinned `wasm-bindgen` and Binaryen optimization pass before it
applies the release artifact budget and launches the static output in pinned
Chromium. The browser proof requires successful Wasm initialization, at least
two retained frames, non-blank composited pixels, a viewport-filling canvas,
and no page or request errors.
The untouched generated source and manifest, metadata, hashes, logs, packaged
web files, a screenshot, and the JSON report are retained in
`target/generated-project-parity-evidence`.

The publish workflow waits for the reusable platform-readiness workflow before
starting its release preflight, so this proof is release-blocking. The native
renderer jobs independently scaffold the same unchanged template on Windows and
Linux to prove that the generated desktop binary launches there too.

## Native renderer runtime smoke

Platform readiness does not treat a successful native compile as renderer
evidence. `native_renderer_smoke` rejects `KAEL_HEADLESS`, opens and activates a
real window, advances four visually different retained scene revisions, reads
the selected GPU identity, and exports the final scene through the backend at
device-pixel resolution. The gate decodes the PNG and checks its dimensions,
visible-pixel ratio, color diversity, and luminance range before exiting itself
within a 20-second deadline.

Linux runs the X11 surface path under a private Xvfb server and selects Mesa
lavapipe explicitly on the GPU-less hosted runner. This proves Blade/Vulkan
window, presentation, and readback behavior with an honestly reported software
adapter; it is not a hardware-throughput or native-Wayland-compositor claim.
Windows runs the same scene through Direct3D 11. Normal applications prefer a
compatible hardware adapter and fall back to WARP if enumeration finds none.
Hosted CI explicitly sets the strictly parsed `KAEL_FORCE_WARP=1` proof switch,
then requires the adapter to identify itself as software. This is likewise a
correctness/liveness gate rather than Direct3D hardware performance evidence.

```sh
# Linux, with Xvfb and Mesa Vulkan packages installed
KAEL_NATIVE_RENDERER_USE_SOFTWARE=1 \
  bash scripts/ci/verify-linux-native-renderer.sh

# Windows
pwsh -File scripts/ci/verify-windows-native-renderer.ps1
```

Both scripts then invoke `kael new`, build the untouched generated source, and
prove that its visible native window is mapped. Windows closes the starter via
its normal Win32 window lifecycle. The Linux starter is deliberately an
interactive app with no test-only exit branch, so CI captures its X11 window
geometry and pixels and then stops it externally under a bound. Evidence lives
under `target/native-renderer-smoke/{linux,windows}` and includes adapter logs,
the decoded-scene PNG, generated-source snapshots, and native-window evidence.

## Native WebView runtime smoke

Platform readiness executes `webview_smoke` against WKWebView on macOS,
WebView2 on Windows, and the maintained GTK4 + WebKitGTK 6 host on both
Weston/XWayland and native Wayland. The
macOS verifier explicitly removes `KAEL_HEADLESS` from the child environment
and requires page-load, page-to-host IPC, host-to-page messaging, JavaScript
result, and current-URL stages plus successful focus and zoom commands before
accepting the final marker:

```sh
bash scripts/ci/verify-macos-wkwebview.sh
```

The log is retained in `target/macos-wkwebview-smoke/wkwebview.log`. This is a
real platform runtime gate; a headless capability check cannot satisfy it.

The native-Wayland gate runs
`scripts/ci/run-linux-webview-wayland-gtk4.sh`. It requires the focused
same-surface composition proof and the production `PlatformWindow` proof,
including retained-scene PNG export, raw Wayland handles, app-owned custom
protocol navigation and subresources, page/host IPC, JavaScript results, URL
state, and clean shutdown under a headless Weston compositor.

`scripts/ci/run-linux-webview-xwayland.sh` selects the same production host
through GDK's X11 backend and requires X11 raw handles, GSK scene export, the
native XI2 pointer-lock implementation to acquire and release after a native
context menu has been shown, the full WebView protocol/IPC stages, event-driven
idle behavior, and clean shutdown without terminating XWayland. The legacy
feature spelling is compiled in isolation to prove it redirects to the
maintained host and cannot reintroduce GTK3.

Kael ships a headless test platform so you can drive real windows, views, and
input from ordinary unit tests — no display server, GPU, or windowing backend
required. Tests run the same on macOS, Windows, and Linux CI.

## `#[kael::test]`

Annotate a test with `#[kael::test]` and declare a `&mut TestAppContext`
parameter. The macro builds the headless app context, seeds the RNG, and tears
everything down afterward:

```rust
use kael::{TestAppContext, WindowOptionsBuilder};

#[kael::test]
fn opens_a_window(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        cx.open_window(WindowOptionsBuilder::new().title("Test"), |_, cx| {
            cx.new(|_| MyView::default())
        })
            .unwrap()
    });
    window.update(cx, |view, _window, _cx| {
        assert!(view.is_ready());
    }).unwrap();
}
```

Parameter types the macro recognizes:

| Parameter | What you get |
| --- | --- |
| `&mut TestAppContext` | A headless app context (open one per `&mut TestAppContext` parameter). |
| `BackgroundExecutor` | The test's deterministic background executor. |
| `StdRng` | A seeded RNG (seed `0`, or `SEED` env var, or `#[kael::test(seed = N)]`). |

Async tests are supported — declare the fn `async` and the macro drives it on the
test executor.

## `TestAppContext` and `VisualTestContext`

`TestAppContext` is the entry point. For tests that need a rendered window, use
`add_window_view`, which opens a maximized headless window, renders your root
view, and hands back the view plus a `VisualTestContext` scoped to that window:

```rust
let (view, vcx) = cx.add_window_view(|_window, _cx| MyView::default());
```

`VisualTestContext` is where window-level testing happens:

| Method | Use |
| --- | --- |
| `simulate_click(point, modifiers)` | Synthesize a left mouse down + up. |
| `simulate_mouse_move` / `simulate_mouse_down` / `simulate_mouse_up` | Lower-level pointer events. |
| `simulate_keystrokes("cmd-s a b")` | Type a space-separated keystroke sequence. |
| `dispatch_action(MyAction)` | Dispatch an action into the focused tree. |
| `run_until_parked()` | Flush the executor so effects, notifies, and redraws settle. |
| `draw(origin, space, fn)` | Lay out and paint an element directly. |

The golden rule: after mutating view state or simulating input, call
`run_until_parked()` before asserting, so pending effects and re-renders complete.

## Testing `kael_ui` components

`kael_ui` is a normal crate, so the same pattern works for its components. The
trick is enabling the headless platform: add `kael` as a **dev-dependency** with
the `test-support` feature so `TestAppContext` is in scope during tests.

```toml
# crates/kael_ui/Cargo.toml
[dev-dependencies]
kael = { path = "../kael", version = "0.4.0", features = ["test-support"] }
```

> `test-support` is platform-agnostic: the test platform mocks the windowing
> layer, so it does **not** pull the Linux Wayland/X11 crates into a macOS or
> Windows test build. (Linux CI that needs real-window rendering under tests can
> opt in with the `test-support-linux-windowing` feature.)

Then write a root harness view that renders the component under test and assert
on observable behavior. A button-click test, end to end:

```rust
use std::cell::Cell;
use std::rc::Rc;
use kael::{div, point, px, Context, IntoElement, ParentElement as _, Render, Styled as _,
    InteractiveElement as _, TestAppContext, Window};
use kael_ui::prelude::*;

struct Harness { clicks: Rc<Cell<usize>> }

impl Render for Harness {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div().size_full().child(
            div().absolute().top(px(0.0)).left(px(0.0)).child(
                Button::new("btn", "Click me")
                    .size(ButtonSize::Lg)
                    .on_click(move |_e, _w, _cx| clicks.set(clicks.get() + 1)),
            ),
        )
    }
}

#[kael::test]
fn button_click_fires_handler(cx: &mut TestAppContext) {
    cx.update(|cx| kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark()));

    let clicks = Rc::new(Cell::new(0));
    let (_view, vcx) = cx.add_window_view(|_w, _cx| Harness { clicks: clicks.clone() });

    vcx.simulate_click(point(px(40.0), px(20.0)), Default::default());
    vcx.run_until_parked();

    assert_eq!(clicks.get(), 1);
}
```

Notes:

- Install a theme first — most components read `Theme::dark()` tokens during
  render and `kael_ui::theme::install_theme` makes them available.
- Position the component deterministically (here, top-left) so the simulated
  click lands on it.
- State that lives on the view (toggles, counters, transition state) is asserted
  by calling `view.update(vcx, |view, _| { ... })` after `run_until_parked()`.

See `crates/kael_ui/tests/component_tests.rs` for the full set of patterns,
including an implicit-transition test on a real `div`.

## CI recipe

Run the suites with the `runtime_shaders` feature so macOS CI does not need
Xcode's `metal` compiler, and the test-support platform stays headless:

```yaml
# .github/workflows/ci.yml (excerpt)
jobs:
  test:
    strategy:
      matrix:
        os: [macos-latest, windows-latest, ubuntu-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.97.1
      - name: Test kael
        run: cargo test -p kael --lib --features test-support,runtime_shaders
      - name: Test kael_ui
        run: cargo test -p kael_ui --features kael/runtime_shaders
      - name: Test xtask
        run: cargo test -p xtask
```

Locally, the same commands work:

```sh
cargo test -p kael --lib --features test-support,runtime_shaders
cargo test -p kael_ui --features kael/runtime_shaders
cargo test -p xtask
```

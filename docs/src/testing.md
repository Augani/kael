# Testing

Kael ships a headless test platform so you can drive real windows, views, and
input from ordinary unit tests — no display server, GPU, or windowing backend
required. Tests run the same on macOS, Windows, and Linux CI.

## `#[kael::test]`

Annotate a test with `#[kael::test]` and declare a `&mut TestAppContext`
parameter. The macro builds the headless app context, seeds the RNG, and tears
everything down afterward:

```rust
use kael::TestAppContext;

#[kael::test]
fn opens_a_window(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| MyView::default()))
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
kael = { path = "../kael", version = "0.3.0", features = ["test-support"] }
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

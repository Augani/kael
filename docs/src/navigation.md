# Navigation

Multi-screen apps push and pop views on a navigation stack. The canonical API is
`kael::Navigator` in the core crate.

## Navigator

```rust
use kael::{Navigator, Route, Transition, navigator};

let nav = navigator(Route::new("inbox", inbox_view));

nav.push(
    Route::new("thread", thread_view).with_memento(ThreadScroll { offset }),
    Transition::SlideLeft,
    window,
    cx,
);

nav.pop(Transition::SlideRight, window, cx);
```

- **Routes** pair a stable id with an `AnyView`.
- **Mementos** carry restorable per-route state (scroll positions, selections):
  attach with `with_memento`, read back with `route.memento::<T>()` when the
  route resurfaces.
- **Events**: `Navigator` emits `RouteChangeEvent`, so views can
  `cx.subscribe` to react to navigation (analytics, focus restoration).
- **Transitions**: pushes and pops animate with `Transition` (slide, fade, or
  custom).

`Navigator` is a screen stack, not a URL router — there is no path matching or
deep linking yet; those are tracked for a future release.

`Navigator` also offers `replace`, `replace_stack`, and `pop_to_root` for
rewriting the stack, each taking a `Transition` to animate the change.

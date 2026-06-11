# Navigation

Multi-screen apps push and pop views on a navigation stack. The canonical API is
`kael::Navigator` in the core crate.

## Navigator

```rust
use kael::{Navigator, Route, navigator};

let nav = navigator(Route::new("inbox", inbox_view));

nav.push(
    Route::new("thread", thread_view).with_memento(ThreadScroll { offset }),
    window,
    cx,
);

nav.pop(window, cx);
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

## ViewRouter (kael_ui, legacy)

`kael_ui::components::view_router::ViewRouter` predates `Navigator` and overlaps
with it (push/pop/replace with `PageTransition` animations). It remains
supported for existing apps, but `Navigator` is the one being invested in —
prefer it for new code. `ViewRouter` may be folded into `Navigator` before 1.0.

| | `kael::Navigator` | `kael_ui::ViewRouter` |
|---|---|---|
| Stack push/pop | yes | yes (+ replace) |
| Restorable route state | mementos | no |
| Route-change events | `RouteChangeEvent` | no |
| Transition animations | slide/fade/custom | slide/fade presets |
| Status | canonical | legacy, kept for compatibility |

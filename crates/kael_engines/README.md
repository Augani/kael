# kael_engines

`kael_engines` provides dependency-light algorithms and state models that are
useful across native applications. It can be used with Kael or on its own; none
of its modules depend on Kael's renderer or UI crate.

## Included primitives

- `bidi`: Unicode bidirectional classification, weak-type helpers, and visual
  ordering through UAX #9 rule L2. Glyph shaping, cursor mapping, combining-mark
  adjustment, and mirrored glyph selection belong in the text renderer.
- `linebreak`: UAX #14 break opportunities with extended-grapheme-safe hard
  wrapping for fixed-cell text. Proportional text should wrap using shaped glyph
  advances.
- `undo`: bounded snapshot undo/redo with transaction and coalescing helpers.
- `canvas`: vector/canvas data types, export validation, visible-tile queries,
  and a byte-and-entry-bounded tile cache.
- `crash_report`: bounded, serializable Rust panic records and a panic hook that
  preserves an existing hook. Native faults still require the optional
  `kael_diagnostics` out-of-process crash service.
- `dashboard`: chart/query state, a capacity-bounded query lifecycle model,
  and a bounded single-record CSV parser. Applications still execute queries
  and remove state they no longer need.
- `game_loop`: deterministic fixed-timestep frame scheduling with interpolation,
  display-delta clamping, bounded catch-up work, pause/reset controls, and
  dropped-time telemetry. Its monotonic clock works in native and WebAssembly
  builds.
- `ide`: deterministic in-memory project/search models and language-server
  lifecycle state. Limited search keeps its result buffer bounded. The
  application still owns file watching, durable indexing, and operating-system
  process supervision.

## Undo example

```rust
use kael_engines::undo::UndoHistory;

let mut document = UndoHistory::new(String::from("draft"));
document.edit(|text| text.push_str(" one"));
document.edit(|text| text.push_str(" two"));

assert_eq!(document.current(), "draft one two");
assert!(document.undo());
assert_eq!(document.current(), "draft one");
```

## Fixed-timestep game loop

Keep simulation updates deterministic even when display refresh rates vary. The
clock returns a bounded update count and an interpolation value for rendering;
it never executes application code itself.

```rust
use std::time::Duration;

use kael_engines::game_loop::{FixedFrameClock, FixedFrameClockConfig};

let config = FixedFrameClockConfig::from_updates_per_second(60)?
    .with_max_frame_delta(Duration::from_millis(250))
    .with_max_catch_up_steps(8);
let mut clock = FixedFrameClock::new(config)?;

let frame = clock.advance_by(Duration::from_millis(17));
for _ in frame.updates() {
    // update_simulation(frame.fixed_timestep());
}
let _render_alpha = frame.interpolation_alpha();
assert!(frame.update_steps() <= 8);
# Ok::<(), Box<dyn std::error::Error>>(())
```

In a Kael `Render` implementation, call `clock.tick()`, run the returned fixed
updates, interpolate the rendered state, and call
`window.request_animation_frame()` while the simulation is active. Stop
requesting frames when paused. `tick()` uses a browser-compatible monotonic
clock; `advance_by(Duration)` is the deterministic path for replays and tests.
Monitor `FrameAdvance::dropped_time()` or
`FixedFrameClock::total_dropped_time()` to detect stalls and sustained overload.

Caches, schedulers, result buffers, and parsers provided by this crate have
explicit or conservative limits. Collections that represent application-owned
data, such as project and search entries, remain caller-owned and intentionally
grow only when the caller adds data.

## License

Licensed under the Apache License, Version 2.0. See
[LICENSE-APACHE](LICENSE-APACHE).

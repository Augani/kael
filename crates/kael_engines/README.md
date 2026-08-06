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

Caches, schedulers, result buffers, and parsers provided by this crate have
explicit or conservative limits. Collections that represent application-owned
data, such as project and search entries, remain caller-owned and intentionally
grow only when the caller adds data.

## License

Licensed under the Apache License, Version 2.0. See
[LICENSE-APACHE](LICENSE-APACHE).

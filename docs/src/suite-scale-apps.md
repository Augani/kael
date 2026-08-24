# Suite-scale Applications

Kael maintains one source-level release workload for the four surfaces common
to an office suite. It compiles as a native desktop application and as
WebAssembly without changing view code:

```bash
# Desktop
cargo run -p kael_ui --example suite_scale_smoke

# Browser package
bash scripts/build-browser-suite-smoke.sh
python3 -m http.server 8133 --directory target/browser-suite-smoke
```

The reference app is not a DOCX renderer, spreadsheet formula engine, or slide
layout engine. It is the framework-scale proof beneath those product layers: it
exercises retained views, virtual mounting, bounded caches, editing state,
search/undo, immediate canvas batches, spatial culling, damage, rich pointer
input, and fixed-step animation using the same Rust source on both targets.

## Maintained workload contract

| Surface | Logical workload | Per-view retained work |
| --- | ---: | ---: |
| Sheets | 1,000,000 rows × 16,384 columns | at most 2,048 mounted cells; 8-tile LRU in the live grid |
| Docs | 250,000 blocks (5,209 pages) | at most 6 pages / 288 blocks; sparse edits and 64 undo transactions |
| Slides | 10,000 slides | at most 16 thumbnails and one retained slide surface |
| Whiteboard | 100,000 shapes | at most 2,048 visible shapes / 4,096 spatial candidates; 512 KiB tile payload cache |

The live example uses `VirtualSheetGrid` over the full 16,384-column address
space. Rows and columns are both mounted from their current viewports, and the
model responds to generation-scoped, row-major tile requests. No row vector,
column-definition vector, or cell matrix scales with the logical sheet size.

The CI probe also enforces generous regression ceilings: building the 100,000
shape spatial index must finish within 30 seconds and a viewport query within 2
seconds. Typical optimized runs are much faster, but the ceilings tolerate
shared CI machines while still catching an accidental linear full-scene render.

## Architecture for a product suite

Keep the document model logical and make the view a window into it:

- A sheet stores sparse edits and bounded row-major tiles, not 16,384 resident
  strings per row. `VirtualSheetGrid` de-duplicates generation-safe requests,
  caps its LRU and pending set, and mounts only intersecting rows and columns.
  Use a Kael worker for formula recalculation, indexing, or remote queries.
- A document stores immutable base blocks plus sparse edits. Mount pages with a
  virtual list, keep undo transactions bounded, and execute full-document search
  as bounded chunks through the existing worker bridge.
- A deck mounts only its thumbnail viewport and reuses one retained slide
  surface as selection changes.
- A whiteboard indexes retained shape bounds once, queries the viewport through
  `SpatialIndex`, invalidates moved bounds with `TileDamageTracker`, caches only
  visible tiles, and feeds `PointerInputEvent::stroke_samples()` into a bounded
  stroke pipeline. Drive simulations with `FixedFrameClock`, then request the
  next host animation frame.

This separation is what makes one codebase portable: native and browser hosts
provide windows, input, timing, GPU presentation, storage, workers, and file
boundaries while the application owns the same models and retained views.

## Exact current boundaries

- Browser full-document search and spreadsheet recalculation should run through
  Kael's typed worker bridge; the synchronous reference search API is chunked
  and capped, but it does not silently create a worker.
- Browser export cannot include cross-origin WebViews, protected surfaces, or
  other live hosted content. Export the retained scene before mounting such a
  surface, or export the hosted document through its own API.
- Browser pointer events include mouse, simultaneous touch, pen pressure/tilt,
  capture, cancellation, and bounded coalesced samples. Desktop compatibility
  mouse input uses the same event type; raw native touch/pen streams still
  depend on their platform backends. Browser pointer lock is exposed through
  the portable game-input API; synthesized pinch coverage remains partial.
- Browser secondary windows are independent retained canvases hosted inside
  the page, not operating-system windows. The smoke proves focus, presentation,
  and close cleanup for that browser model.
- Retained GPU presentation and device-pixel scene export have release coverage
  for WebGL 2 in browsers, Metal on macOS, Direct3D 11 on Windows, and the
  Blade/Vulkan X11 surface path on Linux. Linux hosted CI selects lavapipe, so
  that gate proves software-renderer correctness and liveness rather than
  hardware throughput or native-Wayland compositor integration.
- The browser owns IME candidate UI, native print settings, and security policy
  around cross-origin/network/file access. Kael exposes typed capability reports
  for these boundaries rather than claiming unavailable desktop behavior.

Run `bash scripts/verify-browser-suite-smoke.sh` before release to execute the
workload in real Chrome at both 1280 × 720 and 760 × 720, including
compressed select-all, bounded page cache, virtual page/thumbnail mounting,
retained whiteboard drawing, responsive offscreen mounting, and a synthetic pen
sequence through the browser pointer bridge. The same gate also proves:

- the primary and a focused secondary Kael window present independent,
  non-uniform retained pixels; closing the secondary surface restores focus to
  the primary and removes its browser host;
- `on_open_urls` receives the current URL across real `hashchange`,
  `history.back()`, and `history.forward()` transitions without a document
  reload;
- the presented WebGL frame exports as a nontrivial PNG before any hosted live
  surface is mounted; and
- the million-row, 16,384-column live grid mounts no more than 64 rows, 16
  columns, or 1,024 cells, and keeps the whole primary accessibility tree at or
  below 768 mounted semantic nodes. These are counts from the rendered browser
  accessibility tree, not only a model-side prediction.

The smoke registers `on_reopen`, but CI does not pretend to force a browser
back-forward-cache restore. Browser `pagehide`/suspension is not synthesized as
a native quit. Test BFCache restoration separately in products that depend on
that lifecycle detail.

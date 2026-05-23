# Kael Performance Review

Date: 2026-05-23

Scope: Workspace-wide performance review of the crates under `crates/`, with deeper attention to the `kael` rendering/windowing crate and the newly fixed `crispness_showcase` window-fill regression.

This is a code-reading review, not a profiler trace. Treat the priorities as an implementation roadmap, then validate each high-impact item with timings.

## Executive Summary

The `crispness_showcase` lag was a real rendering hot-path regression. The root cause was CPU rasterization of layout-sized shadows during resize/fill. That path has been fixed by sending box shadows through the existing GPU `Shadow` primitive.

The next highest performance risks are:

1. macOS renderer texture allocation on every resize event.
2. Blocking filesystem, SQLite, PDF, and media work hidden behind `async` APIs.
3. Full-document/content cloning in document undo/history APIs.
4. Cache implementations that scan or clone linearly on hot operations.
5. UI paint paths that add per-frame quads/work for idle scrollbars or blur surfaces.

The good news: the architecture already has several strong pieces: GPU primitive batching, demand-driven frame polling, list virtualization via `sum_tree`, text caches/pools, WAL-backed SQLite, and a benchmarking crate. The main work is to keep expensive work off the UI thread and make allocation/reallocation paths capacity-based.

## Highest Priority Findings

### P0 Fixed: CPU shadow rasterization during resize

Location: `crates/kael/src/window.rs`, `Window::paint_shadows`

The old path built `ShadowAtlasParams` and called `shadow_cache::rasterize_shadow` on atlas misses. Because params included scaled bounds, resizing a shadowed element created new atlas entries and rasterized large bitmaps synchronously.

Current state: `paint_shadows` emits `Shadow` primitives. This keeps resize cost out of the CPU bitmap path.

Keep this invariant: live, layout-sized box shadows must not be CPU-rasterized on the UI thread.

### P1: Resize still reallocates full-window Metal textures

Locations:

- `crates/kael/src/platform/mac/window.rs`, `set_frame_size`
- `crates/kael/src/platform/mac/metal_renderer.rs`, `update_drawable_size`

Every size event recreates path intermediate textures, cached surface textures, and blur textures. On large displays and high scale factors this can still cause jank even after the shadow fix.

Fix:

- Track current drawable size and return early for unchanged sizes.
- Use capacity semantics: grow immediately, avoid shrinking until resize settles.
- Allocate blur textures only when the scene has blur rects.
- Add counters for texture creation per fill/resize.

Acceptance target: one allocation burst for final size, not a burst for every intermediate resize event.

### P1: Several `async` APIs do blocking work inline

Crates affected:

- `kael_storage`: SQLite operations are `async` but lock and run `rusqlite` synchronously.
- `kael_document`: `open`, save, autosave, versions, and recent documents use blocking `std::fs`.
- `kael_pdf`: `open`, `save`, text extraction, page rendering, and sidecar IO are synchronous behind async APIs.
- `kael-media` / `kael_audio`: decode/probe/engine creation can run under mutexes and from user-facing calls.

Fix:

- Either make these APIs explicitly synchronous, or run the blocking work on a background executor/thread pool.
- Do not call these APIs from paint, layout, input dispatch, activation, or resize callbacks unless they are off-thread.
- Add docs that identify UI-thread-safe vs blocking APIs.

### P1: Document content history clones entire documents

Location: `crates/kael_document/src/document.rs`

`Document::Content` must be `Clone`, and `modify`, undo, redo, restore, save, and listener dispatch clone whole content values. This is acceptable for tiny content models but dangerous for rich docs.

Fix:

- Move from full snapshots to deltas/commands, persistent data structures, or `Arc`-backed immutable snapshots.
- Bound history by memory bytes, not only entry count.
- Defer autosave serialization so each keystroke/change does not synchronously serialize and write.

### P1: Disk cache scans the whole cache on put/stats/eviction

Locations:

- `crates/kael_cache/src/disk.rs`
- `crates/kael_cache/src/manager.rs`

`put` calls `total_size`, eviction walks all files, and stats calls `total_size`. This is fine for tiny caches but becomes O(number of cache files) on hot cache operations.

Fix:

- Maintain an index with size, modified/accessed time, namespace, and path.
- Update the index on `put`, `get`, `remove`, and eviction.
- Avoid calling `stats()` from hot UI paths if it walks disk.

### P1: Memory cache eviction is O(n)

Location: `crates/kael_cache/src/memory.rs`

`evict_one` scans all entries. Good enough for small caches, but it becomes noticeable with many entries or frequent churn.

Fix:

- Use an LRU queue/list plus priority buckets.
- Track approximate byte size, not only entry count.

## Crate-By-Crate Notes

### `kael`

This is the highest-impact crate.

Strengths:

- Demand-driven frame polling is restored in `Window::should_poll_for_frames`.
- Renderer batches primitives and uses GPU paths for shadows, paths, blur, quads, sprites, and surfaces.
- Text system has caching/pooling for font IDs, metrics, raster bounds, wrappers, and font runs.
- List virtualization uses `sum_tree`, which is a good fit for variable-height lists.

Risks and fixes:

- macOS `update_drawable_size` reallocates multiple full-window textures on every resize. Use size early-return and capacity allocation.
- `draw_blur_rects` does multiple passes per blur rect. Consolidate blur regions or cache static blurred surfaces where possible.
- `draw` calls `next_drawable`; if drawable acquisition blocks, this can show as frame hitching. Track wait time separately.
- `ensure_buffer_size` computes scene byte needs every draw. Keep it, but add telemetry for buffer growth and failed first render attempts.
- `bounds_changed` always calls `refresh`; during live resize this causes full layout/draw. Consider resize coalescing for interactive resize, while still rendering final size immediately.
- `dispatch_key_event` may force a draw if dirty before dispatch. This is correct for input targeting, but it means expensive invalidations can move into input latency. Instrument input-to-present latency.
- `paint_auto_scrollbars` paints thumbs for every tracked scroll container while scrollable. Hide/fade idle overlay scrollbars to avoid idle quads.
- Conservative snapping improves crispness but can increase overdraw. Keep it for clips/masks/paths; audit unnecessary use on simple fills.

### `kael_cache`

Strengths:

- Simple two-tier structure.
- Memory and disk stats exist.

Risks:

- Disk operations are synchronous.
- Disk size accounting walks the whole tree.
- Memory cache clones values on get.
- Memory cache eviction scans all entries.

Fix next:

- Add cache index and byte accounting.
- Provide async/background variants for disk cache operations.
- Return `Arc<[u8]>` or borrowed values where possible to reduce cloning.

### `kael_storage`

Strengths:

- SQLite uses WAL and `synchronous=NORMAL`.
- API is small and easy to reason about.

Risks:

- `async` methods run blocking SQLite calls under one `Mutex<Connection>`.
- Long queries block all other database work.
- `query_one` currently materializes all rows via `query` then checks length.
- JSON KV store clones the whole `BTreeMap` and rewrites the entire file on every set/remove.
- KV store notifies observers after persistence, which is good, but persistence is under the lock.

Fix next:

- Add a blocking DB executor or connection worker.
- Stream or limit queries where callers do not need full `Vec<T>`.
- Implement `query_one` directly with SQLite row APIs.
- For KV, update in-place state after writing a temp snapshot outside the lock, or move to SQLite for larger preference stores.

### `kael_document`

Strengths:

- Clear autosave/version/recent document boundaries.
- Atomic-ish temp file writes are used in several places.

Risks:

- Full content clone on each modify/undo/save/restore.
- Blocking file reads/writes in `async` methods.
- Autosave can serialize and write on each modify.
- Version metadata is read/written repeatedly and uses `Vec::remove(0)` in a loop.

Fix next:

- Use delta history or shared immutable snapshots.
- Debounce autosave and write on a background executor.
- Cache version metadata per document key.
- Use `VecDeque` or index arithmetic for version trimming.

### `kael_pdf`

Strengths:

- Document metadata/page descriptors are cached.
- Rendered page pixels are `Arc<[u8]>`.

Risks:

- PDF open/save/text extraction/rendering are synchronous behind async methods.
- `page_text` extracts text every time; `search` and `render` call it repeatedly.
- `render_page_preview` is a placeholder CPU raster path and loops over pixels for annotations.
- Page annotations clone on every access.

Fix next:

- Cache page text by page index.
- Cache rendered previews by `(page_index, scale, annotation_generation)`.
- Move render/text extraction to a background worker.
- Return shared annotation slices or `Arc<[PageAnnotation]>` for large annotation sets.

### `kael-media` and `kael_audio`

Strengths:

- Media sources are cloneable and use `Arc` for bytes.
- Video frame streaming exists, so callers are not forced to decode all frames.

Risks:

- `decode_video_frames` decodes the full video into memory.
- Bytes/readers may be staged to temp files for FFmpeg.
- Audio decode/probe and engine creation may happen under locks.
- `AudioPlayer` clones listener vectors and playback states on notifications.
- No periodic position updates are emitted, only operation-time updates.

Fix next:

- Mark `decode_video_frames` as debug/test utility or add a hard frame/memory limit.
- Add async/background decode APIs.
- Avoid doing decode/engine creation while holding player state locks.
- Add bounded decoded-audio cache with byte accounting.

### `kael_net`

Strengths:

- Simple request/response types.
- Retry policy is explicit.

Risks:

- `ApiResponse::text` clones the full body.
- Offline queue insertion/removal is O(n), acceptable only for small queues.
- Retry policy has no jitter, which can herd retries.
- `QueuedRequest.created_at` is currently `0`, which limits age-based cleanup.

Fix next:

- Add `text_ref` or `body_bytes` accessors to avoid clones.
- Add jitter to retry delays.
- Persist offline queue only if needed, with bounded body sizes.

### `http_client`

Strengths:

- Uses streaming `AsyncBody`.
- Trait design keeps transport swappable.

Risks:

- `HttpClientWithUrl::base_url` clones the base URL string on every URL build.
- Base URL is guarded by a mutex even though it changes rarely.

Fix next:

- Store base URL as `Arc<str>` under an `RwLock`, or parse/store `Url` once.
- Avoid formatting URLs repeatedly in hot request loops.

### `kael_i18n`

Strengths:

- Catalog and bundle lookups are simple hash-map reads.

Risks:

- `keys()` and `available_locales()` allocate/sort new vectors.
- Catalogs store owned `String` for all keys/values.

Fix next:

- Cache sorted locale/key lists if called often.
- Use `Arc<str>` if catalogs are shared widely.

### `kael_engines`

Strengths:

- Workload models are straightforward.

Risks:

- `TileCache` is unbounded by memory.
- Dashboard stats recompute min/max/mean/sum by scanning each call.
- Query scheduler stores jobs in a `Vec`, so lookup/removal will become linear.

Fix next:

- Add byte limits and LRU eviction to tile cache.
- Cache aggregate stats per series and invalidate on mutation.
- Use `HashMap` by job id plus a queue for scheduling order.

### `kael_release`

Risks:

- `parse_semver` is minimal and does not handle pre-release/build metadata.
- Update manifest validation only validates shape, not download behavior.

Fix next:

- Use `kael_semantic_version` or a full semver parser.
- Stream downloads and verify SHA-256 while streaming, never after loading full artifacts into memory.

### `kael_notifications` and `kael_share`

Risks:

- Linux notifications spawn a thread for fallback behavior.
- Share/open operations spawn platform commands and should never run from paint/layout/input hot paths.

Fix next:

- Route through a platform service executor.
- Add timeout/error telemetry for shell-command platform operations.

### `sum_tree`

Strengths:

- This is a good core data structure for virtualized text/list workloads.
- `Arc` tree structure supports cheap cloning and structural sharing.

Risks:

- Parallel/rayon paths should be benchmarked against small collections; rayon overhead can dominate.
- Tree base is small in tests and moderate in production; good, but benchmark with actual list sizes.

Fix next:

- Keep as-is unless profiler points here.
- Add benchmark cases matching Kael list workloads.

### `collections`, `semantic_version`, `refineable`, `derive_refineable`, `kael-macros`, `util_macros`, `media`

Risk level is low for runtime app performance.

Notes:

- Proc-macro crates affect build time, not runtime.
- `collections` aliases use fast hashers, which is good for internal maps but should not be used for untrusted hash-floodable inputs.
- `semantic_version` is small and fine, but release/update code should not maintain its own parser.
- `media` is a macOS sys binding crate; runtime performance depends on callers.

## Cross-Cutting Recommendations

### Add Performance Instrumentation First

Add counters/timers around:

- Window fill latency.
- `set_frame_size`.
- `update_drawable_size`.
- Metal texture creation count.
- `bounds_changed`.
- `draw_roots`.
- `Scene::finish`.
- `MetalRenderer::draw`.
- `next_drawable` wait time.
- Text layout/raster cache misses.
- Disk/cache/document/PDF blocking operations.

### Define Threading Rules

Document three classes of API:

- UI-thread safe: cheap, non-blocking, no disk/network/decode.
- Background required: filesystem, SQLite, PDF, media decode, cache eviction.
- Render-thread only: GPU/renderer resource mutation.

Then audit calls from input, resize, layout, paint, and activation.

### Build Benchmarks That Match Real Apps

Add benchmarks/examples for:

- Shadow-heavy resize.
- Blur-heavy resize.
- Many nested scroll containers.
- Large variable-height list.
- Text-heavy document viewport.
- Large JSON KV preference file.
- PDF page text search/render.
- Document edit/undo with large content.

## Prioritized Fix Backlog

1. Mac renderer resize allocation: early return, capacity textures, lazy blur textures.
2. Add resize/fill instrumentation and benchmark.
3. Move blocking storage/document/PDF/media work off UI thread.
4. Replace document full-content undo with delta or shared snapshots.
5. Add disk cache index and memory cache byte-aware LRU.
6. Cache PDF page text and rendered previews.
7. Hide/fade idle auto scrollbars.
8. Add retry jitter and response body no-clone accessors in networking.
9. Add tile cache memory limits and dashboard aggregate caching.
10. Remove or clearly quarantine old CPU shadow cache if no backend needs it.

## Acceptance Targets

- `crispness_showcase` window fill: under 150 ms in debug, under 50 ms in release.
- Idle active window: no continuous display-link callbacks after the input grace period.
- Resize: no CPU work proportional to shadow bitmap area.
- Resize: texture creation count is bounded and visible in telemetry.
- Storage/document/PDF/media: no blocking operations on paint/layout/input/resize paths.
- Large document edit: memory growth bounded by configured undo byte budget.
- PDF page render/search: repeated calls hit cache.


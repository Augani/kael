# Kael-maintained rasterlottie fork

This source derives from published `rasterlottie` **0.2.1**, upstream commit
[c2cf0560a320776bed01d0942914f1fba258285f](https://github.com/neodyland/rasterlottie/tree/c2cf0560a320776bed01d0942914f1fba258285f).
Kael maintains the separate package **`kael_rasterlottie` 0.4.1**, with the Rust
library name `rasterlottie`. Existing entrypoints and features remain available;
the fork adds archive and image admission policies with finite defaults and
exact cached-image payload statistics.
Original README, copyright notices, MIT/Apache-2.0 licenses, corpus/golden tests,
and fixtures are preserved. Nine upstream Rust files have reviewed changes.

## Distribution and ZIP dependency

The optional `dotlottie` feature uses ZIP 9, default features disabled, with
`deflate` enabled. The loader reads entries in memory and uses no filesystem
extraction, permission, or symlink APIs. Duplicate names and inconsistent local
and central headers are rejected through ZIP 9's per-read consistency checks.
Stored/deflated, ZIP64, and archive-comment compatibility are tested.

The maintained manifest identifies the fork and explicitly copies the Kael
workspace lint policy. Original-policy checks on the historical ZIP-only spike
produced the same 169 library diagnostics as upstream, including item ordering
and pre-existing unwraps. That historical comparison does not claim the
upstream aggressive style rules passed. Source changes here implement admission;
they do not reorder the rasterizer to satisfy those rules.

Upstream examples and benchmark tools remain in the checkout and are omitted
from publication. Source tests, corpus, golden references, licenses, provenance,
and reconstruction patch are packaged. Consumers depend directly on
`package = "kael_rasterlottie"` with the `rasterlottie` alias, version, and path.
Registry publication must upload this named fork before Kael. Preparing the fork
does not publish it; complete release/consumer archive verification is separate.

## Admission policies

`Animation::from_dotlottie_bytes` now uses `DotLottieLimits::default()`.
`Animation::from_dotlottie_bytes_with_limits` accepts an explicit policy. Byte
limits are inclusive; zero rejects nonempty data.

| Archive resource | Default admission limit |
| --- | --- |
| Compressed input before ZIP construction | 16 MiB |
| Entry count after metadata construction | 4096 |
| Expanded manifest | 1 MiB |
| Expanded selected animation JSON | 16 MiB |
| Expanded encoded image per embedding | 16 MiB |
| Aggregate bytes materialized from selected entries | 64 MiB |

Declared oversize entries fail before payload reads or output reservations.
Accepted entries receive one fallible reservation for their bounded declared
size; the reader still enforces actual bytes and makes a final nonempty EOF read
for CRC validation. Repeated embeddings of the same path are charged again.
Missing optional images keep the upstream behavior; an oversize, corrupt, or
inconsistent required/read entry returns an error. Unselected archive payloads
are not decompressed. All image assets embedded from the selected animation
count, including assets whose pixels are never rendered.

`Renderer::new`, `Renderer::default`, and `Renderer::target_corpus` use
`ImageLimits::default()`. `Renderer::with_image_limits` chooses a different policy
that is retained by prepared animations and their worker resources.

| Image resource | Default admission limit |
| --- | --- |
| Source and declared target width/height | 8192 each |
| Encoded bytes per image | 16 MiB |
| Aggregate resolver-returned encoded bytes per preparation | 64 MiB |
| Native decoder output per image | 256 MiB |
| RGBA8 source or target per image | 256 MiB each |
| Decode/conversion/resize working buffers per image | 512 MiB |
| Decoded target buffers retained per prepared animation | 256 MiB |

Admission checks declared target dimensions, encoded size, actual source header,
native decoder byte count, RGBA conversion/target sizes, and aggregate retained
pixels before pixel decoding or resize. Working accounting includes the actual
Triangle RGBA32F intermediate (`source width * target height * 16`) and
conservatively rounded sampling-weight capacities, in addition to native decode,
RGBA conversion, and target buffers. All arithmetic is checked, and buffers must
fit `isize::MAX` even when callers raise limits on 32-bit WebAssembly.

Decoded-byte reservations roll back when decoding fails. Cache hits reuse the
same image and consume no extra budget. Unused embedded malformed image data
remains lazy. Later frames that first use an over-budget asset receive an error;
earlier cached images remain available. Resolver-returned bytes are checked per
callback and in aggregate before retention or worker copying, with repeated
callback results charged again.

These are admission bounds for requested content and buffers, not allocator,
process RSS, GPU, or global application limits. Important remaining boundaries:

- The compressed-source check precedes ZIP construction, but the entry-count
  check follows metadata parsing. It does not cap constructor metadata
  allocations or ZIP 9's search/retry of earlier end-of-directory candidates.
- Base64 strings can occupy about four thirds of archive image bytes. Parsed
  model allocations and inline image strings are additional; general JSON
  parsing outside the archive entrypoint retains upstream behavior.
- Decoder allocation limits are best effort. Decoder-private storage and
  allocator overhead are additional to the requested-buffer estimate.
- Resolver callbacks allocate before returning. Admission bounds accepted bytes,
  not the callback's own allocations.
- GIF workers clone admitted external encoded sources and have independent
  decoded budgets. Concurrent workers and prepared animations multiply memory;
  a host still needs its own global budget. Kael currently uses the finite
  defaults; this change adds no custom Kael-level image policy API.

### Host cache accounting

`PreparedAnimation::cached_decoded_image_bytes()` exposes the committed target
RGBA8 **payload** counter without allocating or decoding assets. It is zero
before the first image use, unchanged on cache hits and failed decode admission,
and grows when later frames decode another asset. Without the `images` feature
it returns zero. It does not report allocator capacities, encoded strings,
parsed models, shape/timeline/path caches, transient scratch, or total RSS.

Kael uses this counter after rendering to admit idle worker preparations. Its
local LRU retains at most four preparations and 64 MiB of decoded image payloads
per thread. Metadata-only process-wide reservations bound all idle caches to
16 preparations and 256 MiB of those payloads. Preparations remain on their
creating thread; weak source identities and normal LRU retirement are preserved.
Checkout releases the idle reservation, and reinsertion measures lazy image
growth again. Retired resources are destroyed outside cache/budget locks before
their quota is released. A valid preparation that is too large or cannot obtain
quota still renders and then bypasses caching.

These host limits concern **idle cached image payloads and idle entry counts**.
Active preparations, GIF workers, parsed models and other caches, decoder/render
scratch, posters, ready/output frames and allocator overhead remain additional.
They are not a total Lottie, process RSS, or GPU memory limit. The standalone
fork still has no host-global budget of its own.

### Completed frame ownership

The completed main output pixmap transfers its premultiplied RGBA vector into
`RasterFrame` through `Pixmap::take`, rather than allocating and copying another
full canvas before dropping the first. The public frame and borrowed scratch
APIs keep their existing pixel representation and behavior. This removes a
transient duplicate output buffer; layer/mask/effect scratch, images, parsed
models, retained frames and concurrent calls remain additional. It is not a
new global memory or RSS limit.

The native-only `output_allocation` integration test exercises the public
prepared-frame renderer with a real 16-by-8 animation and transparent, opaque
and premultiplied backgrounds. A small thread-local allocation watch checks one
512-byte output allocation, exact pixels, and balanced release after dropping
the frame. Preparation, warmup and assertions are outside the watch, which is
also disabled during unwinding. The counter measures requested Rust allocation
payloads, not allocator overhead or RSS; portable golden tests independently
cover pixels without this native allocator watch.

## Provenance and exact reconstruction

The pinned registry archive is
[`rasterlottie-0.2.1.crate`](https://static.crates.io/crates/rasterlottie/rasterlottie-0.2.1.crate),
SHA-256 `384686130a2b365f82258f5e444c512e57989addcf2b45e9913c4ef6f0e95a24`.
The pristine 74-file `UPSTREAM-SHA256.json` identifies every original byte.
`zip9-compatibility.patch` contains all source, manifest, test, and historical
lockfile changes against that archive. Apply it with `patch -p1 -E` in the
extracted package directory. Copy the pristine `Cargo.toml.orig` to
`UPSTREAM-Cargo.toml`, and copy this record, the hash index, and patch alongside
it. The CI verifier performs extraction and reconstruction with checked paths,
finite file/byte bounds, and zero patch fuzz.

The exact intentionally modified original files are:

- `Cargo.toml`: named fork, ZIP 9, target-only browser test dependency, publication
  list, and explicit Kael project lint policy.
- `Cargo.lock`: renamed without byte changes to `UPSTREAM-Cargo.lock`.
- `src/dotlottie.rs`: archive limits, bounded reads and reservations, consistency
  checks, repeated-entry accounting, and focused reader regressions.
- `src/lib.rs`: public policy exports.
- `src/model.rs`: additive archive-policy entrypoint and API documentation.
- `src/render/assets.rs`: lazy image admission, retained reservations, working
  accounting, resolver guards, worker policy propagation, cached-payload counter
  access, and regressions.
- `src/render/mod.rs` and `src/render/renderer/mod.rs`: policy exports.
- `src/render/renderer/prepare.rs`: renderer policy selection and retention,
  additive prepared-image payload statistics and no-images counter regression.
- `src/render/renderer_types.rs`: image policy types/defaults and renderer field.
- `src/render/renderer/raster.rs`: transfer the completed pixmap vector into
  `RasterFrame` without a duplicate output allocation.

All other original source, standalone tests, fixtures, licenses, and metadata
must retain their pinned hashes. Added content consists of the original
manifest/lock copies, `tests/output_allocation.rs`, `tests/zip9_compatibility.rs`,
and the latter's glyph fixture copied from upstream's
`renderer_draws_glyph_text_layers`, plus the three provenance files.
The verifier requires both the observed modified-original set and additions to
match their explicit reviewed lists, then compares every reconstructed file
hash with the checkout. Changing another original file or adding another file
requires a separately reviewed update to those lists.

Cargo reserves `Cargo.toml.orig` during archive construction. The checkout keeps
the upstream original unchanged, publishes its identical `UPSTREAM-Cargo.toml`
copy, and lets Cargo generate the named fork's own manifest companion.
`UPSTREAM-Cargo.lock` is excluded from publication and is historical evidence;
the containing workspace resolves ZIP 9 and generates the release lockfile.

## Validation

The prior source-frozen admission checkpoint passed 116 unit tests, one corpus
test, one golden test, 14 native integration tests, and two doctests; its actual
Chrome WebAssembly run passed 15 integration tests, including safe rejection of
a 2.4 GB target under a raised 32-bit policy. Its evidence remains historical.
The cached-image counter extension adds a no-images unit regression and a
portable lazy-decode/hit/growth/failed-admission integration regression. Kael's
host cache tests separately exercise byte/entry LRU admission, shared quota,
weak identity, cancellation, errors, unwinding, reentrancy and thread teardown
with tiny injected budgets. These validate requested-buffer/cache admission and
compatibility, not hard RSS ceilings, every decoder format, or additional Lottie
feature support.

The earlier dependency-only
[checkpoint d40166f](https://github.com/Augani/kael/commit/d40166fb132dd2c24cd93c44017b919daf21339f)
and its frozen evidence retain their original ZIP-only scope. Their
all-implementation-unchanged and 119421-byte isolated-package facts are
historical and are not reused as current admission-source/package claims.

```sh
python3 scripts/ci/verify-rasterlottie-fork.py --archive /path/to/rasterlottie-0.2.1.crate
cargo test --locked -p kael_rasterlottie --all-features
cargo clippy --locked -p kael_rasterlottie --all-features --all-targets -- -D warnings
# Configure ChromeDriver and a matching wasm-bindgen-test-runner first:
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test --locked -p kael_rasterlottie --no-default-features \
  --features dotlottie,images,text --target wasm32-unknown-unknown \
  --test zip9_compatibility
```

# Kael-maintained rasterlottie fork

This is the published `rasterlottie` **0.2.1** source from
[neodyland/rasterlottie commit c2cf0560a320776bed01d0942914f1fba258285f](https://github.com/neodyland/rasterlottie/tree/c2cf0560a320776bed01d0942914f1fba258285f),
distributed as the separate package **`kael_rasterlottie` 0.4.1** with the Rust
library name `rasterlottie`. This is a maintained fork, not an upstream release.
The public Rust API, features, implementation, original upstream tests and
README, copyright notices, and MIT/Apache-2.0 license texts are unchanged.

## Changes and distribution

The optional `dotlottie` feature uses **ZIP 9** with default features disabled and
`deflate` enabled, replacing the upstream ZIP 8 dependency. No extraction API,
file permissions, symlinks, or filesystem writes are used by the loader. ZIP 9
compatibility needs no Rust implementation change. This upgrades the actual ZIP
dependency; it does not suppress an advisory or rename the vulnerable dependency.

The package name/version, description, project links, and publication file list
identify the maintained fork. The maintained manifest explicitly adopts the
same lint policy as the Kael workspace; the original upstream manifest remains
unchanged in provenance. A strict original-policy comparison produced the same
169 library diagnostics in both the original and the ZIP-only candidate,
including item ordering and pre-existing unwraps. The fork does not claim those
upstream style rules passed, and it does not rewrite the rendering implementation
to satisfy them. Upstream examples/benchmark tools remain in the
checkout and are omitted from the release archive. Source tests, corpus fixtures,
and golden references remain included. Consumers use the real named dependency
with `package = "kael_rasterlottie"`, the library alias `rasterlottie`, and a
version; they do not rely on a root-only `[patch.crates-io]` override. The fork
must be published before Kael. Publishing is a separate release step; preparing
this source does not publish it to a registry.

## Provenance and reconstruction

The source archive is
[`rasterlottie-0.2.1.crate`](https://static.crates.io/crates/rasterlottie/rasterlottie-0.2.1.crate),
SHA-256 `384686130a2b365f82258f5e444c512e57989addcf2b45e9913c4ef6f0e95a24`.
`UPSTREAM-SHA256.json` records every original registry file hash.
`zip9-compatibility.patch` contains every source, manifest, test, and lockfile
change. To reconstruct:
verify the archive checksum, extract it, and run `patch -p1 -E <
/path/to/zip9-compatibility.patch` from the extracted package directory. Copy this
record, the patch, and `UPSTREAM-SHA256.json` alongside the sources, and copy the
pristine `Cargo.toml.orig` to `UPSTREAM-Cargo.toml`. Original implementation,
upstream fixture, and license hashes must still match
the upstream hash list. The new glyph fixture is copied from the original
`renderer_draws_glyph_text_layers` test; the four new integration tests add ZIP
container coverage without changing the rendering contract.

Cargo reserves `Cargo.toml.orig` when constructing an archive. The checkout keeps
that original file unchanged; the explicit publication list omits it and ships
its identical copy as `UPSTREAM-Cargo.toml`. Cargo generates the fork's own
normalized-manifest companion. The original nested `Cargo.lock` is renamed to
`UPSTREAM-Cargo.lock` and
excluded from the release archive. It records historical dependencies only; the
containing workspace resolves ZIP 9 and generates the published fork lockfile.

## Compatibility evidence and limits

The ZIP-only 0.2.1 spike passes 98 upstream unit tests, one corpus test, one golden
image test, and two doctests on native Rust. The named fork additionally passes
108 upstream unit tests with all features enabled, the corpus/golden/doctests,
and the four compatibility tests. The tracked `zip9_compatibility` target and
the original independent spike harness exercise four tests on each platform,
without ignored tests: stored and deflated
manifest selection, exact embedded-PNG output, embedded glyph-text foreground
and background pixels, and malformed/truncated archive rejection. Browser
execution uses the real WebAssembly package in headless Chrome. These checks
establish loader/render compatibility; they do not make unsupported Lottie
features supported or prove general archive memory admission.

Archive-entry decompression and image decoding retain upstream behavior. The
loader's `read_to_end` has no per-entry or aggregate decompressed-byte cap.
A host's compressed-source limit does not provide such a cap. Bounded archive
admission is a separate required follow-up, not part of this dependency change.

The fork keeps these checks runnable without an artifact-only harness:

```sh
cargo test --locked -p kael_rasterlottie --no-default-features --features dotlottie,images,text
# With ChromeDriver configured and wasm-bindgen-test-runner installed:
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test --locked -p kael_rasterlottie --no-default-features \
  --features dotlottie,images,text --target wasm32-unknown-unknown \
  --test zip9_compatibility
```

The browser test dependency is target-conditional and is not a runtime
dependency of native Kael or the rasterizer library.

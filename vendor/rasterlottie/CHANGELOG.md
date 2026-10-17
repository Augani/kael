# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and the project follows Semantic Versioning.

## [0.2.1]

### Added

- Regression coverage for parallel GIF encoding parity, cropped GIF frame visual parity, and partial-frame output behavior.
- Validation coverage for non-default feature combinations so optional `dotlottie`, `gif`, `images`, `text`, and `tracing` builds stay warning-free.

### Changed

- Accelerated GIF export by parallelizing frame rendering and palette quantization work across worker threads.
- Reduced GIF encoding work and output size by emitting cropped subframes when only part of the canvas changes.

### Fixed

- Preserved GIF disposal behavior when cropped frames need transparent clears or can safely keep static pixels.
- Removed feature-gated unused warnings and a `text`-only test build regression that appeared in non-default feature combinations.

## [0.2.0]

### Added

- Optional `dotlottie` feature for loading `.lottie` archives from packaged ZIP containers.
- `Animation::from_dotlottie_bytes` for parsing packaged archives through the public API.
- Manifest-aware animation selection for `.lottie` archives, including explicit initial animation IDs and fallback to the first declared animation.
- Embedded archive image extraction for `.lottie` inputs when the `images` feature is enabled.
- `.lottie` input support in the `rasterlottie_cli` and `benchmark_render` examples.

### Changed

- Expanded the README usage examples to document `.lottie` workflows and feature-gated loading.
- Added regression coverage for `.lottie` parsing and packaged image loading.

### Fixed

- Corrected GIF frame timing quantization so requested output FPS is preserved more accurately within GIF centisecond delay limits.
- Added regression coverage for non-integer centisecond GIF frame delays.

## [0.1.1]

### Added

- `#[cfg_attr(docsrs, feature(doc_cfg))]` attribute for documentation generation.
- 3 Badges for crates.io, docs.rs, and GitHub Actions status.
- GitHub Actions workflows for CI verification and crates.io publishing.
- This changelog.

### Changed

- Restricted publishing to crates.io in package metadata.
- Expanded packaging and release metadata for crates.io publication.

## [0.1.0]

### Added

- Initial public release of `rasterlottie`.
- Deterministic support analysis for the current target corpus.
- Headless RGBA frame rendering backed by `tiny-skia`.
- GIF export for supported animations.
- Optional image asset support for embedded and resolver-backed external images.
- Optional glyph-backed text rendering support.
- Corpus, fixture, and image-diff regression coverage for the supported subset.

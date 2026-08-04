# `kael_util`

Shared, low-level utilities used by Kael's core crates. The default build contains the helpers
needed by the native application runtime without pulling optional archive or schema tooling.

Optional features:

- `archive` enables bounded, path-safe ZIP extraction.
- `schema` enables JSON Schema transformation helpers.
- `test-support` exposes Kael's fixture and assertion utilities.

This crate is part of the [Kael](https://github.com/Augani/kael) native application framework.
Application authors should normally depend on `kael`; this crate exists as a focused shared
dependency for Kael's published crate graph.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).

# kael_collections

Standard collection type re-exports for Kael. The hash-map and hash-set aliases
use the fast, deterministic Fx hasher and are intended for trusted in-process
keys. Use the standard library's randomized collections for attacker-controlled
keys that require hash-flood resistance.

Part of the [Kael](https://github.com/Augani/kael) GPU-accelerated Rust UI framework. See the [documentation](https://augani.github.io/kael/) for usage and guides.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).

# `kael_sum_tree`

A persistent B+ tree for ordered data with application-defined summaries. Kael uses it for
large collections that need cheap clones, incremental updates, and fast seeks across dimensions
such as bytes, characters, rows, or time.

## Quick start

```rust
use kael_sum_tree::TreeMap;

let mut priorities = TreeMap::default();
priorities.extend([("editor", 1), ("terminal", 2), ("editor", 3)]);

assert_eq!(priorities.get(&"editor"), Some(&3));
assert_eq!(priorities.iter().count(), 2);
```

The normal dependency surface is deliberately small. Enable the optional `parallel` feature only
when constructing or extending very large trees benefits from Rayon:

```toml
[dependencies]
kael_sum_tree = { version = "0.4", features = ["parallel"] }
```

This crate is part of the [Kael](https://github.com/Augani/kael) native application framework.
See its [API documentation](https://docs.rs/kael_sum_tree) for the tree, cursor, map, and set APIs.

## License

Licensed under the Apache License, Version 2.0. See the included
[LICENSE-APACHE](LICENSE-APACHE).

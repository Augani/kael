# kael_collections

Fast, deterministic collection aliases used throughout Kael. The crate provides
Fx-hashed versions of `HashMap`, `HashSet`, `IndexMap`, and `IndexSet`, alongside
the standard collection re-exports expected by framework internals.

Use these aliases for trusted, in-process keys where predictable hashing and low
overhead matter. Use the standard library's randomized `HashMap` and `HashSet`
for attacker-controlled keys that require hash-flood resistance.

```rust
use kael_collections::{HashMap, IndexMap};

let mut lookup = HashMap::default();
lookup.insert("workspace", 1);

let mut ordered = IndexMap::default();
ordered.insert("first", 1);
ordered.insert("second", 2);

assert_eq!(lookup["workspace"], 1);
assert_eq!(ordered.keys().copied().collect::<Vec<_>>(), ["first", "second"]);
```

See the [Kael guide](https://augani.github.io/kael/) for framework-level usage.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/Augani/kael/blob/main/LICENSE-APACHE).

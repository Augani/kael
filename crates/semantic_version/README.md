# kael_semantic_version

Strict parsing, ordering, display, and Serde support for stable semantic-version
core triplets (`major.minor.patch`) used by Kael's platform and updater APIs.
Pre-release and build metadata are intentionally rejected rather than silently
discarded.

```rust
use kael_semantic_version::SemanticVersion;

let current: SemanticVersion = "1.4.2".parse()?;
let available = SemanticVersion::new(2, 0, 0);

assert!(available > current);
assert_eq!(current.major(), 1);
assert_eq!(current.to_string(), "1.4.2");
# Ok::<(), kael_semantic_version::ParseSemanticVersionError>(())
```

Each component is a `u64`, so accepted versions and their ordering are stable
across target architectures. Parse failures use `ParseSemanticVersionError`
rather than a framework-wide error type. Serialized values use the familiar
string form.

This crate deliberately models only stable release triplets. Applications that
need the complete SemVer grammar, including pre-release precedence and build
metadata, should use a full SemVer implementation.

See the [Kael guide](https://augani.github.io/kael/) for framework-level usage.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).

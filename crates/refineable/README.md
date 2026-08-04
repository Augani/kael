# kael_refineable

Typed partial updates and ordered configuration cascades for Kael.

`#[derive(Refineable)]` generates a companion refinement type whose fields are
optional overrides. [`Cascade`](https://docs.rs/kael_refineable/latest/kael_refineable/struct.Cascade.html)
combines refinements in priority order, which is useful for themes, settings,
and other layered application configuration.

```rust
use kael_refineable::Refineable;

#[derive(Clone, Default, Refineable)]
struct Preferences {
    accent: String,
    compact: bool,
}

let preferences = Preferences::default().refined(PreferencesRefinement {
    accent: Some("violet".to_owned()),
    compact: Some(true),
});

assert_eq!(preferences.accent, "violet");
assert!(preferences.compact);
```

See the [Kael guide](https://augani.github.io/kael/) for framework-level usage.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/Augani/kael/blob/main/LICENSE-APACHE).

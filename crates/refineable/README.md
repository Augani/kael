# `kael_refineable`

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

Regular fields become `Option<T>` in the generated refinement. Existing
`Option<T>` fields stay `Option<T>`: `Some(value)` replaces the value and `None`
means "leave it unchanged," so an optional-field refinement does not represent
clearing a value. Mark a nested refineable value with `#[refineable]` to use its
companion refinement recursively.

`#[refineable(Debug, Serialize, Deserialize)]` adds traits to the generated
type. Serialize omits empty fields, while Deserialize defaults missing nested
refinements. Qualified paths such as `serde::Serialize` are also supported.

## Ordered cascades

`Cascade` combines a base refinement with later, higher-priority slots. Slot
handles are bound to the cascade that created them, and using a foreign slot
returns `InvalidCascadeSlot` instead of mutating the wrong cascade.

```rust
use kael_refineable::{Cascade, Refineable};

#[derive(Clone, Default, Refineable)]
struct Theme {
    accent: String,
}

let mut cascade = Cascade::<Theme>::default();
cascade.base().accent = Some("violet".to_owned());
let override_slot = cascade.reserve();
cascade
    .set(
        override_slot,
        Some(ThemeRefinement {
            accent: Some("amber".to_owned()),
        }),
    )
    .unwrap();

assert_eq!(Theme::from_cascade(&cascade).accent, "amber");
```

See the [Kael guide](https://augani.github.io/kael/) for framework-level usage.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/Augani/kael/blob/main/LICENSE-APACHE).

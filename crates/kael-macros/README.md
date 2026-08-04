# kael-macros

Procedural macros that power Kael's action, element, rendering, context, style,
and deterministic test APIs.

Applications normally consume these macros through the `kael` crate rather
than depending on `kael-macros` directly. Public entry points include:

- `#[derive(Action)]`, `#[derive(IntoElement)]`, `#[derive(AppContext)]`, and
  `#[derive(VisualContext)]`;
- `#[kael::test]` for deterministic framework tests;
- internal style-generation macros used to keep Kael's builder API consistent.

See the [Kael guide](https://augani.github.io/kael/) and
[`kael` API documentation](https://docs.rs/kael) for application-facing usage.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/Augani/kael/blob/main/LICENSE-APACHE).

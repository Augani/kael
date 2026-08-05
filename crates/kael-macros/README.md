# `kael-macros`

Procedural macros behind Kael's action, element, context, style, inspection,
and deterministic-test APIs.

Applications should normally depend on [`kael`](https://docs.rs/kael), which
re-exports the application-facing macros with the runtime traits and types they
generate code against. Direct use of this crate is mainly useful for framework
integrators.

The application-facing entry points are:

- `#[derive(Action)]`, `#[derive(IntoElement)]`, `#[derive(AppContext)]`, and
  `#[derive(VisualContext)]`;
- `#[kael::test]` for deterministic framework tests;
- `register_action!` for manually implemented action types.

`Action` names are application protocol keys. The derive rejects empty,
oversized, control-character, and whitespace-padded names, as well as duplicate
deprecated aliases. Context derives require exactly one argument-free `#[app]`
or `#[window]` marker on a named field. Deterministic tests require at least one
iteration.

The remaining style and inspector macros generate Kael's own builder and
reflection APIs. Most applications use the generated methods through
`kael::Styled` instead of invoking those macros directly.

See the [Kael guide](https://augani.github.io/kael/) and
[`kael-macros` API documentation](https://docs.rs/kael-macros) for the complete
macro reference.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/Augani/kael/blob/main/LICENSE-APACHE).

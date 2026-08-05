# `kael_derive_refineable`

Implementation crate for `#[derive(Refineable)]`.

Application and library code should depend on
[`kael_refineable`](https://docs.rs/kael_refineable), which re-exports this
derive macro alongside the runtime traits and cascade types it generates code
against. The separate crate keeps token-generation code isolated from the small
runtime trait and cascade API.

The macro supports named structs, nested `#[refineable]` fields, generic types,
qualified standard `Option` paths, and additional derives declared with
`#[refineable(...)]`. Unsupported tuple structs, associated types, and optional
nested refinements produce compile diagnostics at the field or derive site.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/Augani/kael/blob/main/LICENSE-APACHE).

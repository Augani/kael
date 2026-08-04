# kael_derive_refineable

Implementation crate for `#[derive(Refineable)]`.

Application and library code should depend on
[`kael_refineable`](https://docs.rs/kael_refineable), which re-exports this
derive macro alongside the runtime traits and cascade types it generates code
against. Keeping the implementation separate avoids a procedural-macro
dependency in the runtime crate itself.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/Augani/kael/blob/main/LICENSE-APACHE).

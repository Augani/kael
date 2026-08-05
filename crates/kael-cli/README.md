# `kael-cli`

The project scaffolder for the
[Kael native application framework](https://github.com/Augani/kael).

```bash
cargo install kael-cli
kael new my_app
cd my_app
cargo run
```

The generated project uses the matching `kael` and `kael_ui` release and can
be created offline after the CLI is installed. Its `rust-toolchain.toml` and
`rust-version` use the same stable Rust release as that Kael release, so local
development and CI agree automatically.

On macOS, generated apps enable runtime Metal shader compilation so `cargo run`
works with either Command Line Tools or full Xcode. Release builds can remove
that feature to precompile shaders when full Xcode is available.

`kael new` reserves the destination atomically and never overwrites an existing
path. Project names follow Cargo and crates.io rules: 1–64 ASCII characters,
starting with a letter or underscore, with Rust keywords and cross-platform
reserved names rejected up front.

The generated app starts with Kael primitives, `kael_ui` components, a branded
theme entry point, and a fallible native window startup path. See the
[Kael guide](https://augani.github.io/kael/) for architecture and component
usage, or the [`kael-cli` documentation](https://docs.rs/kael-cli) for the
embedded scaffolder contract.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/Augani/kael/blob/main/LICENSE-APACHE).

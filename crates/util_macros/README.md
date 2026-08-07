# `kael_util_macros`

Small procedural macros shared by Kael's tests and performance tooling:

- `path!`, `uri!`, and `line_endings!` produce target-correct literals without
  runtime platform branches.
- `perf` marks performance-sensitive tests. Enabling the `perf-enabled` feature
  adds the metadata consumed by `kael_perf`; without it, the macro remains a
  normal test attribute.

Most application code does not need to depend on this crate directly. It is a
support crate for the Kael workspace and its test infrastructure.

`path!` and `uri!` are fixture helpers, not general path or URL normalizers.
They map Unix-style test literals to a `C:` drive on Windows while preserving
explicit drive letters and UNC paths. `line_endings!` normalizes mixed LF/CRLF
fixtures before producing the target-specific literal.

`#[perf]` accepts at most one importance level plus optional positive
`iterations: usize` and `weight: u8` expressions. With `perf-enabled`, generated
benchmark tests ignore zero or malformed iteration environment values, emit the
versioned `kael_perf` metadata protocol, and keep conditional-compilation
attributes paired with their metadata tests.

See the [Kael guide](https://augani.github.io/kael/) for framework-level usage.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/Augani/kael/blob/main/LICENSE-APACHE).

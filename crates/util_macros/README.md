# kael_util_macros

Small procedural macros shared by Kael's tests and performance tooling:

- `path!`, `uri!`, and `line_endings!` produce target-correct literals without
  runtime platform branches.
- `perf` marks performance-sensitive tests. Enabling the `perf-enabled` feature
  adds the metadata consumed by `kael_perf`; without it, the macro remains a
  normal test attribute.

Most application code does not need to depend on this crate directly. It is a
support crate for the Kael workspace and its test infrastructure.

See the [Kael guide](https://augani.github.io/kael/) for framework-level usage.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/Augani/kael/blob/main/LICENSE-APACHE).

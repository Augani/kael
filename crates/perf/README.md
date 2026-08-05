# `kael_perf`

Versioned benchmark data and a small command-line profiler for Kael performance
tests. `kael_util_macros::perf` emits the matching test and metadata functions
when its `perf-enabled` feature is enabled.

Application binaries do not need this crate. It is published because Kael's
optional performance macros use its protocol types, and because framework and
application maintainers can use the profiler to catch regressions in their own
performance-sensitive tests.

## Profiling

Install [Hyperfine](https://github.com/sharkdp/hyperfine), build a test binary
with `kael_util_macros/perf-enabled`, then pass that binary to the profiler:

```text
kael_perf path/to/test-binary --important
kael_perf path/to/test-binary --json=current
```

The importance flags are `--critical`, `--important`, `--average`, `--iffy`, and
`--fluff`. `--quiet` suppresses progress output. A JSON run identifier may use
ASCII letters, digits, `-`, and `_`; saved runs are bounded and written beneath
the invocation directory's `.perf-runs` folder. Run the command from your
workspace root so profile and comparison commands share the same data.

Compare a new run against a baseline with:

```text
kael_perf compare current baseline
kael_perf compare --save=report.md current baseline
```

Positive arrows mean the new run has higher throughput. Only tests present in
both runs and in the same importance category contribute to a comparison.

The Rust API exposes the versioned metadata, timing, and report types used by
the profiler and macro crate. See the [API documentation](https://docs.rs/kael_perf)
and the [Kael guide](https://augani.github.io/kael/) for the wider framework.

## License

Licensed under the [Apache License, Version 2.0](https://github.com/Augani/kael/blob/main/LICENSE-APACHE).

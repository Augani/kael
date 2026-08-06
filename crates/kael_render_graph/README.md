# kael_render_graph

`kael_render_graph` is Kael's small, GPU-backend-neutral render scheduler. It
turns pass and resource declarations into a validated execution order with
read-after-write barriers, transient-resource lifetimes, reusable memory slots,
and deterministic dirty-subtree cache keys.

Use it with Kael, or independently in a renderer that owns its GPU API and
resource types.

## Build a graph

```rust
use kael_render_graph::{PassDesc, RenderGraph, ResourceDesc};

# fn main() -> anyhow::Result<()> {
let mut graph = RenderGraph::new();
let source = graph.add_resource(
    ResourceDesc::imported_texture("source").allocation_class(0x1001),
);
let filtered = graph.add_resource(
    ResourceDesc::transient_texture("filtered").allocation_class(0x1001),
);

let filter = graph.add_pass(
    PassDesc::new("filter")
        .read(source)
        .write(filtered)
        .param_hash(0x5eeda11),
);

let compiled = graph.compile()?;
assert_eq!(compiled.execution_order(), &[filter]);
assert!(compiled.cache_key(filter).is_some());
# Ok(())
# }
```

Compilation rejects unknown resources, multiple writers, unproduced transient
reads, and dependency cycles. A backend can then use:

- `execution_order` to submit passes;
- `barriers` to make writes visible before dependent reads;
- `assign_transient_memory` to reuse compatible, non-overlapping allocations;
- `cache_key` and `changed_passes` to reuse unchanged pass outputs.

Transient resources alias only when their lifetimes do not overlap and their
resource kind and caller-defined `allocation_class` match. Backends should
derive that class from every allocation constraint, including dimensions,
format, sample count, and usage. `CompiledGraph::is_current_for` detects a
compilation made stale by subsequent graph construction.

## Cache identity contract

`PassDesc::param_hash` is caller-defined. It must represent the operation
identity, every output-affecting parameter, and a content generation for any
imported input. `frame_pts` is a convenient identity component for decoded or
otherwise time-varying frames. Omitting an identity component can reuse stale
output.

The opaque 128-bit `CacheKey` is deterministic but is only an in-memory reuse
key. Do not persist it or treat its format as stable across Kael releases. GPU
backends should also scope cached resources by execution context that is not in
the graph, including dimensions, pixel format, color space, and device.

## CPU reference executor

The `reference` module evaluates texture-only graphs over linear,
straight-alpha RGBA `f32` images. It is useful for correctness tests,
deterministic previews, and validating a GPU implementation. Its
`ExecutionCache` applies explicit entry and resident-byte limits and refuses to
reuse images at a different resolution.

The reference executor limits each image to 256 MiB and all returned logical
images to 1 GiB. Graph construction is limited to 100,000 resources, 100,000
passes, 1,000,000 resource references, 4 KiB per diagnostic name, and 16 MiB
across names. Use `try_add_resource` and `try_add_pass` when graph shape is
dynamic so limit failures are returned instead of panicking.

This module is not a GPU backend, a color-management pipeline, or an optimized
production image processor. Applications should execute compiled graphs on
their rendering backend; the reference implementation is the portable oracle.

## License

Licensed under the Apache License, Version 2.0. See
[LICENSE-APACHE](LICENSE-APACHE).

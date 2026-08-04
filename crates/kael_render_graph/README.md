# kael_render_graph

A GPU-agnostic render-graph for Kael and standalone rendering engines. It
validates pass/resource dependencies, schedules passes, computes barriers and
transient-resource aliasing, and tracks time-varying cache invalidation.

```rust
use kael_render_graph::{PassDesc, RenderGraph, ResourceDesc};

let mut graph = RenderGraph::new();
let output = graph.add_resource(ResourceDesc::transient_texture("output"));
graph.add_pass(PassDesc::new("compose").write(output));
let compiled = graph.compile()?;
# Ok::<(), anyhow::Error>(())
```

Licensed under Apache-2.0.

//! Render-graph: a pass/resource DAG with scheduling, transient-resource
//! lifetimes, and — the hard part — a **time-varying cache-invalidation model**.
//!
//! An NLE composites by evaluating, per output frame, a DAG of GPU passes into
//! offscreen buffers. Naïvely re-running the whole graph every frame is wasteful;
//! naïvely caching sub-trees corrupts the preview when a clip frame or an effect
//! keyframe changes. This crate models invalidation explicitly: every pass gets a
//! cache key derived from **(topology + per-pass param hash + frame PTS + the
//! cache keys of its producers)**, so a change propagates to exactly the passes
//! whose output it can affect and no further.
//!
//! The graph here is GPU-agnostic — it computes *what* to execute and *what can
//! be reused*; a backend executes it. This separation keeps the scheduling and
//! invalidation logic pure and fully testable.

#![deny(missing_docs)]

use std::collections::VecDeque;

use anyhow::{bail, Result};

/// Handle to a resource declared in a [`RenderGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceId(pub u32);

/// Handle to a pass declared in a [`RenderGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PassId(pub u32);

/// The kind of a graph resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// A 2D render target / sampled texture.
    Texture,
    /// A GPU buffer.
    Buffer,
}

/// Declaration of a graph resource.
#[derive(Debug, Clone)]
pub struct ResourceDesc {
    /// Human-readable name, for diagnostics.
    pub name: String,
    /// The kind of resource.
    pub kind: ResourceKind,
    /// Whether the resource is imported (externally owned) rather than transient.
    ///
    /// Transient resources are owned by the graph and may alias memory with other
    /// transients whose lifetimes do not overlap.
    pub imported: bool,
}

impl ResourceDesc {
    /// A transient texture resource with the given name.
    pub fn transient_texture(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: ResourceKind::Texture,
            imported: false,
        }
    }

    /// An imported (externally owned) texture resource with the given name.
    pub fn imported_texture(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: ResourceKind::Texture,
            imported: true,
        }
    }
}

/// Declaration of a pass: what it reads, what it writes, and what makes its
/// output vary.
#[derive(Debug, Clone)]
pub struct PassDesc {
    /// Human-readable name, for diagnostics.
    pub name: String,
    /// Resources this pass samples/reads.
    pub reads: Vec<ResourceId>,
    /// Resources this pass renders to/writes.
    pub writes: Vec<ResourceId>,
    /// A hash of the pass's parameters (e.g. keyframed effect values). Bump this
    /// whenever a parameter that affects the output changes.
    pub param_hash: u64,
    /// The presentation timestamp of a time-varying input (e.g. a decoded clip
    /// frame), if any. A pass with a `frame_pts` re-evaluates whenever it changes.
    pub frame_pts: Option<i64>,
}

impl PassDesc {
    /// Start describing a pass.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            reads: Vec::new(),
            writes: Vec::new(),
            param_hash: 0,
            frame_pts: None,
        }
    }

    /// Declare that this pass reads `resource`.
    pub fn read(mut self, resource: ResourceId) -> Self {
        self.reads.push(resource);
        self
    }

    /// Declare that this pass writes `resource`.
    pub fn write(mut self, resource: ResourceId) -> Self {
        self.writes.push(resource);
        self
    }

    /// Set the parameter hash for this pass.
    pub fn param_hash(mut self, hash: u64) -> Self {
        self.param_hash = hash;
        self
    }

    /// Set the time-varying frame PTS for this pass.
    pub fn frame_pts(mut self, pts: i64) -> Self {
        self.frame_pts = Some(pts);
        self
    }
}

/// A mutable render-graph description.
#[derive(Debug, Default)]
pub struct RenderGraph {
    resources: Vec<ResourceDesc>,
    passes: Vec<PassDesc>,
}

/// Lifetime of a resource over the execution order, as `[first, last]` indices
/// into [`CompiledGraph::order`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLifetime {
    /// The resource this lifetime describes.
    pub resource: ResourceId,
    /// Order index of the first pass that touches it.
    pub first_pass_order: usize,
    /// Order index of the last pass that touches it.
    pub last_pass_order: usize,
}

/// A synchronization point: `resource` written by `after` must be made visible
/// to `before` (a read-after-write dependency) before that pass executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Barrier {
    /// The resource the barrier synchronizes.
    pub resource: ResourceId,
    /// The producing pass.
    pub after: PassId,
    /// The consuming pass.
    pub before: PassId,
}

/// Assignment of transient resources to reusable physical memory slots.
///
/// Transient resources whose lifetimes do not overlap can share a slot, so
/// `slot_count` is typically smaller than the number of transient resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransientAllocation {
    /// Per-resource slot index (`None` for imported or unused resources).
    pub slot_of: Vec<Option<usize>>,
    /// Total number of distinct slots required.
    pub slot_count: usize,
}

/// A validated, scheduled graph with per-pass cache keys, resource lifetimes,
/// barriers, and transient-memory aliasing.
#[derive(Debug, Clone)]
pub struct CompiledGraph {
    order: Vec<PassId>,
    cache_keys: Vec<u64>,
    lifetimes: Vec<Option<ResourceLifetime>>,
    transient: Vec<bool>,
    barriers: Vec<Barrier>,
}

impl RenderGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a resource, returning its handle.
    pub fn add_resource(&mut self, desc: ResourceDesc) -> ResourceId {
        let id = ResourceId(self.resources.len() as u32);
        self.resources.push(desc);
        id
    }

    /// Declare a pass, returning its handle.
    pub fn add_pass(&mut self, desc: PassDesc) -> PassId {
        let id = PassId(self.passes.len() as u32);
        self.passes.push(desc);
        id
    }

    /// The declaration of `pass`, or `None` if the handle is unknown.
    pub fn pass(&self, pass: PassId) -> Option<&PassDesc> {
        self.passes.get(pass.0 as usize)
    }

    /// Validate and schedule the graph: checks resource handles, enforces a
    /// single writer per resource, topologically orders the passes (erroring on
    /// cycles), computes per-pass cache keys, and computes resource lifetimes.
    pub fn compile(&self) -> Result<CompiledGraph> {
        let resource_count = self.resources.len();
        let pass_count = self.passes.len();

        let mut writer: Vec<Option<PassId>> = vec![None; resource_count];
        for (index, pass) in self.passes.iter().enumerate() {
            let pass_id = PassId(index as u32);
            for &resource in pass.reads.iter().chain(pass.writes.iter()) {
                if resource.0 as usize >= resource_count {
                    bail!(
                        "pass '{}' references unknown resource {:?}",
                        pass.name,
                        resource
                    );
                }
            }
            for &resource in &pass.writes {
                if let Some(existing) = writer[resource.0 as usize] {
                    bail!(
                        "resource {:?} is written by multiple passes ({:?} and {:?}); each resource must have a single writer",
                        resource,
                        existing,
                        pass_id
                    );
                }
                writer[resource.0 as usize] = Some(pass_id);
            }
        }

        let order = self.topological_order(&writer)?;
        let cache_keys = self.compute_cache_keys(&order, &writer);
        let lifetimes = self.compute_lifetimes(&order, resource_count);

        let mut keys_by_pass = vec![0u64; pass_count];
        for (position, &pass_id) in order.iter().enumerate() {
            keys_by_pass[pass_id.0 as usize] = cache_keys[position];
        }

        let mut barriers = Vec::new();
        for (reader_index, pass) in self.passes.iter().enumerate() {
            for &resource in &pass.reads {
                if let Some(producer) = writer[resource.0 as usize] {
                    if producer.0 as usize != reader_index {
                        barriers.push(Barrier {
                            resource,
                            after: producer,
                            before: PassId(reader_index as u32),
                        });
                    }
                }
            }
        }

        let transient = self
            .resources
            .iter()
            .map(|resource| !resource.imported)
            .collect();

        Ok(CompiledGraph {
            order,
            cache_keys: keys_by_pass,
            lifetimes,
            transient,
            barriers,
        })
    }

    fn topological_order(&self, writer: &[Option<PassId>]) -> Result<Vec<PassId>> {
        let pass_count = self.passes.len();
        let mut in_degree = vec![0usize; pass_count];
        let mut edges: Vec<Vec<usize>> = vec![Vec::new(); pass_count];

        for (reader_index, pass) in self.passes.iter().enumerate() {
            for &resource in &pass.reads {
                if let Some(producer) = writer[resource.0 as usize] {
                    let producer_index = producer.0 as usize;
                    if producer_index == reader_index {
                        continue;
                    }
                    edges[producer_index].push(reader_index);
                    in_degree[reader_index] += 1;
                }
            }
        }

        let mut ready: VecDeque<usize> = (0..pass_count)
            .filter(|&index| in_degree[index] == 0)
            .collect();
        let mut order = Vec::with_capacity(pass_count);

        while let Some(index) = ready.pop_front() {
            order.push(PassId(index as u32));
            for &next in &edges[index] {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    ready.push_back(next);
                }
            }
        }

        if order.len() != pass_count {
            bail!("render graph contains a cycle");
        }
        Ok(order)
    }

    fn compute_cache_keys(&self, order: &[PassId], writer: &[Option<PassId>]) -> Vec<u64> {
        let mut keys_by_pass = vec![0u64; self.passes.len()];
        let mut keys_in_order = Vec::with_capacity(order.len());

        for &pass_id in order {
            let pass = &self.passes[pass_id.0 as usize];
            let mut key = FNV_OFFSET;
            key = fnv_mix(key, pass.param_hash);
            key = fnv_mix(key, pass.frame_pts.unwrap_or(i64::MIN) as u64);

            let mut reads = pass.reads.clone();
            reads.sort_unstable();
            for resource in reads {
                key = fnv_mix(key, resource.0 as u64);
                if let Some(producer) = writer[resource.0 as usize] {
                    key = fnv_mix(key, keys_by_pass[producer.0 as usize]);
                }
            }

            keys_by_pass[pass_id.0 as usize] = key;
            keys_in_order.push(key);
        }

        keys_in_order
    }

    fn compute_lifetimes(
        &self,
        order: &[PassId],
        resource_count: usize,
    ) -> Vec<Option<ResourceLifetime>> {
        let mut order_position = vec![0usize; self.passes.len()];
        for (position, &pass_id) in order.iter().enumerate() {
            order_position[pass_id.0 as usize] = position;
        }

        let mut lifetimes: Vec<Option<ResourceLifetime>> = vec![None; resource_count];
        for (pass_index, pass) in self.passes.iter().enumerate() {
            let position = order_position[pass_index];
            for &resource in pass.reads.iter().chain(pass.writes.iter()) {
                let slot = &mut lifetimes[resource.0 as usize];
                match slot {
                    None => {
                        *slot = Some(ResourceLifetime {
                            resource,
                            first_pass_order: position,
                            last_pass_order: position,
                        });
                    }
                    Some(lifetime) => {
                        lifetime.first_pass_order = lifetime.first_pass_order.min(position);
                        lifetime.last_pass_order = lifetime.last_pass_order.max(position);
                    }
                }
            }
        }
        lifetimes
    }
}

impl CompiledGraph {
    /// The passes in a valid execution order.
    pub fn execution_order(&self) -> &[PassId] {
        &self.order
    }

    /// The cache key for `pass`, or `None` if the handle is unknown.
    pub fn cache_key(&self, pass: PassId) -> Option<u64> {
        self.cache_keys.get(pass.0 as usize).copied()
    }

    /// The computed lifetime of `resource`, or `None` if it is unused.
    pub fn lifetime(&self, resource: ResourceId) -> Option<ResourceLifetime> {
        self.lifetimes.get(resource.0 as usize).copied().flatten()
    }

    /// Resources whose lifetimes do not overlap `resource`'s and could therefore
    /// share its transient memory. Imported resources are not considered.
    pub fn non_overlapping(&self, resource: ResourceId) -> Vec<ResourceId> {
        let Some(target) = self.lifetime(resource) else {
            return Vec::new();
        };
        self.lifetimes
            .iter()
            .filter_map(|entry| *entry)
            .filter(|other| other.resource != resource)
            .filter(|other| {
                other.last_pass_order < target.first_pass_order
                    || other.first_pass_order > target.last_pass_order
            })
            .map(|other| other.resource)
            .collect()
    }

    /// The passes whose cache key differs from `previous` — i.e. the passes that
    /// must be re-executed; all others may reuse their cached output.
    pub fn changed_passes(&self, previous: &CompiledGraph) -> Vec<PassId> {
        let mut changed = Vec::new();
        for (index, &key) in self.cache_keys.iter().enumerate() {
            let pass = PassId(index as u32);
            if previous.cache_key(pass) != Some(key) {
                changed.push(pass);
            }
        }
        changed
    }

    /// Read-after-write synchronization points the backend must insert.
    pub fn barriers(&self) -> &[Barrier] {
        &self.barriers
    }

    /// Assign transient resources to reusable memory slots using a greedy
    /// lifetime-interval coloring: transients whose lifetimes do not overlap
    /// share a slot. Imported resources are never assigned a slot.
    pub fn assign_transient_memory(&self) -> TransientAllocation {
        let mut items: Vec<ResourceLifetime> = self
            .lifetimes
            .iter()
            .enumerate()
            .filter_map(|(index, lifetime)| {
                let lifetime = (*lifetime)?;
                if *self.transient.get(index).unwrap_or(&false) {
                    Some(lifetime)
                } else {
                    None
                }
            })
            .collect();
        items.sort_by_key(|lifetime| (lifetime.first_pass_order, lifetime.last_pass_order));

        let mut slot_of = vec![None; self.lifetimes.len()];
        let mut slot_last_use: Vec<usize> = Vec::new();

        for lifetime in items {
            let free_slot = slot_last_use
                .iter()
                .position(|&last| last < lifetime.first_pass_order);
            let slot = match free_slot {
                Some(slot) => {
                    slot_last_use[slot] = lifetime.last_pass_order;
                    slot
                }
                None => {
                    slot_last_use.push(lifetime.last_pass_order);
                    slot_last_use.len() - 1
                }
            };
            slot_of[lifetime.resource.0 as usize] = Some(slot);
        }

        TransientAllocation {
            slot_of,
            slot_count: slot_last_use.len(),
        }
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(mut hash: u64, value: u64) -> u64 {
    for byte in value.to_le_bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Chain {
        graph: RenderGraph,
        decode: PassId,
        effect: PassId,
        present: PassId,
        frame: ResourceId,
        composited: ResourceId,
    }

    fn build_chain(decode_pts: i64, effect_param: u64) -> Chain {
        let mut graph = RenderGraph::new();
        let frame = graph.add_resource(ResourceDesc::transient_texture("frame"));
        let composited = graph.add_resource(ResourceDesc::transient_texture("composited"));
        let backbuffer = graph.add_resource(ResourceDesc::imported_texture("backbuffer"));

        let decode = graph.add_pass(PassDesc::new("decode").write(frame).frame_pts(decode_pts));
        let effect = graph.add_pass(
            PassDesc::new("effect")
                .read(frame)
                .write(composited)
                .param_hash(effect_param),
        );
        let present = graph.add_pass(PassDesc::new("present").read(composited).write(backbuffer));

        Chain {
            graph,
            decode,
            effect,
            present,
            frame,
            composited,
        }
    }

    #[test]
    fn schedules_in_dependency_order() {
        let chain = build_chain(0, 0);
        let compiled = chain.graph.compile().unwrap();
        let order = compiled.execution_order();
        let pos = |p: PassId| order.iter().position(|&q| q == p).unwrap();
        assert!(pos(chain.decode) < pos(chain.effect));
        assert!(pos(chain.effect) < pos(chain.present));
    }

    #[test]
    fn detects_cycles() {
        let mut graph = RenderGraph::new();
        let a = graph.add_resource(ResourceDesc::transient_texture("a"));
        let b = graph.add_resource(ResourceDesc::transient_texture("b"));
        graph.add_pass(PassDesc::new("p").read(b).write(a));
        graph.add_pass(PassDesc::new("q").read(a).write(b));
        assert!(graph.compile().is_err());
    }

    #[test]
    fn rejects_multiple_writers() {
        let mut graph = RenderGraph::new();
        let r = graph.add_resource(ResourceDesc::transient_texture("r"));
        graph.add_pass(PassDesc::new("p").write(r));
        graph.add_pass(PassDesc::new("q").write(r));
        assert!(graph.compile().is_err());
    }

    #[test]
    fn rejects_unknown_resource() {
        let mut graph = RenderGraph::new();
        graph.add_pass(PassDesc::new("p").write(ResourceId(99)));
        assert!(graph.compile().is_err());
    }

    #[test]
    fn identical_graphs_produce_identical_keys() {
        let a = build_chain(100, 7).graph.compile().unwrap();
        let b = build_chain(100, 7).graph.compile().unwrap();
        assert_eq!(a.cache_keys, b.cache_keys);
    }

    #[test]
    fn changing_effect_param_invalidates_only_it_and_dependents() {
        let base = build_chain(100, 7);
        let base_compiled = base.graph.compile().unwrap();

        let changed = build_chain(100, 8);
        let changed_compiled = changed.graph.compile().unwrap();

        assert_eq!(
            base_compiled.cache_key(base.decode),
            changed_compiled.cache_key(changed.decode),
            "decode is upstream of the effect and must not change"
        );
        assert_ne!(
            base_compiled.cache_key(base.effect),
            changed_compiled.cache_key(changed.effect)
        );
        assert_ne!(
            base_compiled.cache_key(base.present),
            changed_compiled.cache_key(changed.present),
            "present depends on the effect and must be invalidated"
        );

        let changed_ids = changed_compiled.changed_passes(&base_compiled);
        assert!(changed_ids.contains(&changed.effect));
        assert!(changed_ids.contains(&changed.present));
        assert!(!changed_ids.contains(&changed.decode));
    }

    #[test]
    fn changing_frame_pts_invalidates_whole_downstream() {
        let base = build_chain(100, 7).graph.compile().unwrap();
        let next = build_chain(101, 7).graph.compile().unwrap();
        // decode, effect, and present all change because the frame changed.
        assert_eq!(next.changed_passes(&base).len(), 3);
    }

    #[test]
    fn independent_branches_are_isolated() {
        let mut graph = RenderGraph::new();
        let in_a = graph.add_resource(ResourceDesc::imported_texture("in_a"));
        let in_b = graph.add_resource(ResourceDesc::imported_texture("in_b"));
        let out_a = graph.add_resource(ResourceDesc::transient_texture("out_a"));
        let out_b = graph.add_resource(ResourceDesc::transient_texture("out_b"));
        let pass_a = graph.add_pass(PassDesc::new("a").read(in_a).write(out_a).param_hash(1));
        let pass_b = graph.add_pass(PassDesc::new("b").read(in_b).write(out_b).param_hash(2));
        let base = graph.compile().unwrap();

        let mut graph2 = RenderGraph::new();
        let in_a2 = graph2.add_resource(ResourceDesc::imported_texture("in_a"));
        let in_b2 = graph2.add_resource(ResourceDesc::imported_texture("in_b"));
        let out_a2 = graph2.add_resource(ResourceDesc::transient_texture("out_a"));
        let out_b2 = graph2.add_resource(ResourceDesc::transient_texture("out_b"));
        let _ = graph2.add_pass(PassDesc::new("a").read(in_a2).write(out_a2).param_hash(1));
        let _ = graph2.add_pass(PassDesc::new("b").read(in_b2).write(out_b2).param_hash(99));
        let changed = graph2.compile().unwrap();

        assert_eq!(base.cache_key(pass_a), changed.cache_key(pass_a));
        assert_ne!(base.cache_key(pass_b), changed.cache_key(pass_b));
    }

    #[test]
    fn computes_resource_lifetimes() {
        let chain = build_chain(0, 0);
        let compiled = chain.graph.compile().unwrap();
        let frame = compiled.lifetime(chain.frame).unwrap();
        // `frame` is written by decode (order 0) and read by effect (order 1).
        assert_eq!(frame.first_pass_order, 0);
        assert_eq!(frame.last_pass_order, 1);
        // `frame` and `composited` overlap (effect touches both), so they cannot alias.
        assert!(!compiled
            .non_overlapping(chain.frame)
            .contains(&chain.composited));
    }

    #[test]
    fn barriers_track_read_after_write() {
        let chain = build_chain(0, 0);
        let compiled = chain.graph.compile().unwrap();
        let barriers = compiled.barriers();

        assert!(barriers
            .iter()
            .any(|barrier| barrier.resource == chain.frame
                && barrier.after == chain.decode
                && barrier.before == chain.effect));
        assert!(barriers
            .iter()
            .any(|barrier| barrier.resource == chain.composited
                && barrier.after == chain.effect
                && barrier.before == chain.present));
    }

    #[test]
    fn imported_resource_gets_no_transient_slot() {
        let mut graph = RenderGraph::new();
        let frame = graph.add_resource(ResourceDesc::transient_texture("frame"));
        let backbuffer = graph.add_resource(ResourceDesc::imported_texture("backbuffer"));
        graph.add_pass(PassDesc::new("decode").write(frame));
        graph.add_pass(PassDesc::new("present").read(frame).write(backbuffer));

        let allocation = graph.compile().unwrap().assign_transient_memory();
        assert!(allocation.slot_of[frame.0 as usize].is_some());
        assert!(allocation.slot_of[backbuffer.0 as usize].is_none());
    }

    #[test]
    fn disjoint_transients_share_a_slot() {
        let mut graph = RenderGraph::new();
        let t1 = graph.add_resource(ResourceDesc::transient_texture("t1"));
        let t2 = graph.add_resource(ResourceDesc::transient_texture("t2"));
        let t3 = graph.add_resource(ResourceDesc::transient_texture("t3"));
        graph.add_pass(PassDesc::new("a").write(t1));
        graph.add_pass(PassDesc::new("b").read(t1).write(t2));
        graph.add_pass(PassDesc::new("c").read(t2).write(t3));
        graph.add_pass(PassDesc::new("d").read(t3));

        let allocation = graph.compile().unwrap().assign_transient_memory();
        // t1 [0,1] and t3 [2,3] do not overlap and reuse the same slot; t2 [1,2]
        // overlaps both, so two slots total.
        assert_eq!(allocation.slot_count, 2);
        assert_eq!(
            allocation.slot_of[t1.0 as usize],
            allocation.slot_of[t3.0 as usize]
        );
        assert_ne!(
            allocation.slot_of[t1.0 as usize],
            allocation.slot_of[t2.0 as usize]
        );
    }
}

/// A CPU reference executor for a compiled [`RenderGraph`].
///
/// Runs each pass in schedule order over linear straight-alpha RGBA `f32`
/// images, producing the logical result a GPU backend should — a correctness
/// oracle and the "preview == export" reference (export determinism, V10).
pub mod reference {
    use std::collections::HashMap;

    use anyhow::{anyhow, Result};

    use super::{CompiledGraph, PassId, RenderGraph, ResourceId};

    /// A linear, straight-alpha RGBA image.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Image {
        /// Width in pixels.
        pub width: u32,
        /// Height in pixels.
        pub height: u32,
        /// Row-major `[r, g, b, a]` pixels in linear light.
        pub pixels: Vec<[f32; 4]>,
    }

    impl Image {
        /// A transparent-black image.
        pub fn new(width: u32, height: u32) -> Self {
            Self {
                width,
                height,
                pixels: vec![[0.0; 4]; (width * height) as usize],
            }
        }

        /// An image filled with a single color.
        pub fn filled(width: u32, height: u32, color: [f32; 4]) -> Self {
            Self {
                width,
                height,
                pixels: vec![color; (width * height) as usize],
            }
        }

        /// The pixel at `(x, y)`.
        pub fn pixel(&self, x: u32, y: u32) -> [f32; 4] {
            self.pixels[(y * self.width + x) as usize]
        }
    }

    /// A pass implementation: read `inputs` (in the pass's declared read order)
    /// and write `output`.
    pub type PassOp<'a> = Box<dyn Fn(&[&Image], &mut Image) + 'a>;

    /// Execute `compiled` over CPU images, returning the image written for each
    /// resource. `imported` supplies externally-owned inputs; `ops` supplies a
    /// closure per pass (passes without an op leave their output cleared).
    pub fn execute<'ops>(
        graph: &RenderGraph,
        compiled: &CompiledGraph,
        width: u32,
        height: u32,
        imported: &HashMap<ResourceId, Image>,
        ops: &HashMap<PassId, PassOp<'ops>>,
    ) -> Result<HashMap<ResourceId, Image>> {
        let mut images: HashMap<ResourceId, Image> = imported.clone();

        for &pass_id in compiled.execution_order() {
            let pass = graph
                .pass(pass_id)
                .ok_or_else(|| anyhow!("compiled graph references unknown {pass_id:?}"))?;

            let mut inputs = Vec::with_capacity(pass.reads.len());
            for resource in &pass.reads {
                let image = images.get(resource).ok_or_else(|| {
                    anyhow!("pass '{}' reads unproduced {:?}", pass.name, resource)
                })?;
                inputs.push(image);
            }

            let mut output = Image::new(width, height);
            if let Some(op) = ops.get(&pass_id) {
                op(&inputs, &mut output);
            }

            for &resource in &pass.writes {
                images.insert(resource, output.clone());
            }
        }

        Ok(images)
    }

    /// A pass op that fills the output with a constant color.
    pub fn fill(color: [f32; 4]) -> PassOp<'static> {
        Box::new(move |_inputs, output| {
            for pixel in output.pixels.iter_mut() {
                *pixel = color;
            }
        })
    }

    /// A pass op compositing `inputs[0]` (top) over `inputs[1]` (bottom) with
    /// straight-alpha source-over.
    pub fn blend_over(inputs: &[&Image], output: &mut Image) {
        if inputs.len() < 2 {
            return;
        }
        let (top, bottom) = (inputs[0], inputs[1]);
        for (index, pixel) in output.pixels.iter_mut().enumerate() {
            let t = top.pixels[index];
            let b = bottom.pixels[index];
            let out_a = t[3] + b[3] * (1.0 - t[3]);
            let mut out = [0.0f32; 4];
            out[3] = out_a;
            for channel in 0..3 {
                out[channel] = if out_a <= f32::EPSILON {
                    0.0
                } else {
                    (t[channel] * t[3] + b[channel] * b[3] * (1.0 - t[3])) / out_a
                };
            }
            *pixel = out;
        }
    }

    /// Separable blend modes for compositing one layer over another, following the
    /// W3C Compositing and Blending Level 1 separable-blend definitions (the set a
    /// non-linear editor exposes for clip blending).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum BlendMode {
        /// Source-over (no color blending).
        Normal,
        /// Multiplies layer colors (darkens).
        Multiply,
        /// Inverse-multiplies (lightens).
        Screen,
        /// Clamped additive (linear dodge).
        Add,
        /// Keeps the darker of the two colors per channel.
        Darken,
        /// Keeps the lighter of the two colors per channel.
        Lighten,
        /// Multiply on dark backdrops, screen on light ones (contrast).
        Overlay,
        /// Like [`BlendMode::Overlay`] but keyed on the source (harsh spotlight).
        HardLight,
        /// Gentle dodge/burn keyed on the source.
        SoftLight,
        /// Brightens the backdrop toward the source.
        ColorDodge,
        /// Darkens the backdrop toward the source.
        ColorBurn,
        /// Absolute per-channel difference.
        Difference,
        /// Like [`BlendMode::Difference`] but with lower contrast.
        Exclusion,
    }

    impl BlendMode {
        fn apply(self, backdrop: f32, source: f32) -> f32 {
            match self {
                Self::Normal => source,
                Self::Multiply => backdrop * source,
                Self::Screen => backdrop + source - backdrop * source,
                Self::Add => (backdrop + source).min(1.0),
                Self::Darken => backdrop.min(source),
                Self::Lighten => backdrop.max(source),
                Self::Overlay => hard_light(source, backdrop),
                Self::HardLight => hard_light(backdrop, source),
                Self::SoftLight => soft_light(backdrop, source),
                Self::ColorDodge => {
                    if backdrop == 0.0 {
                        0.0
                    } else if source >= 1.0 {
                        1.0
                    } else {
                        (backdrop / (1.0 - source)).min(1.0)
                    }
                }
                Self::ColorBurn => {
                    if backdrop >= 1.0 {
                        1.0
                    } else if source == 0.0 {
                        0.0
                    } else {
                        1.0 - ((1.0 - backdrop) / source).min(1.0)
                    }
                }
                Self::Difference => (backdrop - source).abs(),
                Self::Exclusion => backdrop + source - 2.0 * backdrop * source,
            }
        }
    }

    fn hard_light(backdrop: f32, source: f32) -> f32 {
        if source <= 0.5 {
            2.0 * backdrop * source
        } else {
            1.0 - 2.0 * (1.0 - backdrop) * (1.0 - source)
        }
    }

    fn soft_light(backdrop: f32, source: f32) -> f32 {
        if source <= 0.5 {
            backdrop - (1.0 - 2.0 * source) * backdrop * (1.0 - backdrop)
        } else {
            let d = if backdrop <= 0.25 {
                ((16.0 * backdrop - 12.0) * backdrop + 4.0) * backdrop
            } else {
                backdrop.sqrt()
            };
            backdrop + (2.0 * source - 1.0) * (d - backdrop)
        }
    }

    /// A pass op compositing `inputs[0]` (top) over `inputs[1]` (bottom) with the
    /// given blend `mode`, per the W3C compositing-and-blending model.
    pub fn blend(mode: BlendMode) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            if inputs.len() < 2 {
                return;
            }
            let (top, bottom) = (inputs[0], inputs[1]);
            for (index, pixel) in output.pixels.iter_mut().enumerate() {
                let s = top.pixels[index];
                let b = bottom.pixels[index];
                let (a_s, a_b) = (s[3], b[3]);
                let out_a = a_s + a_b * (1.0 - a_s);
                let mut out = [0.0f32; 4];
                out[3] = out_a;
                for channel in 0..3 {
                    let blended = mode.apply(b[channel], s[channel]);
                    let premultiplied = a_s * (1.0 - a_b) * s[channel]
                        + a_s * a_b * blended
                        + a_b * (1.0 - a_s) * b[channel];
                    out[channel] = if out_a <= f32::EPSILON {
                        0.0
                    } else {
                        premultiplied / out_a
                    };
                }
                *pixel = out;
            }
        })
    }

    /// A pass op that translates `inputs[0]` by `(dx, dy)` pixels (nearest, with
    /// transparent fill outside the source).
    pub fn translate(dx: i32, dy: i32) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            let (width, height) = (output.width as i32, output.height as i32);
            for y in 0..height {
                for x in 0..width {
                    let (sx, sy) = (x - dx, y - dy);
                    let value = if sx >= 0
                        && sx < source.width as i32
                        && sy >= 0
                        && sy < source.height as i32
                    {
                        source.pixels[(sy as u32 * source.width + sx as u32) as usize]
                    } else {
                        [0.0; 4]
                    };
                    output.pixels[(y as u32 * output.width + x as u32) as usize] = value;
                }
            }
        })
    }

    /// A two-input transition op: linear crossfade (dissolve) from `inputs[0]`
    /// to `inputs[1]` by `mix` in `0..=1` (0 = first, 1 = second).
    pub fn crossfade(mix: f32) -> PassOp<'static> {
        let mix = mix.clamp(0.0, 1.0);
        Box::new(move |inputs, output| {
            if inputs.len() < 2 {
                return;
            }
            let (from, to) = (inputs[0], inputs[1]);
            for (index, pixel) in output.pixels.iter_mut().enumerate() {
                let a = from.pixels[index];
                let b = to.pixels[index];
                let mut out = [0.0f32; 4];
                for channel in 0..4 {
                    out[channel] = a[channel] * (1.0 - mix) + b[channel] * mix;
                }
                *pixel = out;
            }
        })
    }

    /// A two-input transition op: a hard horizontal wipe revealing `inputs[1]`
    /// from the left over `inputs[0]` as `progress` goes `0..=1`.
    pub fn wipe_horizontal(progress: f32) -> PassOp<'static> {
        let progress = progress.clamp(0.0, 1.0);
        Box::new(move |inputs, output| {
            if inputs.len() < 2 {
                return;
            }
            let (from, to) = (inputs[0], inputs[1]);
            let boundary = (progress * output.width as f32) as u32;
            for y in 0..output.height {
                for x in 0..output.width {
                    let index = (y * output.width + x) as usize;
                    output.pixels[index] = if x < boundary {
                        to.pixels[index]
                    } else {
                        from.pixels[index]
                    };
                }
            }
        })
    }

    /// A two-input transition op: a hard vertical wipe revealing `inputs[1]` from
    /// the top over `inputs[0]` as `progress` goes `0..=1`.
    pub fn wipe_vertical(progress: f32) -> PassOp<'static> {
        let progress = progress.clamp(0.0, 1.0);
        Box::new(move |inputs, output| {
            if inputs.len() < 2 {
                return;
            }
            let (from, to) = (inputs[0], inputs[1]);
            let boundary = (progress * output.height as f32) as u32;
            for y in 0..output.height {
                for x in 0..output.width {
                    let index = (y * output.width + x) as usize;
                    output.pixels[index] = if y < boundary {
                        to.pixels[index]
                    } else {
                        from.pixels[index]
                    };
                }
            }
        })
    }

    /// A two-input transition op: dip through a solid `color` (e.g. dip-to-black).
    /// `inputs[0]` fades to `color` over the first half of `progress`, then `color`
    /// fades to `inputs[1]` over the second half.
    pub fn dip_to_color(color: [f32; 4], progress: f32) -> PassOp<'static> {
        let progress = progress.clamp(0.0, 1.0);
        Box::new(move |inputs, output| {
            if inputs.len() < 2 {
                return;
            }
            let (from, to) = (inputs[0], inputs[1]);
            // mix is the weight of the image (vs the dip color): 1 at the ends,
            // 0 at the midpoint where only the color shows.
            let (source, mix) = if progress < 0.5 {
                (from, 1.0 - progress * 2.0)
            } else {
                (to, progress * 2.0 - 1.0)
            };
            for (index, pixel) in output.pixels.iter_mut().enumerate() {
                let edge = source.pixels[index];
                let mut out = [0.0f32; 4];
                for channel in 0..4 {
                    out[channel] = edge[channel] * mix + color[channel] * (1.0 - mix);
                }
                *pixel = out;
            }
        })
    }

    /// A two-input transition op: an iris (circular) wipe revealing `inputs[1]` in an
    /// expanding circle from the center over `inputs[0]` as `progress` goes `0..=1`.
    pub fn iris(progress: f32) -> PassOp<'static> {
        let progress = progress.clamp(0.0, 1.0);
        Box::new(move |inputs, output| {
            if inputs.len() < 2 {
                return;
            }
            let (from, to) = (inputs[0], inputs[1]);
            let (cx, cy) = (output.width as f32 / 2.0, output.height as f32 / 2.0);
            let max_radius = (cx * cx + cy * cy).sqrt();
            let radius = progress * max_radius;
            for y in 0..output.height {
                for x in 0..output.width {
                    let index = (y * output.width + x) as usize;
                    let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                    output.pixels[index] = if (dx * dx + dy * dy).sqrt() < radius {
                        to.pixels[index]
                    } else {
                        from.pixels[index]
                    };
                }
            }
        })
    }

    /// A two-input transition op: `inputs[1]` slides in from the right, pushing
    /// `inputs[0]` out to the left, as `progress` goes `0..=1`.
    pub fn push_horizontal(progress: f32) -> PassOp<'static> {
        let progress = progress.clamp(0.0, 1.0);
        Box::new(move |inputs, output| {
            if inputs.len() < 2 {
                return;
            }
            let (from, to) = (inputs[0], inputs[1]);
            let width = output.width;
            let shift = (progress * width as f32) as u32;
            let split = width - shift;
            for y in 0..output.height {
                let row = y * width;
                for x in 0..width {
                    let index = (row + x) as usize;
                    output.pixels[index] = if x < split {
                        from.pixels[(row + x + shift) as usize]
                    } else {
                        to.pixels[(row + (x - split)) as usize]
                    };
                }
            }
        })
    }

    fn sample_bilinear(image: &Image, u: f32, v: f32) -> [f32; 4] {
        let max_x = image.width as i32 - 1;
        let max_y = image.height as i32 - 1;
        let x0 = u.floor() as i32;
        let y0 = v.floor() as i32;
        let (fx, fy) = (u - x0 as f32, v - y0 as f32);
        let get = |x: i32, y: i32| {
            let cx = x.clamp(0, max_x) as u32;
            let cy = y.clamp(0, max_y) as u32;
            image.pixels[(cy * image.width + cx) as usize]
        };
        let (c00, c10, c01, c11) = (
            get(x0, y0),
            get(x0 + 1, y0),
            get(x0, y0 + 1),
            get(x0 + 1, y0 + 1),
        );
        let mut out = [0.0f32; 4];
        for channel in 0..4 {
            let top = c00[channel] * (1.0 - fx) + c10[channel] * fx;
            let bottom = c01[channel] * (1.0 - fx) + c11[channel] * fx;
            out[channel] = top * (1.0 - fy) + bottom * fy;
        }
        out
    }

    /// A single-input op that bilinearly resizes `inputs[0]` to fill the output —
    /// image scaling for fit-to-frame and proxy/preview resolution changes.
    pub fn resample() -> PassOp<'static> {
        Box::new(|inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            if source.width == 0 || source.height == 0 {
                return;
            }
            for oy in 0..output.height {
                for ox in 0..output.width {
                    let u = (ox as f32 + 0.5) / output.width as f32 * source.width as f32 - 0.5;
                    let v = (oy as f32 + 0.5) / output.height as f32 * source.height as f32 - 0.5;
                    output.pixels[(oy * output.width + ox) as usize] =
                        sample_bilinear(source, u, v);
                }
            }
        })
    }

    /// A single-input op that bilinearly scales `inputs[0]` into the destination
    /// rectangle `(dst_x, dst_y, dst_width, dst_height)` within the output, leaving the
    /// rest transparent — the picture-in-picture / position-and-scale transform.
    pub fn place(dst_x: i32, dst_y: i32, dst_width: u32, dst_height: u32) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            for oy in 0..output.height {
                for ox in 0..output.width {
                    let index = (oy * output.width + ox) as usize;
                    let rel_x = ox as i32 - dst_x;
                    let rel_y = oy as i32 - dst_y;
                    let inside = dst_width > 0
                        && dst_height > 0
                        && source.width > 0
                        && source.height > 0
                        && rel_x >= 0
                        && (rel_x as u32) < dst_width
                        && rel_y >= 0
                        && (rel_y as u32) < dst_height;
                    output.pixels[index] = if inside {
                        let u = (rel_x as f32 + 0.5) / dst_width as f32 * source.width as f32 - 0.5;
                        let v =
                            (rel_y as f32 + 0.5) / dst_height as f32 * source.height as f32 - 0.5;
                        sample_bilinear(source, u, v)
                    } else {
                        [0.0; 4]
                    };
                }
            }
        })
    }

    /// A single-input op that box-blurs `inputs[0]` with the given pixel `radius` (a
    /// simple defocus). Each output pixel is the average of the `(2·radius+1)²`
    /// neighborhood, clamped at the edges; `radius` 0 is a passthrough.
    pub fn box_blur(radius: u32) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            if source.width != output.width || source.height != output.height {
                return;
            }
            let radius = radius as i32;
            let (width, height) = (output.width as i32, output.height as i32);
            for y in 0..height {
                for x in 0..width {
                    let mut sum = [0.0f32; 4];
                    let mut count = 0.0f32;
                    for dy in -radius..=radius {
                        for dx in -radius..=radius {
                            let sx = (x + dx).clamp(0, width - 1);
                            let sy = (y + dy).clamp(0, height - 1);
                            let pixel = source.pixels[(sy * width + sx) as usize];
                            for channel in 0..4 {
                                sum[channel] += pixel[channel];
                            }
                            count += 1.0;
                        }
                    }
                    let index = (y * width + x) as usize;
                    output.pixels[index] = [
                        sum[0] / count,
                        sum[1] / count,
                        sum[2] / count,
                        sum[3] / count,
                    ];
                }
            }
        })
    }

    /// A single-input separable Gaussian blur of `inputs[0]` with standard deviation
    /// `sigma` (in pixels), edge-clamped. Unlike [`box_blur`], the Gaussian kernel is
    /// smooth and free of box-filter ringing, which makes it the basis for glow, soft
    /// focus, and drop shadows. The kernel is truncated at three standard deviations
    /// and normalized to unit sum, so a flat region is preserved exactly. The two 1D
    /// passes cost O(width·height·radius) rather than the O(width·height·radius²) of a
    /// naive 2D kernel. A non-positive `sigma` (or an empty image) is the identity.
    pub fn gaussian_blur(sigma: f32) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            if source.width != output.width || source.height != output.height {
                return;
            }
            if sigma <= 0.0 || source.pixels.is_empty() {
                output.pixels.copy_from_slice(&source.pixels);
                return;
            }

            let radius = (sigma * 3.0).ceil() as i32;
            let denom = 2.0 * sigma * sigma;
            let mut kernel = Vec::with_capacity((radius * 2 + 1) as usize);
            let mut total = 0.0f32;
            for offset in -radius..=radius {
                let coordinate = offset as f32;
                let weight = (-(coordinate * coordinate) / denom).exp();
                kernel.push(weight);
                total += weight;
            }
            for weight in &mut kernel {
                *weight /= total;
            }

            let (width, height) = (output.width as i32, output.height as i32);
            let mut horizontal = vec![[0.0f32; 4]; source.pixels.len()];
            for y in 0..height {
                for x in 0..width {
                    let mut sum = [0.0f32; 4];
                    for (tap, &weight) in kernel.iter().enumerate() {
                        let sx = (x + tap as i32 - radius).clamp(0, width - 1);
                        let pixel = source.pixels[(y * width + sx) as usize];
                        for channel in 0..4 {
                            sum[channel] += pixel[channel] * weight;
                        }
                    }
                    horizontal[(y * width + x) as usize] = sum;
                }
            }
            for y in 0..height {
                for x in 0..width {
                    let mut sum = [0.0f32; 4];
                    for (tap, &weight) in kernel.iter().enumerate() {
                        let sy = (y + tap as i32 - radius).clamp(0, height - 1);
                        let pixel = horizontal[(sy * width + x) as usize];
                        for channel in 0..4 {
                            sum[channel] += pixel[channel] * weight;
                        }
                    }
                    output.pixels[(y * width + x) as usize] = sum;
                }
            }
        })
    }

    /// A single-input unsharp-mask sharpen of `inputs[0]`: the classic
    /// `result = source + amount * (source - blur(source))`, where the blur is the
    /// [`gaussian_blur`] of radius `sigma`. Boosting the high-frequency residual
    /// raises local contrast at edges with a tunable radius and strength, unlike the
    /// fixed 3x3 [`convolve_3x3`] sharpen kernel. Color channels are clamped to the
    /// display range `[0, 1]`; alpha is passed through unchanged. `amount == 0` (or a
    /// non-positive `sigma`) is the identity, and a flat region is preserved exactly.
    pub fn unsharp_mask(sigma: f32, amount: f32) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            if source.width != output.width || source.height != output.height {
                return;
            }
            let mut blurred = Image::new(source.width, source.height);
            gaussian_blur(sigma)(inputs, &mut blurred);
            for ((out_pixel, source_pixel), blur_pixel) in output
                .pixels
                .iter_mut()
                .zip(&source.pixels)
                .zip(&blurred.pixels)
            {
                for channel in 0..3 {
                    let high_pass = source_pixel[channel] - blur_pixel[channel];
                    out_pixel[channel] =
                        (source_pixel[channel] + amount * high_pass).clamp(0.0, 1.0);
                }
                out_pixel[3] = source_pixel[3];
            }
        })
    }

    /// A single-input op that keys out (sets alpha to 0) pixels of `inputs[0]` whose
    /// Rec.709 luma is below `threshold` — a simple luma key for masking dark areas.
    /// Color channels are preserved; brighter pixels keep their alpha.
    pub fn luma_key(threshold: f32) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            if source.width != output.width || source.height != output.height {
                return;
            }
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                let luma =
                    0.2126 * source_pixel[0] + 0.7152 * source_pixel[1] + 0.0722 * source_pixel[2];
                let alpha = if luma < threshold {
                    0.0
                } else {
                    source_pixel[3]
                };
                *output_pixel = [source_pixel[0], source_pixel[1], source_pixel[2], alpha];
            }
        })
    }

    /// A single-input op that keys out (sets alpha to 0) pixels of `inputs[0]` within
    /// `tolerance` Euclidean RGB distance of the key `color` — a basic chroma (color) key
    /// for green/blue screen. Color channels are preserved.
    pub fn chroma_key(color: [f32; 3], tolerance: f32) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            if source.width != output.width || source.height != output.height {
                return;
            }
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                let distance = ((source_pixel[0] - color[0]).powi(2)
                    + (source_pixel[1] - color[1]).powi(2)
                    + (source_pixel[2] - color[2]).powi(2))
                .sqrt();
                let alpha = if distance <= tolerance {
                    0.0
                } else {
                    source_pixel[3]
                };
                *output_pixel = [source_pixel[0], source_pixel[1], source_pixel[2], alpha];
            }
        })
    }

    /// A single-input op converting straight (non-premultiplied) alpha to premultiplied:
    /// each color channel is multiplied by alpha.
    pub fn premultiply() -> PassOp<'static> {
        Box::new(|inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                let alpha = source_pixel[3];
                *output_pixel = [
                    source_pixel[0] * alpha,
                    source_pixel[1] * alpha,
                    source_pixel[2] * alpha,
                    alpha,
                ];
            }
        })
    }

    /// A single-input op converting premultiplied alpha back to straight: each color
    /// channel is divided by alpha (fully-transparent pixels become zero).
    pub fn unpremultiply() -> PassOp<'static> {
        Box::new(|inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                let alpha = source_pixel[3];
                *output_pixel = if alpha <= f32::EPSILON {
                    [0.0, 0.0, 0.0, alpha]
                } else {
                    [
                        source_pixel[0] / alpha,
                        source_pixel[1] / alpha,
                        source_pixel[2] / alpha,
                        alpha,
                    ]
                };
            }
        })
    }

    /// A single-input op applying a 3x3 convolution `kernel` to `inputs[0]` (edge-clamped)
    /// — the general filter primitive behind sharpen, emboss, and edge detection. Alpha
    /// passes through; RGB results are clamped to `0..=1`.
    pub fn convolve_3x3(kernel: [[f32; 3]; 3]) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            if source.width != output.width || source.height != output.height {
                return;
            }
            let (width, height) = (output.width as i32, output.height as i32);
            for y in 0..height {
                for x in 0..width {
                    let mut acc = [0.0f32; 3];
                    for (ky, row) in kernel.iter().enumerate() {
                        for (kx, weight) in row.iter().enumerate() {
                            let sx = (x + kx as i32 - 1).clamp(0, width - 1);
                            let sy = (y + ky as i32 - 1).clamp(0, height - 1);
                            let pixel = source.pixels[(sy * width + sx) as usize];
                            for channel in 0..3 {
                                acc[channel] += weight * pixel[channel];
                            }
                        }
                    }
                    let index = (y * width + x) as usize;
                    let alpha = source.pixels[index][3];
                    output.pixels[index] = [
                        acc[0].clamp(0.0, 1.0),
                        acc[1].clamp(0.0, 1.0),
                        acc[2].clamp(0.0, 1.0),
                        alpha,
                    ];
                }
            }
        })
    }

    /// A single-input op suppressing green spill: clamps each pixel's green channel to at
    /// most the maximum of its red and blue (the standard green-screen despill). Use after
    /// [`chroma_key`] to remove green fringing on retained subjects. Red, blue, and alpha
    /// pass through.
    pub fn despill_green() -> PassOp<'static> {
        Box::new(|inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                let limit = source_pixel[0].max(source_pixel[2]);
                *output_pixel = [
                    source_pixel[0],
                    source_pixel[1].min(limit),
                    source_pixel[2],
                    source_pixel[3],
                ];
            }
        })
    }

    /// A single-input op adjusting exposure by `stops`: each RGB channel is multiplied by
    /// `2^stops` and clamped to `0..=1` (alpha unchanged). `stops` 0 is a passthrough.
    pub fn exposure(stops: f32) -> PassOp<'static> {
        let gain = 2.0f32.powf(stops);
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                *output_pixel = [
                    (source_pixel[0] * gain).clamp(0.0, 1.0),
                    (source_pixel[1] * gain).clamp(0.0, 1.0),
                    (source_pixel[2] * gain).clamp(0.0, 1.0),
                    source_pixel[3],
                ];
            }
        })
    }

    /// A single-input op applying a `power` gamma curve to each RGB channel (clamped to
    /// `0..=1`, alpha unchanged). `power` 1 is a passthrough; non-positive powers pass
    /// the channel through.
    pub fn gamma(power: f32) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            let curve = |value: f32| {
                if power > 0.0 {
                    value.clamp(0.0, 1.0).powf(power)
                } else {
                    value
                }
            };
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                *output_pixel = [
                    curve(source_pixel[0]),
                    curve(source_pixel[1]),
                    curve(source_pixel[2]),
                    source_pixel[3],
                ];
            }
        })
    }

    /// A single-input primary color-correction op applying per-channel **lift / gamma /
    /// gain** (the colorist three-way / color-wheels model) to `inputs[0]`. Per RGB
    /// channel `c`: `out = clamp(in * gain[c] + lift[c], 0, 1) ^ (1 / gamma[c])` — `gain`
    /// scales (highlights move most), `lift` offsets (shadows move most), and `gamma`
    /// reshapes midtones (a value > 1 brightens them). Alpha is passed through. The
    /// identity is `lift = [0; 3]`, `gamma = [1; 3]`, `gain = [1; 3]`; a non-positive
    /// `gamma[c]` skips that channel's power step.
    pub fn lift_gamma_gain(lift: [f32; 3], gamma: [f32; 3], gain: [f32; 3]) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            let grade = |value: f32, lift: f32, gamma: f32, gain: f32| {
                let scaled = (value * gain + lift).clamp(0.0, 1.0);
                if gamma > 0.0 {
                    scaled.powf(1.0 / gamma)
                } else {
                    scaled
                }
            };
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                *output_pixel = [
                    grade(source_pixel[0], lift[0], gamma[0], gain[0]),
                    grade(source_pixel[1], lift[1], gamma[1], gain[1]),
                    grade(source_pixel[2], lift[2], gamma[2], gain[2]),
                    source_pixel[3],
                ];
            }
        })
    }

    /// A single-input op that mirrors `inputs[0]` left-to-right.
    pub fn flip_horizontal() -> PassOp<'static> {
        Box::new(|inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            if source.width != output.width || source.height != output.height {
                return;
            }
            let width = output.width;
            for y in 0..output.height {
                for x in 0..width {
                    output.pixels[(y * width + x) as usize] =
                        source.pixels[(y * width + (width - 1 - x)) as usize];
                }
            }
        })
    }

    /// A single-input op that mirrors `inputs[0]` top-to-bottom.
    pub fn flip_vertical() -> PassOp<'static> {
        Box::new(|inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            if source.width != output.width || source.height != output.height {
                return;
            }
            let (width, height) = (output.width, output.height);
            for y in 0..height {
                for x in 0..width {
                    output.pixels[(y * width + x) as usize] =
                        source.pixels[((height - 1 - y) * width + x) as usize];
                }
            }
        })
    }

    /// A single-input op that fits `inputs[0]` into the output preserving its aspect ratio,
    /// centered, with transparent bars (letterbox / pillarbox) — for placing content of a
    /// different resolution or aspect on the output frame.
    pub fn letterbox() -> PassOp<'static> {
        Box::new(|inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            if source.width == 0 || source.height == 0 {
                return;
            }
            let (out_w, out_h) = (output.width as f32, output.height as f32);
            let (src_w, src_h) = (source.width as f32, source.height as f32);
            let scale = (out_w / src_w).min(out_h / src_h);
            let fit_w = src_w * scale;
            let fit_h = src_h * scale;
            let offset_x = (out_w - fit_w) / 2.0;
            let offset_y = (out_h - fit_h) / 2.0;
            for y in 0..output.height {
                for x in 0..output.width {
                    let fit_x = x as f32 + 0.5 - offset_x;
                    let fit_y = y as f32 + 0.5 - offset_y;
                    let index = (y * output.width + x) as usize;
                    output.pixels[index] =
                        if fit_x >= 0.0 && fit_x < fit_w && fit_y >= 0.0 && fit_y < fit_h {
                            let u = fit_x / fit_w * src_w - 0.5;
                            let v = fit_y / fit_h * src_h - 0.5;
                            sample_bilinear(source, u, v)
                        } else {
                            [0.0; 4]
                        };
                }
            }
        })
    }

    /// A single-input op adjusting saturation: each RGB channel is blended toward the
    /// Rec.709 luma by `amount` (`1` unchanged, `0` grayscale, `> 1` boost), clamped to
    /// `0..=1`. Alpha passes through. Completes the exposure/gamma/saturation trio.
    pub fn saturation(amount: f32) -> PassOp<'static> {
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                let luma =
                    0.2126 * source_pixel[0] + 0.7152 * source_pixel[1] + 0.0722 * source_pixel[2];
                let mix = |channel: f32| (luma + amount * (channel - luma)).clamp(0.0, 1.0);
                *output_pixel = [
                    mix(source_pixel[0]),
                    mix(source_pixel[1]),
                    mix(source_pixel[2]),
                    source_pixel[3],
                ];
            }
        })
    }

    /// A single-input op that posterizes `inputs[0]` to `levels` discrete steps per RGB
    /// channel (clamped to at least 2) — the stylize/banding effect. Alpha passes through.
    pub fn posterize(levels: u32) -> PassOp<'static> {
        let steps = levels.max(2) as f32 - 1.0;
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            let quantize = |value: f32| (value.clamp(0.0, 1.0) * steps).round() / steps;
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                *output_pixel = [
                    quantize(source_pixel[0]),
                    quantize(source_pixel[1]),
                    quantize(source_pixel[2]),
                    source_pixel[3],
                ];
            }
        })
    }

    /// A single-input op that inverts `inputs[0]`'s color (negative): each RGB channel
    /// becomes `1 - channel`. Alpha passes through.
    pub fn invert() -> PassOp<'static> {
        Box::new(|inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                *output_pixel = [
                    1.0 - source_pixel[0],
                    1.0 - source_pixel[1],
                    1.0 - source_pixel[2],
                    source_pixel[3],
                ];
            }
        })
    }

    /// A single-input op that rotates hue by `radians` around the luminance axis, using
    /// the W3C feColorMatrix hue-rotate matrix (identity at 0, luminance-preserving so
    /// neutrals are unchanged). RGB results are clamped to `0..=1`; alpha passes through.
    pub fn hue_rotate(radians: f32) -> PassOp<'static> {
        let (sin, cos) = radians.sin_cos();
        let matrix = [
            [
                0.213 + cos * 0.787 - sin * 0.213,
                0.715 - cos * 0.715 - sin * 0.715,
                0.072 - cos * 0.072 + sin * 0.928,
            ],
            [
                0.213 - cos * 0.213 + sin * 0.143,
                0.715 + cos * 0.285 + sin * 0.140,
                0.072 - cos * 0.072 - sin * 0.283,
            ],
            [
                0.213 - cos * 0.213 - sin * 0.787,
                0.715 - cos * 0.715 + sin * 0.715,
                0.072 + cos * 0.928 + sin * 0.072,
            ],
        ];
        Box::new(move |inputs, output| {
            let Some(source) = inputs.first() else {
                return;
            };
            for (output_pixel, source_pixel) in output.pixels.iter_mut().zip(&source.pixels) {
                let (r, g, b) = (source_pixel[0], source_pixel[1], source_pixel[2]);
                let channel =
                    |row: [f32; 3]| (row[0] * r + row[1] * g + row[2] * b).clamp(0.0, 1.0);
                *output_pixel = [
                    channel(matrix[0]),
                    channel(matrix[1]),
                    channel(matrix[2]),
                    source_pixel[3],
                ];
            }
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{PassDesc, ResourceDesc};

        #[test]
        fn executes_a_composite() {
            let mut graph = RenderGraph::new();
            let overlay = graph.add_resource(ResourceDesc::transient_texture("overlay"));
            let bg = graph.add_resource(ResourceDesc::imported_texture("bg"));
            let out = graph.add_resource(ResourceDesc::transient_texture("out"));

            let paint = graph.add_pass(PassDesc::new("paint").write(overlay));
            let comp = graph.add_pass(PassDesc::new("comp").read(overlay).read(bg).write(out));
            let compiled = graph.compile().unwrap();

            let mut imported = HashMap::new();
            imported.insert(bg, Image::filled(2, 2, [0.0, 0.0, 1.0, 1.0]));

            let mut ops: HashMap<PassId, PassOp<'static>> = HashMap::new();
            ops.insert(paint, fill([1.0, 0.0, 0.0, 0.5]));
            ops.insert(comp, Box::new(blend_over));

            let result = execute(&graph, &compiled, 2, 2, &imported, &ops).unwrap();
            let pixel = result[&out].pixel(1, 1);
            assert!((pixel[0] - 0.5).abs() < 1e-6, "{pixel:?}");
            assert!((pixel[1] - 0.0).abs() < 1e-6, "{pixel:?}");
            assert!((pixel[2] - 0.5).abs() < 1e-6, "{pixel:?}");
            assert!((pixel[3] - 1.0).abs() < 1e-6, "{pixel:?}");
        }

        #[test]
        fn errors_on_unproduced_input() {
            let mut graph = RenderGraph::new();
            let missing = graph.add_resource(ResourceDesc::transient_texture("missing"));
            let out = graph.add_resource(ResourceDesc::transient_texture("out"));
            graph.add_pass(PassDesc::new("p").read(missing).write(out));
            let compiled = graph.compile().unwrap();
            let result = execute(&graph, &compiled, 1, 1, &HashMap::new(), &HashMap::new());
            assert!(result.is_err());
        }

        #[test]
        fn blend_modes_combine_layers() {
            let gray = Image::filled(1, 1, [0.5, 0.5, 0.5, 1.0]);
            let run = |mode| {
                let mut out = Image::new(1, 1);
                blend(mode)(&[&gray, &gray], &mut out);
                out.pixel(0, 0)[0]
            };
            assert!((run(BlendMode::Normal) - 0.5).abs() < 1e-6);
            assert!((run(BlendMode::Multiply) - 0.25).abs() < 1e-6);
            assert!((run(BlendMode::Screen) - 0.75).abs() < 1e-6);
            assert!((run(BlendMode::Add) - 1.0).abs() < 1e-6);
        }

        #[test]
        fn separable_blend_modes_match_w3c() {
            // Opaque layers reduce the W3C compositing formula to the raw blend, so
            // the output channel equals BlendMode::apply(backdrop, source) exactly.
            let backdrop = Image::filled(1, 1, [0.6, 0.6, 0.6, 1.0]);
            let source = Image::filled(1, 1, [0.25, 0.25, 0.25, 1.0]);
            let run = |mode| {
                let mut out = Image::new(1, 1);
                blend(mode)(&[&source, &backdrop], &mut out);
                out.pixel(0, 0)[0]
            };
            let approx = |actual: f32, expected: f32| (actual - expected).abs() < 1e-5;
            assert!(approx(run(BlendMode::Darken), 0.25));
            assert!(approx(run(BlendMode::Lighten), 0.6));
            assert!(approx(run(BlendMode::HardLight), 0.30));
            assert!(approx(run(BlendMode::Overlay), 0.40));
            assert!(approx(run(BlendMode::Difference), 0.35));
            assert!(approx(run(BlendMode::Exclusion), 0.55));
            assert!(approx(run(BlendMode::ColorDodge), 0.8));
            assert!(approx(run(BlendMode::ColorBurn), 0.0));
            assert!(approx(run(BlendMode::SoftLight), 0.48));
        }

        #[test]
        fn blend_mode_boundary_cases_are_defined() {
            // Dodge/burn have spec-defined behavior at the 0 and 1 source boundaries.
            let white = Image::filled(1, 1, [1.0, 1.0, 1.0, 1.0]);
            let black = Image::filled(1, 1, [0.0, 0.0, 0.0, 1.0]);
            let gray = Image::filled(1, 1, [0.5, 0.5, 0.5, 1.0]);
            let channel = |mode, top: &Image, bottom: &Image| {
                let mut out = Image::new(1, 1);
                blend(mode)(&[top, bottom], &mut out);
                out.pixel(0, 0)[0]
            };
            // A fully-white source dodges the backdrop to white.
            assert!((channel(BlendMode::ColorDodge, &white, &gray) - 1.0).abs() < 1e-6);
            // A fully-black source burns the backdrop to black.
            assert!((channel(BlendMode::ColorBurn, &black, &gray) - 0.0).abs() < 1e-6);
        }

        #[test]
        fn translate_shifts_pixels() {
            let source = Image::filled(2, 2, [1.0, 0.0, 0.0, 1.0]);
            let mut out = Image::new(2, 2);
            translate(1, 0)(&[&source], &mut out);
            assert_eq!(out.pixel(0, 0), [0.0, 0.0, 0.0, 0.0]);
            assert_eq!(out.pixel(1, 0), [1.0, 0.0, 0.0, 1.0]);
        }

        #[test]
        fn crossfade_dissolves_between_inputs() {
            let red = Image::filled(1, 1, [1.0, 0.0, 0.0, 1.0]);
            let blue = Image::filled(1, 1, [0.0, 0.0, 1.0, 1.0]);
            let mut out = Image::new(1, 1);

            crossfade(0.0)(&[&red, &blue], &mut out);
            assert_eq!(out.pixel(0, 0), [1.0, 0.0, 0.0, 1.0]);
            crossfade(1.0)(&[&red, &blue], &mut out);
            assert_eq!(out.pixel(0, 0), [0.0, 0.0, 1.0, 1.0]);
            crossfade(0.5)(&[&red, &blue], &mut out);
            let mid = out.pixel(0, 0);
            assert!((mid[0] - 0.5).abs() < 1e-6 && (mid[2] - 0.5).abs() < 1e-6);
        }

        #[test]
        fn wipe_reveals_second_input_from_left() {
            let red = Image::filled(4, 1, [1.0, 0.0, 0.0, 1.0]);
            let blue = Image::filled(4, 1, [0.0, 0.0, 1.0, 1.0]);
            let mut out = Image::new(4, 1);
            wipe_horizontal(0.5)(&[&red, &blue], &mut out);
            assert_eq!(out.pixel(0, 0), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(out.pixel(1, 0), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(out.pixel(2, 0), [1.0, 0.0, 0.0, 1.0]);
            assert_eq!(out.pixel(3, 0), [1.0, 0.0, 0.0, 1.0]);
        }

        #[test]
        fn wipe_vertical_reveals_from_top() {
            let red = Image::filled(1, 4, [1.0, 0.0, 0.0, 1.0]);
            let blue = Image::filled(1, 4, [0.0, 0.0, 1.0, 1.0]);
            let mut out = Image::new(1, 4);
            wipe_vertical(0.5)(&[&red, &blue], &mut out);
            assert_eq!(out.pixel(0, 0), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(out.pixel(0, 1), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(out.pixel(0, 2), [1.0, 0.0, 0.0, 1.0]);
            assert_eq!(out.pixel(0, 3), [1.0, 0.0, 0.0, 1.0]);
        }

        #[test]
        fn dip_to_color_passes_through_color_at_midpoint() {
            let red = Image::filled(1, 1, [1.0, 0.0, 0.0, 1.0]);
            let blue = Image::filled(1, 1, [0.0, 0.0, 1.0, 1.0]);
            let black = [0.0, 0.0, 0.0, 1.0];
            let run = |progress| {
                let mut out = Image::new(1, 1);
                dip_to_color(black, progress)(&[&red, &blue], &mut out);
                out.pixel(0, 0)
            };
            assert_eq!(run(0.0), [1.0, 0.0, 0.0, 1.0]);
            assert_eq!(run(1.0), [0.0, 0.0, 1.0, 1.0]);
            let mid = run(0.5);
            assert!(
                mid.iter().zip(black).all(|(a, b)| (a - b).abs() < 1e-6),
                "midpoint should be the dip color: {mid:?}"
            );
        }

        #[test]
        fn iris_expands_from_center() {
            let red = Image::filled(3, 3, [1.0, 0.0, 0.0, 1.0]);
            let blue = Image::filled(3, 3, [0.0, 0.0, 1.0, 1.0]);
            let mut out0 = Image::new(3, 3);
            iris(0.0)(&[&red, &blue], &mut out0);
            assert!(out0.pixels.iter().all(|p| *p == [1.0, 0.0, 0.0, 1.0]));
            let mut out1 = Image::new(3, 3);
            iris(1.0)(&[&red, &blue], &mut out1);
            assert!(out1.pixels.iter().all(|p| *p == [0.0, 0.0, 1.0, 1.0]));
            let mut mid = Image::new(3, 3);
            iris(0.2)(&[&red, &blue], &mut mid);
            assert_eq!(mid.pixel(1, 1), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(mid.pixel(0, 0), [1.0, 0.0, 0.0, 1.0]);
        }

        #[test]
        fn push_horizontal_slides_in_from_right() {
            let red = Image::filled(4, 1, [1.0, 0.0, 0.0, 1.0]);
            let blue = Image::filled(4, 1, [0.0, 0.0, 1.0, 1.0]);
            let run = |progress| {
                let mut out = Image::new(4, 1);
                push_horizontal(progress)(&[&red, &blue], &mut out);
                out
            };
            assert!(run(0.0).pixels.iter().all(|p| *p == [1.0, 0.0, 0.0, 1.0]));
            assert!(run(1.0).pixels.iter().all(|p| *p == [0.0, 0.0, 1.0, 1.0]));
            let half = run(0.5);
            assert_eq!(half.pixel(0, 0), [1.0, 0.0, 0.0, 1.0]);
            assert_eq!(half.pixel(3, 0), [0.0, 0.0, 1.0, 1.0]);
        }

        #[test]
        fn resample_is_identity_at_same_size() {
            let mut source = Image::new(2, 2);
            source.pixels = vec![
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
                [1.0, 1.0, 0.0, 1.0],
            ];
            let mut out = Image::new(2, 2);
            resample()(&[&source], &mut out);
            assert_eq!(out.pixels, source.pixels);
        }

        #[test]
        fn resample_upscales_solid_color() {
            let source = Image::filled(2, 2, [0.4, 0.6, 0.8, 1.0]);
            let mut out = Image::new(8, 8);
            resample()(&[&source], &mut out);
            assert!(out.pixels.iter().all(|pixel| pixel
                .iter()
                .zip([0.4, 0.6, 0.8, 1.0])
                .all(|(a, b)| (a - b).abs() < 1e-5)));
        }

        #[test]
        fn place_fills_destination_rect_and_clears_outside() {
            let source = Image::filled(1, 1, [1.0, 0.0, 0.0, 1.0]);
            let mut out = Image::new(4, 4);
            // Place the source into the top-left 2x2 quadrant.
            place(0, 0, 2, 2)(&[&source], &mut out);
            assert_eq!(out.pixel(0, 0), [1.0, 0.0, 0.0, 1.0]);
            assert_eq!(out.pixel(1, 1), [1.0, 0.0, 0.0, 1.0]);
            // Outside the 2x2 rect is transparent.
            assert_eq!(out.pixel(2, 2), [0.0, 0.0, 0.0, 0.0]);
            assert_eq!(out.pixel(3, 0), [0.0, 0.0, 0.0, 0.0]);
        }

        #[test]
        fn place_respects_offset_position() {
            let source = Image::filled(1, 1, [0.0, 1.0, 0.0, 1.0]);
            let mut out = Image::new(4, 4);
            place(2, 1, 1, 1)(&[&source], &mut out);
            assert_eq!(out.pixel(2, 1), [0.0, 1.0, 0.0, 1.0]);
            assert_eq!(out.pixel(0, 0), [0.0, 0.0, 0.0, 0.0]);
        }

        #[test]
        fn box_blur_radius_zero_is_passthrough() {
            let mut source = Image::new(2, 2);
            source.pixels = vec![
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
                [1.0, 1.0, 0.0, 1.0],
            ];
            let mut out = Image::new(2, 2);
            box_blur(0)(&[&source], &mut out);
            assert_eq!(out.pixels, source.pixels);
        }

        #[test]
        fn box_blur_preserves_solids_and_smooths_edges() {
            // A solid fill is unchanged (averaging equal neighbors).
            let solid = Image::filled(3, 3, [0.5, 0.5, 0.5, 1.0]);
            let mut out = Image::new(3, 3);
            box_blur(1)(&[&solid], &mut out);
            assert!(out.pixels.iter().all(|p| (p[0] - 0.5).abs() < 1e-6));

            // A black|white edge smooths monotonically.
            let mut edge = Image::new(4, 1);
            edge.pixels = vec![
                [0.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0, 1.0],
                [1.0, 1.0, 1.0, 1.0],
                [1.0, 1.0, 1.0, 1.0],
            ];
            let mut blurred = Image::new(4, 1);
            box_blur(1)(&[&edge], &mut blurred);
            let red: Vec<f32> = blurred.pixels.iter().map(|p| p[0]).collect();
            assert!((red[0]).abs() < 1e-6 && (red[3] - 1.0).abs() < 1e-6);
            assert!(
                red[1] > red[0] && red[2] > red[1] && red[3] > red[2],
                "{red:?}"
            );
        }

        #[test]
        fn luma_key_masks_dark_pixels_and_keeps_color() {
            let mut source = Image::new(3, 1);
            source.pixels = vec![
                [0.0, 0.0, 0.0, 1.0],    // black, luma 0 -> keyed
                [0.25, 0.25, 0.25, 1.0], // dark gray, luma 0.25 -> keyed
                [1.0, 1.0, 1.0, 1.0],    // white, luma 1 -> kept
            ];
            let mut out = Image::new(3, 1);
            luma_key(0.5)(&[&source], &mut out);
            assert_eq!(out.pixel(0, 0)[3], 0.0);
            assert_eq!(out.pixel(1, 0)[3], 0.0);
            assert_eq!(out.pixel(2, 0)[3], 1.0);
            // Color channels are preserved; only alpha changes.
            assert_eq!(out.pixel(2, 0), [1.0, 1.0, 1.0, 1.0]);
            assert_eq!(&out.pixel(0, 0)[0..3], &[0.0, 0.0, 0.0]);
        }

        #[test]
        fn chroma_key_removes_key_color_and_keeps_others() {
            let mut source = Image::new(3, 1);
            source.pixels = vec![
                [0.0, 1.0, 0.0, 1.0], // pure green -> keyed
                [0.1, 0.9, 0.1, 1.0], // near green (dist ~0.17 < 0.2) -> keyed
                [1.0, 0.0, 0.0, 1.0], // red (far) -> kept
            ];
            let mut out = Image::new(3, 1);
            chroma_key([0.0, 1.0, 0.0], 0.2)(&[&source], &mut out);
            assert_eq!(out.pixel(0, 0)[3], 0.0);
            assert_eq!(out.pixel(1, 0)[3], 0.0);
            assert_eq!(out.pixel(2, 0)[3], 1.0);
            // Color channels are preserved for kept pixels.
            assert_eq!(&out.pixel(2, 0)[0..3], &[1.0, 0.0, 0.0]);
        }

        #[test]
        fn premultiply_unpremultiply_round_trip() {
            let mut source = Image::new(2, 1);
            source.pixels = vec![[1.0, 0.5, 0.0, 0.5], [0.0, 0.0, 0.0, 0.0]];

            let mut premultiplied = Image::new(2, 1);
            premultiply()(&[&source], &mut premultiplied);
            // Straight [1, 0.5, 0] at alpha 0.5 -> premultiplied [0.5, 0.25, 0].
            assert_eq!(premultiplied.pixel(0, 0), [0.5, 0.25, 0.0, 0.5]);
            assert_eq!(premultiplied.pixel(1, 0), [0.0, 0.0, 0.0, 0.0]);

            let mut restored = Image::new(2, 1);
            unpremultiply()(&[&premultiplied], &mut restored);
            assert_eq!(restored.pixel(0, 0), [1.0, 0.5, 0.0, 0.5]);
            // Fully transparent stays zero (no divide by zero).
            assert_eq!(restored.pixel(1, 0), [0.0, 0.0, 0.0, 0.0]);
        }

        #[test]
        fn convolve_identity_is_passthrough() {
            let identity = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]];
            let mut source = Image::new(2, 2);
            source.pixels = vec![
                [0.2, 0.4, 0.6, 1.0],
                [0.1, 0.2, 0.3, 1.0],
                [0.9, 0.8, 0.7, 1.0],
                [0.5, 0.5, 0.5, 1.0],
            ];
            let mut out = Image::new(2, 2);
            convolve_3x3(identity)(&[&source], &mut out);
            for (got, want) in out.pixels.iter().zip(&source.pixels) {
                for channel in 0..3 {
                    assert!(
                        (got[channel] - want[channel]).abs() < 1e-6,
                        "{got:?} vs {want:?}"
                    );
                }
            }
        }

        #[test]
        fn convolve_sharpen_preserves_solids() {
            // A sum-to-one sharpen kernel leaves a flat region unchanged.
            let sharpen = [[0.0, -1.0, 0.0], [-1.0, 5.0, -1.0], [0.0, -1.0, 0.0]];
            let solid = Image::filled(3, 3, [0.5, 0.5, 0.5, 1.0]);
            let mut out = Image::new(3, 3);
            convolve_3x3(sharpen)(&[&solid], &mut out);
            assert!(out.pixels.iter().all(|p| (p[0] - 0.5).abs() < 1e-6));
        }

        #[test]
        fn gaussian_blur_preserves_a_flat_region() {
            let solid = Image::filled(8, 8, [0.3, 0.6, 0.9, 1.0]);
            let mut out = Image::new(8, 8);
            gaussian_blur(2.0)(&[&solid], &mut out);
            for pixel in &out.pixels {
                for channel in 0..4 {
                    assert!((pixel[channel] - solid.pixels[0][channel]).abs() < 1e-5);
                }
            }
        }

        #[test]
        fn gaussian_blur_zero_sigma_is_identity() {
            let mut source = Image::new(3, 3);
            source.pixels[4] = [1.0, 0.5, 0.25, 1.0];
            let mut out = Image::new(3, 3);
            gaussian_blur(0.0)(&[&source], &mut out);
            assert_eq!(out.pixels, source.pixels);
        }

        #[test]
        fn gaussian_blur_spreads_an_impulse_symmetrically() {
            let mut source = Image::new(5, 5);
            source.pixels[2 * 5 + 2] = [1.0, 0.0, 0.0, 0.0];
            let mut out = Image::new(5, 5);
            gaussian_blur(1.0)(&[&source], &mut out);

            let center = out.pixel(2, 2)[0];
            assert!(center > 0.0 && center < 1.0);
            assert!((out.pixel(1, 2)[0] - out.pixel(3, 2)[0]).abs() < 1e-6);
            assert!((out.pixel(2, 1)[0] - out.pixel(2, 3)[0]).abs() < 1e-6);
            assert!((out.pixel(1, 1)[0] - out.pixel(3, 3)[0]).abs() < 1e-6);
            assert!((out.pixel(1, 1)[0] - out.pixel(1, 3)[0]).abs() < 1e-6);
            assert!(out.pixel(1, 2)[0] > 0.0);
            assert!(center > out.pixel(1, 2)[0]);
            assert!(out.pixel(1, 2)[0] > out.pixel(0, 2)[0]);
        }

        #[test]
        fn unsharp_mask_zero_amount_is_identity() {
            let mut source = Image::new(4, 4);
            source.pixels[5] = [0.8, 0.2, 0.5, 1.0];
            source.pixels[10] = [0.1, 0.9, 0.3, 0.5];
            let mut out = Image::new(4, 4);
            unsharp_mask(2.0, 0.0)(&[&source], &mut out);
            for (got, want) in out.pixels.iter().zip(&source.pixels) {
                for channel in 0..4 {
                    assert!(
                        (got[channel] - want[channel]).abs() < 1e-6,
                        "{got:?} vs {want:?}"
                    );
                }
            }
        }

        #[test]
        fn unsharp_mask_preserves_a_flat_region() {
            let solid = Image::filled(6, 6, [0.4, 0.5, 0.6, 1.0]);
            let mut out = Image::new(6, 6);
            unsharp_mask(1.5, 1.5)(&[&solid], &mut out);
            for pixel in &out.pixels {
                for channel in 0..4 {
                    assert!((pixel[channel] - solid.pixels[0][channel]).abs() < 1e-5);
                }
            }
        }

        #[test]
        fn unsharp_mask_overshoots_at_an_edge() {
            let mut source = Image::new(6, 1);
            for x in 0..3 {
                source.pixels[x] = [0.3, 0.3, 0.3, 1.0];
            }
            for x in 3..6 {
                source.pixels[x] = [0.7, 0.7, 0.7, 1.0];
            }
            let mut out = Image::new(6, 1);
            unsharp_mask(1.0, 1.0)(&[&source], &mut out);

            // The dark pixel adjacent to the edge undershoots below 0.3; the bright one
            // overshoots above 0.7 — the signature of an unsharp mask raising contrast.
            assert!(out.pixel(2, 0)[0] < 0.3, "{:?}", out.pixel(2, 0));
            assert!(out.pixel(3, 0)[0] > 0.7, "{:?}", out.pixel(3, 0));
            assert!((out.pixel(2, 0)[3] - 1.0).abs() < 1e-6);
        }

        #[test]
        fn convolve_box_kernel_averages_neighborhood() {
            let box_kernel = [[1.0 / 9.0; 3]; 3];
            let mut source = Image::new(3, 1);
            source.pixels = vec![
                [0.0, 0.0, 0.0, 1.0],
                [0.9, 0.9, 0.9, 1.0],
                [0.0, 0.0, 0.0, 1.0],
            ];
            let mut out = Image::new(3, 1);
            convolve_3x3(box_kernel)(&[&source], &mut out);
            // Center: 3 (vertically-clamped) rows of (0 + 0.9 + 0) / 9 = 0.3.
            assert!(
                (out.pixel(1, 0)[0] - 0.3).abs() < 1e-6,
                "{:?}",
                out.pixel(1, 0)
            );
        }

        #[test]
        fn despill_green_limits_green_to_red_blue_maximum() {
            let mut source = Image::new(3, 1);
            source.pixels = vec![
                [0.3, 0.9, 0.4, 1.0], // green spill -> g = min(0.9, max(0.3, 0.4)) = 0.4
                [0.8, 0.2, 0.1, 1.0], // green already below the limit -> unchanged
                [1.0, 1.0, 1.0, 1.0], // white -> unchanged
            ];
            let mut out = Image::new(3, 1);
            despill_green()(&[&source], &mut out);
            assert_eq!(out.pixel(0, 0), [0.3, 0.4, 0.4, 1.0]);
            assert_eq!(out.pixel(1, 0), [0.8, 0.2, 0.1, 1.0]);
            assert_eq!(out.pixel(2, 0), [1.0, 1.0, 1.0, 1.0]);
        }

        #[test]
        fn exposure_and_gamma_adjust_channels() {
            let mut source = Image::new(1, 1);
            source.pixels = vec![[0.25, 0.5, 1.0, 0.5]];

            // +1 stop doubles each channel (clamped); alpha unchanged.
            let mut brighter = Image::new(1, 1);
            exposure(1.0)(&[&source], &mut brighter);
            assert_eq!(brighter.pixel(0, 0), [0.5, 1.0, 1.0, 0.5]);

            // 0 stops is a passthrough.
            let mut same = Image::new(1, 1);
            exposure(0.0)(&[&source], &mut same);
            assert_eq!(same.pixel(0, 0), source.pixels[0]);

            // Gamma 2 squares; alpha unchanged.
            let mut squared = Image::new(1, 1);
            gamma(2.0)(&[&source], &mut squared);
            assert!((squared.pixel(0, 0)[0] - 0.0625).abs() < 1e-6);
            assert!((squared.pixel(0, 0)[1] - 0.25).abs() < 1e-6);
            assert_eq!(squared.pixel(0, 0)[3], 0.5);

            // Gamma 0.5 is a square root.
            let mut rooted = Image::new(1, 1);
            gamma(0.5)(&[&source], &mut rooted);
            assert!((rooted.pixel(0, 0)[0] - 0.5).abs() < 1e-6);
        }

        #[test]
        fn lift_gamma_gain_identity_passes_through() {
            let mut source = Image::new(2, 1);
            source.pixels = vec![[0.2, 0.5, 0.8, 1.0], [0.0, 0.4, 1.0, 0.25]];
            let mut out = Image::new(2, 1);
            lift_gamma_gain([0.0; 3], [1.0; 3], [1.0; 3])(&[&source], &mut out);
            for (got, want) in out.pixels.iter().zip(&source.pixels) {
                for channel in 0..4 {
                    assert!(
                        (got[channel] - want[channel]).abs() < 1e-6,
                        "{got:?} vs {want:?}"
                    );
                }
            }
        }

        #[test]
        fn lift_gamma_gain_lift_raises_shadows_gain_scales_highlights() {
            let mut source = Image::new(2, 1);
            source.pixels = vec![[0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]];

            // Lift offsets every channel: black is raised to the lift value.
            let mut lifted = Image::new(2, 1);
            lift_gamma_gain([0.2; 3], [1.0; 3], [1.0; 3])(&[&source], &mut lifted);
            assert!((lifted.pixel(0, 0)[0] - 0.2).abs() < 1e-6);
            assert!((lifted.pixel(1, 0)[0] - 1.0).abs() < 1e-6);

            // Gain scales: white drops to the gain value while black (×gain) stays put.
            let mut gained = Image::new(2, 1);
            lift_gamma_gain([0.0; 3], [1.0; 3], [0.5; 3])(&[&source], &mut gained);
            assert!((gained.pixel(0, 0)[0] - 0.0).abs() < 1e-6);
            assert!((gained.pixel(1, 0)[0] - 0.5).abs() < 1e-6);

            // Gamma > 1 brightens midtones: 0.25 ^ (1/2) = 0.5.
            let mut mid = Image::new(1, 1);
            let gray = Image::filled(1, 1, [0.25, 0.25, 0.25, 1.0]);
            lift_gamma_gain([0.0; 3], [2.0; 3], [1.0; 3])(&[&gray], &mut mid);
            assert!((mid.pixel(0, 0)[0] - 0.5).abs() < 1e-6);
        }

        #[test]
        fn lift_gamma_gain_is_per_channel_and_clamped() {
            let gray = Image::filled(1, 1, [0.4, 0.4, 0.4, 0.5]);

            // Only the blue channel is gained; red and green are untouched; blue clamps.
            let mut out = Image::new(1, 1);
            lift_gamma_gain([0.0; 3], [1.0; 3], [1.0, 1.0, 3.0])(&[&gray], &mut out);
            assert!((out.pixel(0, 0)[0] - 0.4).abs() < 1e-6);
            assert!((out.pixel(0, 0)[1] - 0.4).abs() < 1e-6);
            assert!((out.pixel(0, 0)[2] - 1.0).abs() < 1e-6);
            assert_eq!(out.pixel(0, 0)[3], 0.5);
        }

        #[test]
        fn flips_mirror_the_image() {
            let red = [1.0, 0.0, 0.0, 1.0];
            let green = [0.0, 1.0, 0.0, 1.0];
            let blue = [0.0, 0.0, 1.0, 1.0];

            // Horizontal flip reverses columns; flipping twice is the identity.
            let mut source = Image::new(3, 1);
            source.pixels = vec![red, green, blue];
            let mut flipped = Image::new(3, 1);
            flip_horizontal()(&[&source], &mut flipped);
            assert_eq!(flipped.pixels, vec![blue, green, red]);
            let mut twice = Image::new(3, 1);
            flip_horizontal()(&[&flipped], &mut twice);
            assert_eq!(twice.pixels, source.pixels);

            // Vertical flip reverses rows.
            let mut column = Image::new(1, 3);
            column.pixels = vec![red, green, blue];
            let mut flipped_v = Image::new(1, 3);
            flip_vertical()(&[&column], &mut flipped_v);
            assert_eq!(flipped_v.pixels, vec![blue, green, red]);
        }

        #[test]
        fn letterbox_pillarboxes_a_square_into_a_wide_frame() {
            // A 2x2 source into a 4x2 output fits into columns 1-2; columns 0 and 3 are bars.
            let source = Image::filled(2, 2, [1.0, 1.0, 1.0, 1.0]);
            let mut out = Image::new(4, 2);
            letterbox()(&[&source], &mut out);
            assert_eq!(out.pixel(0, 0), [0.0, 0.0, 0.0, 0.0]);
            assert_eq!(out.pixel(3, 0), [0.0, 0.0, 0.0, 0.0]);
            assert_eq!(out.pixel(1, 0), [1.0, 1.0, 1.0, 1.0]);
            assert_eq!(out.pixel(2, 1), [1.0, 1.0, 1.0, 1.0]);
        }

        #[test]
        fn saturation_unchanged_at_one_grayscale_at_zero() {
            let color = [0.8, 0.4, 0.2, 1.0];
            let luma = 0.2126 * 0.8 + 0.7152 * 0.4 + 0.0722 * 0.2;
            let mut source = Image::new(1, 1);
            source.pixels = vec![color];

            // amount 1 leaves color unchanged.
            let mut same = Image::new(1, 1);
            saturation(1.0)(&[&source], &mut same);
            for channel in 0..3 {
                assert!((same.pixel(0, 0)[channel] - color[channel]).abs() < 1e-6);
            }

            // amount 0 collapses each channel to the luma; alpha unchanged.
            let mut gray = Image::new(1, 1);
            saturation(0.0)(&[&source], &mut gray);
            for channel in 0..3 {
                assert!((gray.pixel(0, 0)[channel] - luma).abs() < 1e-5);
            }
            assert_eq!(gray.pixel(0, 0)[3], 1.0);
        }

        #[test]
        fn posterize_quantizes_channels() {
            let mut source = Image::new(1, 1);
            source.pixels = vec![[0.3, 0.6, 0.9, 0.5]];

            // 2 levels round to 0 or 1; alpha passes through.
            let mut two = Image::new(1, 1);
            posterize(2)(&[&source], &mut two);
            assert_eq!(two.pixel(0, 0), [0.0, 1.0, 1.0, 0.5]);

            // 4 levels snap to the nearest of {0, 1/3, 2/3, 1}.
            let mut four = Image::new(1, 1);
            posterize(4)(&[&source], &mut four);
            let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
            assert!(close(four.pixel(0, 0)[0], 1.0 / 3.0));
            assert!(close(four.pixel(0, 0)[1], 2.0 / 3.0));
            assert!(close(four.pixel(0, 0)[2], 1.0));

            // Fewer than 2 levels clamps to 2.
            let mut one = Image::new(1, 1);
            posterize(1)(&[&source], &mut one);
            assert_eq!(one.pixel(0, 0), two.pixel(0, 0));
        }

        #[test]
        fn invert_negates_color_keeps_alpha() {
            let mut source = Image::new(1, 1);
            source.pixels = vec![[0.2, 0.5, 0.8, 0.5]];
            let mut out = Image::new(1, 1);
            invert()(&[&source], &mut out);
            let pixel = out.pixel(0, 0);
            assert!(
                (pixel[0] - 0.8).abs() < 1e-6
                    && (pixel[1] - 0.5).abs() < 1e-6
                    && (pixel[2] - 0.2).abs() < 1e-6
            );
            assert_eq!(pixel[3], 0.5);
            // Inverting twice restores the original.
            let mut back = Image::new(1, 1);
            invert()(&[&out], &mut back);
            for channel in 0..3 {
                assert!((back.pixel(0, 0)[channel] - source.pixels[0][channel]).abs() < 1e-6);
            }
        }

        #[test]
        fn hue_rotate_identity_at_zero_and_preserves_gray() {
            let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
            let color = [0.8, 0.4, 0.2, 1.0];
            let mut source = Image::new(1, 1);
            source.pixels = vec![color];

            // Angle 0 is the identity; alpha passes through.
            let mut at_zero = Image::new(1, 1);
            hue_rotate(0.0)(&[&source], &mut at_zero);
            for channel in 0..3 {
                assert!(close(at_zero.pixel(0, 0)[channel], color[channel]));
            }
            assert_eq!(at_zero.pixel(0, 0)[3], 1.0);

            // A neutral gray is invariant under any rotation (luminance-preserving).
            let gray = Image::filled(1, 1, [0.5, 0.5, 0.5, 1.0]);
            let mut rotated_gray = Image::new(1, 1);
            hue_rotate(2.0)(&[&gray], &mut rotated_gray);
            for channel in 0..3 {
                assert!(close(rotated_gray.pixel(0, 0)[channel], 0.5));
            }

            // A real rotation changes a saturated color.
            let mut shifted = Image::new(1, 1);
            hue_rotate(2.0)(&[&source], &mut shifted);
            assert!(
                (shifted.pixel(0, 0)[0] - color[0]).abs() > 1e-3
                    || (shifted.pixel(0, 0)[1] - color[1]).abs() > 1e-3
            );
        }
    }
}

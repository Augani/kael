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

//! Shared logical subtrees and bounded frame overlays for virtual controls.

use super::{AccessibilityId, AccessibilityNode};
use std::{
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut, Index},
    sync::Arc,
};

/// A child list whose clones share storage until it is changed.
#[derive(Clone, Debug, Eq)]
pub struct AccessibilityChildren(Arc<Vec<AccessibilityId>>);

impl Default for AccessibilityChildren {
    fn default() -> Self {
        static EMPTY: std::sync::OnceLock<Arc<Vec<AccessibilityId>>> = std::sync::OnceLock::new();
        Self(EMPTY.get_or_init(|| Arc::new(Vec::new())).clone())
    }
}

impl PartialEq for AccessibilityChildren {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}
impl From<Vec<AccessibilityId>> for AccessibilityChildren {
    fn from(value: Vec<AccessibilityId>) -> Self {
        Self(Arc::new(value))
    }
}
impl AccessibilityChildren {
    pub(crate) fn shares_storage(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Deref for AccessibilityChildren {
    type Target = Vec<AccessibilityId>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for AccessibilityChildren {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}
impl<'a> IntoIterator for &'a AccessibilityChildren {
    type Item = &'a AccessibilityId;
    type IntoIter = std::slice::Iter<'a, AccessibilityId>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// An immutable logical subtree, independent of physically mounted elements.
///
/// Build when the logical model changes and reuse the same `Arc` on every
/// frame. Node labels and child lists are retained once, including offscreen
/// items. Every node must be reachable from `root` through consistent parents.
pub struct AccessibilitySnapshot {
    /// The subtree's semantic root identity.
    pub root: AccessibilityId,
    pub(crate) nodes: HashMap<AccessibilityId, AccessibilityNode>,
    pub(crate) focused: Option<AccessibilityId>,
    // Prepared once with the logical model; actions never scan all logical rows.
    text_owners: Vec<AccessibilityId>,
    reclaimer: Option<crate::BackgroundExecutor>,
}
impl std::fmt::Debug for AccessibilitySnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessibilitySnapshot")
            .field("root", &self.root)
            .field("node_count", &self.nodes.len())
            .finish()
    }
}
impl PartialEq for AccessibilitySnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root && self.nodes == other.nodes
    }
}
impl Drop for AccessibilitySnapshot {
    fn drop(&mut self) {
        if let Some(executor) = self.reclaimer.take() {
            let nodes = std::mem::take(&mut self.nodes);
            executor
                .spawn(async move {
                    drop(nodes);
                })
                .detach();
        }
    }
}
impl AccessibilitySnapshot {
    /// Validate and retain a complete, uniquely identified logical subtree.
    pub fn new(
        root: AccessibilityId,
        nodes: impl IntoIterator<Item = AccessibilityNode>,
    ) -> anyhow::Result<Arc<Self>> {
        let nodes = nodes.into_iter();
        // Prepared virtual models know their node count. Reserve from the
        // guaranteed lower bound to avoid repeatedly moving large node records.
        let mut map = HashMap::with_capacity(nodes.size_hint().0);
        for node in nodes {
            anyhow::ensure!(
                map.insert(node.id, node).is_none(),
                "duplicate logical accessibility node"
            );
        }
        anyhow::ensure!(
            map.contains_key(&root),
            "logical accessibility root missing"
        );
        let mut seen = HashSet::with_capacity(map.len());
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            anyhow::ensure!(
                seen.insert(id),
                "logical accessibility subtree contains a cycle or duplicate child"
            );
            let node = map
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("logical accessibility child missing"))?;
            for child in &node.children {
                anyhow::ensure!(
                    map.get(child).is_some_and(|node| node.parent == Some(id)),
                    "logical accessibility parent mismatch"
                );
                pending.push(*child);
            }
        }
        anyhow::ensure!(
            seen.len() == map.len(),
            "logical accessibility subtree contains unreachable nodes"
        );
        let focused = map
            .values()
            .find(|node| node.states.contains(super::AccessibilityState::FOCUSED))
            .map(|node| node.id);
        let text_owners = map
            .values()
            .filter(|node| node.text_document.is_some())
            .map(|node| node.id)
            .collect();
        Ok(Arc::new(Self {
            root,
            nodes: map,
            focused,
            text_owners,
            reclaimer: None,
        }))
    }
    /// Build a snapshot whose final logical storage is reclaimed on a worker.
    /// This avoids dropping a large dataset on the paint/event thread when its
    /// outgoing frame and platform providers release their final references.
    pub fn with_reclaim_executor(
        root: AccessibilityId,
        nodes: impl IntoIterator<Item = AccessibilityNode>,
        executor: &crate::BackgroundExecutor,
    ) -> anyhow::Result<Arc<Self>> {
        let mut snapshot = Self::new(root, nodes)?;
        Arc::get_mut(&mut snapshot).unwrap().reclaimer = Some(executor.clone());
        Ok(snapshot)
    }
    /// Look up a logical node without copying its label or children.
    pub fn get(&self, id: AccessibilityId) -> Option<&AccessibilityNode> {
        self.nodes.get(&id)
    }
    /// Number of logical nodes, including the root.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    /// Whether this snapshot has no nodes (validated snapshots always have a root).
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Node storage combining ordinary frame nodes with shared logical snapshots.
///
/// Lookup and frame cloning visit only the bounded frame overlays and subtree
/// roots. Iteration exposes every effective node, with overlays taking priority.
#[derive(Clone, Debug, Default)]
pub struct AccessibilityNodeMap {
    pub(crate) frame: HashMap<AccessibilityId, AccessibilityNode>,
    pub(crate) snapshots: HashMap<AccessibilityId, Arc<AccessibilitySnapshot>>,
    removed: HashSet<AccessibilityId>,
}

#[cfg(test)]
thread_local! {
    static DELTA_CANDIDATE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DELTA_BASE_COMPARE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DELTA_REMOVAL_PROBE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
impl PartialEq for AccessibilityNodeMap {
    fn eq(&self, other: &Self) -> bool {
        self.frame == other.frame
            && self.removed == other.removed
            && self.snapshots.len() == other.snapshots.len()
            && self.snapshots.iter().all(|(id, value)| {
                other
                    .snapshots
                    .get(id)
                    .is_some_and(|old| Arc::ptr_eq(value, old) || value == old)
            })
    }
}
impl AccessibilityNodeMap {
    pub(super) fn text_owner(&self, run: AccessibilityId) -> Option<&AccessibilityNode> {
        let mut owners = self
            .frame
            .values()
            .filter(|node| !self.removed.contains(&node.id))
            .chain(
                self.snapshots
                    .values()
                    .flat_map(|snapshot| snapshot.text_owners.iter())
                    .filter(|id| !self.frame.contains_key(id))
                    .filter_map(|id| self.get(id)),
            )
            .filter(|node| {
                node.text_document
                    .as_ref()
                    .is_some_and(|document| document.run_bytes(run).is_some())
            });
        let owner = owners.next()?;
        // Shared run identities under two controls are ambiguous and unsafe.
        owners.next().is_none().then_some(owner)
    }
    /// Insert or replace an ordinary frame node.
    pub fn insert(
        &mut self,
        id: AccessibilityId,
        node: AccessibilityNode,
    ) -> Option<AccessibilityNode> {
        self.removed.remove(&id);
        self.frame.insert(id, node)
    }
    /// Remove an effective node, masking immutable logical storage if necessary.
    pub fn remove(&mut self, id: &AccessibilityId) -> Option<AccessibilityNode> {
        let node = self.get(id)?.clone();
        self.frame.remove(id);
        if self.base(id).is_some() {
            self.removed.insert(*id);
        }
        Some(node)
    }
    /// Look up a frame or offscreen logical node.
    pub fn get(&self, id: &AccessibilityId) -> Option<&AccessibilityNode> {
        if self.removed.contains(id) {
            return None;
        }
        self.frame.get(id).or_else(|| self.base(id))
    }
    pub(crate) fn base(&self, id: &AccessibilityId) -> Option<&AccessibilityNode> {
        self.snapshots
            .values()
            .find_map(|snapshot| snapshot.nodes.get(id))
    }
    /// Mutate one node using a copy-on-write frame overlay.
    pub fn get_mut(&mut self, id: &AccessibilityId) -> Option<&mut AccessibilityNode> {
        if self.removed.contains(id) {
            return None;
        }
        if !self.frame.contains_key(id) {
            let node = self.base(id)?.clone();
            self.frame.insert(*id, node);
        }
        self.frame.get_mut(id)
    }
    /// Whether any effective node has this identity.
    pub fn contains_key(&self, id: &AccessibilityId) -> bool {
        self.get(id).is_some()
    }
    /// Number of effective nodes. Snapshots must have disjoint identities.
    pub fn len(&self) -> usize {
        self.snapshots
            .values()
            .map(|snapshot| snapshot.len())
            .sum::<usize>()
            + self
                .frame
                .keys()
                .filter(|id| self.base(id).is_none())
                .count()
            - self.removed.len()
    }
    /// Whether there are no effective nodes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Iterate all effective nodes, including offscreen items.
    pub fn iter(&self) -> impl Iterator<Item = (&AccessibilityId, &AccessibilityNode)> {
        self.frame.iter().chain(
            self.snapshots
                .values()
                .flat_map(|snapshot| snapshot.nodes.iter())
                .filter(|(id, _)| !self.frame.contains_key(id) && !self.removed.contains(id)),
        )
    }
    /// Ordinary frame overlays, without scanning immutable logical subtrees.
    #[cfg(any(target_family = "wasm", test))]
    pub(crate) fn frame_nodes(&self) -> impl Iterator<Item = &AccessibilityNode> {
        self.frame.values()
    }
    /// Iterate all effective node identities.
    pub fn keys(&self) -> impl Iterator<Item = &AccessibilityId> {
        self.iter().map(|(id, _)| id)
    }
    /// Iterate all effective nodes.
    pub fn values(&self) -> impl Iterator<Item = &AccessibilityNode> {
        self.iter().map(|(_, node)| node)
    }
    /// Retain an immutable logical subtree; reusing its Arc makes updates cheap.
    ///
    /// Node identities must be disjoint from every other attached snapshot.
    /// Ordinary frame nodes may share those identities: they act as effective
    /// overlays, taking priority over immutable snapshot records.
    pub fn attach(&mut self, snapshot: Arc<AccessibilitySnapshot>) {
        self.removed.retain(|id| !snapshot.nodes.contains_key(id));
        self.snapshots.insert(snapshot.root, snapshot);
        self.removed.retain(|id| {
            self.snapshots
                .values()
                .any(|snapshot| snapshot.nodes.contains_key(id))
        });
    }
    /// Return whether a subtree is the same shared snapshot in another frame.
    pub fn shares_snapshot(&self, root: AccessibilityId, other: &Self) -> bool {
        self.snapshots
            .get(&root)
            .zip(other.snapshots.get(&root))
            .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
    }
    /// Nodes added or changed, and identities removed, relative to another frame.
    /// Shared snapshots are skipped without visiting their rows or labels.
    pub fn delta<'a>(&'a self, previous: Option<&Self>) -> AccessibilityTreeDelta<'a> {
        let Some(previous) = previous else {
            return AccessibilityTreeDelta {
                nodes: self.values().collect(),
                removed: Vec::new(),
            };
        };
        let mut candidates: HashSet<_> = self
            .frame
            .keys()
            .chain(previous.frame.keys())
            .copied()
            .collect();
        candidates.extend(self.removed.iter().chain(previous.removed.iter()).copied());
        #[cfg(test)]
        DELTA_CANDIDATE_COUNT.with(|count| count.set(candidates.len()));
        let mut nodes = Vec::new();
        let mut removed = Vec::new();
        for id in &candidates {
            match (self.get(id), previous.get(id)) {
                (Some(current), Some(old)) if current == old => {}
                (Some(current), _) => nodes.push(current),
                (None, Some(_)) => removed.push(*id),
                _ => {}
            }
        }
        // Only explicit frame overlays and tombstones need a candidate set.
        // Changed immutable snapshots are disjoint: compare their current base
        // records directly, then probe previous bases only for removed IDs. A
        // base masked by an overlay/tombstone was already compared above.
        #[cfg(test)]
        let (mut base_compares, mut removal_probes) = (0, 0);
        for (root, snapshot) in &self.snapshots {
            if self.shares_snapshot(*root, previous) {
                continue;
            }
            for (id, current) in &snapshot.nodes {
                if candidates.contains(id) {
                    continue;
                }
                #[cfg(test)]
                {
                    base_compares += 1;
                }
                if previous.get(id) != Some(current) {
                    nodes.push(current);
                }
            }
        }
        for (root, snapshot) in &previous.snapshots {
            if self.shares_snapshot(*root, previous) {
                continue;
            }
            for id in snapshot.nodes.keys() {
                if candidates.contains(id) {
                    continue;
                }
                #[cfg(test)]
                {
                    removal_probes += 1;
                }
                if !self.contains_key(id) {
                    removed.push(*id);
                }
            }
        }
        #[cfg(test)]
        {
            DELTA_BASE_COMPARE_COUNT.with(|count| count.set(base_compares));
            DELTA_REMOVAL_PROBE_COUNT.with(|count| count.set(removal_probes));
        }
        nodes.sort_unstable_by_key(|node| node.id.0);
        removed.sort_unstable_by_key(|id| id.0);
        AccessibilityTreeDelta { nodes, removed }
    }
}
impl Index<&AccessibilityId> for AccessibilityNodeMap {
    type Output = AccessibilityNode;
    fn index(&self, index: &AccessibilityId) -> &Self::Output {
        self.get(index).expect("accessibility node missing")
    }
}
impl<'a> IntoIterator for &'a AccessibilityNodeMap {
    type Item = (&'a AccessibilityId, &'a AccessibilityNode);
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;
    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

/// A bounded incremental semantic update when retained snapshots are unchanged.
pub struct AccessibilityTreeDelta<'a> {
    /// Current records that were added or changed.
    pub nodes: Vec<&'a AccessibilityNode>,
    /// Identities no longer present in the effective tree.
    pub removed: Vec<AccessibilityId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessibilityRect, AccessibilityRole, AccessibilityState, AccessibilityTree};

    #[derive(Default)]
    struct Changes {
        added: usize,
        updated: usize,
        removed: usize,
    }
    impl accesskit_consumer::TreeChangeHandler for Changes {
        fn node_added(&mut self, _: &accesskit_consumer::Node<'_>) {
            self.added += 1;
        }
        fn node_updated(
            &mut self,
            _: &accesskit_consumer::Node<'_>,
            _: &accesskit_consumer::Node<'_>,
        ) {
            self.updated += 1;
        }
        fn focus_moved(
            &mut self,
            _: Option<&accesskit_consumer::Node<'_>>,
            _: Option<&accesskit_consumer::Node<'_>>,
        ) {
        }
        fn node_removed(&mut self, _: &accesskit_consumer::Node<'_>) {
            self.removed += 1;
        }
    }

    fn assert_delta_matches_effective_maps(
        current: &AccessibilityNodeMap,
        previous: &AccessibilityNodeMap,
    ) {
        let before: HashMap<_, _> = previous.iter().map(|(id, node)| (*id, node)).collect();
        let after: HashMap<_, _> = current.iter().map(|(id, node)| (*id, node)).collect();
        let mut expected_nodes = after
            .iter()
            .filter_map(|(id, node)| (before.get(id) != Some(node)).then_some(*node))
            .collect::<Vec<_>>();
        let mut expected_removed = before
            .keys()
            .filter(|id| !after.contains_key(id))
            .copied()
            .collect::<Vec<_>>();
        expected_nodes.sort_unstable_by_key(|node| node.id.0);
        expected_removed.sort_unstable_by_key(|id| id.0);
        let delta = current.delta(Some(previous));
        assert_eq!(delta.nodes, expected_nodes);
        assert_eq!(delta.removed, expected_removed);
        assert!(
            delta
                .nodes
                .windows(2)
                .all(|pair| pair[0].id.0 < pair[1].id.0)
        );
        assert!(delta.removed.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    fn logical_group(root: u64, rows: &[u64], label: &str) -> Arc<AccessibilitySnapshot> {
        let mut parent = AccessibilityNode::new(AccessibilityRole::Tree);
        parent.id = AccessibilityId(root);
        parent.label = Some(label.into());
        let mut nodes = rows
            .iter()
            .map(|id| {
                let mut node = AccessibilityNode::new(AccessibilityRole::TreeItem);
                node.id = AccessibilityId(*id);
                node.label = Some(format!("{label} row {id}"));
                node.parent = Some(parent.id);
                parent.children.push(node.id);
                node
            })
            .collect::<Vec<_>>();
        nodes.push(parent);
        AccessibilitySnapshot::new(AccessibilityId(root), nodes).unwrap()
    }

    #[test]
    fn delta_matches_effective_maps_for_masks_root_replacement_and_cross_snapshot_moves() {
        let first = logical_group(100, &[101, 102, 103], "first");
        let second = logical_group(200, &[201, 202], "second");
        let shared = logical_group(300, &[301, 302], "shared");
        let mut previous = AccessibilityNodeMap::default();
        previous.attach(first);
        previous.attach(second);
        previous.attach(shared.clone());
        previous.get_mut(&AccessibilityId(101)).unwrap().label = Some("old overlay".into());
        previous.remove(&AccessibilityId(102));
        previous.get_mut(&AccessibilityId(301)).unwrap().bounds =
            Some(AccessibilityRect::new(1.0, 2.0, 3.0, 4.0));
        let mut current = AccessibilityNodeMap::default();
        // Root 100 becomes 110, while row 103 moves to the other changed
        // snapshot. The old-only pass must not report row 103 as removed.
        current.attach(logical_group(110, &[101, 102], "replacement"));
        current.attach(logical_group(200, &[201, 202, 103], "second"));
        current.attach(shared);
        current.remove(&AccessibilityId(201));
        let mut overlay = AccessibilityNode::new(AccessibilityRole::Button);
        overlay.id = AccessibilityId(202);
        overlay.label = Some("current overlay".into());
        current.insert(overlay.id, overlay);
        assert_delta_matches_effective_maps(&current, &previous);
        let delta = current.delta(Some(&previous));
        assert!(delta.removed.contains(&AccessibilityId(100)));
        assert!(delta.removed.contains(&AccessibilityId(201)));
        assert!(!delta.removed.contains(&AccessibilityId(103)));
        assert_eq!(
            delta
                .nodes
                .iter()
                .filter(|node| node.id == AccessibilityId(103))
                .count(),
            1
        );
        assert!(
            delta
                .nodes
                .iter()
                .any(|node| node.id == AccessibilityId(102))
        );
        assert!(
            delta
                .nodes
                .iter()
                .any(|node| node.id == AccessibilityId(301))
        );
        // A frame overlay can move into or out of an unchanged shared base.
        assert_delta_matches_effective_maps(&previous, &current);
    }

    #[test]
    fn delta_matches_naive_maps_through_deterministic_overlay_and_snapshot_churn() {
        let mut seed = 0x5d84_3a91_81e4_ba23_u64;
        let mut next = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            seed
        };
        let mut groups = (0..4)
            .map(|group| {
                let root = 1000 + group * 100;
                logical_group(
                    root,
                    &(1..=12).map(|row| root + row).collect::<Vec<_>>(),
                    "initial",
                )
            })
            .collect::<Vec<_>>();
        let mut previous = AccessibilityNodeMap::default();
        for group in &groups {
            previous.attach(group.clone());
        }
        for round in 0..400 {
            let group = (next() % 4) as usize;
            let root = 1000 + group as u64 * 100;
            let rows = (1..=12)
                .filter(|_| next() % 4 != 0)
                .map(|row| root + row)
                .collect::<Vec<_>>();
            groups[group] = logical_group(root, &rows, &format!("round {round}"));
            let mut current = AccessibilityNodeMap::default();
            for group in &groups {
                if next() % 5 != 0 {
                    current.attach(group.clone());
                }
            }
            for _ in 0..18 {
                let group = next() % 4;
                let id = AccessibilityId(1000 + group * 100 + 1 + next() % 12);
                match next() % 4 {
                    0 => {
                        current.remove(&id);
                    }
                    1 => {
                        if let Some(node) = current.get_mut(&id) {
                            node.label = Some(format!("overlay {round}"));
                            node.parent = Some(AccessibilityId(50_000 + next() % 4));
                        }
                    }
                    _ => {
                        let mut node = AccessibilityNode::new(AccessibilityRole::Button);
                        node.id = id;
                        node.label = Some(format!("frame {round}"));
                        current.insert(id, node);
                    }
                }
            }
            assert_delta_matches_effective_maps(&current, &previous);
            assert_delta_matches_effective_maps(&previous, &current);
            previous = current;
        }
    }

    #[test]
    fn snapshot_root_reparent_and_cached_overlay_reset_preserve_consumer_identity() {
        let mut window = AccessibilityNode::new(AccessibilityRole::Window);
        window.id = AccessibilityId(1);
        let mut previous = AccessibilityTree::new(window);
        for id in [10, 20] {
            let mut group = AccessibilityNode::new(AccessibilityRole::Group);
            group.id = AccessibilityId(id);
            previous.insert(group);
            previous.set_parent(AccessibilityId(id), previous.root);
        }
        let snapshot = logical_group(100, &[101, 102], "stable");
        previous.nodes.attach(snapshot.clone());
        previous.set_parent(AccessibilityId(100), AccessibilityId(10));
        previous.get_mut(AccessibilityId(101)).unwrap().bounds =
            Some(AccessibilityRect::new(1.0, 2.0, 3.0, 4.0));
        let mut current = previous.clone();
        current
            .get_mut(AccessibilityId(10))
            .unwrap()
            .children
            .clear();
        current.set_parent(AccessibilityId(100), AccessibilityId(20));
        // An outgoing cached frame's geometry must disappear while its shared
        // logical node remains present under the newly painted parent.
        current.nodes.frame.remove(&AccessibilityId(101));
        assert!(
            current
                .nodes
                .shares_snapshot(AccessibilityId(100), &previous.nodes)
        );
        assert_delta_matches_effective_maps(&current.nodes, &previous.nodes);
        let mut consumer =
            accesskit_consumer::Tree::new(previous.to_accesskit_tree_update(None, None), true);
        let mut changes = Changes::default();
        consumer.update_and_process_changes(
            current.to_accesskit_tree_update_after(Some(&previous), None, None),
            &mut changes,
        );
        assert_eq!(changes.added, 0);
        assert_eq!(changes.removed, 0);
        let row = consumer
            .state()
            .node_by_tree_local_id(accesskit::NodeId(101), accesskit::TreeId::ROOT)
            .unwrap();
        assert_eq!(row.label().as_deref(), Some("stable row 101"));
        assert!(!row.has_bounds());
        assert_eq!(row.parent().unwrap().locate().0, accesskit::NodeId(100));
        let root = consumer
            .state()
            .node_by_tree_local_id(accesskit::NodeId(100), accesskit::TreeId::ROOT)
            .unwrap();
        assert_eq!(root.parent().unwrap().locate().0, accesskit::NodeId(20));
    }

    #[test]
    fn hundred_thousand_logical_nodes_share_labels_and_export_only_changed_geometry() {
        let window = crate::AccessibilityNode::new(AccessibilityRole::Window);
        let mut root = crate::AccessibilityNode::new(AccessibilityRole::Tree);
        let root_id = root.id;
        let mut rows = Vec::with_capacity(100_001);
        let mut last_id = root_id;
        for index in 0..100_000 {
            let mut row = crate::AccessibilityNode::new(AccessibilityRole::TreeItem)
                .with_label(format!("item {index}"));
            row.parent = Some(root_id);
            last_id = row.id;
            root.children.push(row.id);
            rows.push(row);
        }
        rows.push(root);
        let snapshot = AccessibilitySnapshot::new(root_id, rows).unwrap();
        let mut previous = AccessibilityTree::new(window.clone());
        previous.nodes.attach(snapshot.clone());
        previous.set_parent(root_id, previous.root);
        let mut current = previous.clone();
        assert_eq!(current.node_count(), 100_002);
        assert!(current.nodes.shares_snapshot(root_id, &previous.nodes));
        assert!(std::ptr::eq(
            current.get(last_id).unwrap().label.as_ref().unwrap(),
            previous.get(last_id).unwrap().label.as_ref().unwrap()
        ));
        assert!(current.delta(Some(&previous)).nodes.is_empty());
        assert_eq!(DELTA_BASE_COMPARE_COUNT.with(std::cell::Cell::get), 0);
        assert_eq!(DELTA_REMOVAL_PROBE_COUNT.with(std::cell::Cell::get), 0);
        assert!(
            current
                .to_accesskit_tree_update_after(Some(&previous), None, None)
                .nodes
                .is_empty()
        );
        let mut consumer =
            accesskit_consumer::Tree::new(previous.to_accesskit_tree_update(None, None), true);
        current.get_mut(last_id).unwrap().bounds =
            Some(AccessibilityRect::new(0.0, 0.0, 20.0, 10.0));
        let delta = current.delta(Some(&previous));
        assert_eq!(delta.nodes.len(), 1);
        assert!(delta.removed.is_empty());
        let update = current.to_accesskit_tree_update_after(Some(&previous), None, None);
        assert_eq!(update.nodes.len(), 1);
        let mut changes = Changes::default();
        consumer.update_and_process_changes(update, &mut changes);
        assert_eq!(changes.updated, 1);
        assert_eq!(changes.removed, 0);
        let row = consumer
            .state()
            .node_by_tree_local_id(accesskit::NodeId(last_id.0), accesskit::TreeId::ROOT)
            .unwrap();
        assert_eq!(row.label().as_deref(), Some("item 99999"));
        assert!(row.has_bounds());
        // Removing an overlay clears stale offscreen geometry without copying
        // the underlying label or remounting any physical element.
        let update = previous.to_accesskit_tree_update_after(Some(&current), None, None);
        assert_eq!(update.nodes.len(), 1);
        consumer.update_and_process_changes(update, &mut changes);
        assert!(
            !consumer
                .state()
                .node_by_tree_local_id(accesskit::NodeId(last_id.0), accesskit::TreeId::ROOT)
                .unwrap()
                .has_bounds()
        );
    }

    #[test]
    fn changed_flat_snapshot_does_not_allocate_a_full_logical_candidate_set() {
        let window = AccessibilityNode::new(AccessibilityRole::Window);
        let mut root = AccessibilityNode::new(AccessibilityRole::Tree);
        let root_id = root.id;
        let mut nodes = Vec::with_capacity(100_001);
        for index in 0..100_000 {
            let mut row = AccessibilityNode::new(AccessibilityRole::TreeItem)
                .with_label(format!("logical {index}"));
            row.parent = Some(root_id);
            root.children.push(row.id);
            nodes.push(row);
        }
        let last_id = nodes.last().unwrap().id;
        nodes.push(root);
        let previous_snapshot = AccessibilitySnapshot::new(root_id, nodes).unwrap();
        let mut previous = AccessibilityTree::new(window);
        previous.nodes.attach(previous_snapshot.clone());
        previous.set_parent(root_id, previous.root);
        let replacement = AccessibilitySnapshot::new(
            root_id,
            previous_snapshot.nodes.values().cloned().map(|mut node| {
                if node.id == last_id {
                    node.label = Some("changed offscreen logical row".into());
                }
                node
            }),
        )
        .unwrap();
        let mut current = previous.clone();
        current.nodes.attach(replacement);
        let delta = current.delta(Some(&previous));
        assert_eq!(delta.nodes.len(), 1);
        assert_eq!(delta.nodes[0].id, last_id);
        assert!(delta.removed.is_empty());
        assert_eq!(current.node_count(), 100_002);
        let candidates = DELTA_CANDIDATE_COUNT.with(std::cell::Cell::get);
        assert!(
            candidates <= 2,
            "flat logical replacement allocated {candidates} candidate IDs for two frame overlays"
        );
        // Changed flat snapshots still require linear comparison/removal work.
        assert_eq!(DELTA_BASE_COMPARE_COUNT.with(std::cell::Cell::get), 100_000);
        assert_eq!(
            DELTA_REMOVAL_PROBE_COUNT.with(std::cell::Cell::get),
            100_000
        );
    }

    #[test]
    fn visibility_transitions_remove_and_restore_unchanged_descendants_in_consumer() {
        let root = crate::AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = AccessibilityTree::new(root);
        let group = crate::AccessibilityNode::new(AccessibilityRole::Group);
        let group_id = group.id;
        let child = crate::AccessibilityNode::new(AccessibilityRole::Button).with_label("child");
        let child_id = child.id;
        tree.insert(group);
        tree.set_parent(group_id, tree.root);
        tree.insert(child);
        tree.set_parent(child_id, group_id);
        let mut consumer =
            accesskit_consumer::Tree::new(tree.to_accesskit_tree_update(None, None), true);
        let mut hidden = tree.clone();
        hidden.get_mut(group_id).unwrap().states |= AccessibilityState::HIDDEN;
        let mut changes = Changes::default();
        consumer.update_and_process_changes(
            hidden.to_accesskit_tree_update_after(Some(&tree), None, None),
            &mut changes,
        );
        assert!(
            consumer
                .state()
                .node_by_tree_local_id(accesskit::NodeId(group_id.0), accesskit::TreeId::ROOT)
                .is_none()
        );
        assert!(
            consumer
                .state()
                .node_by_tree_local_id(accesskit::NodeId(child_id.0), accesskit::TreeId::ROOT)
                .is_none()
        );
        consumer.update_and_process_changes(
            tree.to_accesskit_tree_update_after(Some(&hidden), None, None),
            &mut changes,
        );
        assert_eq!(
            consumer
                .state()
                .node_by_tree_local_id(accesskit::NodeId(child_id.0), accesskit::TreeId::ROOT)
                .unwrap()
                .label()
                .as_deref(),
            Some("child")
        );
        assert_eq!(changes.removed, 2);
        assert_eq!(changes.added, 2);
    }

    #[test]
    fn snapshot_rejects_duplicates_missing_parents_cycles_and_orphan_rows() {
        let root = crate::AccessibilityNode::new(AccessibilityRole::Tree);
        assert!(AccessibilitySnapshot::new(root.id, [root.clone(), root.clone()]).is_err());
        let row = crate::AccessibilityNode::new(AccessibilityRole::TreeItem);
        assert!(AccessibilitySnapshot::new(root.id, [root.clone(), row.clone()]).is_err());
        let mut malformed = root.clone();
        malformed.children.push(row.id);
        assert!(AccessibilitySnapshot::new(root.id, [malformed, row.clone()]).is_err());
        let mut cyclic_root = root;
        let mut cyclic_row = row;
        cyclic_root.parent = Some(cyclic_row.id);
        cyclic_root.children.push(cyclic_row.id);
        cyclic_row.parent = Some(cyclic_root.id);
        cyclic_row.children.push(cyclic_root.id);
        assert!(AccessibilitySnapshot::new(cyclic_root.id, [cyclic_root, cyclic_row]).is_err());
    }
    #[test]
    fn logical_removal_and_reinsertion_are_applied_to_retained_consumer() {
        let window = crate::AccessibilityNode::new(AccessibilityRole::Window);
        let mut root = crate::AccessibilityNode::new(AccessibilityRole::Tree);
        let root_id = root.id;
        let mut row =
            crate::AccessibilityNode::new(AccessibilityRole::TreeItem).with_label("offscreen");
        row.parent = Some(root_id);
        let row_id = row.id;
        root.children.push(row_id);
        let snapshot = AccessibilitySnapshot::new(root_id, [root, row.clone()]).unwrap();
        let mut previous = AccessibilityTree::new(window);
        previous.nodes.attach(snapshot);
        previous.set_parent(root_id, previous.root);
        let mut consumer =
            accesskit_consumer::Tree::new(previous.to_accesskit_tree_update(None, None), true);
        let mut current = previous.clone();
        current.remove(row_id);
        assert!(current.get(row_id).is_none());
        assert_eq!(current.node_count(), 2);
        assert_eq!(current.delta(Some(&previous)).removed, vec![row_id]);
        let mut changes = Changes::default();
        consumer.update_and_process_changes(
            current.to_accesskit_tree_update_after(Some(&previous), None, None),
            &mut changes,
        );
        assert_eq!(changes.removed, 1);
        assert!(
            consumer
                .state()
                .node_by_tree_local_id(accesskit::NodeId(row_id.0), accesskit::TreeId::ROOT)
                .is_none()
        );
        let removed = current.clone();
        current.insert(row);
        assert_eq!(current.node_count(), 3);
        consumer.update_and_process_changes(
            current.to_accesskit_tree_update_after(Some(&removed), None, None),
            &mut changes,
        );
        assert!(
            consumer
                .state()
                .node_by_tree_local_id(accesskit::NodeId(row_id.0), accesskit::TreeId::ROOT)
                .is_some()
        );
        assert_eq!(changes.added, 1);
    }
}

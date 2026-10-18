//! Immutable tree catalogs with explicit source-generation ownership.
use super::*;
use std::ops::Index;
use std::slice;

#[cfg(test)]
mod tests;

/// One explicit immutable source generation, reusable across disclosure models.
///
/// Construction consumes the forest once. Changing labels, hierarchy, disabled
/// flags or icons requires constructing another catalog. Equality of IDs alone
/// never implies equality of content. Old `VirtualTreeModel` constructors remain
/// independent owned constructors, with their existing generic bounds.
///
/// Every model still exposes all displayed logical rows to accessibility,
/// including offscreen rows. The catalog also retains collapsed source payload;
/// this is an explicit caller-owned generation, not an old-snapshot cache.
#[derive(Clone)]
pub struct VirtualTreeCatalog<T: Clone + Eq + Hash + Send + Sync + 'static> {
    data: Arc<CatalogData<T>>,
    executor: BackgroundExecutor,
}

// No Send/Sync bound here: the legacy TreeModelRows::Owned variant must support
// exactly the old VirtualTreeModel<T> domain. Only the public bounded constructor
// installs a retirement closure whose captured data is Send.
pub(super) struct CatalogData<T: Clone> {
    rows: Vec<FlatTreeNode<T>>,
    parents: Vec<Option<usize>>,
    subtree_ends: Vec<usize>,
    indices: Arc<HashMap<T, usize>>,
    reclaimer: Option<Arc<dyn Fn(&mut CatalogData<T>) + Send + Sync>>,
}

impl<T: Clone> Drop for CatalogData<T> {
    fn drop(&mut self) {
        if let Some(reclaim) = self.reclaimer.take() {
            reclaim(self);
        }
    }
}

pub(super) enum TreeModelRows<T: Clone> {
    Owned(Vec<FlatTreeNode<T>>),
    Projected {
        catalog: Arc<CatalogData<T>>,
        visible: Vec<usize>,
    },
}

impl<T: Clone> Default for TreeModelRows<T> {
    fn default() -> Self {
        Self::Owned(Vec::new())
    }
}

impl<T: Clone> TreeModelRows<T> {
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Owned(rows) => rows.len(),
            Self::Projected { visible, .. } => visible.len(),
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub(super) fn get(&self, index: usize) -> Option<&FlatTreeNode<T>> {
        match self {
            Self::Owned(rows) => rows.get(index),
            Self::Projected { catalog, visible } => {
                visible.get(index).map(|index| &catalog.rows[*index])
            }
        }
    }
    pub(super) fn iter(&self) -> TreeModelRowsIter<'_, T> {
        match self {
            Self::Owned(rows) => TreeModelRowsIter::Owned(rows.iter()),
            Self::Projected { catalog, visible } => TreeModelRowsIter::Projected {
                rows: &catalog.rows,
                visible: visible.iter(),
            },
        }
    }
}

impl<T: Clone> Index<usize> for TreeModelRows<T> {
    type Output = FlatTreeNode<T>;
    fn index(&self, index: usize) -> &Self::Output {
        self.get(index)
            .expect("virtual tree row index out of bounds")
    }
}

pub(super) enum TreeModelRowsIter<'a, T: Clone> {
    Owned(slice::Iter<'a, FlatTreeNode<T>>),
    Projected {
        rows: &'a [FlatTreeNode<T>],
        visible: slice::Iter<'a, usize>,
    },
}

impl<'a, T: Clone> Iterator for TreeModelRowsIter<'a, T> {
    type Item = &'a FlatTreeNode<T>;
    fn next(&mut self) -> Option<Self::Item> {
        self.nth(0)
    }
    fn nth(&mut self, count: usize) -> Option<Self::Item> {
        // Delegate nth to the contiguous index iterator. Otherwise the existing
        // Right-key branch's skip(index + 1) would become a linear prefix scan.
        match self {
            Self::Owned(rows) => rows.nth(count),
            Self::Projected { rows, visible } => {
                let rows = *rows;
                visible.nth(count).map(|index| &rows[*index])
            }
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Owned(rows) => rows.size_hint(),
            Self::Projected { visible, .. } => visible.size_hint(),
        }
    }
}
impl<T: Clone> ExactSizeIterator for TreeModelRowsIter<'_, T> {}

struct InputFrame<T: Clone> {
    siblings: std::vec::IntoIter<TreeNode<T>>,
    parent: Option<usize>,
    level: usize,
}

struct PendingForest<T: Clone> {
    frames: Vec<InputFrame<T>>,
    // Own the current still-nested node before calling user Hash/Eq/Clone.
    // An unwind must not strand its descendants in an ordinary local Vec.
    current: Option<TreeNode<T>>,
}

impl<T: Clone> Default for PendingForest<T> {
    fn default() -> Self {
        Self {
            frames: Vec::new(),
            current: None,
        }
    }
}

impl<T: Clone> PendingForest<T> {
    fn empty(&self) -> bool {
        self.frames.is_empty() && self.current.is_none()
    }
}

impl<T: Clone> Drop for PendingForest<T> {
    fn drop(&mut self) {
        // This wrapper is moved into the retirement future. Its iterative Drop
        // also runs if that future is discarded before polling at pool shutdown.
        // No still-nested TreeNode is entrusted to a recursive Vec destructor.
        loop {
            let next = if self.current.is_some() {
                self.current.take()
            } else if let Some(frame) = self.frames.last_mut() {
                frame.siblings.next()
            } else {
                break;
            };
            if let Some(mut node) = next {
                let children = std::mem::take(&mut node.children);
                if !children.is_empty() {
                    self.frames.push(InputFrame {
                        siblings: children.into_iter(),
                        parent: None,
                        level: 0,
                    });
                }
                drop(node);
            } else {
                self.frames.pop();
            }
        }
    }
}

struct CatalogBuildGuard<T: Clone + Send + Sync + 'static> {
    data: Option<CatalogData<T>>,
    input: PendingForest<T>,
    executor: BackgroundExecutor,
}

impl<T: Clone + Send + Sync + 'static> Drop for CatalogBuildGuard<T> {
    fn drop(&mut self) {
        if self.data.is_none() && self.input.empty() {
            return;
        }
        let data = self.data.take();
        let input = std::mem::take(&mut self.input);
        self.executor
            .spawn(async move {
                drop((input, data));
            })
            .detach();
    }
}

impl<T: Clone + Eq + Hash + Send + Sync + 'static> VirtualTreeCatalog<T> {
    /// Consume and validate one complete forest, including collapsed source IDs.
    /// Batch retirement is scheduled on the supplied existing executor; shutdown
    /// follows that executor's cancellation policy rather than a new global pool.
    pub fn from_nodes(
        nodes: Vec<TreeNode<T>>,
        executor: &BackgroundExecutor,
    ) -> Result<Self, VirtualTreeModelError> {
        let mut guard = CatalogBuildGuard {
            data: Some(CatalogData {
                rows: Vec::new(),
                parents: Vec::new(),
                subtree_ends: Vec::new(),
                indices: Arc::new(HashMap::new()),
                reclaimer: None,
            }),
            input: PendingForest {
                frames: vec![InputFrame {
                    siblings: nodes.into_iter(),
                    parent: None,
                    level: 0,
                }],
                current: None,
            },
            executor: executor.clone(),
        };
        while !guard.input.frames.is_empty() {
            let next = {
                let frame = guard.input.frames.last_mut().unwrap();
                frame
                    .siblings
                    .next()
                    .map(|node| (node, frame.parent, frame.level))
            };
            let Some((node, parent, level)) = next else {
                let frame = guard.input.frames.pop().unwrap();
                if let Some(parent) = frame.parent {
                    let data = guard.data.as_mut().unwrap();
                    data.subtree_ends[parent] = data.rows.len();
                }
                continue;
            };
            guard.input.current = Some(node);
            let node = guard.input.current.as_ref().unwrap();
            // Compute this while the guard still owns the nested node. Even a
            // checked-depth failure leaves the complete forest batch-owned.
            let child_level = level.checked_add(1).expect("tree nesting depth overflow");
            let data = guard.data.as_mut().unwrap();
            let index = data.rows.len();
            let indices = Arc::get_mut(&mut data.indices).unwrap();
            if indices.contains_key(&node.id) {
                // Preserve the rejected forest in the same owned batch instead
                // of synchronously dropping an arbitrarily deep rejected node.
                return Err(VirtualTreeModelError::DuplicateId);
            }
            indices.insert(node.id.clone(), index);
            let mut node = guard.input.current.take().unwrap();
            let children = std::mem::take(&mut node.children);
            data.rows.push(FlatTreeNode {
                node_id: node.id,
                label: node.label,
                icon: node.icon,
                icon_color: node.icon_color,
                disabled: node.disabled,
                has_children: !children.is_empty() || node.has_lazy_children,
                level,
                match_ranges: Vec::new(),
            });
            data.parents.push(parent);
            data.subtree_ends.push(index + 1);
            if !children.is_empty() {
                guard.input.frames.push(InputFrame {
                    siblings: children.into_iter(),
                    parent: Some(index),
                    level: child_level,
                });
            }
        }
        let mut data = guard.data.take().unwrap();
        let worker = executor.clone();
        data.reclaimer = Some(Arc::new(move |data| {
            let rows = std::mem::take(&mut data.rows);
            let parents = std::mem::take(&mut data.parents);
            let subtree_ends = std::mem::take(&mut data.subtree_ends);
            let indices = std::mem::take(&mut data.indices);
            worker
                .spawn(async move {
                    drop((rows, parents, subtree_ends, indices));
                })
                .detach();
        }));
        Ok(Self {
            data: Arc::new(data),
            executor: executor.clone(),
        })
    }

    /// Catalog node count, including collapsed source descendants.
    pub fn len(&self) -> usize {
        self.data.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Build the current unfiltered disclosure view of this explicit generation.
    /// Existing `VirtualTreeModel::filtered` remains unchanged in this first slice.
    /// Call the existing `prepare_accessibility` before sharing/painting the model.
    pub fn model(
        &self,
        expanded_ids: &HashSet<T>,
    ) -> Result<VirtualTreeModel<T>, VirtualTreeModelError> {
        let mut model = VirtualTreeModel {
            data: Arc::new(TreeModelData {
                rows: TreeModelRows::Projected {
                    catalog: self.data.clone(),
                    visible: Vec::new(),
                },
                parents: Vec::new(),
                expanded: Vec::new(),
                indices: Arc::new(HashMap::new()),
                catalog_indices: self.data.indices.clone(),
                enabled: Vec::new(),
                reclaimer: None,
                accessibility: None,
            }),
        };
        // Install ownership cleanup before invoking user Hash/Clone operations.
        // Early failure/unwind therefore retains one batch of partial model data.
        model.reclaim_on(&self.executor);
        let data = Arc::get_mut(&mut model.data).unwrap();
        let TreeModelRows::Projected { visible, .. } = &mut data.rows else {
            unreachable!()
        };
        let mut ancestry: Vec<(usize, usize)> = Vec::new();
        let mut source_index = 0;
        while source_index < self.data.rows.len() {
            let row = &self.data.rows[source_index];
            let parent = self.data.parents[source_index];
            while ancestry
                .last()
                .is_some_and(|(source, _)| Some(*source) != parent)
            {
                ancestry.pop();
            }
            debug_assert!(
                parent.is_none()
                    || ancestry
                        .last()
                        .is_some_and(|(source, _)| Some(*source) == parent)
            );
            let index = visible.len();
            let expanded = expanded_ids.contains(&row.node_id);
            visible.push(source_index);
            data.parents
                .push(ancestry.last().map(|(_, visible)| *visible));
            data.expanded.push(expanded);
            if !row.disabled {
                data.enabled.push(index);
            }
            ancestry.push((source_index, index));
            source_index = if row.has_children && !expanded {
                self.data.subtree_ends[source_index]
            } else {
                source_index + 1
            };
        }
        if visible.len() == self.data.rows.len() {
            // Source and visible order are identical, so the complete validated
            // catalog map serves both roles without new owned generic keys.
            data.indices = self.data.indices.clone();
        } else {
            let indices = Arc::get_mut(&mut data.indices).unwrap();
            indices.reserve(visible.len());
            for (index, source) in visible.iter().enumerate() {
                if indices
                    .insert(self.data.rows[*source].node_id.clone(), index)
                    .is_some()
                {
                    return Err(VirtualTreeModelError::DuplicateId);
                }
            }
        }
        Ok(model)
    }
}

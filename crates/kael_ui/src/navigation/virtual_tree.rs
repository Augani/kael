//! Viewport-mounted trees for large file explorers and project hierarchies.
//!
//! Build a [`VirtualTreeModel`] when data, expansion, or filtering changes, and
//! cheaply clone it on redraws. [`VirtualTreeState`] owns one keyboard focus
//! handle and retains the active logical ID as row positions change. Only the
//! viewport's rows are mounted. A retained logical accessibility snapshot exposes
//! every displayed row, including offscreen items, without copying labels on
//! redraw. Stable semantic IDs follow model IDs across snapshot replacements.

use super::tree::{
    FlatTreeNode, TreeList, TreeListDensity, TreeNode, filter_tree, flatten_filtered_tree,
    flatten_tree, parent_indices,
};
use crate::components::icon::Icon;
use crate::theme::Theme;
use kael::{prelude::*, *};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

/// Invalid input for a visible tree snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtualTreeModelError {
    /// Two source nodes have the same ID. IDs must identify catalog nodes uniquely.
    DuplicateId,
    /// Preparation must happen before the model is shared or cloned.
    SharedPreparation,
    /// A supplied catalog omits a currently displayed node ID.
    IncompleteCatalog,
}

impl fmt::Display for VirtualTreeModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId => write!(f, "virtual tree contains duplicate node IDs"),
            Self::IncompleteCatalog => write!(f, "accessibility catalog omits displayed node IDs"),
            Self::SharedPreparation => write!(
                f,
                "prepare accessibility before sharing the virtual tree model"
            ),
        }
    }
}

impl std::error::Error for VirtualTreeModelError {}

struct TreeModelData<T: Clone> {
    rows: Vec<FlatTreeNode<T>>,
    parents: Vec<Option<usize>>,
    expanded: Vec<bool>,
    indices: Arc<HashMap<T, usize>>,
    catalog_indices: Arc<HashMap<T, usize>>,
    enabled: Vec<usize>,
    reclaimer: Option<Arc<dyn Fn(&mut TreeModelData<T>) + Send + Sync>>,
    accessibility: Option<Arc<PreparedTreeAccessibility<T>>>,
}

impl<T: Clone> Drop for TreeModelData<T> {
    fn drop(&mut self) {
        if let Some(reclaim) = self.reclaimer.take() {
            reclaim(self);
        }
    }
}

/// An immutable, reusable snapshot of a tree's displayed rows.
///
/// Cloning a model only clones an `Arc`. Construction takes linear space in
/// catalog and displayed rows. An unfiltered snapshot visually flattens only
/// expanded branches, while source IDs retain accessibility identity across
/// disclosure/filter changes. Duplicate source IDs are rejected. A filtered
/// snapshot searches all input nodes and retains matching ancestors.
#[derive(Clone)]
pub struct VirtualTreeModel<T: Clone + Eq + Hash + 'static> {
    data: Arc<TreeModelData<T>>,
}

impl<T: Clone + Eq + Hash + 'static> VirtualTreeModel<T> {
    /// Flatten the currently expanded branches. Rebuild only when the model or
    /// expansion changes, and retain this snapshot in the application's state.
    pub fn new(
        nodes: &[TreeNode<T>],
        expanded_ids: &HashSet<T>,
    ) -> Result<Self, VirtualTreeModelError> {
        let mut model = Self::from_rows(flatten_tree(nodes, expanded_ids, 0), expanded_ids, false)?;
        // Every hidden source node has a collapsed displayed ancestor. If all
        // displayed branches are expanded, the validated rows already cover
        // the source catalog and its index can be shared without another pass.
        if model
            .data
            .rows
            .iter()
            .zip(&model.data.expanded)
            .any(|(row, expanded)| row.has_children && !*expanded)
        {
            let displayed_rows = model.len();
            model.set_accessibility_catalog_ids_with_capacity(source_ids(nodes), displayed_rows)?;
        }
        Ok(model)
    }

    /// Build a case-insensitive filtered snapshot with highlighted labels.
    /// With `auto_expand_matches`, ancestors of matches display their children.
    pub fn filtered(
        nodes: &[TreeNode<T>],
        expanded_ids: &HashSet<T>,
        filter: &str,
        auto_expand_matches: bool,
    ) -> Result<Self, VirtualTreeModelError> {
        if filter.is_empty() {
            return Self::new(nodes, expanded_ids);
        }
        let filtered = filter_tree(nodes, filter);
        let mut model = Self::from_rows(
            flatten_filtered_tree(&filtered, expanded_ids, 0, auto_expand_matches),
            expanded_ids,
            auto_expand_matches,
        )?;
        let displayed_rows = model.len();
        model.set_accessibility_catalog_ids_with_capacity(source_ids(nodes), displayed_rows)?;
        Ok(model)
    }

    pub(super) fn from_rows(
        rows: Vec<FlatTreeNode<T>>,
        expanded_ids: &HashSet<T>,
        auto_expand_matches: bool,
    ) -> Result<Self, VirtualTreeModelError> {
        let parents = parent_indices(&rows);
        let mut indices = HashMap::with_capacity(rows.len());
        let mut expanded = Vec::with_capacity(rows.len());
        let mut enabled = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            if indices.insert(row.node_id.clone(), index).is_some() {
                return Err(VirtualTreeModelError::DuplicateId);
            }
            expanded.push(
                expanded_ids.contains(&row.node_id)
                    || (auto_expand_matches
                        && rows
                            .get(index + 1)
                            .is_some_and(|next| next.level > row.level)),
            );
            if !row.disabled {
                enabled.push(index);
            }
        }
        let indices = Arc::new(indices);
        Ok(Self {
            data: Arc::new(TreeModelData {
                rows,
                parents,
                expanded,
                catalog_indices: indices.clone(),
                indices,
                enabled,
                reclaimer: None,
                accessibility: None,
            }),
        })
    }

    /// Supply all IDs still owned by a catalog, including collapsed or filtered
    /// items. Prepare this metadata on the model-building worker before sharing
    /// the model or preparing its accessibility snapshot. Surviving IDs retain
    /// semantic identity across disclosure; IDs removed from this catalog retire.
    /// `new` and `filtered` populate this from the complete source tree already.
    pub fn set_accessibility_catalog_ids(
        &mut self,
        ids: impl IntoIterator<Item = T>,
    ) -> Result<(), VirtualTreeModelError> {
        self.set_accessibility_catalog_ids_with_capacity(ids, 0)
    }

    // Factories know every validated displayed ID belongs to the source catalog.
    // Public arbitrary catalogs keep their existing zero-capacity/error path.
    fn set_accessibility_catalog_ids_with_capacity(
        &mut self,
        ids: impl IntoIterator<Item = T>,
        capacity: usize,
    ) -> Result<(), VirtualTreeModelError> {
        let data = Arc::get_mut(&mut self.data)
            .filter(|data| data.accessibility.is_none())
            .ok_or(VirtualTreeModelError::SharedPreparation)?;
        let mut catalog = HashMap::with_capacity(capacity);
        for (index, id) in ids.into_iter().enumerate() {
            if catalog.insert(id, index).is_some() {
                return Err(VirtualTreeModelError::DuplicateId);
            }
        }
        if !data.indices.keys().all(|id| catalog.contains_key(id)) {
            return Err(VirtualTreeModelError::IncompleteCatalog);
        }
        // Share the visible map when every catalog item is displayed. This
        // keeps the common fully expanded case from duplicating owned keys.
        data.catalog_indices = if catalog.len() == data.indices.len() {
            data.indices.clone()
        } else {
            Arc::new(catalog)
        };
        Ok(())
    }

    /// Reclaim large filesystem snapshots on the worker even when the last Arc
    /// belongs to an outgoing UI frame. Attach before sharing a fresh model.
    pub(super) fn reclaim_on(&mut self, executor: &BackgroundExecutor)
    where
        T: Send + Sync,
    {
        let executor = executor.clone();
        Arc::get_mut(&mut self.data)
            .expect("attach reclaimer before sharing model")
            .reclaimer = Some(Arc::new(move |data| {
            let rows = std::mem::take(&mut data.rows);
            let parents = std::mem::take(&mut data.parents);
            let expanded = std::mem::take(&mut data.expanded);
            let indices = std::mem::take(&mut data.indices);
            let catalog_indices = std::mem::take(&mut data.catalog_indices);
            let enabled = std::mem::take(&mut data.enabled);
            let accessibility = data.accessibility.take();
            executor
                .spawn(async move {
                    drop((
                        rows,
                        parents,
                        expanded,
                        indices,
                        catalog_indices,
                        enabled,
                        accessibility,
                    ));
                })
                .detach();
        }));
    }

    /// Number of displayed logical rows, including disabled rows.
    pub fn len(&self) -> usize {
        self.data.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.rows.is_empty()
    }

    /// Resolve a logical ID to its current displayed position.
    pub fn index_of(&self, id: &T) -> Option<usize> {
        self.data.indices.get(id).copied()
    }

    /// Logical ID at a displayed position.
    pub fn id_at(&self, index: usize) -> Option<&T> {
        self.data.rows.get(index).map(|row| &row.node_id)
    }

    /// Label at a displayed position. This borrows the snapshot's shared label.
    pub fn label_at(&self, index: usize) -> Option<&SharedString> {
        self.data.rows.get(index).map(|row| &row.label)
    }

    /// Zero-based hierarchy depth at a displayed position.
    pub fn level_at(&self, index: usize) -> Option<usize> {
        self.data.rows.get(index).map(|row| row.level)
    }

    /// Displayed position of a row's parent, if it has one.
    pub fn parent_index(&self, index: usize) -> Option<usize> {
        self.data.parents.get(index).copied().flatten()
    }

    fn enabled_index(&self, id: &T) -> Option<usize> {
        self.index_of(id)
            .filter(|index| !self.data.rows[*index].disabled)
    }
}

fn source_ids<T: Clone>(nodes: &[TreeNode<T>]) -> impl Iterator<Item = T> + '_ {
    let mut pending = vec![nodes.iter()];
    std::iter::from_fn(move || {
        loop {
            let current = pending.last_mut()?;
            if let Some(node) = current.next() {
                pending.push(node.children.iter());
                return Some(node.id.clone());
            }
            pending.pop();
        }
    })
}

/// Cheap semantic preparation input captured from a tree's persistent state.
///
/// Clone on the UI thread and move to the same worker that builds a large model.
/// Only the current immutable ID map is retained; surviving logical IDs reuse
/// their semantic identity, and absent IDs retire with outgoing snapshots.
#[derive(Clone)]
pub struct VirtualTreeAccessibilityContext<T: Clone + Eq + Hash + 'static> {
    root: AccessibilityId,
    previous: Option<Arc<PreparedTreeAccessibility<T>>>,
    label: SharedString,
    has_toggle: bool,
}

struct PreparedTreeAccessibility<T: Clone> {
    root: AccessibilityId,
    label: SharedString,
    ids: Arc<Vec<AccessibilityId>>,
    model_indices: Arc<HashMap<T, usize>>,
    catalog_ids: Arc<Vec<AccessibilityId>>,
    catalog_indices: Arc<HashMap<T, usize>>,
    indices: HashMap<AccessibilityId, usize>,
    snapshot: Option<Arc<AccessibilitySnapshot>>,
    has_toggle: bool,
    reclaimer: Option<Arc<dyn Fn(&mut PreparedTreeAccessibility<T>) + Send + Sync>>,
}
impl<T: Clone> Drop for PreparedTreeAccessibility<T> {
    fn drop(&mut self) {
        if let Some(reclaim) = self.reclaimer.take() {
            reclaim(self);
        }
    }
}
impl<T: Clone + Eq + Hash + 'static> VirtualTreeModel<T> {
    /// Prepare every logical node and stable semantic ID before first paint.
    ///
    /// Call on the model-building worker before cloning/sharing the fresh model.
    /// First paint then attaches the prepared Arc and maps in constant time.
    /// Context toggle policy must match the rendered component. Its root label
    /// may change later without rebuilding logical rows.
    pub fn prepare_accessibility(
        &mut self,
        context: VirtualTreeAccessibilityContext<T>,
        executor: &BackgroundExecutor,
    ) -> Result<(), VirtualTreeModelError>
    where
        T: Send + Sync,
    {
        if Arc::get_mut(&mut self.data).is_none() {
            return Err(VirtualTreeModelError::SharedPreparation);
        }
        let mut prepared = build_accessibility(self, &context, Some(executor));
        let reclaim_executor = executor.clone();
        Arc::get_mut(&mut prepared).unwrap().reclaimer = Some(Arc::new(move |data| {
            let ids = std::mem::take(&mut data.ids);
            let indices = std::mem::take(&mut data.indices);
            let model_indices = std::mem::take(&mut data.model_indices);
            let catalog_indices = std::mem::take(&mut data.catalog_indices);
            let catalog_ids = std::mem::take(&mut data.catalog_ids);
            let snapshot = data.snapshot.take();
            reclaim_executor
                .spawn(async move {
                    drop((
                        ids,
                        indices,
                        model_indices,
                        catalog_ids,
                        catalog_indices,
                        snapshot,
                    ));
                })
                .detach();
        }));
        Arc::get_mut(&mut self.data).unwrap().accessibility = Some(prepared);
        self.reclaim_on(executor);
        Ok(())
    }
}

// Borrow every generated field until reuse has been decided. This deliberately
// lists the default fields too: an old record with extra metadata cannot silently
// survive a generator change merely because label/topology happen to match.
struct LogicalTreeRecord<'a> {
    id: AccessibilityId,
    role: AccessibilityRole,
    states: AccessibilityState,
    label: &'a str,
    level: Option<usize>,
    parent: Option<AccessibilityId>,
    children: &'a [AccessibilityId],
    actions: &'a [AccessibilityAction],
}
impl LogicalTreeRecord<'_> {
    fn matches(&self, node: &AccessibilityNode) -> bool {
        // No `..`: adding a node field must trigger an explicit reuse decision.
        let AccessibilityNode {
            id,
            role,
            states,
            label,
            description,
            value,
            placeholder,
            level,
            row_count,
            column_count,
            row_index,
            column_index,
            row_span,
            column_span,
            sort_direction,
            bounds,
            actions,
            children,
            parent,
            active_descendant,
            text_document,
            text_selection,
            text_geometry,
        } = node;
        *id == self.id
            && *role == self.role
            && *states == self.states
            && label.as_deref() == Some(self.label)
            && *level == self.level
            && *parent == self.parent
            && children.as_slice() == self.children
            && actions.as_slice() == self.actions
            && description.is_none()
            && value.is_none()
            && placeholder.is_none()
            && row_count.is_none()
            && column_count.is_none()
            && row_index.is_none()
            && column_index.is_none()
            && row_span.is_none()
            && column_span.is_none()
            && sort_direction.is_none()
            && bounds.is_none()
            && active_descendant.is_none()
            && text_document.is_none()
            && text_selection.is_none()
            && text_geometry.is_none()
    }

    fn into_node(self) -> AccessibilityNode {
        let mut node = AccessibilityNode::new(self.role).with_label(self.label.to_owned());
        node.id = self.id;
        node.states = self.states;
        node.level = self.level;
        node.parent = self.parent;
        node.actions = self.actions.to_vec();
        // Preserve the shared empty child list for leaves. Only actual parents
        // need a new child allocation when a fresh record is required.
        if !self.children.is_empty() {
            node.children = self.children.to_vec().into();
        }
        node
    }
}

fn build_accessibility<T: Clone + Eq + Hash + 'static>(
    model: &VirtualTreeModel<T>,
    context: &VirtualTreeAccessibilityContext<T>,
    executor: Option<&BackgroundExecutor>,
) -> Arc<PreparedTreeAccessibility<T>> {
    let mut catalog_ids = vec![AccessibilityId(0); model.data.catalog_indices.len()];
    for (logical_id, index) in model.data.catalog_indices.iter() {
        catalog_ids[*index] = context
            .previous
            .as_ref()
            .and_then(|data| {
                data.catalog_indices
                    .get(logical_id)
                    .map(|index| data.catalog_ids[*index])
            })
            .unwrap_or_default();
    }
    let catalog_ids = Arc::new(catalog_ids);
    let ids = if Arc::ptr_eq(&model.data.catalog_indices, &model.data.indices) {
        catalog_ids.clone()
    } else {
        Arc::new(
            model
                .data
                .rows
                .iter()
                .map(|row| catalog_ids[model.data.catalog_indices[&row.node_id]])
                .collect(),
        )
    };
    // Complete topology before choosing records. Parent keys are positions only
    // inside this transient builder; retained records use stable semantic IDs.
    // There is one Vec per displayed parent with children, not one per leaf.
    let mut children: HashMap<usize, Vec<AccessibilityId>> = HashMap::new();
    for (index, id) in ids.iter().enumerate() {
        let parent = model.parent_index(index).map_or(0, |parent| parent + 1);
        children.entry(parent).or_default().push(*id);
    }
    let previous = context
        .previous
        .as_ref()
        .and_then(|data| data.snapshot.as_ref());
    let mut shared = executor.map(|executor| {
        AccessibilitySnapshot::sharing_builder(context.root, model.len() + 1, executor)
    });
    // The no-executor fallback preserves the legacy owned-constructor contract.
    let mut nodes = if shared.is_some() {
        Vec::new()
    } else {
        Vec::with_capacity(model.len() + 1)
    };
    let mut indices = HashMap::with_capacity(model.len());
    {
        let mut emit = |record: LogicalTreeRecord<'_>| {
            if let Some(builder) = shared.as_mut() {
                if let Some(previous) = previous.filter(|snapshot| {
                    snapshot
                        .get(record.id)
                        .is_some_and(|node| record.matches(node))
                }) {
                    builder.reuse_from(previous, record.id)
                } else {
                    builder.insert(record.into_node())
                }
                .expect("validated virtual tree has unique accessibility identities");
            } else {
                nodes.push(record.into_node());
            }
        };
        emit(LogicalTreeRecord {
            id: context.root,
            role: AccessibilityRole::Tree,
            states: AccessibilityState::NONE,
            label: context.label.as_ref(),
            level: None,
            parent: None,
            children: children.get(&0).map(Vec::as_slice).unwrap_or(&[]),
            actions: &[AccessibilityAction::Focus],
        });
        for (index, row) in model.data.rows.iter().enumerate() {
            let id = ids[index];
            indices.insert(id, index);
            let mut states = AccessibilityState::NONE;
            if row.disabled {
                states |= AccessibilityState::DISABLED;
            }
            if row.has_children {
                states |= if model.data.expanded[index] {
                    AccessibilityState::EXPANDED
                } else {
                    AccessibilityState::COLLAPSED
                };
            }
            let actions = [
                AccessibilityAction::Focus,
                AccessibilityAction::Click,
                AccessibilityAction::ScrollToVisible,
                if model.data.expanded[index] {
                    AccessibilityAction::Collapse
                } else {
                    AccessibilityAction::Expand
                },
            ];
            let action_count = if row.disabled {
                0
            } else if row.has_children && context.has_toggle {
                4
            } else {
                3
            };
            emit(LogicalTreeRecord {
                id,
                role: AccessibilityRole::TreeItem,
                states,
                label: row.label.as_ref(),
                level: Some(row.level + 1),
                parent: Some(
                    model
                        .parent_index(index)
                        .map_or(context.root, |parent| ids[parent]),
                ),
                children: children.get(&(index + 1)).map(Vec::as_slice).unwrap_or(&[]),
                actions: &actions[..action_count],
            });
        }
    }
    let snapshot = if let Some(builder) = shared {
        builder.finish()
    } else {
        AccessibilitySnapshot::new(context.root, nodes)
    }
    .expect("validated virtual tree produces a consistent accessibility subtree");
    Arc::new(PreparedTreeAccessibility {
        root: context.root,
        label: context.label.clone(),
        ids,
        model_indices: model.data.indices.clone(),
        catalog_ids,
        catalog_indices: model.data.catalog_indices.clone(),
        indices,
        snapshot: Some(snapshot),
        has_toggle: context.has_toggle,
        reclaimer: None,
    })
}

/// Persistent interaction state for one virtual tree.
///
/// One focus handle serves the entire tree. Row focus follows logical IDs, not
/// flattened positions, and falls back to the nearest surviving ancestor after
/// a collapse/filter/model change. Ancestor IDs use space proportional to the
/// active row's depth; the state does not retain old snapshots or their rows.
/// Selection is controlled separately by the app.
pub struct VirtualTreeState<T: Clone + Eq + Hash + 'static> {
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
    active_id: Option<T>,
    active_ancestors: Vec<T>,
    last_model: std::sync::Weak<TreeModelData<T>>,
    last_active_index: Option<usize>,
    last_rendered_range: Range<usize>,
    accessibility_root: AccessibilityId,
    accessibility_data: Option<Arc<PreparedTreeAccessibility<T>>>,
    accessibility_snapshot: Option<Arc<AccessibilitySnapshot>>,
    accessibility_model: std::sync::Weak<TreeModelData<T>>,
    accessibility_current_model: Option<VirtualTreeModel<T>>,
    accessibility_toggle: bool,
}

impl<T: Clone + Eq + Hash + 'static> VirtualTreeState<T> {
    pub fn new(cx: &mut App) -> Self {
        Self {
            focus_handle: cx.focus_handle().tab_index(0).tab_stop(true),
            scroll_handle: UniformListScrollHandle::new(),
            active_id: None,
            active_ancestors: Vec::new(),
            last_model: std::sync::Weak::new(),
            last_active_index: None,
            last_rendered_range: 0..0,
            accessibility_root: AccessibilityId::new(),
            accessibility_data: None,
            accessibility_snapshot: None,
            accessibility_model: std::sync::Weak::new(),
            accessibility_current_model: None,
            accessibility_toggle: false,
        }
    }

    pub fn active_id(&self) -> Option<&T> {
        self.active_id.as_ref()
    }

    /// The most recent row range requested by the native virtual list. The
    /// native list also requests one measurement row before viewport layout.
    pub fn last_rendered_range(&self) -> Range<usize> {
        self.last_rendered_range.clone()
    }

    pub fn scroll_handle(&self) -> &UniformListScrollHandle {
        &self.scroll_handle
    }

    /// Activate an enabled displayed ID and request that it scroll into view.
    /// Returns false for missing or disabled rows. Notify the owning entity and
    /// refresh its window after calling this outside a component event handler.
    pub fn activate(&mut self, id: &T, model: &VirtualTreeModel<T>) -> bool {
        let Some(index) = model.enabled_index(id) else {
            return false;
        };
        self.set_active_index(index, model);
        self.scroll_handle
            .scroll_to_item(index, ScrollStrategy::Top);
        true
    }

    /// Scroll to any displayed ID without changing selection or keyboard focus.
    pub fn scroll_to_id(
        &self,
        id: &T,
        model: &VirtualTreeModel<T>,
        strategy: ScrollStrategy,
    ) -> bool {
        let Some(index) = model.index_of(id) else {
            return false;
        };
        self.scroll_handle.scroll_to_item(index, strategy);
        true
    }

    fn set_active_index(&mut self, index: usize, model: &VirtualTreeModel<T>) {
        self.active_id = Some(model.data.rows[index].node_id.clone());
        self.last_active_index = Some(index);
        self.active_ancestors.clear();
        let mut parent = model.parent_index(index);
        while let Some(index) = parent {
            self.active_ancestors
                .push(model.data.rows[index].node_id.clone());
            parent = model.parent_index(index);
        }
    }

    fn reconcile(&mut self, model: &VirtualTreeModel<T>, selected_id: Option<&T>) {
        if self.last_model.as_ptr() == Arc::as_ptr(&model.data) {
            return;
        }
        self.last_model = Arc::downgrade(&model.data);
        let previous_index = self.last_active_index;
        let existing = self
            .active_id
            .as_ref()
            .and_then(|id| model.enabled_index(id));
        let index = existing
            .or_else(|| {
                self.active_ancestors
                    .iter()
                    .find_map(|id| model.enabled_index(id))
            })
            .or_else(|| selected_id.and_then(|id| model.enabled_index(id)))
            .or_else(|| model.data.enabled.first().copied());
        if let Some(index) = index {
            self.set_active_index(index, model);
            if (previous_index.is_some() && previous_index != Some(index))
                || (previous_index.is_none() && selected_id.is_some())
            {
                self.scroll_handle
                    .scroll_to_item(index, ScrollStrategy::Top);
            }
        } else {
            self.active_id = None;
            self.last_active_index = None;
            self.active_ancestors.clear();
        }
    }

    /// Capture constant-time input for worker-side logical accessibility preparation.
    pub fn accessibility_preparation_context(
        &self,
        label: impl Into<SharedString>,
        has_toggle: bool,
    ) -> VirtualTreeAccessibilityContext<T> {
        VirtualTreeAccessibilityContext {
            root: self.accessibility_root,
            previous: self.accessibility_data.clone(),
            label: label.into(),
            has_toggle,
        }
    }

    /// Stable semantic identity for a currently displayed logical item.
    pub fn accessibility_id(&self, id: &T) -> Option<AccessibilityId> {
        let data = self.accessibility_data.as_ref()?;
        data.model_indices.get(id).map(|index| data.ids[*index])
    }

    fn logical_accessibility(
        &mut self,
        model: &VirtualTreeModel<T>,
        label: &SharedString,
        has_toggle: bool,
        _executor: &BackgroundExecutor,
    ) -> Arc<AccessibilitySnapshot> {
        if self.accessibility_model.as_ptr() == Arc::as_ptr(&model.data)
            && self.accessibility_toggle == has_toggle
            && self
                .accessibility_data
                .as_ref()
                .is_some_and(|data| data.label == *label)
        {
            return self.accessibility_snapshot.as_ref().unwrap().clone();
        }
        self.accessibility_model = Arc::downgrade(&model.data);
        self.accessibility_current_model = Some(model.clone());
        self.accessibility_toggle = has_toggle;
        let prepared = model
            .data
            .accessibility
            .as_ref()
            .filter(|data| {
                data.root == self.accessibility_root
                    && data.has_toggle == has_toggle
                    && data.label == *label
            })
            .cloned()
            .unwrap_or_else(|| {
                let mut context = self.accessibility_preparation_context(label.clone(), has_toggle);
                // A relabeled worker snapshot still owns the initial row identities.
                // Once published state exists, its current identities stay authoritative.
                if context.previous.is_none() {
                    context.previous = model
                        .data
                        .accessibility
                        .as_ref()
                        .filter(|data| data.root == self.accessibility_root)
                        .cloned();
                }
                build_accessibility(model, &context, Some(_executor))
            });
        let snapshot = prepared.snapshot.as_ref().unwrap().clone();
        self.accessibility_data = Some(prepared);
        self.accessibility_snapshot = Some(snapshot.clone());
        snapshot
    }
}

impl<T: Clone + Eq + Hash + 'static> Focusable for VirtualTreeState<T> {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NavigationAction {
    Move(usize),
    Toggle(usize, bool),
    Select(usize),
    None,
}

fn navigation_action<T: Clone + Eq + Hash + 'static>(
    model: &VirtualTreeModel<T>,
    active: Option<usize>,
    key: &str,
) -> NavigationAction {
    use NavigationAction::*;
    let enabled = &model.data.enabled;
    match key {
        "home" => return enabled.first().copied().map_or(None, Move),
        "end" => return enabled.last().copied().map_or(None, Move),
        _ => {}
    }
    let Some(index) = active else {
        return None;
    };
    let row = &model.data.rows[index];
    match key {
        "up" => {
            let position = enabled.partition_point(|candidate| *candidate < index);
            position
                .checked_sub(1)
                .map_or(None, |position| Move(enabled[position]))
        }
        "down" => {
            let position = enabled.partition_point(|candidate| *candidate <= index);
            enabled.get(position).copied().map_or(None, Move)
        }
        "right" if row.has_children && !model.data.expanded[index] => Toggle(index, true),
        "right" if row.has_children => model.data.rows[index + 1..]
            .iter()
            .enumerate()
            .take_while(|(_, child)| child.level > row.level)
            .find(|(_, child)| child.level == row.level + 1 && !child.disabled)
            .map_or(None, |(offset, _)| Move(index + offset + 1)),
        "left" if row.has_children && model.data.expanded[index] => Toggle(index, false),
        "left" => {
            let mut parent = model.parent_index(index);
            while let Some(index) = parent {
                if !model.data.rows[index].disabled {
                    return Move(index);
                }
                parent = model.parent_index(index);
            }
            None
        }
        "enter" | "space" => Select(index),
        _ => None,
    }
}

type SelectHandler<T> = Rc<dyn Fn(&T, &mut Window, &mut App)>;
type ToggleHandler<T> = Rc<dyn Fn(&T, bool, &mut Window, &mut App)>;
type RowDecorator<T> = Rc<dyn Fn(&T, Stateful<Div>, &mut Window, &mut App) -> Stateful<Div>>;

/// A themed, viewport-mounted tree using the native uniform list renderer.
///
/// Give this element a bounded height (`h`, `h_full`, or a flex allocation).
/// Keep its model and state in your view; constructing the model in `render`
/// would repeat flattening work. Arrow keys move logical focus, Left/Right
/// collapse/expand via the application callback, and Enter/Space select.
#[derive(IntoElement)]
pub struct VirtualTreeList<T: Clone + Eq + Hash + 'static> {
    id: ElementId,
    model: VirtualTreeModel<T>,
    state: Entity<VirtualTreeState<T>>,
    label: SharedString,
    selected_id: Option<T>,
    density: TreeListDensity,
    highlight_matches: bool,
    on_select: Option<SelectHandler<T>>,
    on_toggle: Option<ToggleHandler<T>>,
    on_activate: Option<SelectHandler<T>>,
    row_decorator: Option<RowDecorator<T>>,
    style: StyleRefinement,
}

impl<T: Clone + Eq + Hash + 'static> VirtualTreeList<T> {
    pub fn new(
        id: impl Into<ElementId>,
        model: VirtualTreeModel<T>,
        state: Entity<VirtualTreeState<T>>,
    ) -> Self {
        Self {
            id: id.into(),
            model,
            state,
            label: "Tree navigation".into(),
            selected_id: None,
            density: TreeListDensity::Balanced,
            highlight_matches: true,
            on_select: None,
            on_toggle: None,
            on_activate: None,
            row_decorator: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn selected_id(mut self, id: T) -> Self {
        self.selected_id = Some(id);
        self
    }

    pub fn density(mut self, density: TreeListDensity) -> Self {
        self.density = density;
        self
    }

    pub fn highlight_matches(mut self, highlight: bool) -> Self {
        self.highlight_matches = highlight;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(&T, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    /// Handle Enter/Space activation separately from pointer selection.
    /// Without this callback, activation uses `on_select` for compatibility.
    pub fn on_activate(mut self, handler: impl Fn(&T, &mut Window, &mut App) + 'static) -> Self {
        self.on_activate = Some(Rc::new(handler));
        self
    }

    /// Add row interactions or trailing content without rebuilding the virtual
    /// renderer. This runs only for mounted rows. Preserve the row's height,
    /// focus and accessibility attributes to keep navigation coherent.
    pub fn decorate_row(
        mut self,
        decorate: impl Fn(&T, Stateful<Div>, &mut Window, &mut App) -> Stateful<Div> + 'static,
    ) -> Self {
        self.row_decorator = Some(Rc::new(decorate));
        self
    }

    /// Request a controlled expansion change. Rebuild the model with the new
    /// expanded IDs in the callback and notify the owning view.
    pub fn on_toggle(
        mut self,
        handler: impl Fn(&T, bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

impl<T: Clone + Eq + Hash + 'static> Styled for VirtualTreeList<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn activate_row<T: Clone + Eq + Hash + 'static>(
    state: &Entity<VirtualTreeState<T>>,
    model: &VirtualTreeModel<T>,
    index: usize,
    window: &mut Window,
    cx: &mut App,
) {
    state.update(cx, |state, cx| {
        window.focus(&state.focus_handle);
        state.set_active_index(index, model);
        state
            .scroll_handle
            .scroll_to_item(index, ScrollStrategy::Top);
        cx.notify();
    });
    window.refresh();
}

// Capture only tokens used by rows rather than cloning every theme shadow,
// motion preset, and unused token into the renderer on each frame.
struct RowTheme {
    background: Hsla,
    foreground: Hsla,
    muted_foreground: Hsla,
    accent: Hsla,
    accent_foreground: Hsla,
    ring: Hsla,
    radius_sm: Pixels,
    font_family: SharedString,
}

impl RowTheme {
    fn of(cx: &App) -> Self {
        let tokens = &Theme::of(cx).tokens;
        Self {
            background: tokens.background,
            foreground: tokens.foreground,
            muted_foreground: tokens.muted_foreground,
            accent: tokens.accent,
            accent_foreground: tokens.accent_foreground,
            ring: tokens.ring,
            radius_sm: tokens.radius_sm,
            font_family: tokens.font_family.clone(),
        }
    }
}

impl<T: Clone + Eq + Hash + 'static> RenderOnce for VirtualTreeList<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state.update(cx, |state, _| {
            state.reconcile(&self.model, self.selected_id.as_ref());
        });
        let executor = cx.background_executor().clone();
        let snapshot = self.state.update(cx, |state, _| {
            state.logical_accessibility(
                &self.model,
                &self.label,
                self.on_toggle.is_some(),
                &executor,
            )
        });
        let semantic_root = snapshot.root;
        let active_descendant = self
            .state
            .read(cx)
            .active_id
            .as_ref()
            .and_then(|id| self.state.read(cx).accessibility_id(id));
        window.register_accessibility_snapshot(snapshot.clone());
        // Only selection changes need a logical overlay beyond mounted geometry.
        if let Some(id) = self
            .selected_id
            .as_ref()
            .and_then(|id| self.state.read(cx).accessibility_id(id))
        {
            let mut node = snapshot.get(id).unwrap().clone();
            node.states |= AccessibilityState::SELECTED;
            window.register_accessibility_node(node);
        }
        let action_state = self.state.clone();
        let action_select = self.on_select.clone();
        let action_toggle = self.on_toggle.clone();
        window.on_accessibility_subtree_action(
            snapshot,
            move |request, window, cx| {
                if request.node_id == semantic_root {
                    let state = action_state.read(cx);
                    window.focus(&state.focus_handle);
                    if let (Some(id), Some(model)) = (
                        state.active_id.as_ref(),
                        state.accessibility_current_model.as_ref(),
                    ) {
                        state.scroll_to_id(id, model, ScrollStrategy::Top);
                    }
                    return;
                }
                // Resolve against the latest semantic model at execution time.
                // A queued action must not use an outgoing frame's row index.
                let target = {
                    let state = action_state.read(cx);
                    state
                        .accessibility_data
                        .as_ref()
                        .unwrap()
                        .indices
                        .get(&request.node_id)
                        .copied()
                        .zip(state.accessibility_current_model.clone())
                };
                let Some((index, action_model)) = target else {
                    return;
                };
                let Some(row) = action_model.data.rows.get(index) else {
                    return;
                };
                if row.disabled {
                    return;
                }
                match request.action {
                    AccessibilityAction::Focus => {
                        activate_row(&action_state, &action_model, index, window, cx);
                    }
                    AccessibilityAction::ScrollToVisible => {
                        action_state.read(cx).scroll_to_id(
                            &row.node_id,
                            &action_model,
                            ScrollStrategy::Top,
                        );
                        window.refresh();
                    }
                    AccessibilityAction::Click => {
                        activate_row(&action_state, &action_model, index, window, cx);
                        if let Some(select) = &action_select {
                            select(&row.node_id, window, cx);
                        }
                    }
                    AccessibilityAction::Expand | AccessibilityAction::Collapse => {
                        activate_row(&action_state, &action_model, index, window, cx);
                        if let Some(toggle) = &action_toggle {
                            toggle(
                                &row.node_id,
                                request.action == AccessibilityAction::Expand,
                                window,
                                cx,
                            );
                        }
                    }
                    _ => {}
                }
            },
            cx,
        );
        let focus_handle = self.state.read(cx).focus_handle.clone();
        let scroll_handle = self.state.read(cx).scroll_handle.clone();
        let row_height = self.density.row_height();
        let indent_step = self.density.indent();
        let text_size = self.density.text_size();
        let theme = RowTheme::of(cx);
        let hover_color = crate::astryx::overlay_hover(theme.background.l < 0.5);
        let model = self.model.clone();
        let state = self.state.clone();
        let tree_id = self.id.clone();
        let key_model = self.model.clone();
        let key_state = self.state.clone();
        let key_select = self.on_activate.clone().or_else(|| self.on_select.clone());
        let key_toggle = self.on_toggle.clone();
        let selected_id = self.selected_id;
        let on_select = self.on_select;
        let on_toggle = self.on_toggle;
        let row_decorator = self.row_decorator;
        let highlight_matches = self.highlight_matches;
        let root_focus = focus_handle.clone();
        let root_state = self.state.clone();
        let root_model = self.model.clone();
        let root_focused = focus_handle.is_focused(window);

        let mut root_attributes = AccessibilityAttributes::new(AccessibilityRole::Tree)
            .id(semantic_root)
            .label(self.label.to_string())
            .state(AccessibilityState::FOCUSED, root_focused)
            .actions(vec![AccessibilityAction::Focus]);
        root_attributes.active_descendant = active_descendant;
        div()
            .id(self.id)
            .accessibility(root_attributes)
            .on_accessibility_action(AccessibilityAction::Focus, move |_, window, cx| {
                window.focus(&root_focus);
                if let Some(id) = root_state.read(cx).active_id.as_ref() {
                    root_state
                        .read(cx)
                        .scroll_to_id(id, &root_model, ScrollStrategy::Top);
                }
                window.refresh();
            })
            .w_full()
            .h_full()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .bg(theme.background)
            .on_key_down(move |event, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                let active = key_state
                    .read(cx)
                    .active_id
                    .as_ref()
                    .and_then(|id| key_model.enabled_index(id));
                let action = navigation_action(&key_model, active, &event.keystroke.key);
                let handled = match action {
                    NavigationAction::Move(index) => {
                        activate_row(&key_state, &key_model, index, window, cx);
                        true
                    }
                    NavigationAction::Toggle(index, expanded) => {
                        if let Some(handler) = key_toggle.as_ref() {
                            handler(&key_model.data.rows[index].node_id, expanded, window, cx);
                            true
                        } else {
                            false
                        }
                    }
                    NavigationAction::Select(index) => {
                        if let Some(handler) = key_select.as_ref() {
                            handler(&key_model.data.rows[index].node_id, window, cx);
                            true
                        } else {
                            false
                        }
                    }
                    NavigationAction::None => false,
                };
                if handled {
                    cx.stop_propagation();
                    window.prevent_default();
                }
            })
            .child(
                uniform_list(
                    ElementId::NamedChild(Box::new(tree_id.clone()), "rows".into()),
                    model.len(),
                    move |range, window, cx| {
                        state.update(cx, |state, _| state.last_rendered_range = range.clone());
                        let active_id = state.read(cx).active_id.clone();
                        let focused = state.read(cx).focus_handle.is_focused(window);
                        range
                            .map(|index| {
                                let row = &model.data.rows[index];
                                let selected = selected_id.as_ref() == Some(&row.node_id);
                                let active = active_id.as_ref() == Some(&row.node_id);
                                let expanded = model.data.expanded[index];
                                let mut flags = AccessibilityState::NONE;
                                if selected {
                                    flags |= AccessibilityState::SELECTED;
                                }
                                if row.disabled {
                                    flags |= AccessibilityState::DISABLED;
                                }
                                if row.has_children {
                                    flags |= if expanded {
                                        AccessibilityState::EXPANDED
                                    } else {
                                        AccessibilityState::COLLAPSED
                                    };
                                }
                                let mut actions = Vec::new();
                                if !row.disabled {
                                    actions.extend([
                                        AccessibilityAction::Focus,
                                        AccessibilityAction::Click,
                                        AccessibilityAction::ScrollToVisible,
                                    ]);
                                    if row.has_children && on_toggle.is_some() {
                                        actions.push(if expanded {
                                            AccessibilityAction::Collapse
                                        } else {
                                            AccessibilityAction::Expand
                                        });
                                    }
                                }
                                let mut item = div()
                                    .id(ElementId::NamedChild(
                                        Box::new(tree_id.clone()),
                                        format!("row-{index}").into(),
                                    ))
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::TreeItem)
                                            .id(state
                                                .read(cx)
                                                .accessibility_id(&row.node_id)
                                                .unwrap())
                                            .level(row.level + 1)
                                            .states(flags)
                                            .actions(actions),
                                    )
                                    .w_full()
                                    .h(row_height)
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .pr(px(8.0))
                                    .pl(px(8.0 + row.level as f32 * indent_step))
                                    .rounded(theme.radius_sm)
                                    .text_size(text_size)
                                    .line_height(px(20.0))
                                    .font_family(theme.font_family.clone())
                                    .text_color(if row.disabled {
                                        theme.muted_foreground
                                    } else {
                                        theme.foreground
                                    })
                                    .bg(if selected {
                                        theme.accent
                                    } else {
                                        kael::transparent_black()
                                    })
                                    .when(!row.disabled, |item| {
                                        item.cursor_pointer().hover(|style| style.bg(hover_color))
                                    })
                                    .when(active && focused, |item| {
                                        item.border_1().border_color(theme.ring)
                                    });
                                if !row.disabled {
                                    let click_state = state.clone();
                                    let click_model = model.clone();
                                    let select = on_select.clone();
                                    item = item.on_click(move |_, window, cx| {
                                        activate_row(&click_state, &click_model, index, window, cx);
                                        if let Some(handler) = select.as_ref() {
                                            handler(
                                                &click_model.data.rows[index].node_id,
                                                window,
                                                cx,
                                            );
                                        }
                                    });
                                }
                                let disclosure = if row.has_children {
                                    div()
                                        .id(ElementId::NamedChild(
                                            Box::new(tree_id.clone()),
                                            format!("toggle-{index}").into(),
                                        ))
                                        .w(px(24.0))
                                        .h(px(24.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            Icon::new(if expanded {
                                                "chevron-down"
                                            } else {
                                                "chevron-right"
                                            })
                                            .size(px(12.0))
                                            .color(theme.muted_foreground),
                                        )
                                        .when(!row.disabled && on_toggle.is_some(), |toggle| {
                                            let toggle_model = model.clone();
                                            let toggle_state = state.clone();
                                            let toggle_handler = on_toggle.clone().unwrap();
                                            toggle.cursor_pointer().on_click(
                                                move |_, window, cx| {
                                                    cx.stop_propagation();
                                                    activate_row(
                                                        &toggle_state,
                                                        &toggle_model,
                                                        index,
                                                        window,
                                                        cx,
                                                    );
                                                    toggle_handler(
                                                        &toggle_model.data.rows[index].node_id,
                                                        !expanded,
                                                        window,
                                                        cx,
                                                    );
                                                },
                                            )
                                        })
                                        .into_any_element()
                                } else {
                                    div().w(px(24.0)).h(px(24.0)).into_any_element()
                                };
                                let label = TreeList::<T>::render_highlighted_text(
                                    &row.label,
                                    &row.match_ranges,
                                    if selected {
                                        theme.accent_foreground.opacity(0.3)
                                    } else {
                                        theme.accent.opacity(0.3)
                                    },
                                    highlight_matches,
                                    true,
                                );
                                let item = item
                                    .child(disclosure)
                                    .children(row.icon.as_ref().map(|icon| {
                                        Icon::new(icon.clone()).size(px(16.0)).color(
                                            if row.disabled {
                                                theme.muted_foreground
                                            } else {
                                                row.icon_color.unwrap_or(theme.muted_foreground)
                                            },
                                        )
                                    }))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .child(label),
                                    );
                                if let Some(decorate) = row_decorator.as_ref() {
                                    decorate(&row.node_id, item, window, cx)
                                } else {
                                    item
                                }
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .without_accessibility_wrappers()
                .track_scroll(scroll_handle)
                .track_focus(&focus_handle)
                .w_full()
                .h_full()
                .flex_1()
                .min_h(px(0.0)),
            )
            .map(|mut element| {
                element.style().refine(&self.style);
                element
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn sample_nodes() -> Vec<TreeNode<usize>> {
        vec![
            TreeNode::new(0, "root").with_children(vec![
                TreeNode::new(1, "disabled child").disabled(true),
                TreeNode::new(2, "branch").with_children(vec![TreeNode::new(3, "leaf")]),
                TreeNode::new(4, "last child"),
            ]),
            TreeNode::new(5, "disabled root").disabled(true),
            TreeNode::new(6, "last root"),
        ]
    }

    #[::core::prelude::v1::test]
    fn model_reuses_storage_and_rejects_duplicate_visible_ids() {
        let nodes = sample_nodes();
        let model = VirtualTreeModel::new(&nodes, &HashSet::from([0, 2])).unwrap();
        let clone = model.clone();
        assert!(Arc::ptr_eq(&model.data, &clone.data));
        assert_eq!(model.len(), 7);
        assert_eq!(model.index_of(&3), Some(3));
        assert_eq!(model.parent_index(3), Some(2));
        assert_eq!(model.level_at(3), Some(2));
        assert_eq!(model.id_at(7), None);
        assert_eq!(model.index_of(&99), None);
        assert!(Arc::ptr_eq(
            &model.data.catalog_indices,
            &model.data.indices
        ));

        let collapsed = VirtualTreeModel::new(&nodes, &HashSet::new()).unwrap();
        assert_eq!(collapsed.len(), 3);
        assert_eq!(collapsed.data.catalog_indices.len(), 7);
        assert!(!Arc::ptr_eq(
            &collapsed.data.catalog_indices,
            &collapsed.data.indices
        ));
        let lazy = VirtualTreeModel::new(
            &[TreeNode::new(99, "lazy").with_lazy_children(true)],
            &HashSet::new(),
        )
        .unwrap();
        assert!(lazy.data.rows[0].has_children);
        assert!(!lazy.data.expanded[0]);
        assert_eq!(lazy.data.catalog_indices.len(), 1);

        let duplicate = vec![TreeNode::new(1, "a"), TreeNode::new(1, "b")];
        assert!(matches!(
            VirtualTreeModel::new(&duplicate, &HashSet::new()),
            Err(VirtualTreeModelError::DuplicateId),
        ));
    }

    #[::core::prelude::v1::test]
    fn fully_displayed_factory_reuses_validated_index_without_recloning_catalog() {
        thread_local! {
            static HASH_CALLS: Cell<usize> = const { Cell::new(0) };
            static CLONE_CALLS: Cell<usize> = const { Cell::new(0) };
        }
        #[derive(Debug, PartialEq, Eq)]
        struct CountedId(usize);
        impl Clone for CountedId {
            fn clone(&self) -> Self {
                CLONE_CALLS.with(|calls| calls.set(calls.get() + 1));
                Self(self.0)
            }
        }
        impl Hash for CountedId {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                HASH_CALLS.with(|calls| calls.set(calls.get() + 1));
                self.0.hash(state);
            }
        }

        const ROOTS: usize = 25;
        const CHILDREN: usize = 4_000;
        const ITEMS: usize = ROOTS * (CHILDREN + 1);
        let nodes: Vec<_> = (0..ROOTS)
            .map(|root| {
                let base = root * (CHILDREN + 1);
                TreeNode::new(CountedId(base), "root").with_children(
                    (1..=CHILDREN)
                        .map(|child| TreeNode::new(CountedId(base + child), "file"))
                        .collect(),
                )
            })
            .collect();
        let expanded = (0..ROOTS)
            .map(|root| CountedId(root * (CHILDREN + 1)))
            .collect();
        HASH_CALLS.with(|calls| calls.set(0));
        CLONE_CALLS.with(|calls| calls.set(0));
        let model = VirtualTreeModel::new(&nodes, &expanded).unwrap();
        assert_eq!(model.len(), ITEMS);
        assert_eq!(
            CLONE_CALLS.with(Cell::get),
            ITEMS * 2,
            "only flattening rows and indexing rows should clone IDs"
        );
        assert_eq!(
            HASH_CALLS.with(Cell::get),
            ITEMS * 2 + ROOTS,
            "only row indexing, expansion lookup, and nonempty source branches should hash IDs"
        );
        assert!(Arc::ptr_eq(
            &model.data.catalog_indices,
            &model.data.indices
        ));
    }

    #[::core::prelude::v1::test]
    fn catalog_accepts_arbitrary_order_and_hidden_ids() {
        let nodes: Vec<_> = [10, 20, 30]
            .into_iter()
            .map(|id| TreeNode::new(id, "row"))
            .collect();
        let mut model = VirtualTreeModel::new(&nodes, &HashSet::new()).unwrap();
        model.set_accessibility_catalog_ids([30, 10, 20]).unwrap();
        assert!(Arc::ptr_eq(
            &model.data.catalog_indices,
            &model.data.indices
        ));
        for ids in [[10, 20, 30, 40], [10, 40, 20, 30], [40, 30, 10, 20]] {
            model.set_accessibility_catalog_ids(ids).unwrap();
            assert_eq!(model.data.catalog_indices.len(), ids.len());
            for (index, id) in ids.into_iter().enumerate() {
                assert_eq!(model.data.catalog_indices.get(&id), Some(&index));
            }
        }
        model.set_accessibility_catalog_ids([10, 20, 30]).unwrap();
        assert!(Arc::ptr_eq(
            &model.data.catalog_indices,
            &model.data.indices
        ));
        assert!(!model.data.catalog_indices.contains_key(&40));

        let mut empty = VirtualTreeModel::<usize>::new(&[], &HashSet::new()).unwrap();
        assert!(Arc::ptr_eq(
            &empty.data.catalog_indices,
            &empty.data.indices
        ));
        empty.set_accessibility_catalog_ids([99]).unwrap();
        assert_eq!(empty.data.catalog_indices.get(&99), Some(&0));
        empty.set_accessibility_catalog_ids([]).unwrap();
        assert!(Arc::ptr_eq(
            &empty.data.catalog_indices,
            &empty.data.indices
        ));
    }

    #[::core::prelude::v1::test]
    fn invalid_catalog_does_not_replace_previous_metadata() {
        let nodes: Vec<_> = [10, 20, 30]
            .into_iter()
            .map(|id| TreeNode::new(id, "row"))
            .collect();
        let mut model = VirtualTreeModel::new(&nodes, &HashSet::new()).unwrap();
        model
            .set_accessibility_catalog_ids([10, 20, 30, 40])
            .unwrap();
        let original = model.data.catalog_indices.clone();
        for ids in [
            vec![10, 20, 10, 30],
            vec![10, 20, 30, 40, 40],
            vec![10, 40, 20, 30, 40],
        ] {
            assert_eq!(
                model.set_accessibility_catalog_ids(ids),
                Err(VirtualTreeModelError::DuplicateId)
            );
            assert!(Arc::ptr_eq(&model.data.catalog_indices, &original));
        }
        for ids in [vec![], vec![10, 20], vec![40, 10, 20]] {
            assert_eq!(
                model.set_accessibility_catalog_ids(ids),
                Err(VirtualTreeModelError::IncompleteCatalog)
            );
            assert!(Arc::ptr_eq(&model.data.catalog_indices, &original));
        }
    }

    #[::core::prelude::v1::test]
    fn hidden_duplicate_source_ids_are_rejected_after_matching_prefix() {
        let nodes = vec![TreeNode::new(0, "root").with_children(vec![
            TreeNode::new(1, "hidden"),
            TreeNode::new(1, "hidden duplicate"),
        ])];
        for result in [
            VirtualTreeModel::new(&nodes, &HashSet::new()),
            VirtualTreeModel::filtered(&nodes, &HashSet::new(), "root", false),
        ] {
            assert!(matches!(result, Err(VirtualTreeModelError::DuplicateId)));
        }
    }

    #[::core::prelude::v1::test]
    fn keyboard_navigation_skips_disabled_rows_and_preserves_hierarchy() {
        let model = VirtualTreeModel::new(&sample_nodes(), &HashSet::from([0, 2])).unwrap();
        assert_eq!(
            navigation_action(&model, Some(0), "down"),
            NavigationAction::Move(2)
        );
        assert_eq!(
            navigation_action(&model, Some(2), "up"),
            NavigationAction::Move(0)
        );
        assert_eq!(
            navigation_action(&model, Some(4), "down"),
            NavigationAction::Move(6)
        );
        assert_eq!(
            navigation_action(&model, Some(0), "right"),
            NavigationAction::Move(2)
        );
        assert_eq!(
            navigation_action(&model, Some(3), "left"),
            NavigationAction::Move(2)
        );
        assert_eq!(
            navigation_action(&model, Some(2), "left"),
            NavigationAction::Toggle(2, false)
        );
        assert_eq!(
            navigation_action(&model, Some(3), "enter"),
            NavigationAction::Select(3)
        );
        assert_eq!(
            navigation_action(&model, Some(0), "end"),
            NavigationAction::Move(6)
        );
        assert_eq!(
            navigation_action(&model, Some(6), "home"),
            NavigationAction::Move(0)
        );
        assert_eq!(
            navigation_action(&model, Some(6), "down"),
            NavigationAction::None
        );

        let collapsed = VirtualTreeModel::new(&sample_nodes(), &HashSet::new()).unwrap();
        assert_eq!(
            navigation_action(&collapsed, Some(0), "right"),
            NavigationAction::Toggle(0, true)
        );
        let empty = VirtualTreeModel::<usize>::new(&[], &HashSet::new()).unwrap();
        assert_eq!(
            navigation_action(&empty, None, "end"),
            NavigationAction::None
        );
    }

    #[::core::prelude::v1::test]
    fn filtering_auto_expands_ancestors_and_preserves_unicode_highlights() {
        let nodes =
            vec![TreeNode::new(0, "root").with_children(vec![TreeNode::new(1, "İstanbul")])];
        let model = VirtualTreeModel::filtered(&nodes, &HashSet::new(), "ST", true).unwrap();
        assert_eq!(model.len(), 2);
        assert!(model.data.expanded[0]);
        assert_eq!(model.data.rows[1].match_ranges, vec![(1, 3)]);
        assert_eq!(
            navigation_action(&model, Some(0), "left"),
            NavigationAction::Toggle(0, false)
        );
        let empty = VirtualTreeModel::filtered(&nodes, &HashSet::new(), "absent", true).unwrap();
        assert!(empty.is_empty());
    }

    #[::core::prelude::v1::test]
    fn logical_focus_survives_insertions_and_falls_back_to_collapsed_ancestors() {
        let cx = TestAppContext::single();
        cx.update(|cx| {
            let nodes = sample_nodes();
            let expanded = HashSet::from([0, 2]);
            let model = VirtualTreeModel::new(&nodes, &expanded).unwrap();
            let mut state = VirtualTreeState::new(cx);
            state.reconcile(&model, Some(&3));
            assert_eq!(state.active_id(), Some(&3));
            assert!(!state.activate(&1, &model));
            assert!(!state.activate(&99, &model));

            let mut inserted = vec![TreeNode::new(99, "inserted root")];
            inserted.extend(nodes.clone());
            let inserted_model = VirtualTreeModel::new(&inserted, &expanded).unwrap();
            state.reconcile(&inserted_model, None);
            assert_eq!(state.active_id(), Some(&3));
            assert_eq!(state.scroll_handle.deferred_item_index(), Some(4));

            let collapsed = VirtualTreeModel::new(&nodes, &HashSet::from([0])).unwrap();
            state.reconcile(&collapsed, None);
            assert_eq!(state.active_id(), Some(&2));
            let roots = VirtualTreeModel::new(&nodes, &HashSet::new()).unwrap();
            state.reconcile(&roots, None);
            assert_eq!(state.active_id(), Some(&0));

            let empty = VirtualTreeModel::new(&[], &HashSet::new()).unwrap();
            state.reconcile(&empty, None);
            assert_eq!(state.active_id(), None);
        });
    }

    #[::core::prelude::v1::test]
    fn semantic_identity_survives_disclosure_and_filter_but_retires_deleted_catalog_ids() {
        let cx = TestAppContext::single();
        let worker = cx.background_executor.clone();
        cx.update(|cx| {
            let nodes =
                vec![TreeNode::new(0, "project").with_children(vec![TreeNode::new(1, "document")])];
            let expanded = HashSet::from([0]);
            let mut state = VirtualTreeState::new(cx);
            let mut adopt = |mut model: VirtualTreeModel<usize>| {
                model
                    .prepare_accessibility(
                        state.accessibility_preparation_context("files", true),
                        &worker,
                    )
                    .unwrap();
                state.logical_accessibility(&model, &"files".into(), true, &worker);
                model
            };
            let initial = adopt(VirtualTreeModel::new(&nodes, &expanded).unwrap());
            let original = initial.data.accessibility.as_ref().unwrap().ids[1];
            let collapsed = adopt(VirtualTreeModel::new(&nodes, &HashSet::new()).unwrap());
            let prepared = collapsed.data.accessibility.as_ref().unwrap();
            assert_eq!(collapsed.len(), 1);
            assert_eq!(prepared.catalog_indices.len(), 2);
            assert!(
                !prepared.indices.contains_key(&original),
                "collapsed native handles cannot resolve actions"
            );
            let restored = adopt(VirtualTreeModel::new(&nodes, &expanded).unwrap());
            assert_eq!(
                restored.data.accessibility.as_ref().unwrap().ids[1],
                original
            );

            let filtered =
                adopt(VirtualTreeModel::filtered(&nodes, &expanded, "missing", true).unwrap());
            assert_eq!(filtered.len(), 0);
            assert_eq!(
                filtered
                    .data
                    .accessibility
                    .as_ref()
                    .unwrap()
                    .catalog_indices
                    .len(),
                2
            );
            let restored = adopt(VirtualTreeModel::new(&nodes, &expanded).unwrap());
            assert_eq!(
                restored.data.accessibility.as_ref().unwrap().ids[1],
                original
            );

            let removed =
                adopt(VirtualTreeModel::new(&[TreeNode::new(0, "project")], &expanded).unwrap());
            assert_eq!(
                removed
                    .data
                    .accessibility
                    .as_ref()
                    .unwrap()
                    .catalog_indices
                    .len(),
                1
            );
            let reinserted = adopt(VirtualTreeModel::new(&nodes, &expanded).unwrap());
            assert_ne!(
                reinserted.data.accessibility.as_ref().unwrap().ids[1],
                original
            );
        });
    }

    struct TestTreeView {
        model: VirtualTreeModel<usize>,
        state: Entity<VirtualTreeState<usize>>,
        selected: Rc<Cell<Option<usize>>>,
        toggles: Rc<RefCell<Vec<(usize, bool)>>>,
        outside_focus: FocusHandle,
    }

    impl Render for TestTreeView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let selected = self.selected.clone();
            let toggles = self.toggles.clone();
            div().flex().flex_col().children([
                VirtualTreeList::new("test-tree", self.model.clone(), self.state.clone())
                    .when_some(self.selected.get(), |tree, id| tree.selected_id(id))
                    .on_select(move |id, _, _| selected.set(Some(*id)))
                    .on_toggle(move |id, expanded, _, _| toggles.borrow_mut().push((*id, expanded)))
                    .w(px(400.0))
                    .h(px(180.0))
                    .into_any_element(),
                div()
                    .id("outside-tree")
                    .track_focus(&self.outside_focus)
                    .w(px(20.0))
                    .h(px(20.0))
                    .into_any_element(),
            ])
        }
    }

    #[::core::prelude::v1::test]
    fn native_fixture_hierarchy_retains_twenty_five_projects_and_four_thousand_children() {
        let nodes = (0..25)
            .map(|directory| {
                let root = directory * 4001;
                TreeNode::new(root, format!("Project {directory:02}")).with_children(
                    (1..=4000)
                        .map(|file| TreeNode::new(root + file, format!("document_{file:04}.rs")))
                        .collect(),
                )
            })
            .collect::<Vec<_>>();
        let expanded = (0..25)
            .map(|directory| directory * 4001)
            .collect::<HashSet<_>>();
        let mut model = VirtualTreeModel::new(&nodes, &expanded).unwrap();
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
        let state = cx.new(|cx| VirtualTreeState::new(cx));
        let context = cx.update(|cx| {
            state
                .read(cx)
                .accessibility_preparation_context("Tree navigation", true)
        });
        let executor = cx.background_executor.clone();
        let worker = executor.clone();
        let model = executor.block_test(executor.spawn(async move {
            model.prepare_accessibility(context, &worker).unwrap();
            model
        }));
        let root = model.data.accessibility.as_ref().unwrap().root;
        let (view, window) = cx.add_window_view(|_, cx| TestTreeView {
            model,
            state,
            selected: Rc::new(Cell::new(None)),
            toggles: Rc::new(RefCell::new(Vec::new())),
            outside_focus: cx.focus_handle().tab_index(0).tab_stop(true),
        });
        let previous = window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            let projects = &tree.get(root).unwrap().children;
            assert_eq!(projects.len(), 25);
            assert!(view.read(cx).state.read(cx).last_rendered_range.len() <= 6);
            for project in projects {
                let project = tree.get(*project).unwrap();
                assert_eq!(project.parent, Some(root));
                assert_eq!(project.children.len(), 4000);
                for child in &project.children {
                    assert_eq!(tree.get(*child).unwrap().parent, Some(project.id));
                }
            }
            let update = tree.to_accesskit_tree_update(None, None);
            let native_root = update.nodes.iter().find(|(id, _)| id.0 == root.0).unwrap();
            assert_eq!(native_root.1.children().len(), 25);
            for project in projects {
                let native_project = update
                    .nodes
                    .iter()
                    .find(|(id, _)| id.0 == project.0)
                    .unwrap();
                assert_eq!(native_project.1.children().len(), 4000);
            }
            tree.clone()
        });
        window.update(|window, cx| window.focus(&view.read(cx).state.read(cx).focus_handle));
        window.simulate_keystrokes("end");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            assert_eq!(tree.get(root).unwrap().children.len(), 25);
            let last = *tree.get(root).unwrap().children.last().unwrap();
            assert_eq!(tree.get(last).unwrap().children.len(), 4000);
            let update = tree.to_accesskit_tree_update_after(Some(&previous), None, None);
            assert!(
                update.nodes.len() < 30,
                "viewport focus/geometry delta stays bounded"
            );
            if let Some((_, native_root)) = update.nodes.iter().find(|(id, _)| id.0 == root.0) {
                assert_eq!(native_root.children().len(), 25);
            }
        });
    }

    #[::core::prelude::v1::test]
    fn outgoing_models_retire_when_rows_scroll_out_of_view() {
        let nodes = (0..1000)
            .map(|id| TreeNode::new(id, format!("File {id}")))
            .collect::<Vec<_>>();
        let mut model = VirtualTreeModel::new(&nodes, &HashSet::new()).unwrap();
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
        let state = cx.new(|cx| VirtualTreeState::new(cx));
        let worker = cx.background_executor.clone();
        let context = cx.update(|cx| {
            state
                .read(cx)
                .accessibility_preparation_context("Tree navigation", true)
        });
        model.prepare_accessibility(context, &worker).unwrap();
        let outgoing = Arc::downgrade(&model.data);
        let (view, window) = cx.add_window_view(|_, cx| TestTreeView {
            model,
            state,
            selected: Rc::new(Cell::new(None)),
            toggles: Rc::new(RefCell::new(Vec::new())),
            outside_focus: cx.focus_handle().tab_index(0).tab_stop(true),
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.focus(&view.read(cx).state.read(cx).focus_handle);
        });
        window.simulate_keystrokes("end");
        window.update(|window, cx| {
            window.draw(cx).clear();
            view.update(cx, |view, cx| {
                let mut replacement = VirtualTreeModel::new(&nodes, &HashSet::new()).unwrap();
                let context = view
                    .state
                    .read(cx)
                    .accessibility_preparation_context("Tree navigation", true);
                replacement.prepare_accessibility(context, &worker).unwrap();
                view.model = replacement;
                cx.notify();
            });
            window.draw(cx).clear();
            window.draw(cx).clear();
        });
        assert!(
            outgoing.upgrade().is_none(),
            "offscreen mounted-row callbacks must not retain an outgoing logical model"
        );
    }

    #[::core::prelude::v1::test]
    fn hundred_thousand_rows_mount_only_the_viewport_and_end_scrolls_into_view() {
        let nodes = (0..100_000)
            .map(|id| TreeNode::new(id, format!("File {id}")))
            .collect::<Vec<_>>();
        let mut model = VirtualTreeModel::new(&nodes, &HashSet::new()).unwrap();
        drop(nodes);
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
        let state = cx.new(|cx| VirtualTreeState::new(cx));
        let context = cx.update(|cx| {
            state
                .read(cx)
                .accessibility_preparation_context("Tree navigation", true)
        });
        let executor = cx.background_executor.clone();
        let worker = executor.clone();
        let model = executor.block_test(executor.spawn(async move {
            model.prepare_accessibility(context, &worker).unwrap();
            model
        }));
        let prepared = model.data.accessibility.as_ref().unwrap().clone();
        let (view, window) = cx.add_window_view(|_, cx| TestTreeView {
            model,
            state,
            selected: Rc::new(Cell::new(None)),
            toggles: Rc::new(RefCell::new(Vec::new())),
            outside_focus: cx.focus_handle().tab_index(0).tab_stop(true),
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            let state = view.read(cx).state.read(cx);
            assert!(
                Arc::ptr_eq(state.accessibility_data.as_ref().unwrap(), &prepared),
                "first paint adopts prepared metadata rather than rebuilding labels/maps"
            );
            assert_eq!(state.active_id(), Some(&0));
            assert!(state.last_rendered_range.len() <= 6);
            assert!(state.last_rendered_range.start < 10);
            let logical = window
                .accessibility_tree()
                .nodes
                .values()
                .filter(|node| node.role == AccessibilityRole::TreeItem)
                .count();
            assert_eq!(logical, 100_000);
            let logical_root = prepared.root;
            let root = window.accessibility_tree().get(logical_root).unwrap();
            let base = prepared
                .snapshot
                .as_ref()
                .unwrap()
                .get(logical_root)
                .unwrap();
            assert_eq!(root.children.len(), 100_000);
            assert_eq!(
                root.children.as_ptr(),
                base.children.as_ptr(),
                "mounted rows must preserve the shared logical child list"
            );
            assert!(
                !window
                    .accessibility_tree()
                    .nodes
                    .values()
                    .any(|node| matches!(
                        node.role,
                        AccessibilityRole::List | AccessibilityRole::ListItem
                    ))
            );
            let mounted = window
                .accessibility_tree()
                .nodes
                .values()
                .filter(|node| node.role == AccessibilityRole::TreeItem && node.bounds.is_some())
                .count();
            assert!(mounted > 0 && mounted <= 6, "mounted {mounted} tree rows");
            window.focus(&state.focus_handle);
            window.draw(cx).clear();
        });
        window.simulate_keystrokes("end");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let view = view.read(cx);
            let state = view.state.read(cx);
            assert_eq!(state.active_id(), Some(&99_999));
            assert!(state.last_rendered_range.contains(&99_999));
            assert!(state.last_rendered_range.len() <= 6);
            assert_eq!(window.accessibility_tree().focused_node_count(), 1);
            let focused = window.accessibility_tree().focused_node().unwrap();
            let active = window.accessibility_tree().nodes[&focused]
                .active_descendant
                .unwrap();
            assert_eq!(
                window.accessibility_tree().nodes[&active].label.as_deref(),
                Some("File 99999")
            );
            // Every row shares the tree's focus handle. Tab advances directly
            // to the next control instead of traversing mounted row handles.
            window.focus_next();
            assert!(view.outside_focus.is_focused(window));
        });
        window.update(|window, cx| {
            window.focus(&view.read(cx).state.read(cx).focus_handle);
        });
        window.simulate_keystrokes("enter");
        window.update(|_, cx| assert_eq!(view.read(cx).selected.get(), Some(99_999)));
    }

    #[::core::prelude::v1::test]
    fn mounted_accessibility_actions_are_wired_and_disabled_rows_are_inert() {
        let model = VirtualTreeModel::new(&sample_nodes(), &HashSet::from([0])).unwrap();
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
        let (view, window) = cx.add_window_view(|_, cx| TestTreeView {
            model,
            state: cx.new(|cx| VirtualTreeState::new(cx)),
            selected: Rc::new(Cell::new(None)),
            toggles: Rc::new(RefCell::new(Vec::new())),
            outside_focus: cx.focus_handle().tab_index(0).tab_stop(true),
        });
        let root_id = window.update(|window, cx| {
            window.draw(cx).clear();
            let rows = window
                .accessibility_tree()
                .nodes
                .values()
                .filter(|node| node.role == AccessibilityRole::TreeItem)
                .collect::<Vec<_>>();
            let root = rows
                .iter()
                .find(|node| node.label.as_deref() == Some("root"))
                .unwrap();
            assert_eq!(root.level, Some(1));
            assert!(root.states.contains(AccessibilityState::EXPANDED));
            for action in [
                AccessibilityAction::Focus,
                AccessibilityAction::Click,
                AccessibilityAction::Collapse,
            ] {
                assert!(root.actions.contains(&action));
                assert!(window.has_accessibility_action_handler(root.id, action));
            }
            let disabled = rows
                .iter()
                .find(|node| node.label.as_deref() == Some("disabled child"))
                .unwrap();
            assert!(disabled.states.contains(AccessibilityState::DISABLED));
            assert!(disabled.actions.is_empty());
            assert_eq!(disabled.level, Some(2));
            root.id
        });
        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                root_id,
                AccessibilityAction::Collapse,
            ));
        });
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(*view.read(cx).toggles.borrow(), vec![(0, false)]);
            assert_eq!(view.read(cx).state.read(cx).active_id(), Some(&0));
            assert_eq!(window.accessibility_tree().focused_node_count(), 1);
        });
    }

    #[::core::prelude::v1::test]
    fn initial_selected_row_below_the_viewport_is_activated_and_scrolled() {
        let nodes = (0..1_000)
            .map(|id| TreeNode::new(id, format!("File {id}")))
            .collect::<Vec<_>>();
        let model = VirtualTreeModel::new(&nodes, &HashSet::new()).unwrap();
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
        let (view, window) = cx.add_window_view(|_, cx| TestTreeView {
            model,
            state: cx.new(|cx| VirtualTreeState::new(cx)),
            selected: Rc::new(Cell::new(Some(999))),
            toggles: Rc::new(RefCell::new(Vec::new())),
            outside_focus: cx.focus_handle().tab_index(0).tab_stop(true),
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            let state = view.read(cx).state.read(cx);
            assert_eq!(state.active_id(), Some(&999));
            assert!(state.last_rendered_range().contains(&999));
        });
    }

    #[::core::prelude::v1::test]
    fn offscreen_actions_focus_survives_scroll_and_queued_actions_resolve_current_ids() {
        let nodes = (0..1_000)
            .map(|id| TreeNode::new(id, format!("File {id}")))
            .collect::<Vec<_>>();
        let model = VirtualTreeModel::new(&nodes, &HashSet::new()).unwrap();
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
        let (view, window) = cx.add_window_view(|_, cx| TestTreeView {
            model,
            state: cx.new(|cx| VirtualTreeState::new(cx)),
            selected: Rc::new(Cell::new(None)),
            toggles: Rc::new(RefCell::new(Vec::new())),
            outside_focus: cx.focus_handle().tab_index(0).tab_stop(true),
        });
        let target = window.update(|window, cx| {
            window.draw(cx).clear();
            let target = view.read(cx).state.read(cx).accessibility_id(&800).unwrap();
            assert!(
                window
                    .accessibility_tree()
                    .get(target)
                    .unwrap()
                    .bounds
                    .is_none()
            );
            for action in [
                AccessibilityAction::Focus,
                AccessibilityAction::Click,
                AccessibilityAction::ScrollToVisible,
            ] {
                assert!(window.has_accessibility_action_handler(target, action));
            }
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                target,
                AccessibilityAction::Click,
            ));
            target
        });
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            let view = view.read(cx);
            let state = view.state.read(cx);
            assert_eq!(state.active_id(), Some(&800));
            assert_eq!(view.selected.get(), Some(800));
            assert!(state.last_rendered_range().contains(&800));
            // Scrolling independently of keyboard navigation must preserve the
            // active accessible item even when its physical row is unmounted.
            state.scroll_to_id(&0, &view.model, ScrollStrategy::Top);
            window.refresh();
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            let state = view.read(cx).state.read(cx);
            assert!(!state.last_rendered_range().contains(&800));
            let root = window
                .accessibility_tree()
                .get(window.accessibility_tree().focused_node().unwrap())
                .unwrap();
            assert_eq!(root.active_descendant, Some(target));
            assert!(
                window
                    .accessibility_tree()
                    .get(target)
                    .unwrap()
                    .bounds
                    .is_none()
            );
            assert!(
                window
                    .accessibility_tree()
                    .get(target)
                    .unwrap()
                    .states
                    .contains(AccessibilityState::SELECTED)
            );
            let previous = window.accessibility_tree().clone();
            let snapshot = state.accessibility_snapshot.as_ref().unwrap().clone();
            window.refresh();
            window.draw(cx).clear();
            assert!(Arc::ptr_eq(
                view.read(cx)
                    .state
                    .read(cx)
                    .accessibility_snapshot
                    .as_ref()
                    .unwrap(),
                &snapshot
            ));
            assert!(
                window
                    .accessibility_tree()
                    .to_accesskit_tree_update_after(Some(&previous), None, None)
                    .nodes
                    .is_empty()
            );
        });
        let mut replacement_nodes = vec![TreeNode::new(2_000, "inserted")];
        replacement_nodes.extend(nodes.clone());
        let mut replacement = VirtualTreeModel::new(&replacement_nodes, &HashSet::new()).unwrap();
        let context = window.update(|_, cx| {
            view.read(cx)
                .state
                .read(cx)
                .accessibility_preparation_context("Tree navigation", true)
        });
        let executor = window.update(|_, cx| cx.background_executor().clone());
        let worker = executor.clone();
        let replacement = executor.block_test(executor.spawn(async move {
            replacement.prepare_accessibility(context, &worker).unwrap();
            replacement
        }));
        window.update(|window, cx| {
            view.read(cx).selected.set(None);
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                target,
                AccessibilityAction::Click,
            ));
            view.update(cx, |view, cx| {
                view.model = replacement;
                cx.notify();
            });
            window.draw(cx).clear();
            assert_eq!(
                view.read(cx).state.read(cx).accessibility_id(&800),
                Some(target)
            );
        });
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).selected.get(), Some(800));
            assert_eq!(view.read(cx).state.read(cx).active_id(), Some(&800));
        });
        let remaining = replacement_nodes
            .into_iter()
            .filter(|row| row.id != 800)
            .collect::<Vec<_>>();
        let mut replacement = VirtualTreeModel::new(&remaining, &HashSet::new()).unwrap();
        let context = window.update(|_, cx| {
            view.read(cx)
                .state
                .read(cx)
                .accessibility_preparation_context("Tree navigation", true)
        });
        let worker = executor.clone();
        let replacement = executor.block_test(executor.spawn(async move {
            replacement.prepare_accessibility(context, &worker).unwrap();
            replacement
        }));
        window.update(|window, cx| {
            view.read(cx).selected.set(None);
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                target,
                AccessibilityAction::Click,
            ));
            view.update(cx, |view, cx| {
                view.model = replacement;
                cx.notify();
            });
            window.draw(cx).clear();
            assert!(
                view.read(cx)
                    .state
                    .read(cx)
                    .accessibility_id(&800)
                    .is_none()
            );
            assert!(window.accessibility_tree().get(target).is_none());
        });
        window.run_until_parked();
        window.update(|_, cx| assert_eq!(view.read(cx).selected.get(), None));
    }

    fn adopt_record_model(
        mut model: VirtualTreeModel<usize>,
        state: &mut VirtualTreeState<usize>,
        worker: &BackgroundExecutor,
        label: &str,
        has_toggle: bool,
    ) -> VirtualTreeModel<usize> {
        let label: SharedString = label.to_owned().into();
        model
            .prepare_accessibility(
                state.accessibility_preparation_context(label.clone(), has_toggle),
                worker,
            )
            .unwrap();
        state.logical_accessibility(&model, &label, has_toggle, worker);
        model
    }

    fn record_snapshot(model: &VirtualTreeModel<usize>) -> &Arc<AccessibilitySnapshot> {
        model
            .data
            .accessibility
            .as_ref()
            .unwrap()
            .snapshot
            .as_ref()
            .unwrap()
    }

    fn record_id(model: &VirtualTreeModel<usize>, id: usize) -> AccessibilityId {
        let data = model.data.accessibility.as_ref().unwrap();
        data.ids[model.index_of(&id).unwrap()]
    }

    #[::core::prelude::v1::test]
    fn disclosure_shares_records_by_semantics_not_shifted_positions_or_hidden_history() {
        let mut cx = TestAppContext::single();
        let worker = cx.background_executor.clone();
        let mut state = cx.update(VirtualTreeState::new);
        let nodes = vec![
            TreeNode::new(0, "Project 日本語").with_children(vec![TreeNode::new(1, "👩🏽‍💻 café")]),
            TreeNode::new(2, "Project 2").with_children(vec![TreeNode::new(3, "tail")]),
        ];
        let all = HashSet::from([0, 2]);
        let initial = adopt_record_model(
            VirtualTreeModel::new(&nodes, &all).unwrap(),
            &mut state,
            &worker,
            "files",
            true,
        );
        let first = record_snapshot(&initial).clone();
        let old_leaf = record_id(&initial, 1);
        let tail = record_id(&initial, 3);
        let root = first.root;
        let collapsed = adopt_record_model(
            VirtualTreeModel::new(&nodes, &HashSet::from([2])).unwrap(),
            &mut state,
            &worker,
            "files",
            true,
        );
        let second = record_snapshot(&collapsed);
        assert_eq!(collapsed.index_of(&3), Some(2));
        assert_eq!(initial.index_of(&3), Some(3));
        assert_eq!(second.len(), 4);
        assert!(second.get(old_leaf).is_none());
        assert!(std::ptr::eq(
            first.get(root).unwrap(),
            second.get(root).unwrap()
        ));
        assert!(std::ptr::eq(
            first.get(tail).unwrap(),
            second.get(tail).unwrap()
        ));
        assert!(!std::ptr::eq(
            first.get(record_id(&initial, 0)).unwrap(),
            second.get(record_id(&collapsed, 0)).unwrap()
        ));
        assert_eq!(
            second.get(record_id(&collapsed, 0)).unwrap().children.len(),
            0
        );
        assert_eq!(first.get(record_id(&initial, 0)).unwrap().children.len(), 1);
        let restored = adopt_record_model(
            VirtualTreeModel::new(&nodes, &all).unwrap(),
            &mut state,
            &worker,
            "files",
            true,
        );
        assert_eq!(record_id(&restored, 1), old_leaf);
        assert!(
            !std::ptr::eq(
                first.get(old_leaf).unwrap(),
                record_snapshot(&restored).get(old_leaf).unwrap()
            ),
            "an external old snapshot is not a hidden payload cache"
        );
        assert!(std::ptr::eq(
            second.get(tail).unwrap(),
            record_snapshot(&restored).get(tail).unwrap()
        ));
        assert_eq!(
            first.get(old_leaf).unwrap().label.as_deref(),
            Some("👩🏽‍💻 café")
        );
        drop(initial);
        drop(collapsed);
        drop(first);
        cx.run_until_parked();
        assert_eq!(record_snapshot(&restored).len(), 5);
    }

    #[::core::prelude::v1::test]
    fn hundred_thousand_disclosure_records_share_full_offscreen_hierarchy() {
        let mut cx = TestAppContext::single();
        let worker = cx.background_executor.clone();
        let mut state = cx.update(VirtualTreeState::new);
        let nodes = (0..25)
            .map(|project| {
                let root = project * 4001;
                TreeNode::new(root, format!("Project {project:02} 日本語")).with_children(
                    (1..=4000)
                        .map(|file| {
                            TreeNode::new(root + file, format!("document_{file:04} 👩🏽‍💻 café"))
                        })
                        .collect(),
                )
            })
            .collect::<Vec<_>>();
        let all = (0..25)
            .map(|project| project * 4001)
            .collect::<HashSet<_>>();
        let mut current = adopt_record_model(
            VirtualTreeModel::new(&nodes, &all).unwrap(),
            &mut state,
            &worker,
            "files",
            true,
        );
        assert_eq!(record_snapshot(&current).len(), 100026);
        for project in [0, 12, 24] {
            let parent_id = project * 4001;
            let before = record_snapshot(&current).clone();
            let parent = record_id(&current, parent_id);
            let old_leaf = record_id(&current, parent_id + 4000);
            let mut expanded = all.clone();
            expanded.remove(&parent_id);
            let collapsed = adopt_record_model(
                VirtualTreeModel::new(&nodes, &expanded).unwrap(),
                &mut state,
                &worker,
                "files",
                true,
            );
            let after = record_snapshot(&collapsed);
            assert_eq!(after.len(), 96026);
            assert!(after.get(old_leaf).is_none());
            let mut reused = usize::from(std::ptr::eq(
                before.get(before.root).unwrap(),
                after.get(after.root).unwrap(),
            ));
            for index in 0..collapsed.len() {
                let id = collapsed.data.accessibility.as_ref().unwrap().ids[index];
                let node = after.get(id).unwrap();
                assert_eq!(
                    node.label.as_deref(),
                    Some(collapsed.label_at(index).unwrap().as_ref())
                );
                assert_eq!(node.level, Some(collapsed.level_at(index).unwrap() + 1));
                let expected_parent = collapsed
                    .parent_index(index)
                    .map(|parent| collapsed.data.accessibility.as_ref().unwrap().ids[parent])
                    .unwrap_or(after.root);
                assert_eq!(node.parent, Some(expected_parent));
                if std::ptr::eq(before.get(id).unwrap(), node) {
                    reused += 1;
                }
                assert_eq!(
                    node.children.len(),
                    if node.level == Some(1) && id != parent {
                        4000
                    } else {
                        0
                    }
                );
            }
            assert_eq!(reused, 96025);
            assert_eq!(after.get(parent).unwrap().children.len(), 0);
            assert_eq!(before.get(parent).unwrap().children.len(), 4000);
            assert_eq!(after.get(after.root).unwrap().children.len(), 25);
            current = adopt_record_model(
                VirtualTreeModel::new(&nodes, &all).unwrap(),
                &mut state,
                &worker,
                "files",
                true,
            );
            let restored = record_snapshot(&current);
            assert_eq!(record_id(&current, parent_id + 4000), old_leaf);
            assert!(!std::ptr::eq(
                before.get(old_leaf).unwrap(),
                restored.get(old_leaf).unwrap()
            ));
            assert_eq!(restored.get(parent).unwrap().children.len(), 4000);
            drop(before);
            drop(collapsed);
            cx.run_until_parked();
        }
    }

    #[::core::prelude::v1::test]
    fn shared_tree_records_rebuild_changed_labels_topology_policy_and_defaults() {
        let cx = TestAppContext::single();
        let worker = cx.background_executor.clone();
        let mut state = cx.update(VirtualTreeState::new);
        let mut nodes = vec![
            TreeNode::new(0, "Project")
                .with_children(vec![TreeNode::new(1, "日本語"), TreeNode::new(2, "café")]),
            TreeNode::new(10, "Other").with_children(vec![TreeNode::new(11, "tail")]),
        ];
        let all = HashSet::from([0, 10]);
        let mut current = adopt_record_model(
            VirtualTreeModel::new(&nodes, &all).unwrap(),
            &mut state,
            &worker,
            "files",
            true,
        );
        let mut before = record_snapshot(&current).clone();
        let leaf = record_id(&current, 1);
        let sibling = record_id(&current, 2);
        nodes[0].children[0].label = "日本語 changed".into();
        current = adopt_record_model(
            VirtualTreeModel::new(&nodes, &all).unwrap(),
            &mut state,
            &worker,
            "files",
            true,
        );
        assert!(!std::ptr::eq(
            before.get(leaf).unwrap(),
            record_snapshot(&current).get(leaf).unwrap()
        ));
        assert!(std::ptr::eq(
            before.get(sibling).unwrap(),
            record_snapshot(&current).get(sibling).unwrap()
        ));
        assert_eq!(before.get(leaf).unwrap().label.as_deref(), Some("日本語"));
        before = record_snapshot(&current).clone();
        nodes[0].children.swap(0, 1);
        current = adopt_record_model(
            VirtualTreeModel::new(&nodes, &all).unwrap(),
            &mut state,
            &worker,
            "files",
            true,
        );
        assert!(!std::ptr::eq(
            before.get(record_id(&current, 0)).unwrap(),
            record_snapshot(&current)
                .get(record_id(&current, 0))
                .unwrap()
        ));
        assert!(std::ptr::eq(
            before.get(leaf).unwrap(),
            record_snapshot(&current).get(leaf).unwrap()
        ));
        before = record_snapshot(&current).clone();
        let moved = nodes[0].children.remove(1);
        nodes[1].children.push(moved);
        current = adopt_record_model(
            VirtualTreeModel::new(&nodes, &all).unwrap(),
            &mut state,
            &worker,
            "files",
            true,
        );
        assert!(!std::ptr::eq(
            before.get(leaf).unwrap(),
            record_snapshot(&current).get(leaf).unwrap()
        ));
        assert_eq!(
            record_snapshot(&current).get(leaf).unwrap().parent,
            Some(record_id(&current, 10))
        );
        before = record_snapshot(&current).clone();
        nodes[1].children[1].disabled = true;
        current = adopt_record_model(
            VirtualTreeModel::new(&nodes, &all).unwrap(),
            &mut state,
            &worker,
            "files",
            true,
        );
        assert!(!std::ptr::eq(
            before.get(leaf).unwrap(),
            record_snapshot(&current).get(leaf).unwrap()
        ));
        assert!(
            record_snapshot(&current)
                .get(leaf)
                .unwrap()
                .actions
                .is_empty()
        );
        before = record_snapshot(&current).clone();
        current = adopt_record_model(
            VirtualTreeModel::new(&nodes, &all).unwrap(),
            &mut state,
            &worker,
            "renamed files",
            false,
        );
        assert!(!std::ptr::eq(
            before.get(before.root).unwrap(),
            record_snapshot(&current).get(before.root).unwrap()
        ));
        assert!(!std::ptr::eq(
            before.get(record_id(&current, 10)).unwrap(),
            record_snapshot(&current)
                .get(record_id(&current, 10))
                .unwrap()
        ));
        assert!(std::ptr::eq(
            before.get(sibling).unwrap(),
            record_snapshot(&current).get(sibling).unwrap()
        ));
    }

    #[::core::prelude::v1::test]
    fn logical_tree_record_matching_checks_every_generated_and_default_field() {
        let make_record = || LogicalTreeRecord {
            id: AccessibilityId(20),
            role: AccessibilityRole::TreeItem,
            states: AccessibilityState::NONE,
            label: "日本語",
            level: Some(1),
            parent: Some(AccessibilityId(10)),
            children: &[],
            actions: &[AccessibilityAction::Focus],
        };
        let pristine = make_record().into_node();
        assert!(make_record().matches(&pristine));
        let mutations: &[fn(&mut AccessibilityNode)] = &[
            |n| n.id = AccessibilityId(21),
            |n| n.role = AccessibilityRole::Button,
            |n| n.states = AccessibilityState::SELECTED,
            |n| n.label = Some("changed".into()),
            |n| n.level = Some(2),
            |n| n.parent = Some(AccessibilityId(99)),
            |n| n.children.push(AccessibilityId(99)),
            |n| n.actions.push(AccessibilityAction::Click),
            |n| n.description = Some("extra".into()),
            |n| n.value = Some(AccessibilityValue::Text("extra".into())),
            |n| n.placeholder = Some("extra".into()),
            |n| n.row_count = Some(1),
            |n| n.column_count = Some(1),
            |n| n.row_index = Some(1),
            |n| n.column_index = Some(1),
            |n| n.row_span = Some(1),
            |n| n.column_span = Some(1),
            |n| n.sort_direction = Some(AccessibilitySortDirection::Ascending),
            |n| n.bounds = Some(AccessibilityRect::new(0.0, 0.0, 1.0, 1.0)),
            |n| n.active_descendant = Some(AccessibilityId(99)),
            |n| n.text_document = Some(AccessibilityTextDocument::new("text")),
            |n| {
                n.text_selection = Some(AccessibilityTextSelection {
                    anchor: 0,
                    focus: 0,
                })
            },
            |n| {
                n.text_geometry = Some(
                    AccessibilityTextGeometry::new(
                        &AccessibilityTextDocument::new("text"),
                        Vec::new(),
                    )
                    .unwrap(),
                )
            },
        ];
        for (index, mutate) in mutations.iter().enumerate() {
            let mut altered = pristine.clone();
            mutate(&mut altered);
            assert!(
                !make_record().matches(&altered),
                "field mutation {index} must force a new record"
            );
        }
    }

    #[::core::prelude::v1::test]
    fn shared_tree_preparation_preserves_legacy_owned_fallback_and_retires_old_generations() {
        let mut cx = TestAppContext::single();
        let worker = cx.background_executor.clone();
        let mut state = cx.update(VirtualTreeState::new);
        let nodes = vec![TreeNode::new(0, "root").with_children(vec![TreeNode::new(1, "leaf")])];
        let all = HashSet::from([0]);
        let model = VirtualTreeModel::new(&nodes, &all).unwrap();
        let owned = build_accessibility(
            &model,
            &state.accessibility_preparation_context("files", true),
            None,
        );
        let owned_snapshot = owned.snapshot.as_ref().unwrap();
        let legacy_context = VirtualTreeAccessibilityContext {
            root: owned.root,
            previous: Some(owned.clone()),
            label: "files".into(),
            has_toggle: true,
        };
        let shared = build_accessibility(&model, &legacy_context, Some(&worker));
        assert_eq!(shared.snapshot.as_ref().unwrap(), owned_snapshot);
        drop(legacy_context);
        let mut current = adopt_record_model(model, &mut state, &worker, "files", true);
        for _ in 0..32 {
            let old_model = Arc::downgrade(&current.data);
            let old_prepared = Arc::downgrade(current.data.accessibility.as_ref().unwrap());
            let old_snapshot = Arc::downgrade(record_snapshot(&current));
            let next = adopt_record_model(
                VirtualTreeModel::new(&nodes, &all).unwrap(),
                &mut state,
                &worker,
                "files",
                true,
            );
            assert!(std::ptr::eq(
                record_snapshot(&current)
                    .get(record_id(&current, 1))
                    .unwrap(),
                record_snapshot(&next).get(record_id(&next, 1)).unwrap()
            ));
            current = next;
            cx.run_until_parked();
            assert!(old_model.upgrade().is_none());
            assert!(old_prepared.upgrade().is_none());
            assert!(old_snapshot.upgrade().is_none());
        }
    }

    #[::core::prelude::v1::test]
    fn accessibility_root_label_updates_on_the_same_tree_model() {
        let cx = TestAppContext::single();
        let worker = cx.background_executor.clone();
        let mut state = cx.update(VirtualTreeState::new);
        let model = VirtualTreeModel::new(&[TreeNode::new(7, "Entry")], &HashSet::new()).unwrap();
        let first = state.logical_accessibility(&model, &"Files".into(), false, &worker);
        let row = state.accessibility_id(&7).unwrap();
        let renamed =
            state.logical_accessibility(&model, &"Contacts 日本語".into(), false, &worker);
        assert_eq!(
            first.get(first.root).unwrap().label.as_deref(),
            Some("Files")
        );
        assert_eq!(
            renamed.get(renamed.root).unwrap().label.as_deref(),
            Some("Contacts 日本語")
        );
        assert_eq!(renamed.root, first.root);
        assert_eq!(state.accessibility_id(&7), Some(row));
        assert_eq!(renamed.get(row).unwrap().label.as_deref(), Some("Entry"));
        assert!(!Arc::ptr_eq(&first, &renamed));
        let reused = state.logical_accessibility(&model, &"Contacts 日本語".into(), false, &worker);
        assert!(Arc::ptr_eq(&renamed, &reused));
    }

    #[::core::prelude::v1::test]
    fn accessibility_rejects_worker_preparation_for_a_different_root_label() {
        let cx = TestAppContext::single();
        let worker = cx.background_executor.clone();
        let mut state = cx.update(VirtualTreeState::new);
        let mut model =
            VirtualTreeModel::new(&[TreeNode::new(7, "Entry")], &HashSet::new()).unwrap();
        model
            .prepare_accessibility(
                state.accessibility_preparation_context("Queued", false),
                &worker,
            )
            .unwrap();
        let prepared = record_snapshot(&model).clone();
        let displayed = state.logical_accessibility(&model, &"Visible".into(), false, &worker);
        assert_eq!(
            prepared.get(prepared.root).unwrap().label.as_deref(),
            Some("Queued")
        );
        assert_eq!(
            displayed.get(displayed.root).unwrap().label.as_deref(),
            Some("Visible")
        );
        assert_eq!(displayed.root, prepared.root);
        let row = state.accessibility_id(&7).unwrap();
        assert_eq!(displayed.get(row).unwrap().label.as_deref(), Some("Entry"));
        assert!(!Arc::ptr_eq(&prepared, &displayed));
        let matching = state.logical_accessibility(&model, &"Queued".into(), false, &worker);
        assert!(Arc::ptr_eq(&prepared, &matching));
        assert_eq!(state.accessibility_id(&7), Some(row));
        assert_eq!(
            displayed.get(displayed.root).unwrap().label.as_deref(),
            Some("Visible")
        );
    }
}

// Functional coverage for arbitrary public catalogs. No capacity, hash-count or
// latency assertion is a maintained public contract.
#[cfg(test)]
mod catalog_reserve_regressions {
    use super::*;

    struct LooseUpperCatalog {
        ids: std::vec::IntoIter<usize>,
    }

    impl Iterator for LooseUpperCatalog {
        type Item = usize;

        fn next(&mut self) -> Option<Self::Item> {
            self.ids.next()
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            // This is a legal loose upper bound, including for an empty input.
            (0, Some(usize::MAX))
        }
    }

    fn loose(ids: Vec<usize>) -> LooseUpperCatalog {
        LooseUpperCatalog {
            ids: ids.into_iter(),
        }
    }

    #[::core::prelude::v1::test]
    fn arbitrary_catalog_loose_upper_bound_keeps_validation_and_previous_metadata() {
        let nodes = vec![TreeNode::new(10, "Workspace 日本語").with_children(vec![
            TreeNode::new(11, "inactive child").disabled(true),
            TreeNode::new(12, "資料"),
        ])];
        let mut model = VirtualTreeModel::new(&nodes, &HashSet::new()).unwrap();
        assert_eq!(model.len(), 1);
        assert_eq!(model.id_at(0), Some(&10));
        assert_eq!(
            model.label_at(0).map(AsRef::<str>::as_ref),
            Some("Workspace 日本語")
        );
        model
            .set_accessibility_catalog_ids(loose(vec![12, 10, 11]))
            .unwrap();
        let original = model.data.catalog_indices.clone();
        for (ids, error) in [
            (vec![], VirtualTreeModelError::IncompleteCatalog),
            (vec![12, 11], VirtualTreeModelError::IncompleteCatalog),
            (vec![12, 10, 11, 12], VirtualTreeModelError::DuplicateId),
        ] {
            assert_eq!(model.set_accessibility_catalog_ids(loose(ids)), Err(error));
            assert!(Arc::ptr_eq(&original, &model.data.catalog_indices));
            assert_eq!(model.len(), 1);
            assert_eq!(model.id_at(0), Some(&10));
            for (index, id) in [12, 10, 11].into_iter().enumerate() {
                assert_eq!(model.data.catalog_indices.get(&id), Some(&index));
            }
        }
        model
            .set_accessibility_catalog_ids(loose(vec![10]))
            .unwrap();
        assert!(Arc::ptr_eq(
            &model.data.indices,
            &model.data.catalog_indices
        ));
        assert_eq!(model.index_of(&10), Some(0));
        assert_eq!(model.index_of(&11), None);
    }
}

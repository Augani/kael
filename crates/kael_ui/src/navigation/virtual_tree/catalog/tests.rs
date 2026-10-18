//! Prospective prototype regressions; not compiled or executed.
use super::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn forest() -> Vec<TreeNode<usize>> {
    vec![
        TreeNode::new(0, "Project 日本語").with_children(vec![
            TreeNode::new(1, "disabled").disabled(true),
            TreeNode::new(2, "branch").with_children(vec![TreeNode::new(3, "👩🏽‍💻 café")]),
        ]),
        TreeNode::new(4, "lazy").with_lazy_children(true),
        TreeNode::new(5, "tail"),
    ]
}

#[::core::prelude::v1::test]
fn legacy_non_send_ids_and_projected_disclosure_keep_existing_observables() {
    // A new private storage variant must not narrow the legacy public ID domain.
    let legacy = VirtualTreeModel::new(
        &[TreeNode::new(Rc::new("local".to_owned()), "local ID")],
        &HashSet::new(),
    )
    .unwrap();
    assert_eq!(
        legacy.label_at(0).map(|label| label.as_ref()),
        Some("local ID")
    );
    let mut cx = TestAppContext::single();
    let worker = cx.background_executor.clone();
    let nodes = forest();
    let catalog = VirtualTreeCatalog::from_nodes(nodes.clone(), &worker).unwrap();
    assert_eq!(catalog.len(), 6);
    for expanded in [
        HashSet::new(),
        HashSet::from([0]),
        HashSet::from([0, 2]),
        HashSet::from([0, 2, 4]),
        HashSet::from([2, 99]),
    ] {
        let old = VirtualTreeModel::new(&nodes, &expanded).unwrap();
        let projected = catalog.model(&expanded).unwrap();
        assert_eq!(old.len(), projected.len());
        assert_eq!(old.data.parents, projected.data.parents);
        assert_eq!(old.data.expanded, projected.data.expanded);
        assert_eq!(old.data.enabled, projected.data.enabled);
        assert_eq!(old.data.catalog_indices, projected.data.catalog_indices);
        for (index, (a, b)) in old
            .data
            .rows
            .iter()
            .zip(projected.data.rows.iter())
            .enumerate()
        {
            assert_eq!(
                (
                    a.node_id,
                    &a.label,
                    a.level,
                    a.disabled,
                    a.has_children,
                    &a.match_ranges
                ),
                (
                    b.node_id,
                    &b.label,
                    b.level,
                    b.disabled,
                    b.has_children,
                    &b.match_ranges
                )
            );
            assert_eq!(old.id_at(index), projected.id_at(index));
            assert_eq!(old.index_of(&a.node_id), projected.index_of(&a.node_id));
            assert_eq!(
                projected.data.rows.iter().nth(index).map(|row| row.node_id),
                Some(a.node_id)
            );
            let mut skipped = projected.data.rows.iter().skip(index);
            assert_eq!(skipped.next().map(|row| row.node_id), Some(a.node_id));
            for key in ["up", "down", "left", "right", "home", "end", "enter"] {
                assert_eq!(
                    navigation_action(&old, Some(index), key),
                    navigation_action(&projected, Some(index), key)
                );
            }
        }
        assert!(projected.data.rows.iter().nth(projected.len()).is_none());
        assert!(projected.id_at(projected.len()).is_none());
    }
    drop(catalog);
    cx.run_until_parked();
}

fn prepare(
    mut model: VirtualTreeModel<usize>,
    state: &mut VirtualTreeState<usize>,
    worker: &BackgroundExecutor,
) -> VirtualTreeModel<usize> {
    model
        .prepare_accessibility(
            state.accessibility_preparation_context("Catalog files", true),
            worker,
        )
        .unwrap();
    state.logical_accessibility(&model, &"Catalog files".into(), true, worker);
    model
}

#[::core::prelude::v1::test]
fn projected_full_graph_matches_owned_and_current_catalog_controls_identity() {
    let mut cx = TestAppContext::single();
    let worker = cx.background_executor.clone();
    let mut state = cx.update(VirtualTreeState::new);
    let nodes = forest();
    let catalog = VirtualTreeCatalog::from_nodes(nodes.clone(), &worker).unwrap();
    let expanded = HashSet::from([0, 2]);
    let old = prepare(
        VirtualTreeModel::new(&nodes, &expanded).unwrap(),
        &mut state,
        &worker,
    );
    let old_snapshot = old
        .data
        .accessibility
        .as_ref()
        .unwrap()
        .snapshot
        .as_ref()
        .unwrap()
        .clone();
    let current = prepare(catalog.model(&expanded).unwrap(), &mut state, &worker);
    let prepared = current.data.accessibility.as_ref().unwrap();
    let snapshot = prepared.snapshot.as_ref().unwrap();
    assert_eq!(snapshot.as_ref(), old_snapshot.as_ref());
    assert_eq!(snapshot.len(), current.len() + 1);
    let leaf_id = prepared.ids[current.index_of(&3).unwrap()];
    let collapsed = prepare(catalog.model(&HashSet::new()).unwrap(), &mut state, &worker);
    assert!(
        collapsed
            .data
            .accessibility
            .as_ref()
            .unwrap()
            .snapshot
            .as_ref()
            .unwrap()
            .get(leaf_id)
            .is_none()
    );
    let restored = prepare(catalog.model(&expanded).unwrap(), &mut state, &worker);
    assert_eq!(
        restored.data.accessibility.as_ref().unwrap().ids[restored.index_of(&3).unwrap()],
        leaf_id
    );
    let removed_catalog =
        VirtualTreeCatalog::from_nodes(vec![TreeNode::new(0, "only root")], &worker).unwrap();
    let _removed = prepare(
        removed_catalog.model(&HashSet::new()).unwrap(),
        &mut state,
        &worker,
    );
    // Explicit old catalog clones do not count as current content or history.
    let reintroduced = prepare(catalog.model(&expanded).unwrap(), &mut state, &worker);
    assert_ne!(
        reintroduced.data.accessibility.as_ref().unwrap().ids[reintroduced.index_of(&3).unwrap()],
        leaf_id
    );
    cx.run_until_parked();
}

struct CatalogHost {
    model: VirtualTreeModel<usize>,
    state: Entity<VirtualTreeState<usize>>,
    selected: Rc<Cell<Option<usize>>>,
}

impl Render for CatalogHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected.clone();
        div().child(
            VirtualTreeList::new("catalog-tree", self.model.clone(), self.state.clone())
                .on_select(move |id, _, _| selected.set(Some(*id)))
                .on_toggle(|_, _, _, _| {})
                .w(px(400.))
                .h(px(100.)),
        )
    }
}

#[::core::prelude::v1::test]
fn projected_offscreen_actions_resolve_current_rows_and_removed_ids_are_inert() {
    let mut cx = TestAppContext::single();
    cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
    let worker = cx.background_executor.clone();
    let catalog = VirtualTreeCatalog::from_nodes(
        (0..1_000)
            .map(|id| TreeNode::new(id, format!("File {id}")))
            .collect(),
        &worker,
    )
    .unwrap();
    let state = cx.new(|cx| VirtualTreeState::new(cx));
    let mut model = catalog.model(&HashSet::new()).unwrap();
    let context = cx.update(|cx| {
        state
            .read(cx)
            .accessibility_preparation_context("Tree navigation", true)
    });
    model.prepare_accessibility(context, &worker).unwrap();
    let target = model.data.accessibility.as_ref().unwrap().ids[900];
    let (view, window) = cx.add_window_view(|_, _| CatalogHost {
        model,
        state,
        selected: Rc::new(Cell::new(None)),
    });
    window.update(|window, cx| {
        window.draw(cx).clear();
        assert!(view.read(cx).state.read(cx).last_rendered_range.len() < 20);
        assert_eq!(
            window
                .accessibility_tree()
                .nodes
                .values()
                .filter(|node| node.role == AccessibilityRole::TreeItem)
                .count(),
            1_000
        );
        let native = window
            .accessibility_tree()
            .to_accesskit_tree_update(None, None);
        assert_eq!(
            native
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == AccessibilityRole::TreeItem.to_accesskit())
                .count(),
            1_000
        );
        window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
            target,
            AccessibilityAction::Click,
        ));
    });
    window.run_until_parked();
    window.update(|window, cx| {
        window.draw(cx).clear();
        assert_eq!(view.read(cx).selected.get(), Some(900));
        assert!(
            view.read(cx)
                .state
                .read(cx)
                .last_rendered_range
                .contains(&900)
        );
        view.read(cx).selected.set(None);
        // Queue the old ID, replace the current catalog, paint, then drain.
        window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
            target,
            AccessibilityAction::Click,
        ));
        let replacement_catalog =
            VirtualTreeCatalog::from_nodes(vec![TreeNode::new(7, "remaining")], &worker).unwrap();
        let mut replacement = replacement_catalog.model(&HashSet::new()).unwrap();
        let context = view
            .read(cx)
            .state
            .read(cx)
            .accessibility_preparation_context("Tree navigation", true);
        replacement.prepare_accessibility(context, &worker).unwrap();
        view.update(cx, |view, cx| {
            view.model = replacement;
            cx.notify();
        });
        window.draw(cx).clear();
        assert!(window.accessibility_tree().get(target).is_none());
    });
    window.run_until_parked();
    window.update(|_, cx| assert_eq!(view.read(cx).selected.get(), None));
}

struct Payload(Arc<AtomicUsize>);
impl Drop for Payload {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct WitnessId {
    id: usize,
    payload: Arc<Payload>,
}
impl Clone for WitnessId {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            payload: self.payload.clone(),
        }
    }
}
impl PartialEq for WitnessId {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for WitnessId {}
impl Hash for WitnessId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

fn deep_forest(
    count: usize,
    drops: &Arc<AtomicUsize>,
    duplicate: bool,
) -> Vec<TreeNode<WitnessId>> {
    let mut children = Vec::new();
    for id in (0..count).rev() {
        let node = TreeNode::new(
            WitnessId {
                id,
                payload: Arc::new(Payload(drops.clone())),
            },
            "deep",
        )
        .with_children(children);
        children = vec![node];
    }
    if duplicate {
        children.insert(
            0,
            TreeNode::new(
                WitnessId {
                    id: 0,
                    payload: Arc::new(Payload(drops.clone())),
                },
                "duplicate",
            ),
        );
    }
    children
}

#[::core::prelude::v1::test]
fn deep_success_duplicate_and_unpolled_input_drop_release_actual_payloads() {
    const DEPTH: usize = 20_000;
    let mut cx = TestAppContext::single();
    let worker = cx.background_executor.clone();
    for duplicate in [false, true] {
        let drops = Arc::new(AtomicUsize::new(0));
        let result = VirtualTreeCatalog::from_nodes(deep_forest(DEPTH, &drops, duplicate), &worker);
        if duplicate {
            assert!(matches!(result, Err(VirtualTreeModelError::DuplicateId)));
        } else {
            let catalog = result.unwrap();
            assert_eq!(catalog.len(), DEPTH);
            drop(catalog);
        }
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "normal retirement enqueues the whole payload batch"
        );
        cx.run_until_parked();
        assert_eq!(drops.load(Ordering::SeqCst), DEPTH + usize::from(duplicate));
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let input = PendingForest {
        frames: vec![InputFrame {
            siblings: deep_forest(DEPTH, &drops, false).into_iter(),
            parent: None,
            level: 0,
        }],
        current: None,
    };
    let unpolled = async move {
        drop(input);
    };
    drop(unpolled);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        DEPTH,
        "an unpolled batch future still owns iterative input teardown"
    );
}

struct PanicCloneId {
    id: usize,
    panic: Arc<AtomicBool>,
    payload: Arc<Payload>,
}
impl Clone for PanicCloneId {
    fn clone(&self) -> Self {
        assert!(
            !self.panic.load(Ordering::SeqCst),
            "authored ID clone panic"
        );
        Self {
            id: self.id,
            panic: self.panic.clone(),
            payload: self.payload.clone(),
        }
    }
}
impl PartialEq for PanicCloneId {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for PanicCloneId {}
impl Hash for PanicCloneId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[::core::prelude::v1::test]
fn user_clone_unwind_keeps_current_nested_input_in_owned_retirement_batch() {
    let mut cx = TestAppContext::single();
    let worker = cx.background_executor.clone();
    let panic = Arc::new(AtomicBool::new(true));
    let drops = Arc::new(AtomicUsize::new(0));
    let mut children = Vec::new();
    for id in (0..20_000).rev() {
        children = vec![
            TreeNode::new(
                PanicCloneId {
                    id,
                    panic: panic.clone(),
                    payload: Arc::new(Payload(drops.clone())),
                },
                "deep",
            )
            .with_children(children),
        ];
    }
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = VirtualTreeCatalog::from_nodes(children, &worker);
    }));
    assert!(caught.is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    cx.run_until_parked();
    assert_eq!(drops.load(Ordering::SeqCst), 20_000);
}

#[::core::prelude::v1::test]
fn explicit_catalog_generations_share_source_payload_without_retaining_old_models() {
    let mut cx = TestAppContext::single();
    let worker = cx.background_executor.clone();
    let catalog = VirtualTreeCatalog::from_nodes(forest(), &worker).unwrap();
    let expanded = HashSet::from([0, 2]);
    let model = catalog.model(&expanded).unwrap();
    assert!(Arc::ptr_eq(&model.data.indices, &catalog.data.indices));
    for row in model.data.rows.iter() {
        let source = catalog.data.indices[&row.node_id];
        assert!(std::ptr::eq(row, &catalog.data.rows[source]));
    }
    let outgoing = Arc::downgrade(&model.data);
    let source = Arc::downgrade(&catalog.data);
    let next = catalog.model(&HashSet::new()).unwrap();
    drop(model);
    assert!(
        outgoing.upgrade().is_none(),
        "catalogs and new models have no reverse model/snapshot chain"
    );
    drop(catalog);
    cx.run_until_parked();
    assert!(
        source.upgrade().is_some(),
        "current projected model owns its explicit source generation"
    );
    drop(next);
    cx.run_until_parked();
    assert!(source.upgrade().is_none());
}

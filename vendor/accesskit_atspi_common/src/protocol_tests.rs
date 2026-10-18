// Licensed under Apache-2.0 or MIT, matching the adapter.
use crate::*;
use accesskit::{
    Action as AkAction, ActionHandler, ActionRequest, Node, NodeId as LocalId, Role as AkRole,
    Tree, TreeId, TreeUpdate,
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Captured {
    actions: Mutex<Vec<ActionRequest>>,
    edits: Mutex<Vec<TextEditRequest>>,
    states: Mutex<Vec<(State, bool)>>,
}
struct Handler(Arc<Captured>);
impl ActionHandler for Handler {
    fn do_action(&mut self, request: ActionRequest) {
        self.0.actions.lock().unwrap().push(request);
    }
}
impl TextEditHandler for Handler {
    fn edit_text(&mut self, request: TextEditRequest) -> bool {
        self.0.edits.lock().unwrap().push(request);
        true
    }
}
struct Callback(Arc<Captured>);
impl AdapterCallback for Callback {
    fn register_interfaces(&self, _: &Adapter, _: NodeId, _: InterfaceSet) {}
    fn unregister_interfaces(&self, _: &Adapter, _: NodeId, _: InterfaceSet) {}
    fn emit_event(&self, _: &Adapter, event: Event) {
        if let Event::Object {
            event: ObjectEvent::StateChanged(state, enabled),
            ..
        } = event
        {
            self.0.states.lock().unwrap().push((state, enabled));
        }
    }
}
fn parent(role: AkRole, children: &[u64]) -> Node {
    let mut node = Node::new(role);
    node.set_children(children.iter().copied().map(LocalId).collect::<Vec<_>>());
    node
}
fn initial(nodes: Vec<(LocalId, Node)>) -> TreeUpdate {
    TreeUpdate {
        nodes,
        tree: Some(Tree::new(LocalId(0))),
        tree_id: TreeId::ROOT,
        focus: LocalId(1),
    }
}
fn update(nodes: Vec<(LocalId, Node)>) -> TreeUpdate {
    TreeUpdate {
        nodes,
        tree: None,
        tree_id: TreeId::ROOT,
        focus: LocalId(1),
    }
}
fn build(initial: TreeUpdate, atomic: bool) -> (Adapter, Arc<Captured>) {
    let captured = Arc::new(Captured::default());
    let context = AppContext::new(None);
    let handler: Arc<dyn ActionHandlerNoMut + Send + Sync> = if atomic {
        Arc::new(TextEditHandlerWrapper::new(Handler(captured.clone())))
    } else {
        Arc::new(ActionHandlerWrapper::new(Handler(captured.clone())))
    };
    let adapter = Adapter::with_wrapped_action_handler(
        next_adapter_id(),
        &context,
        Callback(captured.clone()),
        initial,
        true,
        WindowBounds::default(),
        handler,
    );
    (adapter, captured)
}
fn child(adapter: &Adapter, parent: &PlatformNode, index: usize) -> PlatformNode {
    adapter.platform_node(parent.child_at_index(index).unwrap().unwrap())
}
fn project(expanded: bool) -> Node {
    let mut node = parent(AkRole::TreeItem, &[2]);
    node.set_expanded(expanded);
    node.add_action(AkAction::Click);
    node.add_action(if expanded {
        AkAction::Collapse
    } else {
        AkAction::Expand
    });
    node
}

#[test]
fn disclosure_actions_states_events_and_retained_guards_are_native_capability_based() {
    let mut document = Node::new(AkRole::TreeItem);
    document.add_action(AkAction::Click);
    document.add_action(AkAction::Focus);
    let (mut adapter, captured) = build(
        initial(vec![
            (LocalId(0), parent(AkRole::Window, &[1])),
            (LocalId(1), project(true)),
            (LocalId(2), document),
        ]),
        false,
    );
    let root = adapter.platform_node(adapter.root_id());
    let branch = child(&adapter, &root, 0);
    let document = child(&adapter, &branch, 0);
    assert!(document.state().contains(State::Visible));
    assert!(
        !document.state().contains(State::Showing),
        "offscreen logical rows do not invent geometry"
    );
    assert_eq!(branch.n_actions().unwrap(), 2);
    assert_eq!(branch.action_name(0).unwrap(), "click");
    assert_eq!(branch.action_name(1).unwrap(), "collapse");
    assert!(branch.state().contains(State::Expandable | State::Expanded));
    assert!(branch.do_action(1).unwrap());
    assert_eq!(
        captured.actions.lock().unwrap()[0].action,
        AkAction::Collapse
    );
    adapter.update(update(vec![(LocalId(1), project(false))]));
    assert!(
        branch
            .state()
            .contains(State::Expandable | State::Collapsed)
    );
    assert!(!branch.state().contains(State::Expanded));
    assert_eq!(branch.action_name(1).unwrap(), "expand");
    assert!(!document.do_action(0).unwrap());
    assert!(!document.grab_focus().unwrap());
    assert!(branch.do_action(1).unwrap());
    adapter.update(update(vec![(LocalId(1), project(true))]));
    assert!(document.do_action(0).unwrap());
    assert!(document == child(&adapter, &branch, 0));
    let mut disabled = project(true);
    disabled.set_disabled();
    adapter.update(update(vec![(LocalId(1), disabled)]));
    assert!(!branch.state().contains(State::Enabled | State::Sensitive));
    assert_eq!(branch.n_actions().unwrap(), 0);
    assert!(!branch.do_action(0).unwrap());
    let mut hidden = project(true);
    hidden.set_hidden();
    adapter.update(update(vec![(LocalId(1), hidden)]));
    assert!(!branch.do_action(0).unwrap());
    assert!(!document.do_action(0).unwrap());
    assert!(
        captured
            .states
            .lock()
            .unwrap()
            .contains(&(State::Collapsed, true))
    );
    assert!(
        captured
            .states
            .lock()
            .unwrap()
            .contains(&(State::Expanded, false))
    );
    let mut removed = update(vec![(LocalId(0), parent(AkRole::Window, &[]))]);
    removed.focus = LocalId(0);
    adapter.update(removed);
    assert!(matches!(document.do_action(0), Err(Error::Defunct)));
    drop(adapter);
    assert!(matches!(branch.do_action(0), Err(Error::Defunct)));
}

fn editor(read_only: bool) -> Node {
    let mut node = parent(AkRole::MultilineTextInput, &[2]);
    node.add_action(AkAction::SetTextSelection);
    node.add_action(AkAction::ScrollIntoView);
    node.add_action(AkAction::SetValue);
    node.add_action(AkAction::ReplaceSelectedText);
    if read_only {
        node.set_read_only();
    }
    node
}
fn text_initial() -> TreeUpdate {
    let mut run = Node::new(AkRole::TextRun);
    run.set_value("A日本🙂\nZ");
    run.set_character_lengths(vec![1, 3, 3, 4, 1, 1]);
    initial(vec![
        (LocalId(0), parent(AkRole::Window, &[1])),
        (LocalId(1), editor(false)),
        (LocalId(2), parent(AkRole::GenericContainer, &[3])),
        (LocalId(3), run),
    ])
}

#[test]
fn editable_text_uses_scalar_offsets_byte_lengths_and_one_immutable_origin_request() {
    let (mut adapter, captured) = build(text_initial(), true);
    let root = adapter.platform_node(adapter.root_id());
    let text = child(&adapter, &root, 0);
    assert!(
        text.interfaces()
            .unwrap()
            .contains(atspi_common::Interface::EditableText)
    );
    assert_eq!(text.text(0, -1).unwrap(), "A日本🙂\nZ");
    assert_eq!(text.character_count().unwrap(), 6);
    assert!(text.insert_text(1, "日本🙂", 6).unwrap());
    assert!(text.delete_text(1, 3).unwrap());
    assert!(!text.insert_text(1, "日本", 2).unwrap());
    assert!(!text.insert_text(-1, "X", 1).unwrap());
    assert!(!text.delete_text(4, 3).unwrap());
    assert!(!text.delete_text(0, 100).unwrap());
    let edits = captured.edits.lock().unwrap();
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].target_node, LocalId(1));
    assert_eq!(edits[0].selection.anchor.node, LocalId(3));
    assert_eq!(edits[0].selection.anchor.character_index, 1);
    assert_eq!(edits[1].selection.focus.character_index, 3);
    assert!(matches!(&edits[0].operation, TextEditOperation::Replace(value) if value == "日本"));
    drop(edits);
    assert!(
        captured.actions.lock().unwrap().is_empty(),
        "partial edit never queues a preceding selection"
    );
    assert!(text.set_text_contents("全体🙂").unwrap());
    assert_eq!(
        captured.actions.lock().unwrap()[0].action,
        AkAction::SetValue
    );
    adapter.update(update(vec![(LocalId(1), editor(true))]));
    assert!(!text.insert_text(1, "X", 1).unwrap());
    assert!(!text.set_text_contents("X").unwrap());
    assert!(text.copy_text(1, 3).unwrap());
    assert!(!text.cut_text(1, 3).unwrap());
    assert!(!text.paste_text(1).unwrap());
    assert!(
        text.set_caret_offset(1).unwrap(),
        "read-only selection is allowed"
    );
    let mut disabled = editor(false);
    disabled.set_disabled();
    adapter.update(update(vec![(LocalId(1), disabled)]));
    assert!(!text.copy_text(1, 3).unwrap());
    assert!(!text.set_caret_offset(1).unwrap());
    assert!(!text.set_selection(0, 1, 3).unwrap());
    drop(adapter);
    assert!(matches!(text.delete_text(1, 3), Err(Error::Defunct)));
}

#[test]
fn ordinary_adapter_preserves_unsupported_atomic_edit_behavior() {
    let (adapter, captured) = build(text_initial(), false);
    let root = adapter.platform_node(adapter.root_id());
    let text = child(&adapter, &root, 0);
    assert!(matches!(
        text.insert_text(1, "X", 1),
        Err(Error::UnsupportedOperation)
    ));
    assert!(matches!(
        text.copy_text(1, 3),
        Err(Error::UnsupportedOperation)
    ));
    assert!(captured.edits.lock().unwrap().is_empty());
    assert!(text.set_text_contents("full value").unwrap());
}

#[test]
fn scalar_substrings_are_exact_and_atomic_mutations_never_round_requested_offsets() {
    let mut update = text_initial();
    let run = &mut update.nodes.last_mut().unwrap().1;
    run.set_value("A👩🏽‍💻e\u{301}\r\nZ");
    run.set_character_lengths(vec![1, 15, 3, 2, 1]);
    let (adapter, captured) = build(update, true);
    let text = child(&adapter, &adapter.platform_node(adapter.root_id()), 0);
    assert_eq!(text.character_count().unwrap(), 10);
    assert_eq!(text.text(2, 3).unwrap(), "🏽");
    assert_eq!(text.text(5, 6).unwrap(), "e");
    assert_eq!(text.text(7, 8).unwrap(), "\r");
    assert_eq!(text.text(8, 9).unwrap(), "\n");
    assert!(!text.set_caret_offset(2).unwrap());
    assert!(!text.set_selection(0, 5, 6).unwrap());
    assert!(!text.insert_text(2, "X", 1).unwrap());
    assert!(!text.delete_text(5, 6).unwrap());
    assert!(!text.copy_text(5, 6).unwrap());
    assert!(!text.set_caret_offset(8).unwrap());
    assert!(captured.actions.lock().unwrap().is_empty());
    assert!(captured.edits.lock().unwrap().is_empty());
    assert!(text.copy_text(5, 7).unwrap());
    assert_eq!(captured.edits.lock().unwrap().len(), 1);
}

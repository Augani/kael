// Licensed under the Apache License, Version 2.0 or MIT, matching the adapter.
#![allow(dead_code)]

#[path = "../../src/context.rs"]
pub(crate) mod context;
#[path = "../../src/filters.rs"]
pub(crate) mod filters;
#[path = "../../src/node.rs"]
pub(crate) mod node;
#[path = "../../src/text_edit.rs"]
pub(crate) mod text_edit;
#[path = "../../src/util.rs"]
pub(crate) mod util;

use accesskit::{Action, ActionRequest, Node, NodeId, Role, TreeId, TreeUpdate};
use accesskit_consumer::{Tree, TreeChangeHandler};
use context::{ActionHandlerNoMut, Context};
use objc2::{
    msg_send, msg_send_id,
    rc::{Id, WeakId},
};
use objc2_app_kit::{NSApplication, NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{MainThreadMarker, NSArray, NSInteger, NSRect, NSString};
use std::{cell::RefCell, rc::Rc, time::Instant};

struct Actions(Rc<RefCell<Vec<ActionRequest>>>);
impl ActionHandlerNoMut for Actions {
    fn do_action(&self, request: ActionRequest) {
        self.0.borrow_mut().push(request);
    }
}
struct AtomicActions(
    Rc<RefCell<Vec<ActionRequest>>>,
    Rc<RefCell<Vec<text_edit::TextEditRequest>>>,
);
impl ActionHandlerNoMut for AtomicActions {
    fn do_action(&self, request: ActionRequest) {
        self.0.borrow_mut().push(request);
    }
    fn supports_text_edits(&self) -> bool {
        true
    }
    fn edit_text(&self, request: text_edit::TextEditRequest) -> bool {
        self.1.borrow_mut().push(request);
        true
    }
}
struct Changes;
impl TreeChangeHandler for Changes {
    fn node_added(&mut self, _: &accesskit_consumer::Node) {}
    fn node_updated(&mut self, _: &accesskit_consumer::Node, _: &accesskit_consumer::Node) {}
    fn focus_moved(
        &mut self,
        _: Option<&accesskit_consumer::Node>,
        _: Option<&accesskit_consumer::Node>,
    ) {
    }
    fn node_removed(&mut self, _: &accesskit_consumer::Node) {}
}

fn model(collapsed: bool) -> TreeUpdate {
    let mut root = Node::new(Role::Tree);
    root.set_children(
        (0..25)
            .map(|project| NodeId(1 + project * 4_001))
            .collect::<Vec<_>>(),
    );
    root.set_active_descendant(NodeId(100_025));
    root.add_action(Action::Focus);
    let mut nodes = vec![(NodeId(0), root)];
    for project in 0..25 {
        let id = 1 + project * 4_001;
        let mut parent = Node::new(Role::TreeItem);
        parent.set_label(format!("Project {:02}", project + 1));
        parent.set_level(1);
        parent.set_expanded(!collapsed || project != 0);
        parent.add_action(Action::Expand);
        parent.add_action(Action::Collapse);
        parent.add_action(Action::Click);
        parent.add_action(Action::Focus);
        parent.set_children(
            (1..=4_000)
                .map(|offset| NodeId(id + offset))
                .collect::<Vec<_>>(),
        );
        nodes.push((NodeId(id), parent));
        for offset in 1..=4_000 {
            let mut row = Node::new(Role::TreeItem);
            row.set_label(format!("document_{offset:04}.rs"));
            row.set_level(2);
            row.add_action(Action::Click);
            row.add_action(Action::Focus);
            row.add_action(Action::ScrollIntoView);
            nodes.push((NodeId(id + offset), row));
        }
    }
    TreeUpdate {
        nodes,
        tree: Some(accesskit::Tree::new(NodeId(0))),
        tree_id: TreeId::ROOT,
        focus: NodeId(0),
    }
}

pub fn run() {
    let marker = MainThreadMarker::new().expect("AppKit protocol test runs on process main thread");
    // A valid main-thread marker satisfies AppKit's allocation requirement.
    let view = unsafe { NSView::new(marker) };
    let actions = Rc::new(RefCell::new(Vec::new()));
    let context = Context::new(
        WeakId::from_id(&view),
        Tree::new(model(false), true),
        Rc::new(Actions(actions.clone())),
        marker,
    );
    let native = |local| {
        context.get_or_create_platform_node(
            context
                .tree
                .borrow()
                .state()
                .node_by_tree_local_id(NodeId(local), TreeId::ROOT)
                .unwrap()
                .id(),
        )
    };
    let root = native(0);
    let project = native(1);
    let first_document = native(2);
    let final_project = native(96_025);
    let last = native(100_025);

    let started = Instant::now();
    let rows: Id<NSArray<node::PlatformNode>> = unsafe { msg_send_id![&*root, accessibilityRows] };
    assert_eq!(rows.len(), 100_025);
    assert_eq!(
        Id::as_ptr(&unsafe { rows.lastObject() }.unwrap()),
        Id::as_ptr(&last)
    );
    let enumeration = started.elapsed();
    let title: Id<NSString> = unsafe { msg_send_id![&*last, accessibilityTitle] };
    assert_eq!(title.to_string(), "document_4000.rs");
    let level: NSInteger = unsafe { msg_send![&*last, accessibilityDisclosureLevel] };
    assert_eq!(level, 1);
    let parent: Id<node::PlatformNode> =
        unsafe { msg_send_id![&*last, accessibilityDisclosedByRow] };
    assert_eq!(Id::as_ptr(&parent), Id::as_ptr(&final_project));
    let disclosed: Id<NSArray<node::PlatformNode>> =
        unsafe { msg_send_id![&*final_project, accessibilityDisclosedRows] };
    assert_eq!(disclosed.len(), 4_000);
    assert_eq!(
        Id::as_ptr(&unsafe { disclosed.lastObject() }.unwrap()),
        Id::as_ptr(&last)
    );
    let focused: bool = unsafe { msg_send![&*last, isAccessibilityFocused] };
    assert!(
        focused,
        "offscreen active descendant is the native focused element"
    );
    let frame: NSRect = unsafe { msg_send![&*last, accessibilityFrame] };
    assert_eq!(
        frame,
        NSRect::ZERO,
        "offscreen semantics do not fabricate geometry"
    );

    let pressed: bool = unsafe { msg_send![&*last, accessibilityPerformPress] };
    assert!(pressed);
    unsafe {
        let _: () = msg_send![&*last, setAccessibilityFocused: true];
    }
    let scroll = NSString::from_str("AXScrollToVisible");
    unsafe {
        let _: () = msg_send![&*last, accessibilityPerformAction: &*scroll];
    }
    unsafe {
        let _: () = msg_send![&*project, setAccessibilityDisclosed: false];
    }
    assert_eq!(
        actions
            .borrow()
            .iter()
            .map(|request| (request.action, request.target_node))
            .collect::<Vec<_>>(),
        [
            (Action::Click, NodeId(100_025)),
            (Action::Focus, NodeId(100_025)),
            (Action::ScrollIntoView, NodeId(100_025)),
            (Action::Collapse, NodeId(1))
        ]
    );
    assert!(
        actions
            .borrow()
            .iter()
            .all(|request| request.target_tree == TreeId::ROOT)
    );
    actions.borrow_mut().clear();

    context
        .tree
        .borrow_mut()
        .update_and_process_changes(model(true), &mut Changes);
    let closed_rows: Id<NSArray<node::PlatformNode>> =
        unsafe { msg_send_id![&*root, accessibilityRows] };
    assert_eq!(closed_rows.len(), 96_025);
    let closed_children: Id<NSArray<node::PlatformNode>> =
        unsafe { msg_send_id![&*project, accessibilityDisclosedRows] };
    assert!(closed_children.is_empty());
    let hidden_children: Id<NSArray<node::PlatformNode>> =
        unsafe { msg_send_id![&*project, accessibilityChildren] };
    assert!(hidden_children.is_empty());
    let hidden_pressed: bool = unsafe { msg_send![&*first_document, accessibilityPerformPress] };
    assert!(
        !hidden_pressed,
        "retained native children cannot dispatch through a collapsed row"
    );
    unsafe {
        let _: () = msg_send![&*project, setAccessibilityDisclosed: true];
    }
    assert_eq!(actions.borrow().last().unwrap().action, Action::Expand);
    context
        .tree
        .borrow_mut()
        .update_and_process_changes(model(false), &mut Changes);
    assert_eq!(
        Id::as_ptr(&native(100_025)),
        Id::as_ptr(&last),
        "surviving semantic rows keep native object identity"
    );

    actions.borrow_mut().clear();
    drop(context);
    let stale_pressed: bool = unsafe { msg_send![&*last, accessibilityPerformPress] };
    assert!(
        !stale_pressed,
        "released adapter cannot dispatch stale actions"
    );
    assert!(actions.borrow().is_empty());
    println!(
        "native NSAccessibility: 100025 rows, 4000 disclosed children, offscreen focus/click/scroll, collapse/expand, stable objects and released lifecycle passed; full row getter {:?}",
        enumeration
    );
}

pub fn run_text_selection() {
    use accesskit::{ActionData, TextPosition, TextSelection};
    use objc2::sel;
    use objc2_foundation::NSRange;
    let marker =
        MainThreadMarker::new().expect("AppKit text-range test runs on process main thread");
    let _application = NSApplication::sharedApplication(marker);
    let view = unsafe { NSView::new(marker) };
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            marker.alloc(),
            NSRect::new(
                objc2_foundation::NSPoint::new(50.0, 50.0),
                objc2_foundation::NSSize::new(200.0, 100.0),
            ),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::NSBackingStoreBuffered,
            false,
        )
    };
    window.setContentView(Some(&view));
    let actions = Rc::new(RefCell::new(Vec::new()));
    let edits = Rc::new(RefCell::new(Vec::new()));
    let first = "A🙂e\u{301}\r\n";
    let second = "第二行";
    let mut input = Node::new(Role::MultilineTextInput);
    input.set_children(vec![NodeId(20)]);
    input.add_action(Action::Focus);
    input.add_action(Action::SetTextSelection);
    input.add_action(Action::SetValue);
    input.add_action(Action::ReplaceSelectedText);
    input.set_custom_actions(vec![
        accesskit::CustomAction {
            id: i32::MIN,
            description: "Copy text".into(),
        },
        accesskit::CustomAction {
            id: i32::MIN + 1,
            description: "Cut text".into(),
        },
        accesskit::CustomAction {
            id: i32::MIN + 2,
            description: "Paste text".into(),
        },
    ]);
    input.add_action(Action::ScrollIntoView);
    input.set_bounds(accesskit::Rect::new(0.0, 0.0, 80.0, 30.0));
    input.set_text_selection(TextSelection {
        anchor: TextPosition {
            node: NodeId(12),
            character_index: 3,
        },
        focus: TextPosition {
            node: NodeId(11),
            character_index: 1,
        },
    });
    let mut container = Node::new(Role::GenericContainer);
    container.set_children(vec![NodeId(11), NodeId(12)]);
    let mut first_run = Node::new(Role::TextRun);
    first_run.set_value(first);
    first_run.set_character_lengths(vec![1, 4, 1, 2, 2]);
    first_run.set_bounds(accesskit::Rect::new(10.0, 5.0, 100.0, 15.0));
    first_run.set_text_direction(accesskit::TextDirection::LeftToRight);
    first_run.set_character_positions(vec![0.0, 10.0, 30.0, 40.0, 40.0]);
    first_run.set_character_widths(vec![10.0, 20.0, 10.0, 0.0, 0.0]);
    let mut second_run = Node::new(Role::TextRun);
    second_run.set_value(second);
    second_run.set_character_lengths(vec![3, 3, 3]);
    let tree = Tree::new(
        TreeUpdate {
            nodes: vec![
                (NodeId(10), input),
                (NodeId(20), container),
                (NodeId(11), first_run),
                (NodeId(12), second_run),
            ],
            tree: Some(accesskit::Tree::new(NodeId(10))),
            tree_id: TreeId::ROOT,
            focus: NodeId(10),
        },
        true,
    );
    let context = Context::new(
        WeakId::from_id(&view),
        tree,
        Rc::new(AtomicActions(actions.clone(), edits.clone())),
        marker,
    );
    let input = context.get_or_create_platform_node(
        context
            .tree
            .borrow()
            .state()
            .node_by_tree_local_id(NodeId(10), TreeId::ROOT)
            .unwrap()
            .id(),
    );
    let settable: bool = unsafe {
        msg_send![&*input, isAccessibilitySelectorAllowed: sel!(setAccessibilitySelectedTextRange:)]
    };
    assert!(settable, "prepared native text selection must be settable");
    let count: NSInteger = unsafe { msg_send![&*input, accessibilityNumberOfCharacters] };
    assert_eq!(
        count, 10,
        "native character count uses UTF-16, including CRLF"
    );
    let range: NSRange = unsafe { msg_send![&*input, accessibilitySelectedTextRange] };
    assert_eq!((range.location, range.length), (1, 9));
    let selected: Option<Id<NSString>> =
        unsafe { msg_send_id![&*input, accessibilitySelectedText] };
    assert_eq!(
        selected.unwrap().to_string(),
        format!("🙂e\u{301}\r\n{second}")
    );
    let value: Option<Id<NSString>> = unsafe { msg_send_id![&*input, accessibilityValue] };
    assert_eq!(
        value.unwrap().to_string(),
        format!("{first}{second}"),
        "native full value comes from retained runs"
    );
    let oversized = NSString::from_str(&"界".repeat(6 * 1024 * 1024));
    assert!(oversized.len_utf16() < 16 * 1024 * 1024);
    let _: () = unsafe { msg_send![&*input, setAccessibilitySelectedText: &*oversized] };
    let _: () = unsafe { msg_send![&*input, setAccessibilityValue: &*oversized] };
    assert!(
        edits.borrow().is_empty(),
        "UTF-8 bytes bound partial edits before copying"
    );
    assert!(
        actions.borrow().is_empty(),
        "UTF-8 bytes bound whole-value edits before copying"
    );
    drop(oversized);
    let small = NSString::from_str("whole 日本🙂");
    let _: () = unsafe { msg_send![&*input, setAccessibilityValue: &*small] };
    let value_action = actions.borrow_mut().pop().unwrap();
    assert_eq!(value_action.action, Action::SetValue);
    assert_eq!(
        value_action.data,
        Some(ActionData::Value("whole 日本🙂".into()))
    );
    for (range, expected) in [
        (NSRange::new(3, 1), "e"),
        (NSRange::new(4, 1), "\u{301}"),
        (NSRange::new(5, 1), "\r"),
        (NSRange::new(6, 1), "\n"),
        (NSRange::new(10, 0), ""),
    ] {
        let text: Option<Id<NSString>> =
            unsafe { msg_send_id![&*input, accessibilityStringForRange: range] };
        assert_eq!(text.unwrap().to_string(), expected, "exact UTF-16 read");
        let styled: Option<Id<objc2_foundation::NSAttributedString>> =
            unsafe { msg_send_id![&*input, accessibilityAttributedStringForRange: range] };
        assert_eq!(
            styled.unwrap().string().to_string(),
            expected,
            "exact attributed UTF-16 read"
        );
    }
    let visible: NSRange = unsafe { msg_send![&*input, accessibilityVisibleCharacterRange] };
    assert_eq!(
        (visible.location, visible.length),
        (0, 7),
        "mounted glyphs and zero-advance line endings retain their real visible positions"
    );
    let frame: NSRect =
        unsafe { msg_send![&*input, accessibilityFrameForRange: NSRange::new(1, 2)] };
    let factor = window.backingScaleFactor();
    assert_eq!(frame.size.width, 20.0 / factor);
    assert_eq!(frame.size.height, 10.0 / factor);
    let point = objc2_foundation::NSPoint::new(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    let hit: NSRange = unsafe { msg_send![&*input, accessibilityRangeForPosition: point] };
    assert_eq!(
        (hit.location, hit.length),
        (1, 2),
        "actual screen glyph hit uses UTF-16"
    );
    let absent: NSRect =
        unsafe { msg_send![&*input, accessibilityFrameForRange: NSRange::new(8, 1)] };
    assert_eq!(
        absent,
        NSRect::ZERO,
        "unmounted runs do not fabricate geometry"
    );
    let _: () =
        unsafe { msg_send![&*input, setAccessibilityVisibleCharacterRange: NSRange::new(8, 1)] };
    let reveal = actions.borrow_mut().pop().unwrap();
    assert_eq!(reveal.action, Action::ScrollIntoView);
    assert_eq!(
        reveal.target_node,
        NodeId(12),
        "reveal retains originating run identity"
    );
    assert_eq!(
        reveal.data,
        Some(ActionData::ScrollHint(accesskit::ScrollHint::TopEdge))
    );
    let unchanged: NSRange = unsafe { msg_send![&*input, accessibilitySelectedTextRange] };
    assert_eq!(
        (unchanged.location, unchanged.length),
        (1, 9),
        "reveal does not mutate selection"
    );
    let invalid: Option<Id<NSString>> =
        unsafe { msg_send_id![&*input, accessibilityStringForRange: NSRange::new(usize::MAX, 10)] };
    assert!(invalid.is_none(), "overflowing native ranges are rejected");
    let _: () = unsafe {
        msg_send![&*input, setAccessibilitySelectedTextRange: NSRange::new(usize::MAX, 10)]
    };
    assert!(actions.borrow().is_empty());
    let _: () =
        unsafe { msg_send![&*input, setAccessibilitySelectedTextRange: NSRange::new(1, 1)] };
    assert!(
        actions.borrow().is_empty(),
        "a native selection cannot split a surrogate/atomic character"
    );
    let _: () =
        unsafe { msg_send![&*input, setAccessibilitySelectedTextRange: NSRange::new(1, 2)] };
    let _: () =
        unsafe { msg_send![&*input, setAccessibilitySelectedTextRange: NSRange::new(8, 1)] };
    let requests = actions.borrow_mut().drain(..).collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.action, Action::SetTextSelection);
        assert_eq!(request.target_node, NodeId(10));
        assert_eq!(request.target_tree, TreeId::ROOT);
    }
    assert_eq!(
        requests[0].data,
        Some(ActionData::SetTextSelection(TextSelection {
            anchor: TextPosition {
                node: NodeId(11),
                character_index: 1
            },
            focus: TextPosition {
                node: NodeId(11),
                character_index: 2
            },
        }))
    );
    assert_eq!(
        requests[1].data,
        Some(ActionData::SetTextSelection(TextSelection {
            anchor: TextPosition {
                node: NodeId(12),
                character_index: 1
            },
            focus: TextPosition {
                node: NodeId(12),
                character_index: 2
            },
        }))
    );
    let partial_settable: bool = unsafe {
        msg_send![&*input, isAccessibilitySelectorAllowed: sel!(setAccessibilitySelectedText:)]
    };
    assert!(partial_settable);
    let _: () = unsafe {
        msg_send![&*input, setAccessibilitySelectedText: &*NSString::from_str("新🙂")]
    };
    let names: Id<NSArray<NSString>> = unsafe { msg_send_id![&*input, accessibilityActionNames] };
    for name in ["Copy text", "Cut text", "Paste text"] {
        assert!(
            (0..names.len()).any(|index| unsafe { names.objectAtIndex(index) }.to_string() == name)
        );
        let _: () =
            unsafe { msg_send![&*input, accessibilityPerformAction: &*NSString::from_str(name)] };
    }
    let captured = edits.borrow_mut().drain(..).collect::<Vec<_>>();
    assert_eq!(captured.len(), 4);
    assert!(
        matches!(&captured[0].operation, text_edit::TextEditOperation::Replace(value) if value == "新🙂")
    );
    assert!(matches!(
        captured[1].operation,
        text_edit::TextEditOperation::Copy
    ));
    assert!(matches!(
        captured[2].operation,
        text_edit::TextEditOperation::Cut
    ));
    assert!(matches!(
        captured[3].operation,
        text_edit::TextEditOperation::Paste
    ));
    for request in captured {
        assert_eq!(request.target_node, NodeId(10));
        assert_eq!(request.selection.anchor.node, NodeId(12));
        assert_eq!(request.selection.focus.node, NodeId(11));
        assert_eq!(request.selection.anchor.character_index, 3);
        assert_eq!(request.selection.focus.character_index, 1);
    }
    assert!(
        actions.borrow().is_empty(),
        "atomic edit never queues a preceding selection"
    );
    let mut read_only = context.tree.borrow().state().root().data().clone();
    read_only.set_read_only();
    context.tree.borrow_mut().update_and_process_changes(
        TreeUpdate {
            nodes: vec![(NodeId(10), read_only)],
            tree: None,
            tree_id: TreeId::ROOT,
            focus: NodeId(10),
        },
        &mut Changes,
    );
    let partial_settable: bool = unsafe {
        msg_send![&*input, isAccessibilitySelectorAllowed: sel!(setAccessibilitySelectedText:)]
    };
    assert!(
        !partial_settable,
        "read-only selected text cannot be replaced"
    );
    let _: () = unsafe {
        msg_send![&*input, setAccessibilitySelectedText: &*NSString::from_str("invalid")]
    };
    for name in ["Copy text", "Cut text", "Paste text"] {
        let _: () =
            unsafe { msg_send![&*input, accessibilityPerformAction: &*NSString::from_str(name)] };
    }
    assert_eq!(
        edits.borrow().len(),
        1,
        "read-only native text only permits Copy"
    );
    assert!(matches!(
        edits.borrow_mut().pop().unwrap().operation,
        text_edit::TextEditOperation::Copy
    ));
    let mut disabled = Node::new(Role::MultilineTextInput);
    disabled.set_children(vec![NodeId(20)]);
    disabled.set_disabled();
    disabled.add_action(Action::SetTextSelection);
    context.tree.borrow_mut().update_and_process_changes(
        TreeUpdate {
            nodes: vec![(NodeId(10), disabled)],
            tree: None,
            tree_id: TreeId::ROOT,
            focus: NodeId(10),
        },
        &mut Changes,
    );
    let settable: bool = unsafe {
        msg_send![&*input, isAccessibilitySelectorAllowed: sel!(setAccessibilitySelectedTextRange:)]
    };
    assert!(!settable);
    let _: () =
        unsafe { msg_send![&*input, setAccessibilitySelectedTextRange: NSRange::new(0, 1)] };
    assert!(
        actions.borrow().is_empty(),
        "disabled text cannot dispatch a selection setter"
    );
    let _: () = unsafe {
        msg_send![&*input, setAccessibilitySelectedText: &*NSString::from_str("invalid")]
    };
    let _: () = unsafe {
        msg_send![&*input, accessibilityPerformAction: &*NSString::from_str("Copy text")]
    };
    assert!(
        edits.borrow().is_empty(),
        "disabled partial edits and clipboard requests are inert"
    );
    let mut hidden = Node::new(Role::MultilineTextInput);
    hidden.set_children(vec![NodeId(20)]);
    hidden.set_hidden();
    hidden.add_action(Action::SetTextSelection);
    hidden.add_action(Action::ReplaceSelectedText);
    context.tree.borrow_mut().update_and_process_changes(
        TreeUpdate {
            nodes: vec![(NodeId(10), hidden)],
            tree: None,
            tree_id: TreeId::ROOT,
            focus: NodeId(10),
        },
        &mut Changes,
    );
    let _: () =
        unsafe { msg_send![&*input, setAccessibilitySelectedTextRange: NSRange::new(0, 1)] };
    let _: () = unsafe {
        msg_send![&*input, setAccessibilitySelectedText: &*NSString::from_str("invalid")]
    };
    assert!(
        actions.borrow().is_empty() && edits.borrow().is_empty(),
        "a focused hidden control cannot dispatch native actions"
    );
    let mut unsupported = Node::new(Role::MultilineTextInput);
    unsupported.set_children(vec![NodeId(20)]);
    context.tree.borrow_mut().update_and_process_changes(
        TreeUpdate {
            nodes: vec![(NodeId(10), unsupported)],
            tree: None,
            tree_id: TreeId::ROOT,
            focus: NodeId(10),
        },
        &mut Changes,
    );
    let settable: bool = unsafe {
        msg_send![&*input, isAccessibilitySelectorAllowed: sel!(setAccessibilitySelectedTextRange:)]
    };
    assert!(!settable);
    let _: () =
        unsafe { msg_send![&*input, setAccessibilitySelectedTextRange: NSRange::new(0, 1)] };
    assert!(
        actions.borrow().is_empty(),
        "unadvertised selection setters are inert"
    );
    window.setContentView(None);
    let detached: NSRect =
        unsafe { msg_send![&*input, accessibilityFrameForRange: NSRange::new(1, 2)] };
    assert_eq!(
        detached,
        NSRect::ZERO,
        "detached views safely reject screen geometry"
    );
    let detached_hit: NSRange = unsafe { msg_send![&*input, accessibilityRangeForPosition: point] };
    assert_eq!((detached_hit.location, detached_hit.length), (0, 0));
    let mut legacy_input = Node::new(Role::MultilineTextInput);
    legacy_input.set_children(vec![NodeId(11)]);
    legacy_input.add_action(Action::ReplaceSelectedText);
    legacy_input.set_text_selection(TextSelection {
        anchor: TextPosition {
            node: NodeId(11),
            character_index: 0,
        },
        focus: TextPosition {
            node: NodeId(11),
            character_index: 1,
        },
    });
    let mut legacy_run = Node::new(Role::TextRun);
    legacy_run.set_value("A");
    legacy_run.set_character_lengths(vec![1]);
    let legacy_context = Context::new(
        WeakId::from_id(&view),
        Tree::new(
            TreeUpdate {
                nodes: vec![(NodeId(10), legacy_input), (NodeId(11), legacy_run)],
                tree: Some(accesskit::Tree::new(NodeId(10))),
                tree_id: TreeId::ROOT,
                focus: NodeId(10),
            },
            true,
        ),
        Rc::new(Actions(actions.clone())),
        marker,
    );
    let legacy = legacy_context
        .get_or_create_platform_node(legacy_context.tree.borrow().state().root().id());
    let settable: bool = unsafe {
        msg_send![&*legacy, isAccessibilitySelectorAllowed: sel!(setAccessibilitySelectedText:)]
    };
    assert!(
        !settable,
        "ordinary adapters do not claim unsupported atomic edits"
    );
    let _: () = unsafe {
        msg_send![&*legacy, setAccessibilitySelectedText: &*NSString::from_str("invalid")]
    };
    assert!(actions.borrow().is_empty());
    drop(context);
    let _: () =
        unsafe { msg_send![&*input, setAccessibilitySelectedTextRange: NSRange::new(0, 1)] };
    assert!(
        actions.borrow().is_empty(),
        "released native text object cannot dispatch"
    );
    let _: () = unsafe {
        msg_send![&*input, setAccessibilitySelectedText: &*NSString::from_str("invalid")]
    };
    assert!(edits.borrow().is_empty());
    println!(
        "native NSAccessibility text selection: full multiline value, UTF-16 range/count, reversed selection, Unicode setter actions, visible glyph geometry/hit tests, immutable reveal/partial edits/clipboard, read-only/disabled and overflow/detached guards and released lifecycle passed"
    );
}

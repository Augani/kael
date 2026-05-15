use crate::{
    AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AnyElement, App, Component, Div, ElementId, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use std::rc::Rc;
use util::ResultExt;

type PrimitiveDiv = crate::elements::div::Stateful<Div>;
type ClickListener = Rc<dyn Fn(&crate::ClickEvent, &mut Window, &mut App)>;

/// Construct a semantic toolbar container.
#[track_caller]
pub fn toolbar(id: impl Into<ElementId>) -> PrimitiveDiv {
    div()
        .id(id)
        .accessibility(AccessibilityAttributes::new(AccessibilityRole::Toolbar))
}

/// Construct a semantic pane container.
#[track_caller]
pub fn pane(id: impl Into<ElementId>) -> PrimitiveDiv {
    div()
        .id(id)
        .accessibility(AccessibilityAttributes::new(AccessibilityRole::Pane))
}

/// Construct a semantic dialog container.
#[track_caller]
pub fn dialog(id: impl Into<ElementId>) -> PrimitiveDiv {
    div()
        .id(id)
        .accessibility(AccessibilityAttributes::new(AccessibilityRole::Dialog))
}

/// Construct a semantic alert container.
#[track_caller]
pub fn alert(id: impl Into<ElementId>) -> PrimitiveDiv {
    div()
        .id(id)
        .accessibility(AccessibilityAttributes::new(AccessibilityRole::Alert))
}

/// Construct a semantic menu container.
#[track_caller]
pub fn menu(id: impl Into<ElementId>) -> PrimitiveDiv {
    div()
        .id(id)
        .tab_group()
        .accessibility(AccessibilityAttributes::new(AccessibilityRole::Menu))
}

/// Construct a semantic menu item primitive.
#[track_caller]
pub fn menu_item(id: impl Into<ElementId>) -> MenuEntry {
    MenuEntry::new(id.into())
}

/// Construct a semantic hyperlink primitive.
#[track_caller]
pub fn link(id: impl Into<ElementId>) -> Link {
    Link::new(id.into())
}

/// Construct a semantic tree container.
#[track_caller]
pub fn tree(id: impl Into<ElementId>) -> PrimitiveDiv {
    div()
        .id(id)
        .tab_group()
        .accessibility(AccessibilityAttributes::new(AccessibilityRole::Tree))
}

/// Construct a semantic tree item primitive.
#[track_caller]
pub fn tree_item(id: impl Into<ElementId>) -> TreeItem {
    TreeItem::new(id.into())
}

/// Construct a semantic separator primitive.
#[track_caller]
pub fn separator(id: impl Into<ElementId>) -> PrimitiveDiv {
    div()
        .id(id)
        .h(px(1.0))
        .w_full()
        .bg(crate::rgb(0xe2e8f0))
        .accessibility(AccessibilityAttributes::new(AccessibilityRole::Separator))
}

/// A focusable menu item primitive with caller-owned visuals.
pub struct MenuEntry {
    element_id: ElementId,
    label: Option<SharedString>,
    disabled: bool,
    on_click: Option<ClickListener>,
    children: Vec<AnyElement>,
}

impl MenuEntry {
    fn new(element_id: ElementId) -> Self {
        Self {
            element_id,
            label: None,
            disabled: false,
            on_click: None,
            children: Vec::new(),
        }
    }

    /// Set the visible label for the menu item.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Disable the menu item.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Register a callback invoked when the item is activated by mouse or keyboard.
    pub fn on_click(
        mut self,
        listener: impl Fn(&crate::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(listener));
        self
    }
}

impl ParentElement for MenuEntry {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for MenuEntry {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let MenuEntry {
            element_id,
            label,
            disabled,
            on_click,
            children,
        } = self;

        let item_id = element_id.to_string();
        let focus_handle = semantic_focus_handle("menu-item", &item_id, window, cx);
        let content = semantic_children(children, label.clone(), SharedString::from("Menu item"));

        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::MenuItem)
            .states(semantic_focus_state(&focus_handle, disabled, window));
        if let Some(label) = label.as_ref() {
            accessibility = accessibility.label(label.to_string());
        }
        if !disabled {
            accessibility =
                accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        let mut root = div()
            .id(element_id)
            .track_focus(&focus_handle)
            .focusable()
            .tab_stop(!disabled)
            .cursor_pointer()
            .accessibility(accessibility)
            .focus_visible(|style: crate::StyleRefinement| style.bg(crate::rgba(0x1d4ed810)));

        if !disabled {
            if let Some(listener) = on_click {
                root = semantic_clickable(root, listener, &["enter", "space"]);
            }
        }

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector = format!("menu-item-{}", item_id);
            root = root.debug_selector(move || selector);
        }

        root.children(content)
    }
}

impl IntoElement for MenuEntry {
    type Element = Component<Self>;

    fn into_element(self) -> Self::Element {
        Component::new(self)
    }
}

/// A focusable hyperlink primitive with built-in external URL support.
pub struct Link {
    element_id: ElementId,
    label: Option<SharedString>,
    url: Option<SharedString>,
    disabled: bool,
    on_click: Option<ClickListener>,
    children: Vec<AnyElement>,
}

impl Link {
    fn new(element_id: ElementId) -> Self {
        Self {
            element_id,
            label: None,
            url: None,
            disabled: false,
            on_click: None,
            children: Vec::new(),
        }
    }

    /// Set the visible label for the link.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Open the given URL when the link is activated.
    pub fn url(mut self, url: impl Into<SharedString>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Disable the link.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Register a callback invoked when the link is activated by mouse or keyboard.
    pub fn on_click(
        mut self,
        listener: impl Fn(&crate::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(listener));
        self
    }
}

impl ParentElement for Link {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Link {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Link {
            element_id,
            label,
            url,
            disabled,
            on_click,
            children,
        } = self;

        let link_id = element_id.to_string();
        let focus_handle = semantic_focus_handle("link", &link_id, window, cx);
        let fallback = label.clone().or_else(|| url.clone());
        let content = semantic_children(children, fallback, SharedString::from("Link"));

        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Link)
            .states(semantic_focus_state(&focus_handle, disabled, window));
        if let Some(label) = label.as_ref().or(url.as_ref()) {
            accessibility = accessibility.label(label.to_string());
        }
        if !disabled {
            accessibility =
                accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        let mut root = div()
            .id(element_id)
            .track_focus(&focus_handle)
            .focusable()
            .tab_stop(!disabled)
            .cursor_pointer()
            .accessibility(accessibility)
            .focus_visible(|style: crate::StyleRefinement| style.bg(crate::rgba(0x1d4ed810)));

        if !disabled {
            let click_handler = match (on_click, url) {
                (Some(listener), _) => Some(listener),
                (None, Some(url)) => Some(semantic_open_url_listener(url)),
                (None, None) => None,
            };

            if let Some(listener) = click_handler {
                root = semantic_clickable(root, listener, &["enter"]);
            }
        }

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector = format!("link-{}", link_id);
            root = root.debug_selector(move || selector);
        }

        root.children(content)
    }
}

impl IntoElement for Link {
    type Element = Component<Self>;

    fn into_element(self) -> Self::Element {
        Component::new(self)
    }
}

/// A focusable tree item primitive with controlled selected and expanded state.
pub struct TreeItem {
    element_id: ElementId,
    label: Option<SharedString>,
    selected: bool,
    expanded: Option<bool>,
    disabled: bool,
    on_click: Option<ClickListener>,
    children: Vec<AnyElement>,
}

impl TreeItem {
    fn new(element_id: ElementId) -> Self {
        Self {
            element_id,
            label: None,
            selected: false,
            expanded: None,
            disabled: false,
            on_click: None,
            children: Vec::new(),
        }
    }

    /// Set the visible label for the tree item.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set whether the tree item is currently selected.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Set whether the tree item is expanded or collapsed.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    /// Disable the tree item.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Register a callback invoked when the item is activated by mouse or keyboard.
    pub fn on_click(
        mut self,
        listener: impl Fn(&crate::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(listener));
        self
    }
}

impl ParentElement for TreeItem {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for TreeItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let TreeItem {
            element_id,
            label,
            selected,
            expanded,
            disabled,
            on_click,
            children,
        } = self;

        let item_id = element_id.to_string();
        let focus_handle = semantic_focus_handle("tree-item", &item_id, window, cx);
        let content = semantic_children(children, label.clone(), SharedString::from("Tree item"));

        let mut states = semantic_focus_state(&focus_handle, disabled, window);
        if selected {
            states |= AccessibilityState::SELECTED;
        }
        if let Some(expanded) = expanded {
            states |= if expanded {
                AccessibilityState::EXPANDED
            } else {
                AccessibilityState::COLLAPSED
            };
        }

        let mut accessibility =
            AccessibilityAttributes::new(AccessibilityRole::TreeItem).states(states);
        if let Some(label) = label.as_ref() {
            accessibility = accessibility.label(label.to_string());
        }
        if !disabled {
            accessibility =
                accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        let mut root = div()
            .id(element_id)
            .track_focus(&focus_handle)
            .focusable()
            .tab_stop(!disabled)
            .cursor_pointer()
            .accessibility(accessibility)
            .focus_visible(|style: crate::StyleRefinement| style.bg(crate::rgba(0x1d4ed810)));

        if !disabled {
            if let Some(listener) = on_click {
                root = semantic_clickable(root, listener, &["enter", "space"]);
            }
        }

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector = format!("tree-item-{}", item_id);
            root = root.debug_selector(move || selector);
        }

        root.children(content)
    }
}

impl IntoElement for TreeItem {
    type Element = Component<Self>;

    fn into_element(self) -> Self::Element {
        Component::new(self)
    }
}

fn semantic_focus_handle(
    kind: &'static str,
    element_id: &str,
    window: &mut Window,
    cx: &mut App,
) -> crate::FocusHandle {
    window
        .use_keyed_state(
            ElementId::named_usize(format!("{}-{}-focus", kind, element_id), 0),
            cx,
            |_, cx| cx.focus_handle(),
        )
        .read(cx)
        .clone()
}

fn semantic_focus_state(
    focus_handle: &crate::FocusHandle,
    disabled: bool,
    window: &Window,
) -> AccessibilityState {
    if disabled {
        AccessibilityState::DISABLED
    } else if focus_handle.is_focused(window) {
        AccessibilityState::FOCUSED
    } else {
        AccessibilityState::NONE
    }
}

fn semantic_children(
    mut children: Vec<AnyElement>,
    label: Option<SharedString>,
    fallback: SharedString,
) -> Vec<AnyElement> {
    if children.is_empty() {
        children.push(div().child(label.unwrap_or(fallback)).into_any_element());
    }

    children
}

fn semantic_clickable(
    root: crate::elements::div::Stateful<Div>,
    listener: ClickListener,
    keys: &'static [&'static str],
) -> crate::elements::div::Stateful<Div> {
    let key_listener = listener.clone();
    root.on_click(move |event, window, cx| {
        listener(event, window, cx);
    })
    .on_key_down(move |event, window, cx| {
        if event.keystroke.modifiers.modified() || !keys.contains(&event.keystroke.key.as_str()) {
            return;
        }

        let button = match event.keystroke.key.as_str() {
            "enter" => crate::KeyboardButton::Enter,
            _ => crate::KeyboardButton::Space,
        };

        key_listener(
            &crate::ClickEvent::Keyboard(crate::KeyboardClickEvent {
                button,
                ..Default::default()
            }),
            window,
            cx,
        );
        window.prevent_default();
    })
}

fn semantic_open_url_listener(url: SharedString) -> ClickListener {
    Rc::new(move |_: &crate::ClickEvent, _: &mut Window, cx: &mut App| {
        cx.open_url(&url).log_err();
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AccessibilityRole, Context, Modifiers, ParentElement, Render, TestAppContext, div,
        security::Capability,
    };

    struct LinkView;

    struct MenuItemView {
        activations: usize,
    }

    struct TreeItemView {
        activations: usize,
    }

    struct SemanticPrimitivesView;

    impl Render for SemanticPrimitivesView {
        fn render(
            &mut self,
            _: &mut crate::Window,
            _: &mut Context<Self>,
        ) -> impl crate::IntoElement {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(toolbar("main_toolbar").child("Toolbar"))
                .child(pane("main_pane").child("Pane"))
                .child(dialog("confirm_dialog").child("Dialog"))
                .child(alert("network_alert").child("Alert"))
                .child(separator("rule"))
                .child(
                    menu("app_menu")
                        .child(menu_item("menu_item_copy").child("Copy"))
                        .child(menu_item("menu_item_paste").child("Paste")),
                )
                .child(link("docs_link").child("Docs"))
                .child(tree("project_tree").child(tree_item("tree_item_src").child("src")))
        }
    }

    impl Render for LinkView {
        fn render(
            &mut self,
            _: &mut crate::Window,
            _: &mut Context<Self>,
        ) -> impl crate::IntoElement {
            link("docs").label("Docs").url("https://example.com/docs")
        }
    }

    impl Render for MenuItemView {
        fn render(
            &mut self,
            _: &mut crate::Window,
            cx: &mut Context<Self>,
        ) -> impl crate::IntoElement {
            menu("file_menu").child(menu_item("copy").label("Copy").on_click(cx.listener(
                |this, _, _, cx| {
                    this.activations += 1;
                    cx.notify();
                },
            )))
        }
    }

    impl Render for TreeItemView {
        fn render(
            &mut self,
            _: &mut crate::Window,
            cx: &mut Context<Self>,
        ) -> impl crate::IntoElement {
            tree("project_tree").child(
                tree_item("src")
                    .label("src")
                    .selected(true)
                    .expanded(true)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.activations += 1;
                        cx.notify();
                    })),
            )
        }
    }

    #[crate::test]
    fn semantic_primitives_register_expected_accessibility_roles(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| SemanticPrimitivesView);

        window.update(|window, cx| {
            window.draw(cx).clear();

            let nodes = window.accessibility_tree.nodes.values().collect::<Vec<_>>();
            assert!(
                nodes
                    .iter()
                    .any(|node| node.role == AccessibilityRole::Toolbar)
            );
            assert!(
                nodes
                    .iter()
                    .any(|node| node.role == AccessibilityRole::Pane)
            );
            assert!(
                nodes
                    .iter()
                    .any(|node| node.role == AccessibilityRole::Dialog)
            );
            assert!(
                nodes
                    .iter()
                    .any(|node| node.role == AccessibilityRole::Alert)
            );
            assert!(
                nodes
                    .iter()
                    .any(|node| node.role == AccessibilityRole::Separator)
            );
            assert!(
                nodes
                    .iter()
                    .any(|node| node.role == AccessibilityRole::Menu)
            );
            assert!(
                nodes
                    .iter()
                    .any(|node| node.role == AccessibilityRole::MenuItem)
            );
            assert!(
                nodes
                    .iter()
                    .any(|node| node.role == AccessibilityRole::Link)
            );
            assert!(
                nodes
                    .iter()
                    .any(|node| node.role == AccessibilityRole::Tree)
            );
            assert!(
                nodes
                    .iter()
                    .any(|node| node.role == AccessibilityRole::TreeItem)
            );
        });
    }

    #[crate::test]
    fn interactive_semantic_primitives_expose_click_actions(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| SemanticPrimitivesView);

        window.update(|window, cx| {
            window.draw(cx).clear();

            let docs = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Link)
                .unwrap();
            assert_eq!(docs.role, AccessibilityRole::Link);
            assert!(docs.actions.contains(&AccessibilityAction::Click));

            let copy = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::MenuItem)
                .unwrap();
            assert_eq!(copy.role, AccessibilityRole::MenuItem);
            assert!(copy.actions.contains(&AccessibilityAction::Click));

            let src = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::TreeItem)
                .unwrap();
            assert_eq!(src.role, AccessibilityRole::TreeItem);
            assert!(src.actions.contains(&AccessibilityAction::Click));
        });
    }

    #[crate::test]
    fn link_opens_configured_url(cx: &mut TestAppContext) {
        cx.update(|app| {
            app.permission_broker
                .grant(app.current_process_id, Capability::OpenExternalUrl);
        });
        let test_cx = cx.clone();

        let (_view, mut window) = cx.add_window_view(|_, _| LinkView);

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("link-docs").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(
            test_cx.opened_url().as_deref(),
            Some("https://example.com/docs")
        );

        window.simulate_keystrokes("enter");
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(
            test_cx.opened_url().as_deref(),
            Some("https://example.com/docs")
        );
    }

    #[crate::test]
    fn menu_item_click_and_space_activate(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| MenuItemView { activations: 0 });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("menu-item-copy").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).activations, 1);
        });

        window.simulate_keystrokes("space");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).activations, 2);
        });
    }

    #[crate::test]
    fn tree_item_tracks_selected_expanded_and_activation(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| TreeItemView { activations: 0 });

        window.update(|window, cx| {
            window.draw(cx).clear();

            let node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::TreeItem)
                .unwrap();
            assert!(node.states.contains(AccessibilityState::SELECTED));
            assert!(node.states.contains(AccessibilityState::EXPANDED));
        });

        let bounds = window.debug_bounds("tree-item-src").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).activations, 1);
        });

        window.simulate_keystrokes("space");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).activations, 2);
        });
    }
}

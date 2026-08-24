use super::window_layer_id;
use crate::{
    AccessibilityAction, AccessibilityActionRequest, AccessibilityId, AccessibilityNode,
    AccessibilityRole, AccessibilityState, AccessibilityTree, AccessibilityValue,
};
use anyhow::{Context as _, Result, anyhow};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use web_sys::{Element, Event, EventTarget, HtmlCanvasElement, HtmlElement, KeyboardEvent};

/// A retained canvas can expose a very large logical data set while only
/// painting a small viewport. Keep the browser accessibility mirror bounded as
/// a final safety net; virtualized controls should already register only their
/// mounted semantic nodes.
const MAX_DOM_ACCESSIBILITY_NODES: usize = 4_096;
const MAX_PENDING_ACCESSIBILITY_ACTIONS: usize = 256;

struct DomEventListener {
    target: EventTarget,
    name: &'static str,
    callback: Closure<dyn FnMut(Event)>,
}

struct SemanticDomNode {
    element: HtmlElement,
}

/// Mirrors Kael's retained accessibility tree into transparent DOM semantics.
///
/// The canvas remains the visual and pointer-input owner. The mirror accepts
/// focus and keyboard/screen-reader activation only, then queues normalized
/// [`AccessibilityActionRequest`] values for the regular Kael action router.
pub(super) struct BrowserAccessibilityManager {
    layer: HtmlElement,
    canvas: HtmlCanvasElement,
    dom_id_prefix: String,
    nodes: HashMap<AccessibilityId, SemanticDomNode>,
    previous_nodes: HashMap<AccessibilityId, AccessibilityNode>,
    advertised_actions: Rc<RefCell<HashMap<AccessibilityId, Vec<AccessibilityAction>>>>,
    pending_actions: Rc<RefCell<VecDeque<AccessibilityActionRequest>>>,
    wake: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    listeners: Vec<DomEventListener>,
    visible: bool,
    canvas_rect: Option<[f64; 4]>,
}

impl BrowserAccessibilityManager {
    pub(super) fn new(canvas: &HtmlCanvasElement) -> Result<Self> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .context("browser accessibility requires a Document")?;
        let body = document
            .body()
            .context("browser accessibility requires a document body")?;
        let surface_id = canvas
            .get_attribute("data-kael-window-surface-id")
            .unwrap_or_else(|| "unknown".to_owned());
        let layer = document
            .create_element("div")
            .map_err(js_error)?
            .dyn_into::<HtmlElement>()
            .map_err(|_| anyhow!("browser accessibility layer was not an HtmlElement"))?;
        layer.set_id(&window_layer_id(canvas, "kael-accessibility-layer"));
        layer
            .set_attribute("data-kael-accessibility-layer", "true")
            .map_err(js_error)?;
        layer
            .set_attribute("role", "presentation")
            .map_err(js_error)?;
        let style = layer.style();
        for (name, value) in [
            ("position", "fixed"),
            ("inset", "0"),
            ("overflow", "visible"),
            ("pointer-events", "none"),
            ("z-index", "2147483645"),
        ] {
            style.set_property(name, value).map_err(js_error)?;
        }
        body.append_child(&layer).map_err(js_error)?;

        // The canvas continues to receive all visual pointer input. It is not
        // hidden from assistive technology because it remains the fallback
        // keyboard focus surface when no semantic child owns focus.
        canvas
            .set_attribute("aria-label", "Kael retained application surface")
            .map_err(js_error)?;
        canvas
            .set_attribute("data-kael-accessibility", "initializing")
            .map_err(js_error)?;

        let advertised_actions = Rc::new(RefCell::new(HashMap::<
            AccessibilityId,
            Vec<AccessibilityAction>,
        >::new()));
        let pending_actions = Rc::new(RefCell::new(VecDeque::new()));
        let wake = Rc::new(RefCell::new(None::<Box<dyn Fn()>>));
        let mut listeners = Vec::new();

        {
            let actions = advertised_actions.clone();
            let pending = pending_actions.clone();
            let wake = wake.clone();
            let layer_for_overflow = layer.clone();
            add_listener(
                &mut listeners,
                layer.clone().unchecked_into(),
                "click",
                move |event| {
                    let Some(node_id) = event_accessibility_id(&event) else {
                        return;
                    };
                    let action = actions
                        .borrow()
                        .get(&node_id)
                        .and_then(|actions| activation_action(actions));
                    if let Some(action) = action {
                        event.prevent_default();
                        event.stop_propagation();
                        queue_action(
                            &pending,
                            &wake,
                            &layer_for_overflow,
                            AccessibilityActionRequest::new(node_id, action),
                        );
                    }
                },
            )?;
        }

        {
            let actions = advertised_actions.clone();
            let pending = pending_actions.clone();
            let wake = wake.clone();
            let layer_for_overflow = layer.clone();
            add_listener(
                &mut listeners,
                layer.clone().unchecked_into(),
                "focusin",
                move |event| {
                    let Some(node_id) = event_accessibility_id(&event) else {
                        return;
                    };
                    if actions
                        .borrow()
                        .get(&node_id)
                        .is_some_and(|actions| actions.contains(&AccessibilityAction::Focus))
                    {
                        queue_action(
                            &pending,
                            &wake,
                            &layer_for_overflow,
                            AccessibilityActionRequest::new(node_id, AccessibilityAction::Focus),
                        );
                    }
                },
            )?;
        }

        {
            let actions = advertised_actions.clone();
            let pending = pending_actions.clone();
            let wake = wake.clone();
            let layer_for_overflow = layer.clone();
            add_listener(
                &mut listeners,
                layer.clone().unchecked_into(),
                "keydown",
                move |event| {
                    let Some(node_id) = event_accessibility_id(&event) else {
                        return;
                    };
                    let Ok(keyboard) = event.dyn_into::<KeyboardEvent>() else {
                        return;
                    };
                    let action = actions
                        .borrow()
                        .get(&node_id)
                        .and_then(|actions| keyboard_action(&keyboard.key(), actions));
                    if let Some(action) = action {
                        keyboard.prevent_default();
                        keyboard.stop_propagation();
                        queue_action(
                            &pending,
                            &wake,
                            &layer_for_overflow,
                            AccessibilityActionRequest::new(node_id, action),
                        );
                    }
                },
            )?;
        }

        Ok(Self {
            layer,
            canvas: canvas.clone(),
            dom_id_prefix: format!("kael-a11y-{surface_id}"),
            nodes: HashMap::new(),
            previous_nodes: HashMap::new(),
            advertised_actions,
            pending_actions,
            wake,
            listeners,
            visible: true,
            canvas_rect: None,
        })
    }

    pub(super) fn set_wake(&mut self, wake: impl Fn() + 'static) {
        *self.wake.borrow_mut() = Some(Box::new(wake));
    }

    pub(super) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        let _ = self
            .layer
            .style()
            .set_property("display", if visible { "block" } else { "none" });
    }

    pub(super) fn sync(&mut self, tree: &AccessibilityTree) -> Result<()> {
        let (ordered_ids, truncated) = reachable_nodes(tree, MAX_DOM_ACCESSIBILITY_NODES);
        let retained: HashSet<_> = ordered_ids.iter().copied().collect();

        for stale_id in self
            .nodes
            .keys()
            .copied()
            .filter(|id| !retained.contains(id))
            .collect::<Vec<_>>()
        {
            if let Some(stale) = self.nodes.remove(&stale_id) {
                stale.element.remove();
            }
        }

        let document = web_sys::window()
            .and_then(|window| window.document())
            .context("browser accessibility requires a Document")?;
        for id in &ordered_ids {
            if self.nodes.contains_key(id) {
                continue;
            }
            let element = document
                .create_element("div")
                .map_err(js_error)?
                .dyn_into::<HtmlElement>()
                .map_err(|_| anyhow!("browser accessibility node was not an HtmlElement"))?;
            element
                .set_attribute("id", &dom_node_id(&self.dom_id_prefix, *id))
                .map_err(js_error)?;
            element
                .set_attribute("data-kael-a11y-id", &id.0.to_string())
                .map_err(js_error)?;
            initialize_node_style(&element)?;
            self.layer.append_child(&element).map_err(js_error)?;
            self.nodes.insert(*id, SemanticDomNode { element });
        }

        let canvas_rect = self.canvas.get_bounding_client_rect();
        let current_canvas_rect = [
            canvas_rect.left(),
            canvas_rect.top(),
            canvas_rect.width(),
            canvas_rect.height(),
        ];
        let canvas_moved = self.canvas_rect != Some(current_canvas_rect);
        for id in &ordered_ids {
            let Some(node) = tree.get(*id) else { continue };
            let previous = self.previous_nodes.get(id);
            let semantic = self
                .nodes
                .get(id)
                .expect("retained accessibility node must have a DOM element");
            if previous.is_none_or(|previous| !same_node_semantics(previous, node)) {
                apply_node(&semantic.element, node)?;
            }
            if canvas_moved || previous.is_none_or(|previous| previous.bounds != node.bounds) {
                apply_bounds(&semantic.element, node, &canvas_rect)?;
            }
        }
        self.canvas_rect = Some(current_canvas_rect);

        // Reconcile semantic ownership without recreating unchanged nodes. CSS
        // `position: fixed` keeps nested descendants in window coordinates.
        for id in &ordered_ids {
            let Some(node) = tree.get(*id) else { continue };
            let parent = if *id == tree.root {
                self.layer.clone()
            } else {
                node.parent
                    .filter(|parent| retained.contains(parent))
                    .and_then(|parent| self.nodes.get(&parent))
                    .map(|parent| parent.element.clone())
                    .unwrap_or_else(|| self.layer.clone())
            };
            let semantic = self
                .nodes
                .get(id)
                .expect("retained accessibility node must have a DOM element");
            if !semantic
                .element
                .parent_element()
                .is_some_and(|actual| actual.is_same_node(Some(parent.as_ref())))
            {
                parent.append_child(&semantic.element).map_err(js_error)?;
            }

            let children_changed = self
                .previous_nodes
                .get(id)
                .is_none_or(|previous| previous.children != node.children);
            if children_changed {
                for child_id in node.children.iter().filter(|id| retained.contains(id)) {
                    if let Some(child) = self.nodes.get(child_id) {
                        semantic
                            .element
                            .append_child(&child.element)
                            .map_err(js_error)?;
                    }
                }
            }
        }

        self.previous_nodes = ordered_ids
            .iter()
            .filter_map(|id| tree.get(*id).cloned().map(|node| (*id, node)))
            .collect();
        *self.advertised_actions.borrow_mut() = self
            .previous_nodes
            .iter()
            .map(|(id, node)| (*id, node.actions.clone()))
            .collect();

        self.canvas
            .set_attribute("data-kael-accessibility", "ready")
            .map_err(js_error)?;
        self.canvas
            .set_attribute(
                "data-kael-accessibility-nodes",
                &ordered_ids.len().to_string(),
            )
            .map_err(js_error)?;
        self.canvas
            .set_attribute(
                "data-kael-accessibility-truncated",
                if truncated { "true" } else { "false" },
            )
            .map_err(js_error)?;
        self.set_visible(self.visible);
        Ok(())
    }

    pub(super) fn drain_actions(&mut self) -> Vec<AccessibilityActionRequest> {
        self.pending_actions.borrow_mut().drain(..).collect()
    }
}

impl Drop for BrowserAccessibilityManager {
    fn drop(&mut self) {
        *self.wake.borrow_mut() = None;
        for listener in self.listeners.drain(..) {
            let _ = listener.target.remove_event_listener_with_callback(
                listener.name,
                listener.callback.as_ref().unchecked_ref(),
            );
        }
        self.nodes.clear();
        self.layer.remove();
    }
}

fn add_listener(
    listeners: &mut Vec<DomEventListener>,
    target: EventTarget,
    name: &'static str,
    callback: impl FnMut(Event) + 'static,
) -> Result<()> {
    let callback = Closure::wrap(Box::new(callback) as Box<dyn FnMut(Event)>);
    target
        .add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())
        .map_err(js_error)?;
    listeners.push(DomEventListener {
        target,
        name,
        callback,
    });
    Ok(())
}

fn queue_action(
    pending: &Rc<RefCell<VecDeque<AccessibilityActionRequest>>>,
    wake: &Rc<RefCell<Option<Box<dyn Fn()>>>>,
    layer: &HtmlElement,
    request: AccessibilityActionRequest,
) {
    let queued = {
        let mut pending = pending.borrow_mut();
        if pending.len() >= MAX_PENDING_ACCESSIBILITY_ACTIONS {
            let _ = layer.set_attribute("data-kael-accessibility-action-overflow", "true");
            false
        } else if pending.back() == Some(&request) {
            false
        } else {
            pending.push_back(request);
            true
        }
    };
    if queued && let Some(wake) = wake.borrow().as_ref() {
        wake();
    }
}

fn event_accessibility_id(event: &Event) -> Option<AccessibilityId> {
    let mut element = event.target()?.dyn_into::<Element>().ok()?;
    loop {
        if let Some(raw_id) = element.get_attribute("data-kael-a11y-id")
            && let Ok(id) = raw_id.parse::<u64>()
        {
            return Some(AccessibilityId(id));
        }
        element = element.parent_element()?;
    }
}

fn activation_action(actions: &[AccessibilityAction]) -> Option<AccessibilityAction> {
    [
        AccessibilityAction::Click,
        AccessibilityAction::Toggle,
        AccessibilityAction::ShowMenu,
    ]
    .into_iter()
    .find(|action| actions.contains(action))
}

fn keyboard_action(key: &str, actions: &[AccessibilityAction]) -> Option<AccessibilityAction> {
    let candidates: &[AccessibilityAction] = match key {
        "Enter" | " " | "Spacebar" => &[
            AccessibilityAction::Click,
            AccessibilityAction::Toggle,
            AccessibilityAction::ShowMenu,
        ],
        "ArrowUp" => &[
            AccessibilityAction::Increment,
            AccessibilityAction::ScrollUp,
        ],
        "ArrowRight" => &[AccessibilityAction::Increment, AccessibilityAction::Expand],
        "ArrowDown" => &[
            AccessibilityAction::Decrement,
            AccessibilityAction::ScrollDown,
        ],
        "ArrowLeft" => &[
            AccessibilityAction::Decrement,
            AccessibilityAction::Collapse,
        ],
        "PageUp" => &[AccessibilityAction::ScrollUp],
        "PageDown" => &[AccessibilityAction::ScrollDown],
        "Escape" => &[AccessibilityAction::Dismiss],
        _ => &[],
    };
    candidates
        .iter()
        .copied()
        .find(|candidate| actions.contains(candidate))
}

fn reachable_nodes(tree: &AccessibilityTree, limit: usize) -> (Vec<AccessibilityId>, bool) {
    if limit == 0 || tree.get(tree.root).is_none() {
        return (Vec::new(), !tree.nodes.is_empty());
    }
    let mut ordered = Vec::with_capacity(tree.node_count().min(limit));
    let mut queue = VecDeque::from([tree.root]);
    let mut visited = HashSet::new();
    let mut truncated = false;
    while let Some(id) = queue.pop_front() {
        if !visited.insert(id) {
            continue;
        }
        let Some(node) = tree.get(id) else { continue };
        if id != tree.root && node.states.contains(AccessibilityState::HIDDEN) {
            continue;
        }
        if ordered.len() == limit {
            truncated = true;
            break;
        }
        ordered.push(id);
        queue.extend(node.children.iter().copied());
    }
    (ordered, truncated)
}

fn apply_node(element: &HtmlElement, node: &AccessibilityNode) -> Result<()> {
    set_or_remove(element, "role", Some(dom_role(node.role)))?;
    set_or_remove(element, "aria-label", node.label.as_deref())?;
    set_or_remove(element, "aria-description", node.description.as_deref())?;
    set_or_remove(element, "title", node.description.as_deref())?;
    set_or_remove(element, "aria-placeholder", node.placeholder.as_deref())?;
    set_owned_or_remove(
        element,
        "aria-level",
        node.level
            .filter(|level| *level > 0)
            .map(|level| level.to_string()),
    )?;
    set_owned_or_remove(
        element,
        "aria-rowcount",
        node.row_count.map(|count| count.to_string()),
    )?;
    set_owned_or_remove(
        element,
        "aria-colcount",
        node.column_count.map(|count| count.to_string()),
    )?;
    set_owned_or_remove(
        element,
        "aria-rowindex",
        node.row_index.map(|index| index.to_string()),
    )?;
    set_owned_or_remove(
        element,
        "aria-colindex",
        node.column_index.map(|index| index.to_string()),
    )?;
    set_owned_or_remove(
        element,
        "aria-rowspan",
        node.row_span.map(|span| span.to_string()),
    )?;
    set_owned_or_remove(
        element,
        "aria-colspan",
        node.column_span.map(|span| span.to_string()),
    )?;
    set_or_remove(
        element,
        "aria-sort",
        node.sort_direction.map(|direction| direction.to_text()),
    )?;
    set_bool_state(
        element,
        "aria-disabled",
        node.states.contains(AccessibilityState::DISABLED),
    )?;
    set_bool_state(
        element,
        "aria-selected",
        node.states.contains(AccessibilityState::SELECTED),
    )?;
    set_bool_state(
        element,
        "aria-pressed",
        node.states.contains(AccessibilityState::PRESSED),
    )?;
    set_bool_state(
        element,
        "aria-required",
        node.states.contains(AccessibilityState::REQUIRED),
    )?;
    set_bool_state(
        element,
        "aria-invalid",
        node.states.contains(AccessibilityState::INVALID),
    )?;
    set_bool_state(
        element,
        "aria-busy",
        node.states.contains(AccessibilityState::BUSY),
    )?;
    set_bool_state(
        element,
        "aria-readonly",
        node.states.contains(AccessibilityState::READ_ONLY),
    )?;

    let expanded = if node.states.contains(AccessibilityState::EXPANDED) {
        Some("true")
    } else if node.states.contains(AccessibilityState::COLLAPSED) {
        Some("false")
    } else {
        None
    };
    set_or_remove(element, "aria-expanded", expanded)?;
    let checked = if node.states.contains(AccessibilityState::INDETERMINATE) {
        Some("mixed")
    } else if node.states.contains(AccessibilityState::CHECKED) {
        Some("true")
    } else if matches!(
        node.role,
        AccessibilityRole::CheckBox | AccessibilityRole::RadioButton | AccessibilityRole::Switch
    ) {
        Some("false")
    } else {
        None
    };
    set_or_remove(element, "aria-checked", checked)?;

    for attribute in [
        "aria-valuenow",
        "aria-valuemin",
        "aria-valuemax",
        "aria-valuetext",
        "data-kael-value-step",
    ] {
        element.remove_attribute(attribute).map_err(js_error)?;
    }
    if let Some(value) = &node.value {
        apply_value(element, value)?;
    }

    let focusable = !node.states.contains(AccessibilityState::DISABLED)
        && (node.actions.contains(&AccessibilityAction::Focus) || node.role.is_interactive());
    set_or_remove(element, "tabindex", focusable.then_some("0"))?;
    set_or_remove(
        element,
        "data-kael-focused",
        node.states
            .contains(AccessibilityState::FOCUSED)
            .then_some("true"),
    )?;
    let actions = node
        .actions
        .iter()
        .map(|action| action.to_text())
        .collect::<Vec<_>>()
        .join(" ");
    set_or_remove(
        element,
        "data-kael-actions",
        (!actions.is_empty()).then_some(actions.as_str()),
    )?;
    if node.role == AccessibilityRole::Dialog {
        element
            .set_attribute("aria-modal", "true")
            .map_err(js_error)?;
    } else {
        element.remove_attribute("aria-modal").map_err(js_error)?;
    }
    Ok(())
}

fn initialize_node_style(element: &HtmlElement) -> Result<()> {
    let style = element.style();
    for (name, value) in [
        ("position", "fixed"),
        ("box-sizing", "border-box"),
        ("margin", "0"),
        ("padding", "0"),
        ("border", "0"),
        ("outline", "0"),
        ("overflow", "hidden"),
        ("pointer-events", "none"),
        ("user-select", "none"),
        ("background", "transparent"),
        ("color", "transparent"),
        // Zero opacity is intentionally avoided because some accessibility
        // engines prune fully transparent content from their navigation tree.
        ("opacity", "0.001"),
    ] {
        style.set_property(name, value).map_err(js_error)?;
    }
    Ok(())
}

fn apply_bounds(
    element: &HtmlElement,
    node: &AccessibilityNode,
    canvas_rect: &web_sys::DomRect,
) -> Result<()> {
    let (left, top, width, height) = node.bounds.map_or_else(
        || {
            (
                canvas_rect.left(),
                canvas_rect.top(),
                canvas_rect.width().max(1.0),
                canvas_rect.height().max(1.0),
            )
        },
        |bounds| {
            (
                canvas_rect.left() + bounds.x,
                canvas_rect.top() + bounds.y,
                bounds.width.max(1.0),
                bounds.height.max(1.0),
            )
        },
    );
    let style = element.style();
    for (name, value) in [
        ("left", format!("{left}px")),
        ("top", format!("{top}px")),
        ("width", format!("{width}px")),
        ("height", format!("{height}px")),
    ] {
        style.set_property(name, &value).map_err(js_error)?;
    }
    Ok(())
}

fn same_node_semantics(previous: &AccessibilityNode, current: &AccessibilityNode) -> bool {
    previous.role == current.role
        && previous.states == current.states
        && previous.label == current.label
        && previous.description == current.description
        && previous.value == current.value
        && previous.placeholder == current.placeholder
        && previous.level == current.level
        && previous.row_count == current.row_count
        && previous.column_count == current.column_count
        && previous.row_index == current.row_index
        && previous.column_index == current.column_index
        && previous.row_span == current.row_span
        && previous.column_span == current.column_span
        && previous.sort_direction == current.sort_direction
        && previous.actions == current.actions
}

fn apply_value(element: &HtmlElement, value: &AccessibilityValue) -> Result<()> {
    match value {
        AccessibilityValue::Text(text) => {
            element
                .set_attribute("aria-valuetext", text)
                .map_err(js_error)?;
        }
        AccessibilityValue::Number(number) => {
            element
                .set_attribute("aria-valuenow", &number.to_string())
                .map_err(js_error)?;
        }
        AccessibilityValue::Range {
            current,
            min,
            max,
            step,
        } => {
            element
                .set_attribute("aria-valuenow", &current.to_string())
                .map_err(js_error)?;
            element
                .set_attribute("aria-valuemin", &min.to_string())
                .map_err(js_error)?;
            element
                .set_attribute("aria-valuemax", &max.to_string())
                .map_err(js_error)?;
            if let Some(step) = step {
                element
                    .set_attribute("data-kael-value-step", &step.to_string())
                    .map_err(js_error)?;
            }
        }
        AccessibilityValue::Toggle(value) => {
            element
                .set_attribute("aria-checked", if *value { "true" } else { "false" })
                .map_err(js_error)?;
        }
    }
    Ok(())
}

fn dom_role(role: AccessibilityRole) -> &'static str {
    match role {
        AccessibilityRole::Application | AccessibilityRole::Window => "application",
        AccessibilityRole::Button => "button",
        AccessibilityRole::TextInput => "textbox",
        AccessibilityRole::StaticText => "text",
        AccessibilityRole::Heading => "heading",
        AccessibilityRole::Group => "group",
        AccessibilityRole::List => "list",
        AccessibilityRole::ListItem => "listitem",
        AccessibilityRole::Table => "table",
        AccessibilityRole::Grid => "grid",
        AccessibilityRole::Row => "row",
        AccessibilityRole::Cell => "cell",
        AccessibilityRole::ColumnHeader => "columnheader",
        AccessibilityRole::RowHeader => "rowheader",
        AccessibilityRole::ScrollBar => "scrollbar",
        AccessibilityRole::Image => "img",
        AccessibilityRole::Link => "link",
        AccessibilityRole::Menu => "menu",
        AccessibilityRole::MenuItem => "menuitem",
        AccessibilityRole::Tab => "tab",
        AccessibilityRole::TabPanel => "tabpanel",
        AccessibilityRole::Toolbar => "toolbar",
        AccessibilityRole::Tree => "tree",
        AccessibilityRole::TreeItem => "treeitem",
        AccessibilityRole::CheckBox => "checkbox",
        AccessibilityRole::RadioButton => "radio",
        AccessibilityRole::Slider => "slider",
        AccessibilityRole::ProgressBar => "progressbar",
        AccessibilityRole::Separator => "separator",
        AccessibilityRole::Pane => "region",
        AccessibilityRole::Dialog => "dialog",
        AccessibilityRole::Alert => "alert",
        AccessibilityRole::ComboBox => "combobox",
        AccessibilityRole::Switch => "switch",
        AccessibilityRole::Unknown => "generic",
    }
}

fn dom_node_id(prefix: &str, id: AccessibilityId) -> String {
    format!("{prefix}-{}", id.0)
}

fn set_bool_state(element: &HtmlElement, name: &str, enabled: bool) -> Result<()> {
    set_or_remove(element, name, enabled.then_some("true"))
}

fn set_owned_or_remove(element: &HtmlElement, name: &str, value: Option<String>) -> Result<()> {
    set_or_remove(element, name, value.as_deref())
}

fn set_or_remove(element: &HtmlElement, name: &str, value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        element.set_attribute(name, value).map_err(js_error)
    } else {
        element.remove_attribute(name).map_err(js_error)
    }
}

fn js_error(value: JsValue) -> anyhow::Error {
    anyhow!(value.as_string().unwrap_or_else(|| format!("{value:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_actions_are_limited_to_advertised_semantics() {
        let actions = vec![
            AccessibilityAction::Focus,
            AccessibilityAction::Toggle,
            AccessibilityAction::Increment,
            AccessibilityAction::Decrement,
        ];
        assert_eq!(
            keyboard_action("Enter", &actions),
            Some(AccessibilityAction::Toggle)
        );
        assert_eq!(
            keyboard_action("ArrowRight", &actions),
            Some(AccessibilityAction::Increment)
        );
        assert_eq!(keyboard_action("Escape", &actions), None);
    }

    #[test]
    fn spreadsheet_roles_map_to_exact_aria_roles() {
        assert_eq!(dom_role(AccessibilityRole::Table), "table");
        assert_eq!(dom_role(AccessibilityRole::Grid), "grid");
        assert_eq!(dom_role(AccessibilityRole::Row), "row");
        assert_eq!(dom_role(AccessibilityRole::Cell), "cell");
        assert_eq!(dom_role(AccessibilityRole::ColumnHeader), "columnheader");
        assert_eq!(dom_role(AccessibilityRole::RowHeader), "rowheader");
    }

    #[test]
    fn reachable_nodes_prunes_hidden_subtrees_and_bounds_output() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let root_id = root.id;
        let mut tree = AccessibilityTree::new(root);
        let visible = AccessibilityNode::new(AccessibilityRole::Button);
        let visible_id = visible.id;
        tree.insert(visible);
        tree.set_parent(visible_id, root_id);
        let hidden = AccessibilityNode::new(AccessibilityRole::Group)
            .with_states(AccessibilityState::HIDDEN);
        let hidden_id = hidden.id;
        tree.insert(hidden);
        tree.set_parent(hidden_id, root_id);
        let hidden_child = AccessibilityNode::new(AccessibilityRole::Button);
        let hidden_child_id = hidden_child.id;
        tree.insert(hidden_child);
        tree.set_parent(hidden_child_id, hidden_id);

        let (nodes, truncated) = reachable_nodes(&tree, 32);
        assert_eq!(nodes, vec![root_id, visible_id]);
        assert!(!truncated);

        let (nodes, truncated) = reachable_nodes(&tree, 1);
        assert_eq!(nodes, vec![root_id]);
        assert!(truncated);
    }
}

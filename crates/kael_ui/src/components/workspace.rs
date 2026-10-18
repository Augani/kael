//! Persistent desktop docking with nested splits, tab groups and floating panes.
//!
//! Register stable pane IDs and keep a `DockWorkspaceState` entity. Pane content
//! is application-owned; only active panes are rendered. Save `layout_json` on
//! `DockWorkspaceEvent::LayoutChanged`, then restore with `restore_json`.

use super::{
    button::{Button, ButtonSize, ButtonVariant},
    icon::Icon,
    split_pane::{SplitDirection, SplitPane, SplitPaneState},
};
use crate::theme::Theme;
use kael::{prelude::*, *};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Split orientation in a persisted workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DockAxis {
    Horizontal,
    Vertical,
}
/// Edge or tab insertion target for pane/group movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockPlacement {
    Tab(usize),
    Left,
    Right,
    Top,
    Bottom,
}
/// A persisted group of stable pane IDs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockGroup {
    pub id: u64,
    pub panes: Vec<String>,
    pub active: String,
}
/// Recursive layout. Splits retain stable IDs and finite, bounded ratios.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DockNode {
    Group(DockGroup),
    Split {
        id: u64,
        axis: DockAxis,
        ratio: f32,
        first: Box<DockNode>,
        second: Box<DockNode>,
    },
}
/// Persisted floating bounds in workspace-local logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Default for DockRect {
    fn default() -> Self {
        Self {
            x: 48.0,
            y: 48.0,
            width: 420.0,
            height: 300.0,
        }
    }
}
/// Floating tab group inside the workspace window, movable and resizable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloatingDock {
    pub group: DockGroup,
    pub bounds: DockRect,
}
/// Versioned serialization contract. Rendering entities and callbacks are never
/// serialized; the pane registry restores those using stable pane IDs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockLayout {
    pub version: u32,
    pub root: Option<DockNode>,
    pub floating: Vec<FloatingDock>,
    pub closed: Vec<String>,
    pub zoomed: Option<u64>,
}
impl DockLayout {
    pub fn group(panes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let panes = panes.into_iter().map(Into::into).collect::<Vec<_>>();
        let root = panes.first().cloned().map(|active| {
            DockNode::Group(DockGroup {
                id: 1,
                panes,
                active,
            })
        });
        Self {
            version: 1,
            root,
            floating: Vec::new(),
            closed: Vec::new(),
            zoomed: None,
        }
    }
    /// Validate persistence before applying it. Unknown pane IDs, duplicate IDs,
    /// invalid geometry and excessively deep graphs leave current state intact.
    pub fn validate(&self, pane_ids: &HashSet<String>) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported workspace layout version".into());
        }
        let mut ids = HashSet::new();
        let mut panes = HashSet::new();
        fn group(
            group: &DockGroup,
            ids: &mut HashSet<u64>,
            panes: &mut HashSet<String>,
            known: &HashSet<String>,
        ) -> Result<(), String> {
            if group.id == 0 || group.id == u64::MAX || !ids.insert(group.id) {
                return Err("duplicate or invalid dock node ID".into());
            }
            if group.panes.is_empty() || !group.panes.contains(&group.active) {
                return Err("empty dock group or missing active pane".into());
            }
            for pane in &group.panes {
                if !known.contains(pane) || !panes.insert(pane.clone()) {
                    return Err("unknown or duplicate pane ID".into());
                }
            }
            Ok(())
        }
        fn validate_node(
            node: &DockNode,
            depth: usize,
            ids: &mut HashSet<u64>,
            panes: &mut HashSet<String>,
            known: &HashSet<String>,
        ) -> Result<(), String> {
            if depth > 32 {
                return Err("workspace exceeds maximum split depth".into());
            }
            match node {
                DockNode::Group(value) => group(value, ids, panes, known),
                DockNode::Split {
                    id,
                    ratio,
                    first,
                    second,
                    ..
                } => {
                    if *id == 0
                        || *id == u64::MAX
                        || !ids.insert(*id)
                        || !ratio.is_finite()
                        || !(0.05..=0.95).contains(ratio)
                    {
                        return Err("invalid split ID or ratio".into());
                    }
                    validate_node(first, depth + 1, ids, panes, known)?;
                    validate_node(second, depth + 1, ids, panes, known)
                }
            }
        }
        if let Some(root) = self.root.as_ref() {
            validate_node(root, 0, &mut ids, &mut panes, pane_ids)?;
        }
        for floating in &self.floating {
            group(&floating.group, &mut ids, &mut panes, pane_ids)?;
            let rect = floating.bounds;
            if [rect.x, rect.y, rect.width, rect.height]
                .iter()
                .any(|value| !value.is_finite())
                || rect.x < 0.0
                || rect.y < 0.0
                || rect.width < 180.0
                || rect.height < 100.0
                || rect.width > 100_000.0
                || rect.height > 100_000.0
            {
                return Err("invalid floating pane bounds".into());
            }
        }
        for pane in &self.closed {
            if !pane_ids.contains(pane) || !panes.insert(pane.clone()) {
                return Err("unknown or duplicate closed pane ID".into());
            }
        }
        if &panes != pane_ids {
            return Err("layout must account for every registered pane".into());
        }
        if self.zoomed.is_some_and(|id| self.find_group(id).is_none()) {
            return Err("zoom target is not a group".into());
        }
        Ok(())
    }
    fn find_group(&self, id: u64) -> Option<&DockGroup> {
        fn visit(node: &DockNode, id: u64) -> Option<&DockGroup> {
            match node {
                DockNode::Group(group) => (group.id == id).then_some(group),
                DockNode::Split { first, second, .. } => {
                    visit(first, id).or_else(|| visit(second, id))
                }
            }
        }
        self.root
            .as_ref()
            .and_then(|node| visit(node, id))
            .or_else(|| {
                self.floating
                    .iter()
                    .find(|pane| pane.group.id == id)
                    .map(|pane| &pane.group)
            })
    }
    fn group_mut(&mut self, id: u64) -> Option<&mut DockGroup> {
        fn visit(node: &mut DockNode, id: u64) -> Option<&mut DockGroup> {
            match node {
                DockNode::Group(group) => (group.id == id).then_some(group),
                DockNode::Split { first, second, .. } => {
                    visit(first, id).or_else(|| visit(second, id))
                }
            }
        }
        self.root
            .as_mut()
            .and_then(|node| visit(node, id))
            .or_else(|| {
                self.floating
                    .iter_mut()
                    .find(|pane| pane.group.id == id)
                    .map(|pane| &mut pane.group)
            })
    }
    fn first_group(&self) -> Option<u64> {
        fn first(node: &DockNode) -> u64 {
            match node {
                DockNode::Group(group) => group.id,
                DockNode::Split { first: child, .. } => first(child),
            }
        }
        self.root.as_ref().map(first)
    }
    fn max_id(&self) -> u64 {
        fn max(node: &DockNode) -> u64 {
            match node {
                DockNode::Group(group) => group.id,
                DockNode::Split {
                    id, first, second, ..
                } => (*id).max(max(first)).max(max(second)),
            }
        }
        self.root.as_ref().map_or(0, max).max(
            self.floating
                .iter()
                .map(|pane| pane.group.id)
                .max()
                .unwrap_or(0),
        )
    }
    fn remove_group(&mut self, id: u64) -> Option<DockGroup> {
        fn take(node: DockNode, id: u64) -> (Option<DockNode>, Option<DockGroup>) {
            match node {
                DockNode::Group(group) if group.id == id => (None, Some(group)),
                DockNode::Group(group) => (Some(DockNode::Group(group)), None),
                DockNode::Split {
                    id: split_id,
                    axis,
                    ratio,
                    first,
                    second,
                } => {
                    let (first, a) = take(*first, id);
                    let (second, b) = take(*second, id);
                    let node = match (first, second) {
                        (Some(first), Some(second)) => Some(DockNode::Split {
                            id: split_id,
                            axis,
                            ratio,
                            first: Box::new(first),
                            second: Box::new(second),
                        }),
                        (first, second) => first.or(second),
                    };
                    (node, a.or(b))
                }
            }
        }
        if let Some(index) = self.floating.iter().position(|pane| pane.group.id == id) {
            return Some(self.floating.remove(index).group);
        }
        let root = self.root.take()?;
        let (root, group) = take(root, id);
        self.root = root;
        group
    }
    fn remove_pane(&mut self, pane: &str) -> Option<u64> {
        fn find(node: &DockNode, pane: &str) -> Option<u64> {
            match node {
                DockNode::Group(group) => {
                    group.panes.iter().any(|id| id == pane).then_some(group.id)
                }
                DockNode::Split { first, second, .. } => {
                    find(first, pane).or_else(|| find(second, pane))
                }
            }
        }
        let id = self
            .root
            .as_ref()
            .and_then(|node| find(node, pane))
            .or_else(|| {
                self.floating
                    .iter()
                    .find(|floating| floating.group.panes.iter().any(|id| id == pane))
                    .map(|floating| floating.group.id)
            })?;
        let group = self.group_mut(id).unwrap();
        let index = group.panes.iter().position(|id| id == pane).unwrap();
        group.panes.remove(index);
        if group.panes.is_empty() {
            self.remove_group(id);
        } else if group.active == pane {
            group.active = group.panes[index.min(group.panes.len() - 1)].clone();
        }
        Some(id)
    }
    fn insert_group(
        &mut self,
        target: u64,
        placement: DockPlacement,
        group: DockGroup,
        split_id: u64,
    ) -> bool {
        if let DockPlacement::Tab(index) = placement {
            let Some(target) = self.group_mut(target) else {
                return false;
            };
            target.active = group.active;
            let index = index.min(target.panes.len());
            target.panes.splice(index..index, group.panes);
            return true;
        }
        fn insert(
            node: &mut DockNode,
            target: u64,
            placement: DockPlacement,
            group: &mut Option<DockGroup>,
            split_id: u64,
        ) -> bool {
            match node {
                DockNode::Group(current) if current.id == target => {
                    let current = node.clone();
                    let moved = DockNode::Group(group.take().unwrap());
                    let (axis, before) = match placement {
                        DockPlacement::Left => (DockAxis::Horizontal, true),
                        DockPlacement::Right => (DockAxis::Horizontal, false),
                        DockPlacement::Top => (DockAxis::Vertical, true),
                        DockPlacement::Bottom => (DockAxis::Vertical, false),
                        _ => unreachable!(),
                    };
                    let (first, second) = if before {
                        (moved, current)
                    } else {
                        (current, moved)
                    };
                    *node = DockNode::Split {
                        id: split_id,
                        axis,
                        ratio: 0.5,
                        first: Box::new(first),
                        second: Box::new(second),
                    };
                    true
                }
                DockNode::Split { first, second, .. } => {
                    insert(first, target, placement, group, split_id)
                        || insert(second, target, placement, group, split_id)
                }
                _ => false,
            }
        }
        self.root
            .as_mut()
            .is_some_and(|root| insert(root, target, placement, &mut Some(group), split_id))
    }
}

/// Stable application-owned pane content. Preserve interactive entity state in
/// the callback's captured entities, since inactive tab contents are unmounted.
#[derive(Clone)]
pub struct DockPane {
    pub id: String,
    pub title: SharedString,
    render: Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>,
}
impl DockPane {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<SharedString>,
        render: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            render: Rc::new(render),
        }
    }
}
#[derive(Clone, Debug)]
pub enum DockWorkspaceEvent {
    LayoutChanged,
    PaneActivated(String),
    PaneClosed(String),
}
struct FloatGesture {
    group: u64,
    origin: Point<Pixels>,
    initial: DockRect,
    resize: bool,
}
/// Persistent layout/controller shared by the workspace element.
pub struct DockWorkspaceState {
    layout: DockLayout,
    panes: HashMap<String, DockPane>,
    split_states: HashMap<u64, Entity<SplitPaneState>>,
    next_id: u64,
    bounds: Bounds<Pixels>,
    gesture: Option<FloatGesture>,
}
impl EventEmitter<DockWorkspaceEvent> for DockWorkspaceState {}
impl DockWorkspaceState {
    pub fn new(
        panes: Vec<DockPane>,
        layout: DockLayout,
        cx: &mut Context<Self>,
    ) -> Result<Self, String> {
        let mut registry = HashMap::new();
        for pane in panes {
            if pane.id.is_empty() || registry.insert(pane.id.clone(), pane).is_some() {
                return Err("pane IDs must be nonempty and unique".into());
            }
        }
        layout.validate(&registry.keys().cloned().collect())?;
        let mut state = Self {
            next_id: layout.max_id() + 1,
            layout,
            panes: registry,
            split_states: HashMap::new(),
            bounds: Bounds::default(),
            gesture: None,
        };
        state.sync_splits(cx);
        Ok(state)
    }
    pub fn layout(&self) -> &DockLayout {
        &self.layout
    }
    pub fn layout_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.layout)
    }
    pub fn restore_json(&mut self, json: &str, cx: &mut Context<Self>) -> Result<(), String> {
        if json.len() > 1_048_576 {
            return Err("workspace snapshot exceeds one MiB".into());
        }
        let layout: DockLayout = serde_json::from_str(json).map_err(|error| error.to_string())?;
        layout.validate(&self.panes.keys().cloned().collect())?;
        self.next_id = layout.max_id() + 1;
        self.layout = layout;
        self.gesture = None;
        self.changed(cx);
        Ok(())
    }
    fn allocate_id(&mut self) -> u64 {
        fn contains(node: &DockNode, id: u64) -> bool {
            match node {
                DockNode::Group(group) => group.id == id,
                DockNode::Split {
                    id: current,
                    first,
                    second,
                    ..
                } => *current == id || contains(first, id) || contains(second, id),
            }
        }
        loop {
            if self.next_id == 0 || self.next_id == u64::MAX {
                self.next_id = 1;
            }
            let id = self.next_id;
            self.next_id += 1;
            if !self
                .layout
                .root
                .as_ref()
                .is_some_and(|root| contains(root, id))
                && !self.layout.floating.iter().any(|pane| pane.group.id == id)
            {
                return id;
            }
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        if self
            .layout
            .zoomed
            .is_some_and(|id| self.layout.find_group(id).is_none())
        {
            self.layout.zoomed = None;
        }
        self.sync_splits(cx);
        cx.emit(DockWorkspaceEvent::LayoutChanged);
        cx.notify();
    }
    fn commit_layout_change(&mut self, previous: DockLayout, cx: &mut Context<Self>) -> bool {
        if self
            .layout
            .zoomed
            .is_some_and(|id| self.layout.find_group(id).is_none())
        {
            self.layout.zoomed = None;
        }
        if self
            .layout
            .validate(&self.panes.keys().cloned().collect())
            .is_err()
        {
            self.layout = previous;
            return false;
        }
        self.changed(cx);
        true
    }
    fn sync_splits(&mut self, cx: &mut Context<Self>) {
        fn collect(node: &DockNode, output: &mut Vec<(u64, DockAxis, f32)>) {
            if let DockNode::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } = node
            {
                output.push((*id, *axis, *ratio));
                collect(first, output);
                collect(second, output);
            }
        }
        let mut splits = Vec::new();
        if let Some(root) = self.layout.root.as_ref() {
            collect(root, &mut splits);
        }
        let ids: HashSet<_> = splits.iter().map(|(id, _, _)| *id).collect();
        self.split_states.retain(|id, _| ids.contains(id));
        for (id, axis, ratio) in splits {
            let state = self
                .split_states
                .entry(id)
                .or_insert_with(|| cx.new(SplitPaneState::new));
            state.update(cx, |state, cx| {
                let direction = match axis {
                    DockAxis::Horizontal => SplitDirection::Horizontal,
                    DockAxis::Vertical => SplitDirection::Vertical,
                };
                if state.direction() != direction {
                    state.set_direction(direction, cx);
                }
                state.set_ratio(ratio, cx);
            });
        }
    }
    pub fn activate(&mut self, group: u64, pane: &str, cx: &mut Context<Self>) -> bool {
        let Some(group) = self.layout.group_mut(group) else {
            return false;
        };
        if !group.panes.iter().any(|id| id == pane) {
            return false;
        }
        group.active = pane.to_string();
        cx.emit(DockWorkspaceEvent::PaneActivated(pane.to_string()));
        self.changed(cx);
        true
    }
    pub fn close(&mut self, pane: &str, cx: &mut Context<Self>) -> bool {
        if self.layout.remove_pane(pane).is_none() {
            return false;
        }
        self.layout.closed.push(pane.to_string());
        cx.emit(DockWorkspaceEvent::PaneClosed(pane.to_string()));
        self.changed(cx);
        true
    }
    pub fn reopen(&mut self, pane: &str, cx: &mut Context<Self>) -> bool {
        let Some(index) = self.layout.closed.iter().position(|id| id == pane) else {
            return false;
        };
        self.layout.closed.remove(index);
        if let Some(id) = self.layout.first_group() {
            let group = self.layout.group_mut(id).unwrap();
            group.panes.push(pane.to_string());
            group.active = pane.to_string();
        } else {
            let id = self.allocate_id();
            self.layout.root = Some(DockNode::Group(DockGroup {
                id,
                panes: vec![pane.to_string()],
                active: pane.to_string(),
            }));
        }
        self.changed(cx);
        true
    }
    pub fn move_pane(
        &mut self,
        pane: &str,
        target: u64,
        placement: DockPlacement,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.layout.find_group(target).is_none() {
            return false;
        }
        let previous = self.layout.clone();
        // Reserve IDs while source nodes are still attached, including rollover.
        let group_id = self.allocate_id();
        let split_id = self.allocate_id();
        let Some(source) = self.layout.remove_pane(pane) else {
            return false;
        };
        if source == target && self.layout.find_group(target).is_none() {
            self.layout = previous;
            return false;
        }
        let group = DockGroup {
            id: group_id,
            panes: vec![pane.to_string()],
            active: pane.to_string(),
        };
        if !self.layout.insert_group(target, placement, group, split_id) {
            self.layout = previous;
            return false;
        }
        self.commit_layout_change(previous, cx)
    }
    pub fn move_group(
        &mut self,
        source: u64,
        target: u64,
        placement: DockPlacement,
        cx: &mut Context<Self>,
    ) -> bool {
        if source == target || self.layout.find_group(target).is_none() {
            return false;
        }
        let previous = self.layout.clone();
        let split_id = self.allocate_id();
        let Some(group) = self.layout.remove_group(source) else {
            return false;
        };
        if !self.layout.insert_group(target, placement, group, split_id) {
            self.layout = previous;
            return false;
        }
        self.commit_layout_change(previous, cx)
    }
    /// Dock a pane against the outer workspace edge, wrapping existing splits.
    pub fn dock_at_edge(
        &mut self,
        pane: &str,
        placement: DockPlacement,
        cx: &mut Context<Self>,
    ) -> bool {
        if matches!(placement, DockPlacement::Tab(_)) {
            return false;
        }
        let previous = self.layout.clone();
        let id = self.allocate_id();
        let split_id = self.allocate_id();
        let Some(_) = self.layout.remove_pane(pane) else {
            return false;
        };
        let group = DockNode::Group(DockGroup {
            id,
            panes: vec![pane.to_string()],
            active: pane.to_string(),
        });
        if let Some(root) = self.layout.root.take() {
            let (axis, before) = match placement {
                DockPlacement::Left => (DockAxis::Horizontal, true),
                DockPlacement::Right => (DockAxis::Horizontal, false),
                DockPlacement::Top => (DockAxis::Vertical, true),
                _ => (DockAxis::Vertical, false),
            };
            let (first, second) = if before { (group, root) } else { (root, group) };
            self.layout.root = Some(DockNode::Split {
                id: split_id,
                axis,
                ratio: if before { 0.25 } else { 0.75 },
                first: Box::new(first),
                second: Box::new(second),
            });
        } else {
            self.layout.root = Some(group);
        }
        self.commit_layout_change(previous, cx)
    }
    pub fn float_group(&mut self, group: u64, bounds: DockRect, cx: &mut Context<Self>) -> bool {
        if !valid_rect(bounds) {
            return false;
        }
        let Some(group) = self.layout.remove_group(group) else {
            return false;
        };
        self.layout.floating.push(FloatingDock { group, bounds });
        self.changed(cx);
        true
    }
    pub fn float_pane(&mut self, pane: &str, bounds: DockRect, cx: &mut Context<Self>) -> bool {
        if !valid_rect(bounds) || self.layout.remove_pane(pane).is_none() {
            return false;
        }
        let id = self.allocate_id();
        self.layout.floating.push(FloatingDock {
            group: DockGroup {
                id,
                panes: vec![pane.to_string()],
                active: pane.to_string(),
            },
            bounds,
        });
        self.changed(cx);
        true
    }
    pub fn dock_floating(&mut self, group: u64, cx: &mut Context<Self>) -> bool {
        let Some(index) = self
            .layout
            .floating
            .iter()
            .position(|pane| pane.group.id == group)
        else {
            return false;
        };
        let floating = self.layout.floating.remove(index);
        if let Some(target) = self.layout.first_group() {
            self.layout
                .insert_group(target, DockPlacement::Tab(usize::MAX), floating.group, 0);
        } else {
            self.layout.root = Some(DockNode::Group(floating.group));
        }
        self.changed(cx);
        true
    }
    pub fn toggle_zoom(&mut self, group: u64, cx: &mut Context<Self>) -> bool {
        if self.layout.find_group(group).is_none() {
            return false;
        }
        self.layout.zoomed = if self.layout.zoomed == Some(group) {
            None
        } else {
            Some(group)
        };
        self.changed(cx);
        true
    }
    pub fn set_split_ratio(&mut self, id: u64, ratio: f32, cx: &mut Context<Self>) -> bool {
        if !ratio.is_finite() {
            return false;
        }
        fn set(node: &mut DockNode, id: u64, ratio: f32) -> bool {
            match node {
                DockNode::Split {
                    id: split_id,
                    ratio: current,
                    first,
                    second,
                    ..
                } => {
                    if *split_id == id {
                        *current = ratio.clamp(0.05, 0.95);
                        true
                    } else {
                        set(first, id, ratio) || set(second, id, ratio)
                    }
                }
                _ => false,
            }
        }
        if !self
            .layout
            .root
            .as_mut()
            .is_some_and(|root| set(root, id, ratio))
        {
            return false;
        }
        self.changed(cx);
        true
    }
    fn update_float_gesture(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.gesture = None;
            return;
        }
        let Some(gesture) = self.gesture.as_ref() else {
            return;
        };
        let dx = f32::from(event.position.x - gesture.origin.x);
        let dy = f32::from(event.position.y - gesture.origin.y);
        let mut rect = gesture.initial;
        let group = gesture.group;
        if gesture.resize {
            rect.width = (rect.width + dx).clamp(180.0, 100_000.0);
            rect.height = (rect.height + dy).clamp(100.0, 100_000.0);
        } else {
            rect.x = (rect.x + dx).clamp(0.0, (f32::from(self.bounds.size.width) - 64.0).max(0.0));
            rect.y = (rect.y + dy).clamp(0.0, (f32::from(self.bounds.size.height) - 32.0).max(0.0));
        }
        self.set_floating_bounds(group, rect, cx);
    }
    fn clamp_floating_to_workspace(&mut self, cx: &mut Context<Self>) {
        let width = f32::from(self.bounds.size.width);
        let height = f32::from(self.bounds.size.height);
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let mut changed = false;
        for floating in &mut self.layout.floating {
            let old = floating.bounds;
            floating.bounds.x = old.x.min((width - 64.0).max(0.0));
            floating.bounds.y = old.y.min((height - 32.0).max(0.0));
            floating.bounds.width = old.width.min(width.max(180.0));
            floating.bounds.height = old.height.min(height.max(100.0));
            changed |= old != floating.bounds;
        }
        if changed {
            cx.emit(DockWorkspaceEvent::LayoutChanged);
            cx.notify();
        }
    }
    pub fn set_floating_bounds(
        &mut self,
        group: u64,
        bounds: DockRect,
        cx: &mut Context<Self>,
    ) -> bool {
        if !valid_rect(bounds) {
            return false;
        }
        let Some(floating) = self
            .layout
            .floating
            .iter_mut()
            .find(|pane| pane.group.id == group)
        else {
            return false;
        };
        if floating.bounds == bounds {
            return false;
        }
        floating.bounds = bounds;
        cx.emit(DockWorkspaceEvent::LayoutChanged);
        cx.notify();
        true
    }
}
fn valid_rect(rect: DockRect) -> bool {
    [rect.x, rect.y, rect.width, rect.height]
        .iter()
        .all(|value| value.is_finite())
        && rect.x >= 0.0
        && rect.y >= 0.0
        && (180.0..=100_000.0).contains(&rect.width)
        && (100.0..=100_000.0).contains(&rect.height)
}

#[derive(Clone)]
enum DockDragSource {
    Pane(String),
    Group(u64),
}
#[derive(Clone)]
struct DockDrag {
    owner: EntityId,
    source: DockDragSource,
    title: SharedString,
}
impl Render for DockDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .p_2()
            .rounded(theme.tokens.radius_sm)
            .bg(theme.tokens.card)
            .border_1()
            .border_color(theme.tokens.border)
            .child(self.title.clone())
    }
}
fn drop_in_group(
    state: &WeakEntity<DockWorkspaceState>,
    data: &DockDrag,
    target: u64,
    placement: DockPlacement,
    cx: &mut App,
) {
    let _ = state.update(cx, |state, cx| {
        if data.owner != cx.entity().entity_id() {
            return;
        }
        match &data.source {
            DockDragSource::Pane(pane) => {
                state.move_pane(pane, target, placement, cx);
            }
            DockDragSource::Group(group) => {
                state.move_group(*group, target, placement, cx);
            }
        }
    });
}

/// Visible desktop workspace. Drag tabs to reorder/merge, drag the group grip to
/// move whole groups, and drag into the edge targets to create nested splits.
/// Ctrl-Shift-arrow docks an active pane against the outer workspace edge.
/// Floating groups stay within this workspace window; they are not OS windows.
#[derive(IntoElement)]
pub struct DockWorkspace {
    id: ElementId,
    state: Entity<DockWorkspaceState>,
    style: StyleRefinement,
}
impl DockWorkspace {
    pub fn new(id: impl Into<ElementId>, state: Entity<DockWorkspaceState>) -> Self {
        Self {
            id: id.into(),
            state,
            style: StyleRefinement::default(),
        }
    }
}
impl Styled for DockWorkspace {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
fn render_dock_node(
    node: &DockNode,
    state: &Entity<DockWorkspaceState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    match node {
        DockNode::Group(group) => render_dock_group(group, false, state, window, cx),
        DockNode::Split {
            id, first, second, ..
        } => {
            let split = state.read(cx).split_states[id].clone();
            let id = *id;
            let resize = state.downgrade();
            let first = render_dock_node(first, state, window, cx);
            let second = render_dock_node(second, state, window, cx);
            SplitPane::new(split)
                .first(first)
                .second(second)
                .on_resize(move |ratio, _, cx| {
                    let _ = resize.update(cx, |state, cx| state.set_split_ratio(id, ratio, cx));
                })
                .into_any_element()
        }
    }
}
fn render_dock_group(
    group: &DockGroup,
    floating: bool,
    state: &Entity<DockWorkspaceState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let group_id = group.id;
    let owner = state.entity_id();
    let weak = state.downgrade();
    let title = state.read(cx).panes[&group.active].title.clone();
    let background = Theme::of(cx).tokens.background;
    let border = Theme::of(cx).tokens.border;
    let accent = Theme::of(cx).tokens.accent;
    let key = state.downgrade();
    let key_panes = group.panes.clone();
    let active = group.active.clone();
    let mut tabs = div()
        .id(("dock-tabs", group_id))
        .flex()
        .items_center()
        .gap_1()
        .min_h(px(34.0))
        .border_b_1()
        .border_color(border)
        .bg(Theme::of(cx).tokens.card)
        .on_key_down(move |event, _, cx| {
            let key_name = event.keystroke.key.as_str();
            if event.keystroke.modifiers.control && event.keystroke.modifiers.shift {
                let placement = match key_name {
                    "left" => Some(DockPlacement::Left),
                    "right" => Some(DockPlacement::Right),
                    "up" => Some(DockPlacement::Top),
                    "down" => Some(DockPlacement::Bottom),
                    _ => None,
                };
                if let Some(placement) = placement {
                    let _ = key.update(cx, |state, cx| state.dock_at_edge(&active, placement, cx));
                    cx.stop_propagation();
                }
            } else if !event.keystroke.modifiers.modified() && matches!(key_name, "left" | "right")
            {
                let current = key_panes
                    .iter()
                    .position(|pane| pane == &active)
                    .unwrap_or(0);
                let next = if key_name == "right" {
                    (current + 1) % key_panes.len()
                } else {
                    (current + key_panes.len() - 1) % key_panes.len()
                };
                let _ = key.update(cx, |state, cx| {
                    state.activate(group_id, &key_panes[next], cx)
                });
                cx.stop_propagation();
            }
        });
    if !floating {
        let drag = DockDrag {
            owner,
            source: DockDragSource::Group(group_id),
            title: title.clone(),
        };
        tabs = tabs.child(
            div()
                .id(("dock-group-grip", group_id))
                .w(px(24.0))
                .h(px(30.0))
                .cursor_grab()
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new("grip-vertical").size(px(14.0)))
                .on_drag(drag, |data: &DockDrag, _, _, cx| cx.new(|_| data.clone())),
        );
    }
    for (index, pane_id) in group.panes.iter().enumerate() {
        let pane = &state.read(cx).panes[pane_id];
        let click = state.downgrade();
        let selected = pane_id == &group.active;
        let pane_id = pane_id.clone();
        let click_id = pane_id.clone();
        let drop = state.downgrade();
        let drag = DockDrag {
            owner,
            source: DockDragSource::Pane(pane_id),
            title: pane.title.clone(),
        };
        tabs = tabs.child(
            div()
                .id((
                    ElementId::from("dock-tab-wrapper"),
                    format!("{group_id}-{index}"),
                ))
                .on_drag(drag, |data: &DockDrag, _, _, cx| cx.new(|_| data.clone()))
                .drag_over::<DockDrag>(move |style, _, _, _| {
                    style.border_l_2().border_color(accent)
                })
                .on_drop(move |data: &DockDrag, _, cx| {
                    drop_in_group(&drop, data, group_id, DockPlacement::Tab(index), cx)
                })
                .child(
                    Button::new(
                        (ElementId::from("dock-tab"), format!("{group_id}-{index}")),
                        pane.title.clone(),
                    )
                    .size(ButtonSize::Sm)
                    .variant(if selected {
                        ButtonVariant::Secondary
                    } else {
                        ButtonVariant::Ghost
                    })
                    .on_click(move |_, _, cx| {
                        let _ =
                            click.update(cx, |state, cx| state.activate(group_id, &click_id, cx));
                    }),
                ),
        );
    }
    let zoom = state.downgrade();
    let float = state.downgrade();
    let close = state.downgrade();
    let close_id = group.active.clone();
    tabs = tabs
        .child(div().flex_1())
        .child(
            Button::new(
                ("dock-zoom", group_id),
                if state.read(cx).layout.zoomed == Some(group_id) {
                    "Restore"
                } else {
                    "Zoom"
                },
            )
            .size(ButtonSize::Sm)
            .variant(ButtonVariant::Ghost)
            .on_click(move |_, _, cx| {
                let _ = zoom.update(cx, |state, cx| state.toggle_zoom(group_id, cx));
            }),
        )
        .child(
            Button::new(
                ("dock-float", group_id),
                if floating { "Dock" } else { "Float" },
            )
            .size(ButtonSize::Sm)
            .variant(ButtonVariant::Ghost)
            .on_click(move |_, _, cx| {
                let _ = float.update(cx, |state, cx| {
                    if floating {
                        state.dock_floating(group_id, cx)
                    } else {
                        state.float_group(group_id, DockRect::default(), cx)
                    }
                });
            }),
        )
        .child(
            Button::new(("dock-close", group_id), "Close")
                .size(ButtonSize::Sm)
                .variant(ButtonVariant::Ghost)
                .on_click(move |_, _, cx| {
                    let _ = close.update(cx, |state, cx| state.close(&close_id, cx));
                }),
        );
    let pane = state.read(cx).panes[&group.active].clone();
    let content = (pane.render)(window, cx);
    let center_drop = state.downgrade();
    let mut body = div()
        .id(("dock-body", group_id))
        .relative()
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0))
        .overflow_hidden()
        .bg(background)
        .accessibility(
            AccessibilityAttributes::new(AccessibilityRole::Group).label(format!("{title} pane")),
        )
        .drag_over::<DockDrag>(move |style, _, _, _| style.border_2().border_color(accent))
        .on_drop(move |data: &DockDrag, _, cx| {
            drop_in_group(
                &center_drop,
                data,
                group_id,
                DockPlacement::Tab(usize::MAX),
                cx,
            )
        })
        .child(content);
    if !floating && cx.has_active_drag() {
        for (name, placement) in [
            ("left", DockPlacement::Left),
            ("right", DockPlacement::Right),
            ("top", DockPlacement::Top),
            ("bottom", DockPlacement::Bottom),
        ] {
            let drop = weak.clone();
            body = body.child(
                div()
                    .id((ElementId::from("dock-edge"), format!("{group_id}-{name}")))
                    .absolute()
                    .bg(accent.opacity(0.25))
                    .when(name == "left", |element| {
                        element
                            .left_0()
                            .top(relative(0.25))
                            .bottom(relative(0.25))
                            .w(px(38.0))
                    })
                    .when(name == "right", |element| {
                        element
                            .right_0()
                            .top(relative(0.25))
                            .bottom(relative(0.25))
                            .w(px(38.0))
                    })
                    .when(name == "top", |element| {
                        element
                            .top_0()
                            .left(relative(0.25))
                            .right(relative(0.25))
                            .h(px(38.0))
                    })
                    .when(name == "bottom", |element| {
                        element
                            .bottom_0()
                            .left(relative(0.25))
                            .right(relative(0.25))
                            .h(px(38.0))
                    })
                    .drag_over::<DockDrag>(move |style, _, _, _| style.bg(accent.opacity(0.75)))
                    .on_drop(move |data: &DockDrag, _, cx| {
                        drop_in_group(&drop, data, group_id, placement, cx);
                        cx.stop_propagation();
                    }),
            );
        }
    }
    div()
        .id(("dock-group", group_id))
        .size_full()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .flex()
        .flex_col()
        .border_1()
        .border_color(border)
        .child(tabs)
        .child(body)
        .into_any_element()
}
impl RenderOnce for DockWorkspace {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        super::model_observer::observe_model(self.id.clone(), &self.state, window, cx);
        let layout = self.state.read(cx).layout.clone();
        let bounds_state = self.state.downgrade();
        let move_state = self.state.downgrade();
        let up_state = self.state.downgrade();
        let background = Theme::of(cx).tokens.background;
        let border = Theme::of(cx).tokens.border;
        let mut root = div()
            .id(self.id)
            .relative()
            .size_full()
            .min_h(px(0.0))
            .min_w(px(0.0))
            .overflow_hidden()
            .bg(background)
            .child(
                canvas(
                    move |bounds: Bounds<Pixels>, _: &mut Window, cx: &mut App| {
                        let _ = bounds_state.update(cx, |state, cx| {
                            state.bounds = bounds;
                            state.clamp_floating_to_workspace(cx);
                        });
                    },
                    move |_: Bounds<Pixels>, _: (), window: &mut Window, _: &mut App| {
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = move_state
                                    .update(cx, |state, cx| state.update_float_gesture(event, cx));
                            }
                        });
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture && event.button == MouseButton::Left
                            {
                                let _ = up_state.update(cx, |state, _| state.gesture = None);
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            );
        if let Some(group) = layout.zoomed.and_then(|id| layout.find_group(id)) {
            root = root.child(render_dock_group(
                group,
                layout
                    .floating
                    .iter()
                    .any(|floating| floating.group.id == group.id),
                &self.state,
                window,
                cx,
            ));
        } else {
            if let Some(node) = layout.root.as_ref() {
                root = root.child(render_dock_node(node, &self.state, window, cx));
            }
            for floating in &layout.floating {
                let group = floating.group.id;
                let rect = floating.bounds;
                let down = self.state.downgrade();
                let resize = self.state.downgrade();
                let key = self.state.downgrade();
                let title = self.state.read(cx).panes[&floating.group.active]
                    .title
                    .clone();
                let content = render_dock_group(&floating.group, true, &self.state, window, cx);
                root = root.child(
                    div()
                        .id(("dock-floating", group))
                        .absolute()
                        .left(px(rect.x))
                        .top(px(rect.y))
                        .w(px(rect.width))
                        .h(px(rect.height))
                        .occlude()
                        .flex()
                        .flex_col()
                        .border_1()
                        .border_color(border)
                        .bg(background)
                        .child(
                            div()
                                .id(("dock-floating-title", group))
                                .focusable()
                                .tab_index(0)
                                .tab_stop(true)
                                .h(px(28.0))
                                .flex_shrink_0()
                                .cursor_grab()
                                .px_2()
                                .bg(Theme::of(cx).tokens.card)
                                .accessibility(
                                    AccessibilityAttributes::new(AccessibilityRole::Group)
                                        .label(format!("Move floating {title}"))
                                        .description("Arrow keys move; Control-arrow resizes"),
                                )
                                .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                                    let _ = down.update(cx, |state, _| {
                                        state.gesture = Some(FloatGesture {
                                            group,
                                            origin: event.position,
                                            initial: rect,
                                            resize: false,
                                        })
                                    });
                                })
                                .on_key_down(move |event, _, cx| {
                                    let delta = if event.keystroke.modifiers.shift {
                                        24.0
                                    } else {
                                        8.0
                                    };
                                    let (dx, dy) = match event.keystroke.key.as_str() {
                                        "left" => (-delta, 0.0),
                                        "right" => (delta, 0.0),
                                        "up" => (0.0, -delta),
                                        "down" => (0.0, delta),
                                        _ => return,
                                    };
                                    let _ = key.update(cx, |state, cx| {
                                        if let Some(floating) = state
                                            .layout
                                            .floating
                                            .iter()
                                            .find(|pane| pane.group.id == group)
                                        {
                                            let mut rect = floating.bounds;
                                            if event.keystroke.modifiers.control {
                                                rect.width = (rect.width + dx).max(180.0);
                                                rect.height = (rect.height + dy).max(100.0);
                                            } else {
                                                rect.x = (rect.x + dx).max(0.0);
                                                rect.y = (rect.y + dy).max(0.0);
                                            }
                                            state.set_floating_bounds(group, rect, cx);
                                        }
                                    });
                                    cx.stop_propagation();
                                })
                                .child(title),
                        )
                        .child(div().flex_1().min_h(px(0.0)).child(content))
                        .child(
                            div()
                                .id(("dock-floating-resize", group))
                                .absolute()
                                .right_0()
                                .bottom_0()
                                .size(px(16.0))
                                .cursor_crosshair()
                                .bg(border)
                                .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                                    let _ = resize.update(cx, |state, _| {
                                        state.gesture = Some(FloatGesture {
                                            group,
                                            origin: event.position,
                                            initial: rect,
                                            resize: true,
                                        })
                                    });
                                    cx.stop_propagation();
                                }),
                        ),
                );
            }
        }
        if !layout.closed.is_empty() {
            let mut closed = div()
                .absolute()
                .bottom_0()
                .left_0()
                .flex()
                .gap_1()
                .p_1()
                .bg(Theme::of(cx).tokens.card);
            for pane in layout.closed {
                let reopen = self.state.downgrade();
                let title = self.state.read(cx).panes[&pane].title.clone();
                closed = closed.child(
                    Button::new(
                        (ElementId::from("dock-reopen"), pane.clone()),
                        format!("Reopen {title}"),
                    )
                    .size(ButtonSize::Sm)
                    .variant(ButtonVariant::Ghost)
                    .on_click(move |_, _, cx| {
                        let _ = reopen.update(cx, |state, cx| state.reopen(&pane, cx));
                    }),
                );
            }
            root = root.child(closed);
        }
        root.map(|mut element| {
            element.style().refine(&self.style);
            element
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn panes() -> Vec<DockPane> {
        ["editor", "files", "properties", "terminal"]
            .into_iter()
            .map(|id| {
                DockPane::new(id, id, move |_, _| {
                    div().child(format!("{id} content")).into_any_element()
                })
            })
            .collect()
    }
    fn state(cx: &mut Context<DockWorkspaceState>) -> DockWorkspaceState {
        DockWorkspaceState::new(
            panes(),
            DockLayout::group(["editor", "files", "properties", "terminal"]),
            cx,
        )
        .unwrap()
    }
    #[::core::prelude::v1::test]
    fn restored_high_ids_remain_unique_when_allocations_wrap() {
        let mut cx = TestAppContext::single();
        for edge in [true, false] {
            let state = cx.new(|cx| {
                DockWorkspaceState::new(
                    panes(),
                    DockLayout {
                        version: 1,
                        root: Some(DockNode::Split {
                            id: if edge { u64::MAX - 2 } else { u64::MAX - 1 },
                            axis: DockAxis::Horizontal,
                            ratio: 0.5,
                            first: Box::new(DockNode::Group(DockGroup {
                                id: 1,
                                panes: vec!["editor".into(), "files".into()],
                                active: "editor".into(),
                            })),
                            second: Box::new(DockNode::Group(DockGroup {
                                id: 2,
                                panes: vec!["properties".into(), "terminal".into()],
                                active: "properties".into(),
                            })),
                        }),
                        floating: Vec::new(),
                        closed: Vec::new(),
                        zoomed: None,
                    },
                    cx,
                )
                .unwrap()
            });
            state.update(&mut cx, |state, cx| {
                if edge {
                    assert!(state.dock_at_edge("files", DockPlacement::Bottom, cx));
                } else {
                    assert!(state.move_group(1, 2, DockPlacement::Left, cx));
                }
                state
                    .layout
                    .validate(&state.panes.keys().cloned().collect())
                    .unwrap();
                let expected = state.layout.clone();
                state
                    .restore_json(&state.layout_json().unwrap(), cx)
                    .unwrap();
                assert_eq!(state.layout, expected);
            });
        }
    }
    #[::core::prelude::v1::test]
    fn runtime_splits_reject_excessive_depth_without_changing_layout() {
        let mut cx = TestAppContext::single();
        let state = cx.new(|cx| {
            let panes = (0..40)
                .map(|i| {
                    DockPane::new(format!("pane-{i}"), "Pane", |_, _| div().into_any_element())
                })
                .collect();
            DockWorkspaceState::new(
                panes,
                DockLayout::group((0..40).map(|i| format!("pane-{i}"))),
                cx,
            )
            .unwrap()
        });
        state.update(&mut cx, |state, cx| {
            for index in 1..=32 {
                assert!(state.move_pane(&format!("pane-{index}"), 1, DockPlacement::Left, cx));
            }
            let before = state.layout.clone();
            let split_count = state.split_states.len();
            assert!(!state.move_pane("pane-33", 1, DockPlacement::Bottom, cx));
            assert_eq!(state.layout, before);
            assert_eq!(state.split_states.len(), split_count);
            assert!(!state.dock_at_edge("pane-34", DockPlacement::Right, cx));
            assert_eq!(state.layout, before);
            assert!(state.float_pane("pane-35", DockRect::default(), cx));
            let floating = state.layout.floating[0].group.id;
            let before = state.layout.clone();
            assert!(!state.move_group(floating, 1, DockPlacement::Top, cx));
            assert_eq!(state.layout, before);
            state
                .restore_json(&state.layout_json().unwrap(), cx)
                .unwrap();
        });
    }
    #[::core::prelude::v1::test]
    fn nested_moves_groups_floating_and_persistence_preserve_unique_panes() {
        let mut cx = TestAppContext::single();
        let state = cx.new(state);
        state.update(&mut cx, |state, cx| {
            assert!(state.move_pane("files", 1, DockPlacement::Left, cx));
            assert!(state.move_pane("properties", 1, DockPlacement::Right, cx));
            assert!(state.move_pane("terminal", 1, DockPlacement::Bottom, cx));
            let files = match state.layout.root.as_ref().unwrap() {
                DockNode::Split { first, .. } => match &**first {
                    DockNode::Group(group) => group.id,
                    _ => panic!(),
                },
                _ => panic!(),
            };
            assert!(state.float_group(files, DockRect::default(), cx));
            assert_eq!(state.layout.floating.len(), 1);
            assert!(state.set_floating_bounds(
                files,
                DockRect {
                    x: 80.0,
                    y: 64.0,
                    width: 500.0,
                    height: 320.0
                },
                cx
            ));
            assert!(state.toggle_zoom(1, cx));
            let saved = state.layout_json().unwrap();
            let expected = state.layout.clone();
            assert!(state.close("properties", cx));
            assert!(state.reopen("properties", cx));
            state.restore_json(&saved, cx).unwrap();
            assert_eq!(state.layout, expected);
            assert!(state.dock_floating(files, cx));
            assert!(state.layout.floating.is_empty());
            state
                .layout
                .validate(&state.panes.keys().cloned().collect())
                .unwrap();
            let before = state.layout.clone();
            assert!(!state.move_group(1, 999, DockPlacement::Left, cx));
            assert_eq!(state.layout, before);
        });
    }
    #[::core::prelude::v1::test]
    fn restore_rejects_corrupt_layout_atomically_and_split_resize_is_persisted() {
        let mut cx = TestAppContext::single();
        let state = cx.new(state);
        state.update(&mut cx, |state, cx| {
            assert!(state.move_pane("files", 1, DockPlacement::Left, cx));
            let id = match state.layout.root.as_ref().unwrap() {
                DockNode::Split { id, .. } => *id,
                _ => panic!(),
            };
            assert!(state.set_split_ratio(id, 0.31, cx));
            assert!(state.layout_json().unwrap().contains("0.31"));
            let before = state.layout.clone();
            let mut corrupt = before.clone();
            corrupt.floating.push(FloatingDock {
                group: DockGroup {
                    id: 1,
                    panes: vec!["editor".into()],
                    active: "editor".into(),
                },
                bounds: DockRect::default(),
            });
            assert!(
                state
                    .restore_json(&serde_json::to_string(&corrupt).unwrap(), cx)
                    .is_err()
            );
            assert_eq!(state.layout, before);
            assert!(!state.set_split_ratio(id, f32::NAN, cx));
            assert!(!state.float_pane(
                "editor",
                DockRect {
                    width: f32::INFINITY,
                    ..DockRect::default()
                },
                cx
            ));
            assert_eq!(state.layout, before);
            assert!(state.close("files", cx));
            assert!(
                !state.split_states.contains_key(&id),
                "pruned splits must release editor entities"
            );
        });
    }
    struct Host {
        state: Entity<DockWorkspaceState>,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            DockWorkspace::new("test-workspace", self.state.clone())
                .w(px(1000.0))
                .h(px(600.0))
        }
    }
    #[::core::prelude::v1::test]
    fn pointer_tab_drop_creates_split_and_floating_pointer_and_keyboard_resize_work() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
        let state = cx.new(state);
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| Host { state }
        });
        let source = window.update(|window, cx| {
            window.draw(cx).clear();
            let bounds = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| {
                    node.label.as_deref() == Some("files") && node.role == AccessibilityRole::Button
                })
                .unwrap()
                .bounds
                .unwrap();
            point(
                px((bounds.x + bounds.width / 2.0) as f32),
                px((bounds.y + bounds.height / 2.0) as f32),
            )
        });
        window.simulate_mouse_down(source, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_move(
            source + point(px(20.0), px(10.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(cx.has_active_drag());
        });
        let target = point(px(500.0), px(580.0));
        window.simulate_mouse_move(target, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
        window.run_until_parked();
        let group = window.update(|_, cx| {
            state.update(cx, |state, cx| {
                let group = match state.layout.root.as_ref().unwrap() {
                    DockNode::Split {
                        axis: DockAxis::Vertical,
                        second,
                        ..
                    } => match &**second {
                        DockNode::Group(group) => {
                            assert_eq!(group.panes, ["files"]);
                            group.id
                        }
                        _ => panic!("expected files group"),
                    },
                    _ => panic!("edge drop did not create vertical split"),
                };
                assert!(state.float_group(group, DockRect::default(), cx));
                group
            })
        });
        let title = window.update(|window, cx| {
            window.draw(cx).clear();
            window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Move floating files"))
                .unwrap()
                .clone()
        });
        let rect = title.bounds.unwrap();
        let start = point(
            px((rect.x + rect.width / 2.0) as f32),
            px((rect.y + rect.height / 2.0) as f32),
        );
        window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_move(
            start + point(px(37.0), px(29.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        window.simulate_mouse_up(
            start + point(px(37.0), px(29.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        let moved = window.update(|_, cx| state.read(cx).layout.floating[0].bounds);
        assert_eq!((moved.x, moved.y), (85.0, 77.0));
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        let handle = point(
            px(moved.x + moved.width - 8.0),
            px(moved.y + moved.height - 8.0),
        );
        window.simulate_mouse_down(handle, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_move(
            handle + point(px(30.0), px(40.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        window.simulate_mouse_up(
            handle + point(px(30.0), px(40.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        window.update(|window, cx| {
            window.draw(cx).clear();
            let bounds = state.read(cx).layout.floating[0].bounds;
            assert_eq!((bounds.width, bounds.height), (450.0, 340.0));
        });
        let focus = window.update(|window, _| {
            window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Move floating files"))
                .unwrap()
                .id
        });
        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                focus,
                AccessibilityAction::Focus,
            ))
        });
        window.run_until_parked();
        window.simulate_keystrokes("right ctrl-right");
        window.update(|_, cx| {
            let bounds = state
                .read(cx)
                .layout
                .floating
                .iter()
                .find(|pane| pane.group.id == group)
                .unwrap()
                .bounds;
            assert_eq!((bounds.x, bounds.width), (93.0, 458.0));
        });
    }

    #[::core::prelude::v1::test]
    fn rendered_tabs_zoom_float_close_and_reopen_dispatch_real_accessibility_actions() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
        let state = cx.new(state);
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| Host { state }
        });
        let click = |window: &mut VisualTestContext, label: &str| {
            let id = window.update(|window, cx| {
                window.draw(cx).clear();
                window
                    .accessibility_tree()
                    .nodes
                    .values()
                    .find(|node| {
                        node.label.as_deref() == Some(label)
                            && node.actions.contains(&AccessibilityAction::Click)
                    })
                    .unwrap_or_else(|| panic!("missing {label} button"))
                    .id
            });
            window.update(|window, _| {
                window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    id,
                    AccessibilityAction::Click,
                ))
            });
            window.run_until_parked();
        };
        click(window, "files");
        window.update(|_, cx| {
            state.update(cx, |state, _| {
                assert_eq!(state.layout.find_group(1).unwrap().active, "files")
            })
        });
        click(window, "Zoom");
        window
            .update(|_, cx| state.update(cx, |state, _| assert_eq!(state.layout.zoomed, Some(1))));
        click(window, "Restore");
        click(window, "Float");
        window.update(|_, cx| {
            state.update(cx, |state, _| assert_eq!(state.layout.floating.len(), 1))
        });
        click(window, "Dock");
        click(window, "Close");
        window.update(|_, cx| {
            state.update(cx, |state, _| assert_eq!(state.layout.closed, ["files"]))
        });
        click(window, "Reopen files");
        window.update(|_, cx| state.update(cx, |state, _| assert!(state.layout.closed.is_empty())));
    }
}

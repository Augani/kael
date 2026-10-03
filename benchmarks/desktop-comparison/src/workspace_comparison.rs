//! Real persistent docking comparison, including tabs, nested splits and zoom.
//! Floating windows/edge docks are excluded: these pinned public APIs differ.
#![recursion_limit = "256"]
#[cfg(all(feature = "kael-engine", feature = "gpui-kit-engine"))]
compile_error!("select exactly one framework engine");
#[cfg(feature = "gpui-kit-engine")]
use gpui_kit as ui;
#[cfg(feature = "gpui-kit-engine")]
use gpui_kit::component::dock::{
    BasePanel, DockArea, DockAreaState, DockLayout, DockPlacement, DockSkin, InsertTarget, NodeId,
    Panel, PanelEvent, PanelId, PanelInfo, PanelState, panel_handle, register_panel,
};
#[cfg(feature = "kael-engine")]
use kael as ui;
#[cfg(feature = "kael-engine")]
use kael_ui::components::workspace::{
    DockAxis, DockGroup, DockLayout, DockNode, DockPane, DockPlacement, DockWorkspace,
    DockWorkspaceState,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use ui::{prelude::*, *};
mod workload_metrics;
use workload_metrics::*;
const CONTRACT: &str = "native-dock-workspace-v1";
const PANES: usize = 12;
const BODY_LINES: usize = 32;
const AREA_ID: &str = "comparison-workspace";
#[derive(Clone, Copy)]
struct Options {
    quick: bool,
    compact: bool,
}
impl Options {
    fn parse() -> Self {
        let mut quick = false;
        let mut compact = false;
        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--quick" => quick = true,
                "--quick-compact" => {
                    quick = true;
                    compact = true;
                }
                _ => panic!("unknown flag {arg}; use --quick or --quick-compact"),
            }
        }
        Self { quick, compact }
    }
}
#[derive(Clone)]
struct PaneData {
    id: String,
    title: SharedString,
    lines: Arc<[SharedString]>,
    hash: String,
}
struct Fixture {
    panes: Arc<[PaneData]>,
    generation: usize,
    hash: String,
    bytes: usize,
}
impl Fixture {
    fn new(generation: usize) -> Arc<Self> {
        let panes = (0..PANES)
            .map(|id| {
                let lines: Arc<[SharedString]> = (0..BODY_LINES)
                    .map(|line| {
                        SharedString::from(format!(
                            "Pane {id:02} / line {line:02} · 日本語 👩‍💻 café · rev {generation:02}"
                        ))
                    })
                    .collect::<Vec<_>>()
                    .into();
                let hash = hash_fields(lines.iter().map(|line| line.as_ref()));
                PaneData {
                    id: pane_id(id),
                    title: format!("Pane {id:02} 日本語").into(),
                    lines,
                    hash,
                }
            })
            .collect::<Vec<_>>();
        let bytes = panes
            .iter()
            .map(|pane| {
                pane.id.len()
                    + pane.title.len()
                    + pane.lines.iter().map(|line| line.len()).sum::<usize>()
            })
            .sum();
        let hash = hash_fields(panes.iter().flat_map(|pane| {
            std::iter::once(pane.id.as_str())
                .chain(std::iter::once(pane.title.as_ref()))
                .chain(pane.lines.iter().map(|line| line.as_ref()))
        }));
        Arc::new(Self {
            panes: panes.into(),
            generation,
            hash,
            bytes,
        })
    }
}
fn pane_id(id: usize) -> String {
    format!("pane-{id:02}")
}
type Source = Rc<RefCell<Arc<Fixture>>>;
type PaintProbe = Rc<RefCell<HashMap<String, (usize, String)>>>;
fn pane_content(id: usize, source: &Source, probe: &PaintProbe) -> AnyElement {
    let fixture = source.borrow();
    let pane = &fixture.panes[id];
    probe
        .borrow_mut()
        .insert(pane.id.clone(), (fixture.generation, pane.hash.clone()));
    div()
        .size_full()
        .overflow_hidden()
        .p(px(12.0))
        .font_family(FONT_FAMILY)
        .text_size(px(FONT_SIZE))
        .line_height(px(LINE_HEIGHT))
        .text_color(rgb(0xe2e6ed))
        .bg(rgb(0x11151b))
        .flex()
        .flex_col()
        .child(div().h(px(28.0)).flex_shrink_0().child(pane.title.clone()))
        .children(
            pane.lines
                .iter()
                .map(|line| div().h(px(LINE_HEIGHT)).flex_shrink_0().child(line.clone())),
        )
        .into_any_element()
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum SemanticLayout {
    Tabs {
        panes: Vec<String>,
        active: String,
    },
    Split {
        axis: String,
        children: Vec<SemanticLayout>,
    },
}
impl SemanticLayout {
    fn base() -> Self {
        let tabs = |first: usize| Self::Tabs {
            panes: (first..first + 4).map(pane_id).collect(),
            active: pane_id(first),
        };
        Self::Split {
            axis: "horizontal".into(),
            children: vec![
                tabs(0),
                Self::Split {
                    axis: "vertical".into(),
                    children: vec![tabs(4), tabs(8)],
                },
            ],
        }
    }
    fn tabs_for(&mut self, id: &str) -> Option<(&mut Vec<String>, &mut String)> {
        match self {
            Self::Tabs { panes, active } if panes.iter().any(|pane| pane == id) => {
                Some((panes, active))
            }
            Self::Split { children, .. } => {
                children.iter_mut().find_map(|child| child.tabs_for(id))
            }
            _ => None,
        }
    }
    fn remove(&mut self, id: &str) -> bool {
        match self {
            Self::Tabs { panes, active } => {
                let Some(index) = panes.iter().position(|pane| pane == id) else {
                    return false;
                };
                panes.remove(index);
                if active == id {
                    *active = panes
                        .get(index.min(panes.len().saturating_sub(1)))
                        .cloned()
                        .unwrap_or_default();
                }
                true
            }
            Self::Split { children, .. } => children.iter_mut().any(|child| child.remove(id)),
        }
    }
    fn normalize(&mut self) {
        if let Self::Split { axis, children } = self {
            for child in children.iter_mut() {
                child.normalize();
            }
            children.retain(|child| !matches!(child,Self::Tabs{panes,..}if panes.is_empty()));
            let mut normalized = Vec::new();
            for child in std::mem::take(children) {
                match child {
                    Self::Split {
                        axis: child_axis,
                        children: grandchildren,
                    } if child_axis == *axis => normalized.extend(grandchildren),
                    other => normalized.push(other),
                }
            }
            *children = normalized;
            if children.len() == 1 {
                *self = children.remove(0);
            }
        }
    }
    fn move_tab(&mut self, pane: &str, target: &str, index: usize) {
        assert!(self.remove(pane));
        self.normalize();
        let (panes, active) = self.tabs_for(target).unwrap();
        panes.insert(index.min(panes.len()), pane.to_string());
        *active = pane.to_string();
    }
    fn split_after(&mut self, pane: &str, target: &str) {
        assert!(self.remove(pane));
        self.normalize();
        fn insert(layout: &mut SemanticLayout, pane: &str, target: &str) -> bool {
            match layout {
                SemanticLayout::Tabs { panes, .. } if panes.iter().any(|id| id == target) => {
                    let old = layout.clone();
                    *layout = SemanticLayout::Split {
                        axis: "vertical".into(),
                        children: vec![
                            old,
                            SemanticLayout::Tabs {
                                panes: vec![pane.to_string()],
                                active: pane.to_string(),
                            },
                        ],
                    };
                    true
                }
                SemanticLayout::Split { children, .. } => {
                    children.iter_mut().any(|child| insert(child, pane, target))
                }
                _ => false,
            }
        }
        assert!(insert(self, pane, target));
        self.normalize();
    }
    fn active_panes(&self) -> Vec<String> {
        match self {
            Self::Tabs { active, .. } => vec![active.clone()],
            Self::Split { children, .. } => children.iter().flat_map(Self::active_panes).collect(),
        }
    }
    fn pane_ids(&self) -> Vec<String> {
        match self {
            Self::Tabs { panes, .. } => panes.clone(),
            Self::Split { children, .. } => children.iter().flat_map(Self::pane_ids).collect(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Persisted {
    version: u32,
    generation: usize,
    fixture_hash: String,
    layout: serde_json::Value,
}
struct PersistenceProof {
    before: String,
    after: String,
}

#[cfg(feature = "gpui-kit-engine")]
struct BenchPanel {
    id: usize,
    source: Source,
    probe: PaintProbe,
    focus: FocusHandle,
}
#[cfg(feature = "gpui-kit-engine")]
impl EventEmitter<PanelEvent> for BenchPanel {}
#[cfg(feature = "gpui-kit-engine")]
impl Focusable for BenchPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
#[cfg(feature = "gpui-kit-engine")]
impl BasePanel for BenchPanel {
    fn panel_name(&self) -> &'static str {
        "comparison-pane"
    }
    fn dump(&self, _: &App) -> PanelState {
        let fixture = self.source.borrow();
        PanelState {
            panel_name: self.panel_name().into(),
            children: vec![],
            info: PanelInfo::panel(
                serde_json::json!({"id":self.id,"generation":fixture.generation,"content_hash":fixture.panes[self.id].hash}),
            ),
        }
    }
}
#[cfg(feature = "gpui-kit-engine")]
impl Panel for BenchPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.source.borrow().panes[self.id].title.clone()
    }
}
#[cfg(feature = "gpui-kit-engine")]
impl Render for BenchPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        pane_content(self.id, &self.source, &self.probe)
    }
}
#[cfg(feature = "kael-engine")]
type Control = DockWorkspaceState;
#[cfg(feature = "gpui-kit-engine")]
type Control = DockArea;

#[cfg(feature = "kael-engine")]
fn initial_layout() -> DockLayout {
    let group = |id, first| {
        DockNode::Group(DockGroup {
            id,
            panes: (first..first + 4).map(pane_id).collect(),
            active: pane_id(first),
        })
    };
    DockLayout {
        version: 1,
        root: Some(DockNode::Split {
            id: 1,
            axis: DockAxis::Horizontal,
            ratio: 0.5,
            first: Box::new(group(2, 0)),
            second: Box::new(DockNode::Split {
                id: 3,
                axis: DockAxis::Vertical,
                ratio: 0.5,
                first: Box::new(group(4, 4)),
                second: Box::new(group(5, 8)),
            }),
        }),
        floating: vec![],
        closed: vec![],
        zoomed: None,
    }
}
fn make_control(
    source: Source,
    probe: PaintProbe,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Control> {
    #[cfg(feature = "kael-engine")]
    {
        let _ = window;
        let panes = (0..PANES)
            .map(|id| {
                let source = source.clone();
                let probe = probe.clone();
                let title = source.borrow().panes[id].title.clone();
                DockPane::new(pane_id(id), title, move |_, _| {
                    pane_content(id, &source, &probe)
                })
            })
            .collect();
        cx.new(|cx| DockWorkspaceState::new(panes, initial_layout(), cx).unwrap())
    }
    #[cfg(feature = "gpui-kit-engine")]
    {
        let restore_source = source.clone();
        let restore_probe = probe.clone();
        register_panel(cx, "comparison-pane", move |context, _, cx| {
            let PanelInfo::Panel(info) = context.info() else {
                panic!("persisted native panel payload");
            };
            let id = info["id"].as_u64().unwrap() as usize;
            assert!(id < PANES);
            let source = restore_source.clone();
            let probe = restore_probe.clone();
            panel_handle(cx.new(|cx| BenchPanel {
                id,
                source,
                probe,
                focus: cx.focus_handle(),
            }))
        });
        let (area, _skin) = DockSkin::dock_area(AREA_ID, Some(1), window, cx);
        let panels = (0..PANES)
            .map(|id| {
                let source = source.clone();
                let probe = probe.clone();
                cx.new(|cx| BenchPanel {
                    id,
                    source,
                    probe,
                    focus: cx.focus_handle(),
                })
            })
            .collect::<Vec<_>>();
        let tabs = |first: usize, cx: &App| {
            (first..first + 4).fold(DockLayout::tabs(), |layout, id| {
                layout.panel_view(panel_handle(panels[id].clone()), cx)
            })
        };
        let layout = DockLayout::h_split().child(tabs(0, cx), None).child(
            DockLayout::v_split()
                .child(tabs(4, cx), None)
                .child(tabs(8, cx), None),
            None,
        );
        area.update(cx, |area, cx| area.set_center(layout, window, cx));
        area
    }
}
#[cfg(feature = "kael-engine")]
fn kael_group(layout: &DockLayout, pane: &str) -> u64 {
    fn find(node: &DockNode, pane: &str) -> Option<u64> {
        match node {
            DockNode::Group(group) => group.panes.iter().any(|id| id == pane).then_some(group.id),
            DockNode::Split { first, second, .. } => {
                find(first, pane).or_else(|| find(second, pane))
            }
        }
    }
    find(layout.root.as_ref().unwrap(), pane).unwrap()
}
#[cfg(feature = "gpui-kit-engine")]
fn gpui_ids(area: &DockArea, pane: &str, cx: &App) -> (PanelId, NodeId) {
    let tree = area.layout(DockPlacement::Center).unwrap();
    let panel=tree.panels().find(|panel|{let state=area.panel(*panel).unwrap().dump(cx);matches!(state.info,PanelInfo::Panel(info)if pane_id(info["id"].as_u64().unwrap()as usize)==pane)}).unwrap();
    (panel, tree.find_panel_node(panel).unwrap())
}
#[cfg(feature = "kael-engine")]
fn semantic_kael(node: &DockNode) -> SemanticLayout {
    match node {
        DockNode::Group(group) => SemanticLayout::Tabs {
            panes: group.panes.clone(),
            active: group.active.clone(),
        },
        DockNode::Split {
            axis,
            first,
            second,
            ..
        } => SemanticLayout::Split {
            axis: if *axis == DockAxis::Horizontal {
                "horizontal"
            } else {
                "vertical"
            }
            .into(),
            children: vec![semantic_kael(first), semantic_kael(second)],
        },
    }
}
#[cfg(feature = "gpui-kit-engine")]
fn semantic_gpui(state: &PanelState) -> SemanticLayout {
    match &state.info {
        PanelInfo::Tabs { active_index } => {
            let panes = state
                .children
                .iter()
                .map(|child| {
                    let PanelInfo::Panel(info) = &child.info else {
                        panic!("native tab leaf")
                    };
                    pane_id(info["id"].as_u64().unwrap() as usize)
                })
                .collect::<Vec<_>>();
            SemanticLayout::Tabs {
                active: panes[*active_index].clone(),
                panes,
            }
        }
        PanelInfo::Stack { axis, .. } => SemanticLayout::Split {
            axis: if *axis == 0 { "horizontal" } else { "vertical" }.into(),
            children: state.children.iter().map(semantic_gpui).collect(),
        },
        _ => panic!("expected native tab or split"),
    }
}
fn semantic_json(value: serde_json::Value) -> SemanticLayout {
    #[cfg(feature = "kael-engine")]
    let mut semantic = {
        let layout: DockLayout = serde_json::from_value(value).unwrap();
        assert!(layout.floating.is_empty() && layout.closed.is_empty());
        semantic_kael(layout.root.as_ref().unwrap())
    };
    #[cfg(feature = "gpui-kit-engine")]
    let mut semantic = {
        let state: DockAreaState = serde_json::from_value(value).unwrap();
        assert!(
            state.left_dock.is_none() && state.right_dock.is_none() && state.bottom_dock.is_none()
        );
        semantic_gpui(&state.center)
    };
    semantic.normalize();
    semantic
}
fn json_close(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a, b) {
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => a
            .as_f64()
            .zip(b.as_f64())
            .is_some_and(|(a, b)| (a - b).abs() <= 0.01),
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| json_close(a, b))
        }
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| json_close(a, b)))
        }
        _ => a == b,
    }
}
struct Workload {
    control: Entity<Control>,
    _observer: Subscription,
    source: Source,
    probe: PaintProbe,
    fixtures: [Arc<Fixture>; 2],
    options: Options,
    expected: SemanticLayout,
    root_ratio: f32,
    zoomed: bool,
    sequence: usize,
    operations: [u64; 9],
    proofs: Vec<PersistenceProof>,
    metrics: Metrics,
}
impl Workload {
    fn new(
        started: Instant,
        options: Options,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let fixtures = [Fixture::new(0), Fixture::new(1)];
        let source = Rc::new(RefCell::new(fixtures[0].clone()));
        let probe = Rc::new(RefCell::new(HashMap::with_capacity(PANES)));
        let control = make_control(source.clone(), probe.clone(), window, cx);
        let observer = cx.observe(&control, |_, _, cx| cx.notify());
        Self {
            control,
            _observer: observer,
            source,
            probe,
            fixtures,
            options,
            expected: SemanticLayout::base(),
            root_ratio: 0.5,
            zoomed: false,
            sequence: 0,
            operations: [0; 9],
            proofs: Vec::with_capacity(256),
            metrics: Metrics::new(started),
        }
    }
    fn native_json(&self, cx: &App) -> serde_json::Value {
        #[cfg(feature = "kael-engine")]
        {
            serde_json::to_value(self.control.read(cx).layout()).unwrap()
        }
        #[cfg(feature = "gpui-kit-engine")]
        {
            serde_json::to_value(self.control.read(cx).dump(cx)).unwrap()
        }
    }
    fn persisted_json(&self, cx: &App) -> String {
        let fixture = self.source.borrow();
        serde_json::to_string(&Persisted {
            version: 1,
            generation: fixture.generation,
            fixture_hash: fixture.hash.clone(),
            layout: self.native_json(cx),
        })
        .unwrap()
    }
    fn restore(&mut self, json: &str, window: &mut Window, cx: &mut Context<Self>) {
        let state: Persisted = serde_json::from_str(json).unwrap();
        assert_eq!(state.version, 1);
        assert_eq!(state.fixture_hash, self.fixtures[state.generation].hash);
        *self.source.borrow_mut() = self.fixtures[state.generation].clone();
        self.control.update(cx, |control, cx| {
            #[cfg(feature = "kael-engine")]
            {
                let _ = window;
                control
                    .restore_json(&serde_json::to_string(&state.layout).unwrap(), cx)
                    .unwrap();
            }
            #[cfg(feature = "gpui-kit-engine")]
            {
                control
                    .load(serde_json::from_value(state.layout).unwrap(), window, cx)
                    .unwrap();
            }
        });
    }
    fn tab(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.control.update(cx, |control, cx| {
            #[cfg(feature = "kael-engine")]
            {
                let _ = window;
                let group = kael_group(control.layout(), id);
                assert!(control.activate(group, id, cx));
            }
            #[cfg(feature = "gpui-kit-engine")]
            {
                let (panel, _) = gpui_ids(control, id, cx);
                control.select_panel(panel, window, cx);
            }
        });
        let (_, active) = self.expected.tabs_for(id).unwrap();
        *active = id.to_string();
    }
    fn move_tab(
        &mut self,
        pane: &str,
        target: &str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.control.update(cx, |control, cx| {
            #[cfg(feature = "kael-engine")]
            {
                let _ = window;
                let target = kael_group(control.layout(), target);
                assert!(control.move_pane(pane, target, DockPlacement::Tab(index), cx));
            }
            #[cfg(feature = "gpui-kit-engine")]
            {
                let (panel, _) = gpui_ids(control, pane, cx);
                let (_, node) = gpui_ids(control, target, cx);
                control.move_panel(
                    panel,
                    InsertTarget::Tabs {
                        node,
                        ix: Some(index),
                        activate: true,
                    },
                    window,
                    cx,
                );
            }
        });
        self.expected.move_tab(pane, target, index);
    }
    fn split(&mut self, pane: &str, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.control.update(cx, |control, cx| {
            #[cfg(feature = "kael-engine")]
            {
                let _ = window;
                let target = kael_group(control.layout(), target);
                assert!(control.move_pane(pane, target, DockPlacement::Bottom, cx));
            }
            #[cfg(feature = "gpui-kit-engine")]
            {
                let (panel, _) = gpui_ids(control, pane, cx);
                let (_, node) = gpui_ids(control, target, cx);
                control.move_panel(
                    panel,
                    InsertTarget::Split {
                        node,
                        placement: gpui_kit::base::Placement::Bottom,
                        size: None,
                    },
                    window,
                    cx,
                );
            }
        });
        self.expected.split_after(pane, target);
    }
    fn resize(&mut self, ratio: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.control.update(cx, |control, cx| {
            #[cfg(feature = "kael-engine")]
            {
                let _ = window;
                let DockNode::Split { id, .. } = control.layout().root.as_ref().unwrap() else {
                    panic!("root split")
                };
                assert!(control.set_split_ratio(*id, ratio, cx));
            }
            #[cfg(feature = "gpui-kit-engine")]
            {
                let root = control.layout(DockPlacement::Center).unwrap().root().id();
                let state = control.dump(cx);
                let PanelInfo::Stack { sizes, .. } = &state.center.info else {
                    panic!("root split")
                };
                let total = sizes.iter().map(|size| f32::from(*size)).sum::<f32>();
                assert!(
                    total.is_finite() && total > 0.0,
                    "native root split not measured"
                );
                control.set_split_sizes(
                    root,
                    vec![px(total * ratio), px(total * (1.0 - ratio))],
                    window,
                    cx,
                );
            }
        });
        self.root_ratio = ratio;
    }
    fn zoom(&mut self, zoom: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.control.update(cx, |control, cx| {
            #[cfg(feature = "kael-engine")]
            {
                let _ = window;
                let group = kael_group(control.layout(), &pane_id(0));
                assert!(control.toggle_zoom(group, cx));
            }
            #[cfg(feature = "gpui-kit-engine")]
            {
                if zoom {
                    let (_, node) = gpui_ids(control, &pane_id(0), cx);
                    control.set_zoomed_in(node, window, cx);
                } else {
                    control.set_zoomed_out(window, cx);
                }
            }
        });
        self.zoomed = zoom;
    }
    fn set_phase(&mut self, phase: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.metrics.set_phase(phase, window);
        // Keep the mutation sequence continuous across phases; a phase cannot
        // start halfway through a split/merge cycle with assumptions reset.
        if phase.starts_with("idle") {
            #[cfg(feature = "kael-engine")]
            window.blur();
            #[cfg(feature = "gpui-kit-engine")]
            window.blur(cx);
        }
        cx.notify();
        window.refresh();
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.metrics.collect(window);
        let started = Instant::now();
        if self.metrics.phase == "churn" && self.sequence.is_multiple_of(32) {
            let generation = 1 - self.source.borrow().generation;
            *self.source.borrow_mut() = self.fixtures[generation].clone();
            // Both controls invalidate their real layout/content through the
            // native persistence API. GPUI rebuilds registered panel entities;
            // Kael reconciles its retained pane registry. The difference is real.
            let snapshot = self.persisted_json(cx);
            self.restore(&snapshot, window, cx);
            self.zoomed = false;
            self.operations[8] += 1;
        }
        let operation = self.sequence % 8;
        match operation {
            0 => self.tab(&pane_id(1 + self.sequence / 8 % 2), window, cx),
            1 => self.move_tab(&pane_id(3), &pane_id(4), 0, window, cx),
            2 => self.split(&pane_id(3), &pane_id(8), window, cx),
            3 => self.move_tab(&pane_id(3), &pane_id(0), 3, window, cx),
            4 => self.resize(
                if (self.sequence / 8).is_multiple_of(2) {
                    0.35
                } else {
                    0.65
                },
                window,
                cx,
            ),
            5 => self.zoom(true, window, cx),
            6 => self.zoom(false, window, cx),
            7 => {
                let before = self.persisted_json(cx);
                self.restore(&before, window, cx);
                let after = self.persisted_json(cx);
                if self.proofs.len() < 256 {
                    self.proofs.push(PersistenceProof { before, after });
                }
            }
            _ => unreachable!(),
        }
        self.operations[operation] += 1;
        self.sequence += 1;
        self.metrics.operation(started);
        cx.notify();
        window.refresh();
    }
    fn check_phase(&mut self, cx: &mut Context<Self>) {
        let native = self.native_json(cx);
        let actual = semantic_json(native.clone());
        assert_eq!(
            actual, self.expected,
            "actual dock tree/tab order/active panels"
        );
        let mut ids = actual.pane_ids();
        ids.sort();
        assert_eq!(ids, (0..PANES).map(pane_id).collect::<Vec<_>>());
        let fixture = self.source.borrow();
        let (mut active, actual_zoomed, ratio) = {
            #[cfg(feature = "kael-engine")]
            {
                let layout = self.control.read(cx).layout();
                let DockNode::Split { ratio, .. } = layout.root.as_ref().unwrap() else {
                    panic!("root split")
                };
                (actual.active_panes(), layout.zoomed.is_some(), *ratio)
            }
            #[cfg(feature = "gpui-kit-engine")]
            {
                let state = self.control.read(cx).dump(cx);
                let PanelInfo::Stack { sizes, .. } = &state.center.info else {
                    panic!("root split")
                };
                let ratio =
                    f32::from(sizes[0]) / sizes.iter().map(|size| f32::from(*size)).sum::<f32>();
                (
                    actual.active_panes(),
                    self.control.read(cx).is_zoomed(),
                    ratio,
                )
            }
        };
        assert_eq!(actual_zoomed, self.zoomed);
        assert!(
            (ratio - self.root_ratio).abs() < 0.005,
            "native persisted resize geometry: actual={ratio} expected={} native={native}",
            self.root_ratio
        );
        if self.zoomed {
            let (_, active_pane) = self.expected.tabs_for(&pane_id(0)).unwrap();
            active = vec![active_pane.clone()];
        }
        for pane in &active {
            let id = ids.iter().position(|id| id == pane).unwrap();
            let painted = self.probe.borrow();
            let (generation, hash) = painted
                .get(pane)
                .expect("active content passed through actual native pane renderer");
            assert_eq!(*generation, fixture.generation);
            assert_eq!(*hash, fixture.panes[id].hash);
        }
        for proof in &self.proofs {
            let before: Persisted = serde_json::from_str(&proof.before).unwrap();
            let after: Persisted = serde_json::from_str(&proof.after).unwrap();
            assert_eq!(before.generation, after.generation);
            assert_eq!(before.fixture_hash, after.fixture_hash);
            assert_eq!(
                semantic_json(before.layout.clone()),
                semantic_json(after.layout.clone())
            );
            assert!(
                json_close(&before.layout, &after.layout),
                "native persisted state round trip; at most0.01px float tolerance"
            );
        }
        self.metrics
            .checks
            .push((self.metrics.phase.to_string(), true));
        self.metrics.oracles.push(serde_json::json!({"phase":self.metrics.phase,"correct":true,"fixture_generation":fixture.generation,"fixture_hash":fixture.hash,
            "actual_layout":actual,"verified_pane_ids":ids,"verified_active_content":active,"native_zoomed":actual_zoomed,"root_split_ratio":ratio,
            "persistence_roundtrips_verified":self.proofs.len(),"native_layout_json_bytes":serde_json::to_vec(&native).unwrap().len(),"content_verified":true}));
    }
    fn report(&self) {
        self.metrics.report(serde_json::json!({"contract":CONTRACT,"quick":self.options.quick,"rows":PANES,"pane_count":PANES,"body_lines_per_pane":BODY_LINES,
            "fixture_hash":self.fixtures[0].hash,"fixture_bytes":self.fixtures[0].bytes,"fixture_retained_datasets":2,"workspace_width_px":WINDOW_WIDTH,"workspace_height_px":CONTENT_HEIGHT,
            "fixture_hashes":[self.fixtures[0].hash,self.fixtures[1].hash],
            "replacement_interval_updates":32,
            "initial_tab_groups":3,"initial_split_count":2,"initial_split_axes":["horizontal","vertical"],"initial_split_ratios":[0.5,0.5],
            "component":if cfg!(feature="kael-engine"){"DockWorkspace"}else{"DockArea+DockSkin"},"native_chrome_geometry":true,"floating_panes":false,"edge_docks":false,
            "operations":{"select_tab":self.operations[0],"move_tab":self.operations[1],"split_pane":self.operations[2],"merge_pane":self.operations[3],"resize_split":self.operations[4],"zoom_group":self.operations[5],"unzoom_group":self.operations[6],"serialize_restore":self.operations[7],"replace_model":self.operations[8]},
            "timing_scope":"native shipped persistent dock controllers; each public layout edit paced separately; JSON encode/decode/native restore included",
            "oracle_scope":"actual native pane IDs/order/active groups/split orientations/zoom/resize plus rendered active Unicode body revision; complete native JSON roundtrip assertions outside timing",
            "model_replacement_scope":"alternate two retained Unicode fixtures and restore current real native layout to invalidate pane models",
            "differences":["Kael uses retained application pane closures; GPUI Kit persistence reconstructs registered Panel entities",
                "Kael binary splits and GPUI Kit normalized n-ary splits are compared by ordered semantic layout with same-axis normalization",
                "Kael tab strip minimum34px; pinned DockSkin native tab bar30px; retain native chrome and disclose content-height difference",
                "Native split handle/minimum-size rules differ; common requested root ratio checked within0.005, persistence float tolerance0.01px",
                "GPUI Kit does not serialize zoom state, so persistence is performed after unzoom; floating panes and edge docks excluded"]}));
    }
}
impl Render for Workload {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.metrics.rendered();
        #[cfg(feature = "kael-engine")]
        let control = DockWorkspace::new(AREA_ID, self.control.clone()).into_any_element();
        #[cfg(feature = "gpui-kit-engine")]
        let control = self.control.clone().into_any_element();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x11151b))
            .text_color(rgb(0xe2e6ed))
            .font_family(FONT_FAMILY)
            .text_size(px(FONT_SIZE))
            .line_height(px(LINE_HEIGHT))
            .child(
                div()
                    .h(px(56.0))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child("Persistent Unicode workspace")
                    .child(format!("{} · 12 panes · {}", engine(), self.metrics.phase)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .overflow_hidden()
                    .child(control),
            )
    }
}
fn launch(started: Instant, options: Options, cx: &mut App) {
    #[cfg(feature = "kael-engine")]
    {
        kael_ui::init(cx);
        let mut theme = kael_ui::theme::Theme::astryx_neutral_dark();
        theme.tokens.font_family = FONT_FAMILY.into();
        kael_ui::theme::install_theme(cx, theme);
    }
    #[cfg(feature = "gpui-kit-engine")]
    {
        gpui_kit::init(cx);
        gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
        gpui_kit::component::Theme::global_mut(cx).font_family = FONT_FAMILY.into();
    }
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(
                    px(if options.compact { 880.0 } else { WINDOW_WIDTH }),
                    px(if options.compact {
                        640.0
                    } else {
                        WINDOW_HEIGHT
                    }),
                ),
                cx,
            ))),
            ..Default::default()
        },
        |window, cx| {
            window.set_window_title("Native persistent workspace comparison");
            let view = cx.new(|cx| Workload::new(started, options, window, cx));
            let weak = view.downgrade();
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    // AppKit can constrain the requested window to the screen.
                    // Normalize the initial split against its measured native
                    // container, before any timed idle/interaction phase.
                    weak.update_in(cx, |view, window, cx| view.resize(0.5, window, cx))
                        .unwrap();
                    cx.background_executor()
                        .timer(Duration::from_millis(40))
                        .await;
                    for phase in PHASES {
                        weak.update_in(cx, |view, window, cx| view.set_phase(phase, window, cx))
                            .unwrap();
                        marker(phase, started);
                        if phase.starts_with("idle") {
                            cx.background_executor()
                                .timer(phase_duration(options.quick, phase))
                                .await;
                        } else {
                            let until = Instant::now() + phase_duration(options.quick, phase);
                            let mut updates = 0;
                            while Instant::now() < until || updates < 8 {
                                cx.background_executor()
                                    .timer(Duration::from_micros(16_667))
                                    .await;
                                weak.update_in(cx, |view, window, cx| view.tick(window, cx))
                                    .unwrap();
                                updates += 1;
                            }
                        }
                        marker("validation", started);
                        weak.update_in(cx, |view, window, _| view.metrics.validation(window))
                            .unwrap();
                        cx.background_executor()
                            .timer(Duration::from_millis(40))
                            .await;
                        weak.update_in(cx, |view, _, cx| view.check_phase(cx))
                            .unwrap();
                    }
                    marker("finished", started);
                    weak.update_in(cx, |view, window, _| {
                        view.metrics.collect(window);
                        view.metrics.snapshot_gpu();
                        view.report();
                    })
                    .unwrap();
                    cx.update(|_, cx| cx.quit()).unwrap();
                })
                .detach();
            view
        },
    )
    .expect("native workspace comparison window");
    cx.activate(true);
}
fn main() {
    let options = Options::parse();
    let started = Instant::now();
    #[cfg(feature = "kael-engine")]
    Application::try_new()
        .expect("native Kael platform")
        .run(move |cx| launch(started, options, cx));
    #[cfg(feature = "gpui-kit-engine")]
    {
        #[cfg(feature = "frame-timing")]
        gpui_kit::profiler::set_trace_enabled(true);
        gpui_kit::application().run(move |cx| launch(started, options, cx));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn semantic_workflow_keeps_exact_panes_and_returns_from_split_to_tabs() {
        let mut layout = SemanticLayout::base();
        layout.move_tab(&pane_id(3), &pane_id(4), 0);
        layout.split_after(&pane_id(3), &pane_id(8));
        assert_eq!(layout.active_panes().len(), 4);
        layout.move_tab(&pane_id(3), &pane_id(0), 3);
        assert_eq!(layout.active_panes().len(), 3);
        let mut panes = layout.pane_ids();
        panes.sort();
        assert_eq!(panes, (0..PANES).map(pane_id).collect::<Vec<_>>());
    }
    #[::core::prelude::v1::test]
    fn fixture_has_all_unicode_panels_and_generation_changes_only_body() {
        let first = Fixture::new(0);
        let second = Fixture::new(1);
        assert_ne!(first.hash, second.hash);
        assert_eq!(first.panes.len(), PANES);
        for (a, b) in first.panes.iter().zip(second.panes.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.title, b.title);
            assert_ne!(a.hash, b.hash);
            assert_eq!(a.lines.len(), BODY_LINES);
        }
    }
    #[::core::prelude::v1::test]
    fn production_fixture_exact_bytes_and_hash_are_pinned() {
        let first = Fixture::new(0);
        let second = Fixture::new(1);
        assert_eq!(first.bytes, 22560);
        assert_eq!(first.hash, "fnv1a64:8575d5bda29bda91");
        assert_eq!(second.hash, "fnv1a64:42c3940aeb810ba1");
    }
}

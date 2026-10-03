//! Matched native virtual-tree navigation using the shipped public controls.
//! Collapse/expand runs through GPUI Kit's routed tree actions and Kael's
//! controlled immutable-model API. Their preparation costs are disclosed.
#![recursion_limit = "256"]
#[cfg(all(feature = "kael-engine", feature = "gpui-kit-engine"))]
compile_error!("select exactly one framework engine");
#[cfg(feature = "gpui-kit-engine")]
use gpui_kit as ui;
#[cfg(feature = "gpui-kit-engine")]
use gpui_kit::component::{
    list::ListItem,
    tree::{Tree, TreeItem, TreeState},
};
#[cfg(feature = "kael-engine")]
use kael as ui;
#[cfg(feature = "kael-engine")]
use kael_ui::navigation::{
    tree::{TreeListDensity, TreeNode},
    virtual_tree::{VirtualTreeList, VirtualTreeModel, VirtualTreeState},
};
#[cfg(feature = "kael-engine")]
use std::collections::HashSet;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use ui::{prelude::*, *};
mod workload_metrics;
use workload_metrics::*;

const CONTRACT: &str = "native-virtual-tree-v1";
const ROOTS: usize = 25;
const TREE_WIDTH: f32 = 780.0;
const ROW_HEIGHT: f32 = 28.0;
const INDENT: f32 = 14.0;
#[cfg(feature = "kael-engine")]
const TREE_LABEL: &str = "Unicode project navigation";
#[derive(Clone, Copy)]
struct Options {
    children: usize,
    quick: bool,
}
impl Options {
    fn parse() -> Self {
        let mut options = Self {
            children: 4000,
            quick: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--quick" => options.quick = true,
                "--children" => {
                    options.children = args
                        .next()
                        .expect("--children needs a count")
                        .parse()
                        .expect("positive count")
                }
                _ => panic!("unknown flag {arg}; use --quick or --children N"),
            }
        }
        assert!((1..=40000).contains(&options.children));
        options
    }
    fn nodes(self) -> usize {
        ROOTS * (self.children + 1)
    }
}
#[derive(Clone)]
struct Record {
    id: SharedString,
    label: SharedString,
    depth: usize,
}
struct Fixture {
    records: Arc<[Record]>,
    generation: usize,
    hash: String,
    bytes: usize,
}
impl Fixture {
    fn new(children: usize, generation: usize) -> Arc<Self> {
        let mut records = Vec::with_capacity(ROOTS * (children + 1));
        for root in 0..ROOTS {
            records.push(Record {
                id: root_id(root).into(),
                label: format!("Project {root:02} · 日本語 café").into(),
                depth: 0,
            });
            for child in 0..children {
                records.push(Record {
                    id: format!("project/{root:02}/file/{child:04}").into(),
                    label: format!("File {child:04} · 👩‍💻 naïve · rev {generation:02}").into(),
                    depth: 1,
                });
            }
        }
        let bytes = records
            .iter()
            .map(|record| record.id.len() + record.label.len())
            .sum();
        let hash = records_hash(records.iter());
        Arc::new(Self {
            records: records.into(),
            generation,
            hash,
            bytes,
        })
    }
}
fn root_id(root: usize) -> String {
    format!("project/{root:02}")
}
fn records_hash<'a>(records: impl IntoIterator<Item = &'a Record>) -> String {
    let fields = records.into_iter().flat_map(|record| {
        [
            record.id.as_ref(),
            record.label.as_ref(),
            if record.depth == 0 { "0" } else { "1" },
        ]
    });
    hash_fields(fields)
}
fn visible_records(
    fixture: &Fixture,
    children: usize,
    collapsed: Option<usize>,
) -> impl Iterator<Item = &Record> {
    fixture
        .records
        .iter()
        .enumerate()
        .filter_map(move |(index, record)| {
            (!(record.depth == 1 && collapsed == Some(index / (children + 1)))).then_some(record)
        })
}
fn visible_len(children: usize, collapsed: Option<usize>) -> usize {
    ROOTS * (children + 1) - if collapsed.is_some() { children } else { 0 }
}
fn visible_record_index(mut index: usize, children: usize, collapsed: Option<usize>) -> usize {
    for root in 0..ROOTS {
        let count = if collapsed == Some(root) {
            1
        } else {
            children + 1
        };
        if index < count {
            return root * (children + 1) + index;
        }
        index -= count;
    }
    panic!("visible index outside fixture")
}
#[cfg(feature = "kael-engine")]
type Control = VirtualTreeState<SharedString>;
#[cfg(feature = "gpui-kit-engine")]
type Control = TreeState;
type RowProbe = Rc<RefCell<Vec<usize>>>;

struct Workload {
    control: Entity<Control>,
    _observer: Subscription,
    fixtures: [Arc<Fixture>; 2],
    options: Options,
    generation: usize,
    collapsed: Option<usize>,
    selected: SharedString,
    scroll_target: SharedString,
    sequence: usize,
    operations: [u64; 7],
    probe: RowProbe,
    validation_painted: bool,
    metrics: Metrics,
    #[cfg(feature = "kael-engine")]
    model: VirtualTreeModel<SharedString>,
}
impl Workload {
    fn new(
        started: Instant,
        options: Options,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let fixtures = [
            Fixture::new(options.children, 0),
            Fixture::new(options.children, 1),
        ];
        let control = cx.new(|cx| Control::new(cx));
        let observer = cx.observe(&control, |_, _, cx| cx.notify());
        #[cfg(feature = "kael-engine")]
        let model = Self::build_model(&fixtures[0], options.children, None, &control, cx);
        #[cfg(feature = "gpui-kit-engine")]
        control.update(cx, |state, cx| {
            state.set_items(Self::build_items(&fixtures[0], options.children), cx)
        });
        let mut this = Self {
            control,
            _observer: observer,
            fixtures,
            options,
            generation: 0,
            collapsed: None,
            selected: root_id(0).into(),
            scroll_target: root_id(0).into(),
            sequence: 0,
            operations: [0; 7],
            probe: Rc::new(RefCell::new(Vec::with_capacity(64))),
            validation_painted: false,
            metrics: Metrics::new(started),
            #[cfg(feature = "kael-engine")]
            model,
        };
        this.select(this.selected.clone(), window, cx);
        this
    }
    #[cfg(feature = "kael-engine")]
    fn build_model(
        fixture: &Fixture,
        children: usize,
        collapsed: Option<usize>,
        control: &Entity<Control>,
        cx: &mut App,
    ) -> VirtualTreeModel<SharedString> {
        let roots = (0..ROOTS)
            .map(|root| {
                let offset = root * (children + 1);
                let record = &fixture.records[offset];
                TreeNode::new(record.id.clone(), record.label.clone()).with_children(
                    fixture.records[offset + 1..offset + children + 1]
                        .iter()
                        .map(|record| TreeNode::new(record.id.clone(), record.label.clone()))
                        .collect(),
                )
            })
            .collect::<Vec<_>>();
        let expanded = (0..ROOTS)
            .filter(|root| collapsed != Some(*root))
            .map(|root| SharedString::from(root_id(root)))
            .collect::<HashSet<_>>();
        let mut model = VirtualTreeModel::new(&roots, &expanded).unwrap();
        let context = control
            .read(cx)
            .accessibility_preparation_context(TREE_LABEL, true);
        model
            .prepare_accessibility(context, cx.background_executor())
            .unwrap();
        model
    }
    #[cfg(feature = "gpui-kit-engine")]
    fn build_items(fixture: &Fixture, children: usize) -> Vec<TreeItem> {
        (0..ROOTS)
            .map(|root| {
                let offset = root * (children + 1);
                let record = &fixture.records[offset];
                TreeItem::new(record.id.clone(), record.label.clone())
                    .expanded(true)
                    .children(
                        fixture.records[offset + 1..offset + children + 1]
                            .iter()
                            .map(|record| TreeItem::new(record.id.clone(), record.label.clone())),
                    )
            })
            .collect()
    }
    fn select(&mut self, id: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = id.clone();
        self.scroll_target = id.clone();
        self.control.update(cx, |control, cx| {
            #[cfg(feature = "kael-engine")]
            {
                assert!(control.activate(&id, &self.model));
                window.focus(&control.focus_handle(cx));
            }
            #[cfg(feature = "gpui-kit-engine")]
            {
                let index = control.index_of(&id).expect("visible selected ID");
                control.set_selected_index(Some(index), cx);
                control.scroll_to_item(index, ScrollStrategy::Top);
                control.focus(window, cx);
            }
            cx.notify();
        });
    }
    fn scroll(&mut self, id: SharedString, cx: &mut Context<Self>) {
        self.scroll_target = id.clone();
        self.control.update(cx, |control, cx| {
            #[cfg(feature = "kael-engine")]
            {
                assert!(control.scroll_to_id(&id, &self.model, ScrollStrategy::Top));
            }
            #[cfg(feature = "gpui-kit-engine")]
            {
                let index = control.index_of(&id).expect("visible scroll ID");
                control.scroll_to_item(index, ScrollStrategy::Top);
            }
            cx.notify();
        });
    }
    fn collapse(
        &mut self,
        root: usize,
        collapse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id: SharedString = root_id(root).into();
        self.select(id, window, cx);
        self.collapsed = collapse.then_some(root);
        #[cfg(feature = "kael-engine")]
        {
            self.model = Self::build_model(
                &self.fixtures[self.generation],
                self.options.children,
                self.collapsed,
                &self.control,
                cx,
            );
        }
        #[cfg(feature = "gpui-kit-engine")]
        {
            if collapse {
                window.dispatch_action(Box::new(gpui_kit::base::actions::SelectLeft), cx);
            } else {
                window.dispatch_action(Box::new(gpui_kit::base::actions::SelectRight), cx);
            }
        }
    }
    fn replace_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generation = 1 - self.generation;
        self.collapsed = None;
        #[cfg(feature = "kael-engine")]
        {
            self.model = Self::build_model(
                &self.fixtures[self.generation],
                self.options.children,
                None,
                &self.control,
                cx,
            );
        }
        #[cfg(feature = "gpui-kit-engine")]
        self.control.update(cx, |control, cx| {
            control.set_items(
                Self::build_items(&self.fixtures[self.generation], self.options.children),
                cx,
            )
        });
        self.select(root_id(0).into(), window, cx);
    }
    fn set_phase(&mut self, phase: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.metrics.set_phase(phase, window);
        self.sequence = 0;
        if phase.starts_with("idle") {
            #[cfg(feature = "kael-engine")]
            window.blur();
            #[cfg(feature = "gpui-kit-engine")]
            window.blur(cx);
        } else {
            self.control.update(cx, |control, cx| {
                #[cfg(feature = "kael-engine")]
                window.focus(&control.focus_handle(cx));
                #[cfg(feature = "gpui-kit-engine")]
                control.focus(window, cx);
            });
        }
        cx.notify();
        window.refresh();
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.metrics.collect(window);
        let started = Instant::now();
        if self.metrics.phase == "churn" && self.sequence.is_multiple_of(30) {
            self.replace_model(window, cx);
            self.operations[6] += 1;
        }
        let operation = self.sequence % 6;
        match operation {
            0 => {
                let index =
                    (self.sequence / 6 * 7919) % visible_len(self.options.children, self.collapsed);
                let record = visible_record_index(index, self.options.children, self.collapsed);
                self.select(
                    self.fixtures[self.generation].records[record].id.clone(),
                    window,
                    cx,
                );
            }
            1 => {
                let index = (self.sequence / 6 * 3571 + self.options.nodes() / 2)
                    % visible_len(self.options.children, self.collapsed);
                let record = visible_record_index(index, self.options.children, self.collapsed);
                self.scroll(
                    self.fixtures[self.generation].records[record].id.clone(),
                    cx,
                );
            }
            2 => self.collapse(self.sequence / 6 % ROOTS, true, window, cx),
            3 => {
                let root = self.collapsed.unwrap_or(self.sequence / 6 % ROOTS);
                self.collapse(root, false, window, cx);
            }
            4 => self.select(root_id(0).into(), window, cx),
            5 => self.select(
                self.fixtures[self.generation]
                    .records
                    .last()
                    .unwrap()
                    .id
                    .clone(),
                window,
                cx,
            ),
            _ => unreachable!(),
        }
        self.operations[operation] += 1;
        self.sequence += 1;
        self.metrics.operation(started);
        cx.notify();
        window.refresh();
    }
    fn check_phase(&mut self, cx: &mut Context<Self>) {
        let fixture = &self.fixtures[self.generation];
        let expected =
            visible_records(fixture, self.options.children, self.collapsed).collect::<Vec<_>>();
        let expected_hash = records_hash(expected.iter().copied());
        let (selected, actual) = self.control.update(cx, |control, cx| {
            #[cfg(feature = "kael-engine")]
            {
                let _ = cx;
                assert_eq!(self.model.len(), expected.len());
                let records = (0..self.model.len())
                    .map(|index| Record {
                        id: self.model.id_at(index).unwrap().clone(),
                        label: self.model.label_at(index).unwrap().clone(),
                        depth: self.model.level_at(index).unwrap(),
                    })
                    .collect::<Vec<_>>();
                (control.active_id().unwrap().clone(), records)
            }
            #[cfg(feature = "gpui-kit-engine")]
            {
                let _ = cx;
                let records = (0..expected.len())
                    .map(|index| {
                        let entry = control.entry(index).expect("every visible node");
                        Record {
                            id: entry.item().id.clone(),
                            label: entry.item().label.clone(),
                            depth: entry.depth(),
                        }
                    })
                    .collect::<Vec<_>>();
                assert!(
                    control.entry(expected.len()).is_none(),
                    "no extra visible nodes"
                );
                (
                    control
                        .selected_item()
                        .expect("single native selection")
                        .id
                        .clone(),
                    records,
                )
            }
        });
        assert_eq!(selected, self.selected, "native logical selection");
        assert_eq!(
            records_hash(actual.iter()),
            expected_hash,
            "every visible control ID/label/depth"
        );
        for (actual, expected) in actual.iter().zip(&expected) {
            assert_eq!(actual.id, expected.id);
            assert_eq!(actual.label, expected.label);
            assert_eq!(actual.depth, expected.depth);
        }
        let mut mounted = self.probe.borrow().clone();
        mounted.sort_unstable();
        mounted.dedup();
        assert!(
            !mounted.is_empty() && mounted.len() <= 64,
            "native tree physically renders bounded rows"
        );
        for index in &mounted {
            assert!(*index < expected.len());
        }
        let target = expected
            .iter()
            .position(|record| record.id == self.scroll_target)
            .unwrap();
        #[cfg(feature = "kael-engine")]
        if !mounted.contains(&target) {
            let state = self.control.read(cx);
            eprintln!(
                "KAEL_TREE_REVEAL_DIAGNOSTIC: rendered_range={:?} scroll={}",
                state.last_rendered_range(),
                state.scroll_handle().to_text(),
            );
        }
        assert!(
            mounted.contains(&target),
            "native reveal target physically rendered: phase={} sequence={} target={} mounted={mounted:?}",
            self.metrics.phase,
            self.sequence,
            target,
        );
        self.metrics
            .checks
            .push((self.metrics.phase.to_string(), true));
        self.metrics.oracles.push(serde_json::json!({"phase":self.metrics.phase,"correct":true,"dataset_generation":fixture.generation,
            "selected_id":selected,"scroll_target_id":self.scroll_target,"collapsed_root":self.collapsed,"visible_nodes":expected.len(),
            "verified_control_nodes":actual.len(),"visible_hash":expected_hash,"fixture_hash":fixture.hash,"native_mounted_rows":mounted.len(),
            "mounted_indices":mounted,"reveal_target_mounted":true}));
    }

    fn validation_ready(&self, cx: &App) -> bool {
        #[cfg(feature = "kael-engine")]
        let target = self.model.index_of(&self.scroll_target);
        #[cfg(feature = "gpui-kit-engine")]
        let target = self.control.read(cx).index_of(&self.scroll_target);
        #[cfg(feature = "kael-engine")]
        let _ = cx;
        self.validation_painted
            && target.is_some_and(|target| self.probe.borrow().contains(&target))
    }
    fn report(&self) {
        self.metrics.report(serde_json::json!({"contract":CONTRACT,"quick":self.options.quick,"rows":self.options.nodes(),"root_count":ROOTS,
            "children_per_root":self.options.children,"fixture_hash":self.fixtures[0].hash,"fixture_bytes":self.fixtures[0].bytes,"fixture_retained_datasets":2,
            "fixture_hashes":[self.fixtures[0].hash,self.fixtures[1].hash],"replacement_interval_updates":30,
            "tree_width_px":TREE_WIDTH,"tree_height_px":CONTENT_HEIGHT,"row_height_px":ROW_HEIGHT,"indent_px":INDENT,
            "component":if cfg!(feature="kael-engine"){"VirtualTreeList"}else{"Tree"},"horizontal_scroll":false,
            "operations":{"selection_reveal":self.operations[0],"vertical_scroll":self.operations[1],"collapse_root":self.operations[2],"expand_root":self.operations[3],"home":self.operations[4],"end_selection":self.operations[5],"replace_model":self.operations[6]},
            "timing_scope":"native shipped virtual tree; public selection/reveal and expansion/model preparation; each update gets a separate paced frame",
            "oracle_scope":"every visible control ID, Unicode label and depth; stable selection, collapsed shape and physically mounted reveal target; validation frames excluded",
            "model_replacement_scope":"alternate two retained immutable label fixtures; rebuild shipped native tree model wrappers/indices in timed application update",
            "differences":["Kael expansion is a controlled flattened immutable model rebuild; GPUI Kit expansion is its public routed SelectLeft/SelectRight action",
                "Kael prepares complete logical accessibility metadata; pinned GPUI Kit Tree exposes mounted rows only",
                "Kael has a disclosure button per row; GPUI Kit ListItem renders an application disclosure glyph inside its native Tree row",
                "Native selection, scrolling, layout, and repaint remain each shipped control's implementation"]}));
    }
}
impl Render for Workload {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(feature = "gpui-kit-engine")]
        let _ = cx;
        self.metrics.rendered();
        if !self.metrics.measuring {
            self.validation_painted = true;
        }
        self.probe.borrow_mut().clear();
        let probe = self.probe.clone();
        #[cfg(feature = "kael-engine")]
        let control = {
            let model = self.model.clone();
            VirtualTreeList::new("tree", self.model.clone(), self.control.clone())
                .label(TREE_LABEL)
                .density(TreeListDensity::Compact)
                .selected_id(self.selected.clone())
                .decorate_row(move |id, row, _, _| {
                    let index = model.index_of(id).unwrap();
                    probe.borrow_mut().push(index);
                    row.font_family(FONT_FAMILY)
                        .text_size(px(FONT_SIZE))
                        .line_height(px(LINE_HEIGHT))
                })
                .on_toggle({
                    let owner = cx.entity();
                    move |id, expanded, window, cx| {
                        let root = id.as_ref().split('/').nth(1).unwrap().parse().unwrap();
                        owner.update(cx, |view, cx| view.collapse(root, !expanded, window, cx));
                    }
                })
                .into_any_element()
        };
        #[cfg(feature = "gpui-kit-engine")]
        let control = Tree::new(&self.control, move |index, entry, _, _, _| {
            probe.borrow_mut().push(index);
            ListItem::new(index)
                .h(px(ROW_HEIGHT))
                .min_h(px(ROW_HEIGHT))
                .py(px(0.0))
                .pl(px(8.0 + INDENT * entry.depth() as f32))
                .pr(px(8.0))
                .font_family(FONT_FAMILY)
                .text_size(px(FONT_SIZE))
                .line_height(px(LINE_HEIGHT))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .w(px(18.0))
                                .flex_shrink_0()
                                .child(if entry.is_folder() {
                                    if entry.is_expanded() { "▾" } else { "▸" }
                                } else {
                                    "·"
                                }),
                        )
                        .child(entry.item().label.clone()),
                )
        })
        .into_any_element();
        div().size_full().flex().flex_col().bg(rgb(0x11151b)).text_color(rgb(0xe2e6ed)).font_family(FONT_FAMILY).text_size(px(FONT_SIZE)).line_height(px(LINE_HEIGHT))
            .child(div().h(px(56.0)).flex_shrink_0().px_4().flex().items_center().justify_between().child("Unicode project navigation").child(format!("{} · {} nodes · {}",engine(),self.options.nodes(),self.metrics.phase)))
            .child(div().flex_1().min_h(px(0.0)).w_full().flex().child(div().w(px(TREE_WIDTH)).h_full().flex_shrink_0().overflow_hidden().child(control))
                .child(div().flex_1().p_4().child(format!("25 projects\n{} Unicode files/project\nSelection · reveal · scroll\nCollapse · expand · replacement",self.options.children))))
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
                size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)),
                cx,
            ))),
            ..Default::default()
        },
        |window, cx| {
            window.set_window_title("Native virtual tree comparison");
            let view = cx.new(|cx| Workload::new(started, options, window, cx));
            let weak = view.downgrade();
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
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
                            while Instant::now() < until || updates < 6 {
                                cx.background_executor()
                                    .timer(Duration::from_micros(16_667))
                                    .await;
                                weak.update_in(cx, |view, window, cx| view.tick(window, cx))
                                    .unwrap();
                                updates += 1;
                            }
                        }
                        marker("validation", started);
                        weak.update_in(cx, |view, window, cx| {
                            view.metrics.validation(window);
                            view.validation_painted = false;
                            cx.notify();
                            window.refresh();
                        })
                        .unwrap();
                        cx.background_executor()
                            .timer(Duration::from_millis(40))
                            .await;
                        let deadline = Instant::now() + Duration::from_secs(2);
                        while !weak
                            .update_in(cx, |view, _, cx| view.validation_ready(cx))
                            .unwrap()
                        {
                            if Instant::now() >= deadline {
                                weak.update_in(cx, |view, _, cx| {
                                    view.check_phase(cx);
                                    assert!(
                                        view.validation_painted,
                                        "native validation frame was not painted"
                                    );
                                })
                                .unwrap();
                                unreachable!("reveal readiness and the complete oracle must agree");
                            }
                            cx.background_executor()
                                .timer(Duration::from_millis(8))
                                .await;
                        }
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
    .expect("native tree comparison window");
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
    fn fixture_is_complete_and_visible_projection_preserves_ids() {
        let fixture = Fixture::new(4, 0);
        assert_eq!(fixture.records.len(), 125);
        for collapsed in [None, Some(0), Some(12), Some(24)] {
            let visible = visible_records(&fixture, 4, collapsed).collect::<Vec<_>>();
            assert_eq!(visible.len(), visible_len(4, collapsed));
            for (index, record) in visible.iter().enumerate() {
                assert_eq!(
                    record.id,
                    fixture.records[visible_record_index(index, 4, collapsed)].id
                );
            }
        }
        assert_ne!(fixture.hash, Fixture::new(4, 1).hash);
    }
    #[::core::prelude::v1::test]
    fn phase_contract_is_not_a_quick_timing_run() {
        assert_eq!(phase_duration(false, "active"), Duration::from_secs(12));
        assert_eq!(phase_duration(false, "idle-before"), Duration::from_secs(5));
        assert_eq!(
            Options {
                children: 4000,
                quick: false
            }
            .nodes(),
            100025
        );
    }
    #[::core::prelude::v1::test]
    fn production_fixture_exact_bytes_and_hash_are_pinned() {
        let first = Fixture::new(4000, 0);
        let second = Fixture::new(4000, 1);
        assert_eq!(first.records.len(), 100025);
        assert_eq!(first.bytes, 6100975);
        assert_eq!(first.hash, "fnv1a64:0fbc5b81edc32f9b");
        assert_eq!(second.hash, "fnv1a64:1bfc4b56d6a0a5eb");
    }
}

//! A native explorer with 100,000 file rows and 25 directory rows.
//!
//! Run with `cargo run -p kael_ui --example virtual_tree`.

use kael_ui::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

kael::actions!(virtual_tree_example, [QuitExplorer]);

const DIRECTORIES: u64 = 25;
const FILES_PER_DIRECTORY: u64 = 4_000;

struct Explorer {
    nodes: Arc<[TreeNode<u64>]>,
    expanded: HashSet<u64>,
    model: VirtualTreeModel<u64>,
    state: Entity<VirtualTreeState<u64>>,
    selected: Option<u64>,
    preparing: bool,
    prepare_task: Option<Task<()>>,
    prepare_generation: Arc<()>,
}

impl Explorer {
    fn new(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| VirtualTreeState::new(cx));
        let mut model = VirtualTreeModel::new(&[], &HashSet::new()).unwrap();
        model
            .prepare_accessibility(
                state
                    .read(cx)
                    .accessibility_preparation_context("Project files", true),
                cx.background_executor(),
            )
            .unwrap();
        let mut view = Self {
            nodes: Arc::from([]),
            expanded: HashSet::new(),
            model,
            state,
            selected: None,
            preparing: false,
            prepare_task: None,
            prepare_generation: Arc::new(()),
        };
        view.start_prepare(cx);
        view
    }

    fn start_prepare(&mut self, cx: &mut Context<Self>) {
        // Coalesce expansion changes while one worker owns model preparation.
        if self.prepare_task.is_some() {
            return;
        }
        self.preparing = true;
        let nodes = self.nodes.clone();
        let expanded = self.expanded.clone();
        let generation = self.prepare_generation.clone();
        let context = self
            .state
            .read(cx)
            .accessibility_preparation_context("Project files", true);
        let executor = cx.background_executor().clone();
        let worker = executor.clone();
        self.prepare_task = Some(cx.spawn(async move |view, cx| {
            let (nodes, expanded, model) = executor
                .spawn(async move {
                    let initial = nodes.is_empty();
                    let nodes = if initial {
                        (0..DIRECTORIES)
                            .map(|directory| {
                                let id = directory * (FILES_PER_DIRECTORY + 1);
                                TreeNode::new(id, format!("Project {:02}", directory + 1))
                                    .with_icon("folder")
                                    .with_children(
                                        (1..=FILES_PER_DIRECTORY)
                                            .map(|file| {
                                                TreeNode::new(
                                                    id + file,
                                                    format!("document_{file:04}.rs"),
                                                )
                                                .with_icon("file")
                                            })
                                            .collect(),
                                    )
                            })
                            .collect::<Vec<_>>()
                            .into()
                    } else {
                        nodes
                    };
                    let expanded = if initial {
                        nodes.iter().map(|node| node.id).collect()
                    } else {
                        expanded
                    };
                    let mut model = VirtualTreeModel::new(&nodes, &expanded)
                        .expect("the example generates unique node IDs");
                    model.prepare_accessibility(context, &worker).unwrap();
                    (nodes, expanded, model)
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                view.prepare_task = None;
                if Arc::ptr_eq(&generation, &view.prepare_generation) {
                    view.nodes = nodes;
                    view.expanded = expanded;
                    view.model = model;
                    view.preparing = false;
                    if std::env::var_os("KAEL_ACCESSIBILITY_SMOKE").is_some() {
                        println!("NATIVE_ACCESSIBILITY_MODEL: rows={}", view.model.len());
                    }
                } else {
                    view.start_prepare(cx);
                }
                cx.notify();
            });
        }));
    }
}

impl Render for Explorer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let select_view = cx.entity();
        let toggle_view = cx.entity();
        let jump_state = self.state.clone();
        let jump_model = self.model.clone();
        let last_id = self
            .model
            .id_at(self.model.len().saturating_sub(1))
            .copied();

        let theme = Theme::of(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_6()
            .gap_4()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_2xl().font_weight(FontWeight::SEMIBOLD).child("Project explorer"))
                            .child(
                                div().text_sm().text_color(theme.tokens.muted_foreground)
                                    .child(if self.preparing { "Loading project files…".to_string() } else { format!("{} displayed rows · one keyboard focus handle", self.model.len()) }),
                            ),
                    )
                    .child(Button::new("jump-last", "Jump to last row").on_click(move |_, window, cx| {
                        if let Some(id) = last_id {
                            jump_state.update(cx, |state, cx| {
                                state.activate(&id, &jump_model);
                                window.focus(&state.focus_handle(cx));
                                cx.notify();
                            });
                            window.refresh();
                        }
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .border_1()
                    .border_color(theme.tokens.border)
                    .rounded(theme.tokens.radius_md)
                    .overflow_hidden()
                    .child(
                        VirtualTreeList::new("explorer", self.model.clone(), self.state.clone())
                            .label("Project files")
                            .density(TreeListDensity::Balanced)
                            .when_some(self.selected, |tree, id| tree.selected_id(id))
                            .on_select(move |id, _, cx| {
                                select_view.update(cx, |view, cx| {
                                    view.selected = Some(*id);
                                    if std::env::var_os("KAEL_ACCESSIBILITY_SMOKE").is_some() {
                                        println!("NATIVE_ACCESSIBILITY_SELECT: id={id}");
                                    }
                                    cx.notify();
                                });
                            })
                            .on_toggle(move |id, expanded, _, cx| {
                                toggle_view.update(cx, |view, cx| {
                                    if std::env::var_os("KAEL_ACCESSIBILITY_SMOKE").is_some() {
                                        println!("NATIVE_ACCESSIBILITY_DISCLOSURE: id={id} expanded={expanded}");
                                    }
                                    if expanded {
                                        view.expanded.insert(*id);
                                    } else {
                                        view.expanded.remove(id);
                                    }
                                    view.prepare_generation = Arc::new(());
                                    view.start_prepare(cx);
                                    cx.notify();
                                });
                            }),
                    ),
            )
            .child(
                div().text_sm().text_color(theme.tokens.muted_foreground)
                    .child(match self.selected {
                        Some(id) => format!("Selected node {id}. Arrow keys navigate; Left/Right collapse or expand; Enter selects."),
                        None => "Arrow keys navigate; Home/End jump; Left/Right collapse or expand; Enter selects.".to_string(),
                    }),
            )
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::try_new()?.run(|cx| {
        kael_ui::init(cx);
        install_theme(cx, Theme::astryx_neutral());
        cx.on_action(|_: &QuitExplorer, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new("cmd-q", QuitExplorer, None),
            KeyBinding::new("ctrl-q", QuitExplorer, None),
        ]);
        cx.set_menus(
            StandardMacMenuBar::new("Kael virtual tree")
                .file_menu(file_menu().action("Quit Kael virtual tree", QuitExplorer))
                .build(),
        );
        if std::env::var_os("KAEL_ACCESSIBILITY_SMOKE").is_some() {
            cx.spawn(async move |cx| {
                kael::Timer::after(Duration::from_secs(120)).await;
                eprintln!("NATIVE_ACCESSIBILITY_TIMEOUT: native client did not finish");
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
        }
        if let Err(error) = cx.open_window(WindowOptions::default(), |window, cx| {
            window.set_window_title("Kael virtual tree");
            cx.new(Explorer::new)
        }) {
            eprintln!("failed to open virtual tree example: {error}");
            cx.quit();
        }
    });
    Ok(())
}

//! Real component tests that drive a window through `kael::TestAppContext`.
//!
//! These exercise `kael_ui` components end-to-end: a root view renders a real
//! component tree into a headless test window, and we simulate input or advance
//! frames to assert on observable behavior. They double as the canonical pattern
//! for testing `kael_ui` apps — see `docs/src/testing.md`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use kael::{
    div, point, px, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    Styled as _, TestAppContext, Window,
};
use kael_ui::prelude::*;

struct ButtonHarness {
    clicks: Rc<Cell<usize>>,
}

impl Render for ButtonHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div().size_full().child(
            div().absolute().top(px(0.0)).left(px(0.0)).child(
                Button::new("harness-button", "Click me")
                    .size(ButtonSize::Lg)
                    .on_click(move |_event, _window, _cx| {
                        clicks.set(clicks.get() + 1);
                    }),
            ),
        )
    }
}

#[kael::test]
fn button_click_fires_handler(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let clicks = Rc::new(Cell::new(0usize));
    let (_view, vcx) = cx.add_window_view(|_window, _cx| ButtonHarness {
        clicks: clicks.clone(),
    });

    vcx.update(|window, cx| {
        window.draw(cx).clear();
        assert_eq!(
            window
                .accessibility_tree()
                .nodes
                .values()
                .filter(|node| node.label.as_deref() == Some("Click me"))
                .count(),
            1,
            "button labels should be announced once"
        );
    });

    vcx.simulate_click(point(px(40.0), px(20.0)), Default::default());
    vcx.run_until_parked();

    assert_eq!(clicks.get(), 1, "button on_click handler should fire once");
}

struct UncontrolledCollapsibleHarness;

impl Render for UncontrolledCollapsibleHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .absolute()
                .top(px(0.0))
                .left(px(0.0))
                .w(px(280.0))
                .child(
                    Collapsible::new()
                        .id("uncontrolled-collapsible")
                        .label("Advanced settings")
                        .default_open(true)
                        .trigger(div().w_full().p(px(12.0)).child("Advanced settings"))
                        .content(div().px(px(12.0)).pb(px(12.0)).child("Hidden details")),
                ),
        )
    }
}

#[kael::test]
fn uncontrolled_collapsible_toggles_without_external_state(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });
    let (_view, vcx) = cx.add_window_view(|_, _| UncontrolledCollapsibleHarness);

    vcx.update(|window, cx| {
        window.draw(cx).clear();
        let nodes = &window.accessibility_tree().nodes;
        let trigger = nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Button
                    && node.label.as_deref() == Some("Advanced settings")
            })
            .expect("collapsible trigger should be exposed");
        assert!(trigger.states.contains(kael::AccessibilityState::EXPANDED));
        assert!(nodes
            .values()
            .any(|node| node.label.as_deref() == Some("Hidden details")));
    });

    vcx.simulate_click(point(px(80.0), px(20.0)), Default::default());
    vcx.run_until_parked();

    vcx.update(|window, cx| {
        window.draw(cx).clear();
        let nodes = &window.accessibility_tree().nodes;
        let trigger = nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Button
                    && node.label.as_deref() == Some("Advanced settings")
            })
            .expect("collapsible trigger should remain exposed");
        assert!(trigger.states.contains(kael::AccessibilityState::COLLAPSED));
        assert!(!nodes
            .values()
            .any(|node| node.label.as_deref() == Some("Hidden details")));
    });
}

struct ThumbnailSemanticsHarness {
    clicks: Rc<Cell<usize>>,
}

impl Render for ThumbnailSemanticsHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div()
            .child(
                Thumbnail::new()
                    .id("interactive-thumbnail")
                    .alt("Project preview")
                    .on_click(move |_, _| clicks.set(clicks.get() + 1)),
            )
            .child(
                Thumbnail::new()
                    .id("loading-thumbnail")
                    .alt("Uploading cover")
                    .loading(true)
                    .loading_animation(false),
            )
            .child(
                Thumbnail::new()
                    .id("disabled-thumbnail")
                    .alt("Unavailable cover")
                    .disabled(true),
            )
    }
}

#[kael::test]
fn thumbnail_accessibility_activation_and_states_are_complete(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });
    let clicks = Rc::new(Cell::new(0));
    let (_view, vcx) = cx.add_window_view(|_, _| ThumbnailSemanticsHarness {
        clicks: clicks.clone(),
    });

    let target = vcx.update(|window, cx| {
        window.draw(cx).clear();
        let nodes = &window.accessibility_tree().nodes;
        let interactive = nodes
            .values()
            .find(|node| node.label.as_deref() == Some("Project preview"))
            .expect("interactive thumbnail should be exposed");
        assert_eq!(interactive.role, kael::AccessibilityRole::Button);
        assert!(interactive
            .actions
            .contains(&kael::AccessibilityAction::Click));

        let loading = nodes
            .values()
            .find(|node| node.label.as_deref() == Some("Uploading cover"))
            .expect("loading thumbnail should be exposed");
        assert!(loading.states.contains(kael::AccessibilityState::BUSY));

        let disabled = nodes
            .values()
            .find(|node| node.label.as_deref() == Some("Unavailable cover"))
            .expect("disabled thumbnail should be exposed");
        assert!(disabled.states.contains(kael::AccessibilityState::DISABLED));
        interactive.id
    });

    vcx.update(|window, _| {
        window.dispatch_accessibility_action_for_test(kael::AccessibilityActionRequest::new(
            target,
            kael::AccessibilityAction::Click,
        ));
    });
    vcx.run_until_parked();
    assert_eq!(clicks.get(), 1);
}

struct AudioPlayerSemanticsHarness {
    state: kael::Entity<AudioPlayerState>,
}

impl Render for AudioPlayerSemanticsHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        AudioPlayer::new(self.state.clone()).title("Field recording")
    }
}

#[kael::test]
fn audio_player_routes_accessibility_slider_actions(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });
    let state = cx.update(|cx| {
        cx.new(|cx| {
            let mut state = AudioPlayerState::new(cx);
            state.set_duration(100.0, cx);
            state.set_current_time(20.0, cx);
            state.set_volume(0.8, cx);
            state
        })
    });
    let (_view, vcx) = cx.add_window_view(|_, _| AudioPlayerSemanticsHarness {
        state: state.clone(),
    });

    let (progress_id, volume_id) = vcx.update(|window, cx| {
        window.draw(cx).clear();
        let tree = window.accessibility_tree();
        assert_eq!(
            tree.nodes
                .values()
                .filter(|node| node.label.as_deref() == Some("Field recording"))
                .count(),
            1,
            "the player title should be announced once"
        );
        let progress = tree
            .nodes
            .values()
            .find(|node| node.label.as_deref() == Some("Playback position"))
            .expect("playback slider should be exposed");
        let volume = tree
            .nodes
            .values()
            .find(|node| node.label.as_deref() == Some("Volume"))
            .expect("volume slider should be exposed");
        assert!(window
            .has_accessibility_action_handler(progress.id, kael::AccessibilityAction::SetValue,));
        assert!(window
            .has_accessibility_action_handler(volume.id, kael::AccessibilityAction::Decrement,));
        (progress.id, volume.id)
    });

    vcx.update(|window, _| {
        window.dispatch_accessibility_action_for_test(
            kael::AccessibilityActionRequest::with_payload(
                progress_id,
                kael::AccessibilityAction::SetValue,
                kael::AccessibilityActionPayload::NumericValue(55.0),
            ),
        );
        window.dispatch_accessibility_action_for_test(kael::AccessibilityActionRequest::new(
            volume_id,
            kael::AccessibilityAction::Decrement,
        ));
    });
    vcx.run_until_parked();

    cx.update(|cx| {
        let state = state.read(cx);
        assert_eq!(state.current_time(), 55.0);
        assert!((state.volume() - 0.75).abs() < f32::EPSILON);
    });
}

struct VideoPlayerSemanticsHarness {
    state: kael::Entity<VideoPlayerState>,
}

impl Render for VideoPlayerSemanticsHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        VideoPlayer::new(self.state.clone()).size(VideoPlayerSize::Sm)
    }
}

#[kael::test]
fn video_player_routes_accessibility_slider_actions(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });
    let state = cx.update(|cx| {
        cx.new(|cx| {
            let mut state = VideoPlayerState::new(cx);
            state.set_title("Component tour", cx);
            state.set_duration(120.0, cx);
            state.set_current_time(30.0, cx);
            state.set_volume(0.8, cx);
            state
        })
    });
    let (_view, vcx) = cx.add_window_view(|_, _| VideoPlayerSemanticsHarness {
        state: state.clone(),
    });

    let (progress_id, volume_id) = vcx.update(|window, cx| {
        window.draw(cx).clear();
        let tree = window.accessibility_tree();
        let progress = tree
            .nodes
            .values()
            .find(|node| node.label.as_deref() == Some("Playback position"))
            .expect("video playback slider should be exposed");
        let volume = tree
            .nodes
            .values()
            .find(|node| node.label.as_deref() == Some("Volume"))
            .expect("video volume slider should be exposed");
        assert!(window
            .has_accessibility_action_handler(progress.id, kael::AccessibilityAction::SetValue,));
        assert!(window
            .has_accessibility_action_handler(volume.id, kael::AccessibilityAction::Increment,));
        (progress.id, volume.id)
    });

    vcx.update(|window, _| {
        window.dispatch_accessibility_action_for_test(
            kael::AccessibilityActionRequest::with_payload(
                progress_id,
                kael::AccessibilityAction::SetValue,
                kael::AccessibilityActionPayload::Value("72".into()),
            ),
        );
        window.dispatch_accessibility_action_for_test(kael::AccessibilityActionRequest::new(
            volume_id,
            kael::AccessibilityAction::Increment,
        ));
    });
    vcx.run_until_parked();

    cx.update(|cx| {
        let state = state.read(cx);
        assert_eq!(state.current_time(), 72.0);
        assert!((state.volume() - 0.85).abs() < f32::EPSILON);
    });
}

struct TransitionHarness {
    hovered: bool,
}

impl Render for TransitionHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let bg = if self.hovered {
            kael_ui::theme::Theme::dark().tokens.primary
        } else {
            kael_ui::theme::Theme::dark().tokens.muted
        };
        div()
            .id("transition-box")
            .size(px(100.0))
            .bg(bg)
            .transition(Duration::from_millis(120))
    }
}

#[kael::test]
fn implicit_transition_renders_across_frames(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let (view, vcx) = cx.add_window_view(|_window, _cx| TransitionHarness { hovered: false });

    vcx.update(|window, _cx| window.refresh());
    vcx.run_until_parked();

    view.update(vcx, |harness, cx| {
        harness.hovered = true;
        cx.notify();
    });
    vcx.run_until_parked();

    view.update(vcx, |harness, _cx| {
        assert!(
            harness.hovered,
            "transition harness should hold its toggled state after re-render"
        );
    });
}

struct CounterHarness {
    count: usize,
}

impl Render for CounterHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        VStack::new()
            .gap(px(8.0))
            .child(div().child(format!("count: {}", self.count)))
            .child(Button::new("inc", "Increment"))
    }
}

struct NavigationSemanticsHarness {
    mobile_open: bool,
}

impl Render for NavigationSemanticsHarness {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        div()
            .size_full()
            .child(
                MobileNavToggle::new()
                    .open(self.mobile_open)
                    .on_toggle(move |open, _, cx| {
                        view.update(cx, |this, cx| {
                            this.mobile_open = open;
                            cx.notify();
                        });
                    }),
            )
            .child(NavItem::new("Archived projects").disabled(true))
    }
}

#[kael::test]
fn navigation_controls_keep_stable_disabled_and_expanded_semantics(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let (_view, vcx) = cx.add_window_view(|_, _| NavigationSemanticsHarness { mobile_open: false });
    let toggle_id = vcx.update(|window, cx| {
        window.draw(cx).clear();
        let tree = window.accessibility_tree();
        let archived = tree
            .nodes
            .values()
            .find(|node| {
                node.label.as_deref() == Some("Archived projects")
                    && node.role == kael::AccessibilityRole::Button
            })
            .expect("disabled navigation item should remain semantic");
        assert!(archived.states.contains(kael::AccessibilityState::DISABLED));

        let toggle = tree
            .nodes
            .values()
            .find(|node| node.label.as_deref() == Some("Open navigation"))
            .expect("mobile navigation toggle should be accessible");
        assert!(toggle.states.contains(kael::AccessibilityState::COLLAPSED));
        toggle.id
    });

    vcx.update(|window, _| {
        window.dispatch_accessibility_action_for_test(kael::AccessibilityActionRequest::new(
            toggle_id,
            kael::AccessibilityAction::Click,
        ));
    });
    vcx.run_until_parked();

    vcx.update(|window, cx| {
        window.draw(cx).clear();
        let toggle = window
            .accessibility_tree()
            .nodes
            .values()
            .find(|node| node.label.as_deref() == Some("Close navigation"))
            .expect("toggle should describe the available close action after opening");
        assert!(toggle.states.contains(kael::AccessibilityState::EXPANDED));
    });
}

#[kael::test]
fn view_state_update_redraws(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let (view, vcx) = cx.add_window_view(|_window, _cx| CounterHarness { count: 0 });

    view.update(vcx, |harness, cx| {
        harness.count += 1;
        cx.notify();
    });
    vcx.run_until_parked();

    view.update(vcx, |harness, _cx| {
        assert_eq!(harness.count, 1, "view state should persist across redraw");
    });
}

struct AccordionHarness {
    changes: Rc<RefCell<Vec<Vec<usize>>>>,
}

impl Render for AccordionHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let changes = self.changes.clone();
        div().size_full().child(
            Accordion::new("settings-accordion")
                .accessibility_label("Settings help")
                .item(|item| item.title("General").content("General settings"))
                .item(|item| item.title("Privacy").content("Privacy settings"))
                .on_change(move |indices, _, _| changes.borrow_mut().push(indices.to_vec())),
        )
    }
}

#[kael::test]
fn accordion_accessibility_click_expands_and_notifies(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let changes = Rc::new(RefCell::new(Vec::new()));
    let (_view, vcx) = cx.add_window_view(|_, _| AccordionHarness {
        changes: changes.clone(),
    });
    let privacy_id = vcx.update(|window, cx| {
        window.draw(cx).clear();
        let tree = window.accessibility_tree();
        assert!(tree.nodes.values().any(|node| {
            node.role == kael::AccessibilityRole::Group
                && node.label.as_deref() == Some("Settings help")
        }));
        tree.nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Button
                    && node.label.as_deref() == Some("Privacy")
            })
            .expect("Privacy accordion header should be accessible")
            .id
    });

    vcx.update(|window, _| {
        window.dispatch_accessibility_action_for_test(kael::AccessibilityActionRequest::new(
            privacy_id,
            kael::AccessibilityAction::Click,
        ));
    });
    vcx.run_until_parked();

    assert_eq!(changes.borrow().as_slice(), [vec![1]]);
    vcx.update(|window, cx| {
        window.draw(cx).clear();
        let privacy = window
            .accessibility_tree()
            .nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Button
                    && node.label.as_deref() == Some("Privacy")
            })
            .expect("Privacy header should remain accessible after expansion");
        assert!(privacy.states.contains(kael::AccessibilityState::EXPANDED));
    });
}

struct PopoverSemanticsHarness;

impl Render for PopoverSemanticsHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            Popover::new("accessible-popover")
                .trigger_button("Open quick actions")
                .content(|window, cx| {
                    cx.new(|cx| {
                        PopoverContent::new(window, cx, |_, _| {
                            div().child("Quick actions available").into_any_element()
                        })
                        .accessibility_label("Quick actions")
                    })
                }),
        )
    }
}

#[kael::test]
fn popover_trigger_supports_accessibility_activation_and_expanded_state(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let (_view, vcx) = cx.add_window_view(|_, _| PopoverSemanticsHarness);
    let trigger_id = vcx.update(|window, cx| {
        window.draw(cx).clear();
        let trigger = window
            .accessibility_tree()
            .nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Button
                    && node.label.as_deref() == Some("Open quick actions")
            })
            .expect("popover trigger button should be accessible");
        assert!(trigger.states.contains(kael::AccessibilityState::COLLAPSED));
        assert!(trigger.actions.contains(&kael::AccessibilityAction::Click));
        trigger.id
    });

    vcx.update(|window, _| {
        window.dispatch_accessibility_action_for_test(kael::AccessibilityActionRequest::new(
            trigger_id,
            kael::AccessibilityAction::Click,
        ));
    });
    vcx.run_until_parked();

    vcx.update(|window, cx| {
        window.draw(cx).clear();
        let tree = window.accessibility_tree();
        let matching_triggers = tree
            .nodes
            .values()
            .filter(|node| {
                node.role == kael::AccessibilityRole::Button
                    && node.label.as_deref() == Some("Open quick actions")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            matching_triggers.len(),
            1,
            "popover trigger must have one stable accessibility node; states={:?}",
            matching_triggers
                .iter()
                .map(|node| node.states)
                .collect::<Vec<_>>()
        );
        let trigger = matching_triggers[0];
        assert!(trigger.states.contains(kael::AccessibilityState::EXPANDED));
        assert!(tree.nodes.values().any(|node| {
            node.role == kael::AccessibilityRole::Group
                && node.label.as_deref() == Some("Quick actions")
        }));
        assert!(tree
            .nodes
            .values()
            .any(|node| node.label.as_deref() == Some("Quick actions available")));
    });
}

struct CommandPaletteSemanticsHarness {
    palette: kael::Entity<CommandPalette>,
}

struct TypographySemanticsHarness;

impl Render for TypographySemanticsHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(h1("Framework overview"))
            .child(
                Heading::new("Display section")
                    .level(HeadingLevel::H3)
                    .heading_type(HeadingType::Display1),
            )
            .child(GradientText::new("Accessible spectrum"))
            .child(
                Marquee::new("accessible-marquee", || {
                    div().child("Scrolling update").into_any_element()
                })
                .accessibility_label("Scrolling update")
                .paused(true),
            )
            .child(KBD::new("mod+shift+p"))
            .child(Link::new("Inactive reference"))
            .child(
                Link::new("External docs")
                    .external(true)
                    .href("https://example.com/docs"),
            )
            .child(CodeBlock::new("let value = 42;").language("rust"))
    }
}

#[kael::test]
fn typography_exposes_heading_roles_levels_without_duplicate_text(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });
    let (_view, vcx) = cx.add_window_view(|_, _| TypographySemanticsHarness);

    vcx.update(|window, cx| {
        window.draw(cx).clear();
        let nodes = &window.accessibility_tree().nodes;
        let is_exposed = |node: &&kael::AccessibilityNode| {
            let mut current = Some(node.id);
            while let Some(id) = current {
                let Some(candidate) = nodes.get(&id) else {
                    break;
                };
                if candidate.states.contains(kael::AccessibilityState::HIDDEN) {
                    return false;
                }
                current = candidate.parent;
            }
            true
        };
        for (label, level) in [("Framework overview", 1), ("Display section", 3)] {
            let matches = nodes
                .values()
                .filter(|node| node.label.as_deref() == Some(label))
                .filter(&is_exposed)
                .collect::<Vec<_>>();
            assert_eq!(matches.len(), 1, "heading text should be announced once");
            assert_eq!(matches[0].role, kael::AccessibilityRole::Heading);
            assert_eq!(matches[0].level, Some(level));
        }
        for label in [
            "Accessible spectrum",
            "Scrolling update",
            "Command + Shift + P",
        ] {
            let matches = nodes
                .values()
                .filter(|node| node.label.as_deref() == Some(label))
                .filter(&is_exposed)
                .collect::<Vec<_>>();
            assert_eq!(matches.len(), 1, "semantic text should be announced once");
            assert_eq!(matches[0].role, kael::AccessibilityRole::StaticText);
        }

        let inactive = nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Link
                    && node.label.as_deref() == Some("Inactive reference")
            })
            .expect("a link without a destination should remain semantic");
        assert!(inactive.states.contains(kael::AccessibilityState::DISABLED));
        assert!(inactive.actions.is_empty());

        let external = nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Link
                    && node.label.as_deref() == Some("External docs")
            })
            .expect("external link should be accessible");
        assert_eq!(
            external.description.as_deref(),
            Some("Opens in an external browser")
        );
        assert!(external.actions.contains(&kael::AccessibilityAction::Click));
        for label in ["Inactive reference", "External docs"] {
            assert_eq!(
                nodes
                    .values()
                    .filter(|node| node.label.as_deref() == Some(label))
                    .count(),
                1,
                "link text should not be announced twice"
            );
        }

        let code = nodes
            .values()
            .filter(|node| node.label.as_deref() == Some("let value = 42;"))
            .collect::<Vec<_>>();
        assert_eq!(
            code.len(),
            1,
            "code should be exposed as one readable value"
        );
        assert_eq!(code[0].role, kael::AccessibilityRole::StaticText);
        assert!(nodes.values().any(|node| {
            node.role == kael::AccessibilityRole::Group
                && node.label.as_deref() == Some("Rust code block")
        }));
    });
}

#[kael::test]
fn code_block_copy_action_writes_the_complete_source(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });
    let (_view, vcx) = cx.add_window_view(|_, _| TypographySemanticsHarness);
    let copy_id = vcx.update(|window, cx| {
        window.draw(cx).clear();
        window
            .accessibility_tree()
            .nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Button
                    && node.label.as_deref() == Some("Copy")
            })
            .expect("code block copy button should be actionable")
            .id
    });

    vcx.update(|window, _| {
        window.dispatch_accessibility_action_for_test(kael::AccessibilityActionRequest::new(
            copy_id,
            kael::AccessibilityAction::Click,
        ));
    });
    vcx.run_until_parked();
    assert_eq!(cx.read_clipboard_text().as_deref(), Some("let value = 42;"));
}

impl Render for CommandPaletteSemanticsHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.palette.clone())
    }
}

#[kael::test]
fn command_palette_items_support_accessibility_activation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let activations = Rc::new(Cell::new(0usize));
    let closes = Rc::new(Cell::new(0usize));
    let action_count = activations.clone();
    let close_count = closes.clone();
    let (_view, vcx) = cx.add_window_view(|_, cx| {
        let palette = cx.new(|cx| {
            CommandPalette::from_commands(
                cx,
                vec![Command::new("run-audit", "Run audit")
                    .description("Review the active surface")
                    .on_select(move |_, _| action_count.set(action_count.get() + 1))],
            )
            .on_close(move |_, _| close_count.set(close_count.get() + 1))
        });
        CommandPaletteSemanticsHarness { palette }
    });

    let command_id = vcx.update(|window, cx| {
        window.draw(cx).clear();
        let tree = window.accessibility_tree();
        assert!(tree.nodes.values().any(|node| {
            node.role == kael::AccessibilityRole::Dialog
                && node.label.as_deref() == Some("Command palette")
        }));
        let command = tree
            .nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Button
                    && node.label.as_deref() == Some("Run audit")
            })
            .expect("command palette item should be actionable");
        assert!(command.actions.contains(&kael::AccessibilityAction::Click));
        command.id
    });

    vcx.update(|window, _| {
        window.dispatch_accessibility_action_for_test(kael::AccessibilityActionRequest::new(
            command_id,
            kael::AccessibilityAction::Click,
        ));
    });
    vcx.run_until_parked();

    assert_eq!(activations.get(), 1);
    assert_eq!(closes.get(), 1);
}

struct FeedbackSemanticsHarness {
    skeleton: kael::Entity<SkeletonLoaderState>,
}

struct ToastSemanticsHarness {
    manager: kael::Entity<ToastManager>,
}

impl Render for ToastSemanticsHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.manager.clone())
    }
}

#[kael::test]
fn managed_toast_announces_one_complete_alert(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });
    let manager = cx.update(|cx| cx.new(ToastManager::new));
    cx.update(|cx| {
        manager.update(cx, |manager, cx| {
            manager.add_toast_no_dismiss(
                ToastItem::new(7, "Changes saved").description("Workspace settings updated"),
                cx,
            );
        });
    });

    let (_view, vcx) = cx.add_window_view(|_, _| ToastSemanticsHarness {
        manager: manager.clone(),
    });
    vcx.update(|window, cx| {
        window.draw(cx).clear();
        let nodes = &window.accessibility_tree().nodes;
        let alert = nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Alert
                    && node.label.as_deref() == Some("Changes saved")
            })
            .expect("managed toast should expose alert semantics");
        assert_eq!(
            alert.description.as_deref(),
            Some("Workspace settings updated")
        );
        assert_eq!(
            nodes
                .values()
                .filter(|node| node.label.as_deref() == Some("Changes saved"))
                .count(),
            1,
            "the visible title should not be announced twice"
        );
    });
}

impl Render for FeedbackSemanticsHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                NumberTicker::new("revenue-ticker", 1_234)
                    .prefix("$")
                    .separator(',')
                    .suffix(" MRR"),
            )
            .child(
                AnimatedProgress::new("upload-progress")
                    .value(0.72)
                    .accessibility_label("File upload"),
            )
            .child(PulseIndicator::new("sync-status").accessibility_label("Synchronization active"))
            .child(
                Alert::info()
                    .title("Review ready")
                    .description("Keyboard and screen-reader checks passed"),
            )
            .child(
                Banner::success("Workspace published")
                    .description("Every collaborator can now see the changes"),
            )
            .child(
                EmptyState::new("empty-feedback", "No queued jobs")
                    .description("New jobs will appear here"),
            )
            .child(
                SkeletonLoader::new("activity-loader", self.skeleton.clone())
                    .accessibility_label("Loading activity feed"),
            )
            .child(Spinner::new().accessibility_label("Refreshing projects"))
            .child(CircularProgress::indeterminate().label("Preparing export"))
    }
}

#[kael::test]
fn animated_feedback_exposes_one_stable_semantic_value(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let skeleton = cx.update(|cx| cx.new(|_| SkeletonLoaderState::new()));
    let (_view, vcx) = cx.add_window_view(|_, _| FeedbackSemanticsHarness {
        skeleton: skeleton.clone(),
    });
    vcx.update(|window, cx| {
        window.draw(cx).clear();
        let nodes = &window.accessibility_tree().nodes;
        assert_eq!(
            nodes
                .values()
                .filter(|node| node.label.as_deref() == Some("$1,234 MRR"))
                .count(),
            1,
            "the ticker should announce one formatted value"
        );
        assert!(
            !nodes.values().any(|node| {
                node.role == kael::AccessibilityRole::StaticText
                    && node.label.as_deref().is_some_and(|label| {
                        label.len() == 1 && label.as_bytes()[0].is_ascii_digit()
                    })
            }),
            "rolling digit columns must stay out of the accessibility tree"
        );

        let progress = nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::ProgressBar
                    && node.label.as_deref() == Some("File upload")
            })
            .expect("animated progress should expose progress-bar semantics");
        assert_eq!(
            progress.value,
            Some(kael::AccessibilityValue::Range {
                current: 72.0,
                min: 0.0,
                max: 100.0,
                step: None,
            })
        );

        let status = nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Image
                    && node.label.as_deref() == Some("Synchronization active")
            })
            .expect("a labelled pulse indicator should expose its visual status");
        assert!(status.value.is_none());

        let alert = nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Alert
                    && node.label.as_deref() == Some("Review ready")
            })
            .expect("alert should expose one complete live-region announcement");
        assert_eq!(
            alert.description.as_deref(),
            Some("Keyboard and screen-reader checks passed")
        );
        assert_eq!(
            nodes
                .values()
                .filter(|node| node.label.as_deref() == Some("Review ready"))
                .count(),
            1,
            "visible alert text should not duplicate the live-region label"
        );

        let banner = nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Alert
                    && node.label.as_deref() == Some("Workspace published")
            })
            .expect("banner should expose one complete live-region announcement");
        assert_eq!(
            banner.description.as_deref(),
            Some("Every collaborator can now see the changes")
        );
        assert_eq!(
            nodes
                .values()
                .filter(|node| node.label.as_deref() == Some("Workspace published"))
                .count(),
            1,
            "visible banner text should not duplicate the live-region label"
        );

        let empty_state = nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::Group
                    && node.label.as_deref() == Some("No queued jobs")
            })
            .expect("empty state should expose one named semantic group");
        assert_eq!(
            empty_state.description.as_deref(),
            Some("New jobs will appear here")
        );
        assert_eq!(
            nodes
                .values()
                .filter(|node| node.label.as_deref() == Some("No queued jobs"))
                .count(),
            1,
            "visible empty-state title should not duplicate its group label"
        );

        let loading = nodes
            .values()
            .find(|node| {
                node.role == kael::AccessibilityRole::ProgressBar
                    && node.label.as_deref() == Some("Loading activity feed")
            })
            .expect("a loading skeleton should expose an indeterminate progress status");
        assert!(loading.states.contains(kael::AccessibilityState::BUSY));
        assert_eq!(
            loading.value,
            Some(kael::AccessibilityValue::Text("Loading".into()))
        );

        for label in ["Refreshing projects", "Preparing export"] {
            let indeterminate = nodes
                .values()
                .find(|node| {
                    node.role == kael::AccessibilityRole::ProgressBar
                        && node.label.as_deref() == Some(label)
                })
                .unwrap_or_else(|| panic!("missing indeterminate progress status: {label}"));
            assert!(indeterminate
                .states
                .contains(kael::AccessibilityState::BUSY));
            assert_eq!(
                indeterminate.value,
                Some(kael::AccessibilityValue::Text("Loading".into()))
            );
        }
    });
}

#[cfg(feature = "markdown")]
struct MarkdownLinkHarness {
    activations: Rc<RefCell<Vec<String>>>,
}

#[cfg(feature = "markdown")]
impl Render for MarkdownLinkHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let activations = self.activations.clone();
        div().size_full().child(
            Markdown::new("[Open docs](https://example.com/docs)").on_link_click(
                move |url, _window, _cx| activations.borrow_mut().push(url.to_string()),
            ),
        )
    }
}

#[cfg(feature = "markdown")]
#[kael::test]
fn markdown_link_uses_the_caller_handler(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let activations = Rc::new(RefCell::new(Vec::new()));
    let (_view, vcx) = cx.add_window_view(|_window, _cx| MarkdownLinkHarness {
        activations: activations.clone(),
    });
    vcx.simulate_click(point(px(24.0), px(12.0)), Default::default());
    vcx.run_until_parked();

    assert_eq!(
        activations.borrow().as_slice(),
        ["https://example.com/docs"]
    );
}

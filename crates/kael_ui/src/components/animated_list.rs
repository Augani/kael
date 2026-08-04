//! Animated list that auto-animates item insert/remove transitions.

use kael::{prelude::FluentBuilder as _, *};
use std::collections::HashSet;
use std::time::Duration;

use crate::animations::{durations, easings, lerp_pixels};

const EXIT_GRACE: Duration = Duration::from_millis(32);

#[derive(Clone, PartialEq, Eq, Hash)]
enum ItemPhase {
    Entering,
    Present,
    Exiting,
}

pub struct AnimatedListState {
    keys: Vec<SharedString>,
    phases: Vec<(SharedString, ItemPhase)>,
    version: usize,
    enter_duration: Duration,
    exit_duration: Duration,
}

impl AnimatedListState {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            keys: Vec::new(),
            phases: Vec::new(),
            version: 0,
            enter_duration: durations::NORMAL,
            exit_duration: durations::FAST,
        }
    }

    pub fn enter_duration(mut self, duration: Duration) -> Self {
        self.enter_duration = duration;
        self
    }

    pub fn exit_duration(mut self, duration: Duration) -> Self {
        self.exit_duration = duration;
        self
    }

    pub fn set_keys(&mut self, new_keys: Vec<SharedString>, cx: &mut Context<Self>) {
        let mut seen = HashSet::new();
        let new_keys = new_keys
            .into_iter()
            .filter(|key| seen.insert(key.clone()))
            .collect::<Vec<_>>();
        let entering: HashSet<SharedString> = {
            let old_set: HashSet<&SharedString> = self.keys.iter().collect();
            new_keys
                .iter()
                .filter(|k| !old_set.contains(k))
                .cloned()
                .collect()
        };
        let exiting: HashSet<SharedString> = {
            let new_set: HashSet<&SharedString> = new_keys.iter().collect();
            self.keys
                .iter()
                .filter(|k| !new_set.contains(k))
                .cloned()
                .collect()
        };

        if entering.is_empty() && exiting.is_empty() && self.keys == new_keys {
            return;
        }

        self.version = self.version.wrapping_add(1);

        if cx.reduce_motion() {
            self.keys = new_keys.clone();
            self.phases = new_keys
                .into_iter()
                .map(|key| (key, ItemPhase::Present))
                .collect();
            cx.notify();
            return;
        }

        let new_set: HashSet<&SharedString> = new_keys.iter().collect();
        let previous_phases = self
            .phases
            .iter()
            .cloned()
            .collect::<std::collections::HashMap<_, _>>();
        let mut new_phases = new_keys
            .iter()
            .map(|key| {
                let phase = if entering.contains(key) {
                    ItemPhase::Entering
                } else {
                    previous_phases
                        .get(key)
                        .filter(|phase| **phase != ItemPhase::Exiting)
                        .cloned()
                        .unwrap_or(ItemPhase::Present)
                };
                (key.clone(), phase)
            })
            .collect::<Vec<_>>();

        for (old_index, (key, _)) in self.phases.iter().enumerate() {
            if !new_set.contains(key) {
                new_phases.insert(
                    old_index.min(new_phases.len()),
                    (key.clone(), ItemPhase::Exiting),
                );
            }
        }

        self.phases = new_phases;
        self.keys = new_keys;
        cx.notify();

        let exit_dur = self.exit_duration;
        let has_exiting = !exiting.is_empty();
        let has_entering = !entering.is_empty();

        if has_exiting {
            let unmount_after = exit_dur + EXIT_GRACE;
            let exiting = exiting.clone();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(unmount_after).await;
                _ = this.update(cx, |state, cx| {
                    state.phases.retain(|(key, phase)| {
                        *phase != ItemPhase::Exiting || !exiting.contains(key)
                    });
                    cx.notify();
                });
            })
            .detach();
        }

        let enter_dur = self.enter_duration;
        if has_entering {
            let entering = entering.clone();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(enter_dur).await;
                _ = this.update(cx, |state, cx| {
                    for (key, phase) in &mut state.phases {
                        if *phase == ItemPhase::Entering && entering.contains(key) {
                            *phase = ItemPhase::Present;
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    pub fn visible_keys(&self) -> Vec<(SharedString, bool, bool)> {
        self.phases
            .iter()
            .map(|(key, phase)| {
                let entering = *phase == ItemPhase::Entering;
                let exiting = *phase == ItemPhase::Exiting;
                (key.clone(), entering, exiting)
            })
            .collect()
    }

    pub fn version(&self) -> usize {
        self.version
    }
}

#[derive(IntoElement)]
pub struct AnimatedList {
    id: ElementId,
    state: Entity<AnimatedListState>,
    children_map: Vec<(SharedString, AnyElement)>,
    style: StyleRefinement,
}

impl AnimatedList {
    pub fn new(id: impl Into<ElementId>, state: Entity<AnimatedListState>) -> Self {
        Self {
            id: id.into(),
            state,
            children_map: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn child_keyed(mut self, key: impl Into<SharedString>, element: impl IntoElement) -> Self {
        self.children_map
            .push((key.into(), element.into_any_element()));
        self
    }
}

impl Styled for AnimatedList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for AnimatedList {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let state = self.state.read(cx);
        let visible = state.visible_keys();
        let version = state.version();
        let enter_dur = state.enter_duration;
        let exit_dur = state.exit_duration;

        let mut children_by_key: std::collections::HashMap<SharedString, AnyElement> =
            self.children_map.into_iter().collect();

        let slide_offset = px(12.0);

        let elements: Vec<AnyElement> = visible
            .into_iter()
            .filter_map(|(key, entering, exiting)| {
                let child = children_by_key.remove(&key);

                child.map(|element| {
                    if entering {
                        div()
                            .child(element)
                            .with_animation(
                                ElementId::Name(format!("al-enter-{}-{}", key, version).into()),
                                Animation::new(enter_dur).with_easing(easings::ease_out_cubic),
                                move |el, delta| {
                                    el.opacity(delta)
                                        .mt(lerp_pixels(slide_offset, px(0.0), delta))
                                },
                            )
                            .into_any_element()
                    } else if exiting {
                        div()
                            .child(element)
                            .with_animation(
                                ElementId::Name(format!("al-exit-{}-{}", key, version).into()),
                                Animation::new(exit_dur).with_easing(easings::ease_in_cubic),
                                move |el, delta| {
                                    el.opacity(1.0 - delta).mt(lerp_pixels(
                                        px(0.0),
                                        slide_offset,
                                        delta,
                                    ))
                                },
                            )
                            .into_any_element()
                    } else {
                        div().child(element).into_any_element()
                    }
                })
            })
            .collect();

        div()
            .id(self.id)
            .flex()
            .flex_col()
            .children(elements)
            .map(|this| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn keyed_items_follow_reorders_without_becoming_stale() {
        let mut cx = kael::TestAppContext::single();
        let state = cx.new(AnimatedListState::new);
        let visible = cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_keys(vec!["a".into(), "b".into(), "c".into()], cx);
                state.set_keys(vec!["c".into(), "a".into(), "b".into()], cx);
            });
            state
                .read(cx)
                .visible_keys()
                .into_iter()
                .map(|(key, _, _)| key)
                .collect::<Vec<_>>()
        });
        assert_eq!(
            visible,
            vec![
                SharedString::from("c"),
                SharedString::from("a"),
                SharedString::from("b")
            ]
        );
    }

    #[::core::prelude::v1::test]
    fn duplicate_keys_are_normalized_at_the_state_boundary() {
        let mut cx = kael::TestAppContext::single();
        let state = cx.new(AnimatedListState::new);
        let visible_count = cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_keys(vec!["a".into(), "a".into(), "b".into()], cx);
            });
            state.read(cx).visible_keys().len()
        });

        assert_eq!(visible_count, 2);
    }

    #[::core::prelude::v1::test]
    fn reduced_motion_applies_key_changes_without_transient_phases() {
        let mut cx = kael::TestAppContext::single();
        cx.set_reduce_motion(true);
        let state = cx.new(AnimatedListState::new);

        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_keys(vec!["a".into(), "b".into()], cx);
                state.set_keys(vec!["b".into(), "c".into()], cx);
            });

            let visible = state.read(cx).visible_keys();
            assert_eq!(visible.len(), 2);
            assert!(
                visible
                    .iter()
                    .all(|(_, entering, exiting)| { !entering && !exiting })
            );
        });
    }
}
